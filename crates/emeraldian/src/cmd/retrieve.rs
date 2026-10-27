//! `emeraldian index`, `emeraldian recall` and `emeraldian similar`, and what
//! `mem recall` shares with them: the vault's retrieval index, and questions
//! put to it.
//!
//! The index lives in the cache directory, not the vault, so the vault stays
//! plain Markdown, and every question brings it up to date first: words and
//! links always, vectors for a few changed notes at a time. The model is
//! EmbeddingGemma 2, run by Ollama unless the config says otherwise, and
//! downloaded the first time `emeraldian index` is run. Without it, questions
//! are answered by words and links alone.

use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use emeraldian_agent::embed::{Api, Embedder, PullStatus};
use emeraldian_core::note::NoteMeta;
use emeraldian_core::retrieval::search::{self, Answer, PassageHit, Question};
use emeraldian_core::retrieval::store::{self, BUSY, Embedding, Store, View};
use emeraldian_core::retrieval::{Embed, Prompts, text};
use emeraldian_core::vault::{Scan, Vault};
use serde_json::{Value, json};

use super::{Args, Ctx, Failure, Output, Result, clip, display, fail};
use crate::clock;

/// Below this cosine a passage has nothing to do with a question, for
/// EmbeddingGemma 2. The best match for a question the vault can't answer
/// still scores 0.61 to 0.70, so the floor sits in that range: at 0.65 it
/// keeps 96.6 to 100 in a hundred of the right answers, and leaves 39 to 81
/// in a hundred of such questions with no answer at all, rather than the
/// nearest thing to one.
const FLOOR: f32 = 0.65;

/// The most notes a vault-wide question embeds before it is answered. Past
/// this the answer would wait on the model, so it answers by words and links
/// for the rest, and says so.
const FRESHEN_VAULT: usize = 25;

/// A question asked within this long of the last keeps the model loaded for
/// as long again, [`KEEP_ASKING`]: agents ask minutes apart, and loading
/// the model again costs about a second. One on its own, and indexing, keep
/// it for Ollama's own [`KEEP_DEFAULT`].
const ASKING: u64 = 30 * 60;
const KEEP_ASKING: &str = "30m";
const KEEP_DEFAULT: &str = "5m";

/// The model behind a real server.
struct Remote(Embedder);

impl Embed for Remote {
    fn embed(&mut self, texts: &[String]) -> std::result::Result<Vec<Vec<f32>>, String> {
        self.0.embed(texts).map_err(|err| err.to_string())
    }
}

impl Ctx {
    fn prompts(&self) -> Prompts {
        Prompts {
            query: self.config.embeddings.query_prompt.clone(),
            document: self.config.embeddings.document_prompt.clone(),
        }
    }

    /// Model, size and prompt, as the index records them: vectors from a
    /// different one can't be compared with these.
    fn signature(&self) -> String {
        let config = &self.config.embeddings;
        format!(
            "{}|{}|{}",
            config.model, config.dimensions, config.document_prompt
        )
    }

    fn remote(&self, keep_alive: Option<String>) -> Result<Embedder> {
        let config = &self.config.embeddings;
        let Some(api) = Api::parse(&config.provider) else {
            return fail(format!(
                "embeddings.provider is `ollama` or `openai`, not `{}`",
                config.provider
            ));
        };
        let keep_alive = if config.keep_alive.trim().is_empty() {
            keep_alive
        } else {
            Some(config.keep_alive.trim().to_string())
        };
        Ok(Embedder {
            api,
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            dimensions: (config.dimensions > 0).then_some(config.dimensions),
            key: std::env::var("EMERALDIAN_EMBEDDINGS_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty()),
            keep_alive,
        })
    }

    /// The vault's index, in the cache directory, named for the vault's path.
    fn store(&mut self) -> Result<Store> {
        let vault = self.root()?;
        let dir = match &self.cache_dir {
            Some(dir) => dir.clone(),
            None => dirs::cache_dir()
                .ok_or_else(|| Failure::Failed("this platform has no cache directory".into()))?
                .join("emeraldian"),
        };
        Ok(Store::open(
            &dir.join("retrieval")
                .join(store::key(&vault.to_string_lossy())),
        ))
    }

    /// The walk that lists the notes under `folder` (all of them for `""`),
    /// to be made on a thread of its own; or the listing already made.
    fn listing(&mut self, folder: &str) -> Result<Listing> {
        if folder.is_empty()
            && let Some(scan) = self.scan.take()
        {
            return Ok(Listing::Made(scan));
        }
        Ok(Listing::Walk {
            root: self.root()?,
            folder: folder.to_string(),
            options: emeraldian_core::vault::ScanOptions {
                include_attachments: false,
                ..self.scan_options()?
            },
        })
    }

    /// The notes in one folder, from a walk of that folder alone.
    fn notes_in(&mut self, folder: &str) -> Result<Vec<NoteMeta>> {
        Ok(self.listing(folder)?.list()?.notes)
    }
}

/// `folder` as the disk spells it, where the disk opens it by the name given.
///
/// A filesystem that ignores case, as macOS's and Windows' do, opens
/// `Projects/` for `projects`. Its notes, listed as `projects/…`, would be new
/// notes to the index, kept beside the same notes under their real paths
/// until a walk of the whole vault found them gone. A folder the disk has no
/// entry for is left as given.
pub(super) fn spelt_on_disk(root: &Path, folder: &str) -> String {
    let mut dir = root.to_path_buf();
    let mut spelt: Vec<String> = Vec::new();
    for name in folder.split('/').filter(|name| !name.is_empty()) {
        let name = if dir.join(name).is_dir() {
            entry_named(&dir, name).unwrap_or_else(|| name.to_string())
        } else {
            name.to_string()
        };
        dir.push(&name);
        spelt.push(name);
    }
    spelt.join("/")
}

/// The entry of `dir` that `name` opens: the one spelt so, if there is one,
/// else one that differs from it only in case.
fn entry_named(dir: &Path, name: &str) -> Option<String> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    if names.iter().any(|entry| entry == name) {
        return Some(name.to_string());
    }
    let lower = name.to_lowercase();
    names
        .into_iter()
        .find(|entry| entry.to_lowercase() == lower)
}

/// The notes a question is put to: listed already, or a walk to make.
enum Listing {
    Made(Scan),
    Walk {
        root: PathBuf,
        folder: String,
        options: emeraldian_core::vault::ScanOptions,
    },
}

impl Listing {
    /// The notes, walked for if need be: a folder that can't be read has
    /// none, but a vault whose root can't be read is an error, as it is for
    /// every command.
    fn list(self) -> std::io::Result<Scan> {
        match self {
            Listing::Made(scan) => Ok(scan),
            Listing::Walk {
                root,
                folder,
                options,
            } => {
                if folder.is_empty() {
                    return Vault::from_path(root).scan(&options);
                }
                let Ok(mut scan) = Vault::from_path(root.join(&folder)).scan(&options) else {
                    return Ok(Scan::default());
                };
                for meta in &mut scan.notes {
                    meta.rel = format!("{folder}/{}", meta.rel);
                }
                scan.folders.clear();
                Ok(scan)
            }
        }
    }
}

/// What a question came back with, and anything to say about how.
pub(super) struct Asked {
    pub answer: Answer,
    pub notice: Option<String>,
    /// Whether the vault is set up for meaning at all: `emeraldian index` has
    /// been run with the model now configured.
    pub meaning: bool,
    /// The index the question was put to, for anything else asked of it.
    pub view: View,
}

/// Puts a question to the index, after bringing the notes under `folder` (all
/// of them for `""`, and as [`spelt_on_disk`]) up to date: words and links
/// always, and vectors for up to `embed_limit` notes when the vault has been
/// set up for meaning.
///
/// Three things are under way at once. The question goes to the model first,
/// on a thread of its own: the model reads one request at a time, so a
/// question sent after the refresh's passages would wait behind them, and a
/// model that has to be loaded is loaded while the vault is walked. The walk
/// has a thread of its own too. And meanwhile the index is opened and, once
/// the question's vector is back, asked: if the refresh then finds nothing
/// changed, as it mostly doesn't, that is the answer, and the question is
/// asked only once the walk is done when something did change.
pub(super) fn answer(
    ctx: &mut Ctx,
    question: &str,
    folder: &str,
    prior: &dyn Fn(&str) -> f32,
    limit: usize,
    embed_limit: usize,
) -> Result<Asked> {
    let mut store = ctx.store()?;
    let folder = spelt_on_disk(&ctx.root()?, folder);
    let folder = folder.as_str();
    let prefix = format!("{folder}/");
    let in_scope = |rel: &str| folder.is_empty() || rel.starts_with(&prefix);
    let mut notices: Vec<String> = Vec::new();

    // Meaning, if this vault has been indexed with the model now configured.
    let signature = ctx.signature();
    let set_up = !store.signature().is_empty();
    let matched = set_up && store.signature() == signature;
    let mut semantic = matched;
    if set_up && !semantic {
        notices.push(
            "the model in the config isn't the one the index was made with; \
             `emeraldian index` embeds the notes again"
                .into(),
        );
    }
    let recent = clock::now().saturating_sub(store.last_question()) < ASKING;
    let keep_alive = Some(if recent { KEEP_ASKING } else { KEEP_DEFAULT }.to_string());
    let prompts = ctx.prompts();
    let query = prompts.query(question);

    // The question's vector: as it was last time it was asked, or from the
    // model, a server's on a thread of its own, a test's stand-in at once.
    let mut model: Option<Box<dyn Embed>> = None;
    let mut asked_already: Option<std::result::Result<Vec<f32>, String>> = None;
    let mut server: Option<Embedder> = None;
    if semantic {
        if let Some(vector) = store.remembered(&signature, &query) {
            asked_already = Some(Ok(vector));
        }
        match ctx.embedder.take() {
            Some(mut stand_in) => {
                if asked_already.is_none() {
                    asked_already =
                        Some(first_vector(stand_in.embed(std::slice::from_ref(&query))));
                }
                model = Some(stand_in);
            }
            None => server = Some(ctx.remote(keep_alive)?),
        }
    }
    let listing = ctx.listing(folder)?;
    let dir = store.dir().to_path_buf();

    let (vector, early, scan) = std::thread::scope(|scope| {
        let asking = match (&asked_already, &server) {
            (None, Some(server)) => {
                let (server, query, signature, dir) = (
                    server.clone(),
                    query.clone(),
                    signature.clone(),
                    dir.clone(),
                );
                Some(scope.spawn(move || {
                    let vector = first_vector(
                        server
                            .embed(std::slice::from_ref(&query))
                            .map_err(|err| err.to_string()),
                    );
                    if let Ok(vector) = &vector {
                        store::remember(&dir, &signature, &query, vector);
                    }
                    vector
                }))
            }
            _ => None,
        };
        let walking = scope.spawn(move || listing.list());
        // The index as it stands, opened while the vault is walked.
        let prepared = store.prepare().is_ok();
        let vector = match asking {
            Some(asking) => Some(
                asking
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
            ),
            None => asked_already.take(),
        };
        let early = prepared.then(|| {
            let vector = vector.as_ref().and_then(|v| v.as_ref().ok());
            let view = store.prepared().expect("prepared");
            let answer = search::ask(
                view,
                &Question {
                    text: question,
                    vector: vector.map(Vec::as_slice),
                    in_scope: &in_scope,
                    prior,
                    floor: FLOOR,
                    limit,
                },
            );
            (answer, vector.is_some())
        });
        let scan = walking
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (vector, early, scan)
    });
    let scan = scan?;
    let mut vector = match vector {
        Some(Ok(vector)) => Some(vector),
        Some(Err(why)) => {
            // The model is out of reach: the refresh doesn't wait on it too,
            // though it still gives a changed note the vectors it has.
            semantic = false;
            notices.push(format!(
                "meaning is unavailable ({why}); answered by words and links"
            ));
            None
        }
        None => None,
    };
    if matched && model.is_none() {
        model = server.map(|server| Box::new(Remote(server)) as Box<dyn Embed>);
    }

    let dims = ctx.config.embeddings.dimensions;
    let embedding = model.as_mut().filter(|_| matched).map(|model| Embedding {
        model: &mut **model,
        prompts: &prompts,
        signature: &signature,
        dims,
        limit: if semantic { embed_limit } else { 0 },
    });
    match store.refresh(&scan.notes, &in_scope, embedding, &mut |_| {}) {
        Ok(report) => {
            if let Some(why) = report.model_error {
                semantic = false;
                vector = None;
                notices.push(format!(
                    "meaning is unavailable ({why}); answered by words and links"
                ));
            } else if semantic && report.pending > 0 {
                notices.push(format!(
                    "{} notes are matched by words until `emeraldian index` embeds them",
                    report.pending
                ));
            }
        }
        // Another process is writing: the index as it stands will do.
        Err(why) if why == BUSY => {}
        Err(why) => notices.push(format!("the index couldn't be brought up to date: {why}")),
    }
    if folder.is_empty() {
        ctx.scan = Some(scan);
    }

    // The view the refresh ended with: the index isn't read twice. The answer
    // given before it is the answer if the refresh changed nothing.
    let view = store.take_view()?;
    let answer = match early {
        Some((Ok(answer), with_vector)) if store.unchanged() && with_vector == vector.is_some() => {
            answer
        }
        _ => search::ask(
            &view,
            &Question {
                text: question,
                vector: vector.as_deref(),
                in_scope: &in_scope,
                prior,
                floor: FLOOR,
                limit,
            },
        )?,
    };
    if semantic {
        store.asked();
    }
    let notice = (!notices.is_empty()).then(|| format!("emeraldian: {}", notices.join("; ")));
    Ok(Asked {
        answer,
        notice,
        meaning: set_up,
        view,
    })
}

/// The one vector a model gave for one text.
fn first_vector(
    got: std::result::Result<Vec<Vec<f32>>, String>,
) -> std::result::Result<Vec<f32>, String> {
    got.and_then(|mut vectors| {
        if vectors.is_empty() {
            Err("the model returned no vector".into())
        } else {
            Ok(vectors.swap_remove(0))
        }
    })
}

/// The line of a passage to show for it: the one with most of the question,
/// else its first that isn't a heading. Lines the commands write around a
/// note, its `Project:` links, are passed over.
///
/// The question and the lines are read as the index reads them: whole terms,
/// stemmed, without the small words that only hold a question together, or
/// `how is the password hashed` would find its line in `This is how it was`.
/// And a term that few of the lines have counts for more than one most of
/// them have, as it says more about which line is meant.
pub(super) fn best_line(
    content: &str,
    start: u32,
    end: u32,
    question: &str,
) -> Option<(usize, String)> {
    let mut wanted = text::question_terms(question);
    wanted.sort();
    wanted.dedup();
    let lines: Vec<(usize, &str)> = content
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim()))
        .filter(|(n, line)| {
            *n >= start as usize
                && *n <= end as usize
                && !line.is_empty()
                && !line.starts_with("Project: [[")
        })
        .collect();
    let terms: Vec<HashSet<String>> = lines
        .iter()
        .map(|(_, line)| text::index_terms(line).into_iter().collect())
        .collect();
    // Each of the question's terms the lines have, by how few of them have
    // it: a little under one for a term in every line, and more the fewer.
    let weights: Vec<(&String, f32)> = wanted
        .iter()
        .filter_map(|term| {
            let having = terms.iter().filter(|line| line.contains(term)).count();
            (having > 0).then(|| (term, (1.0 + lines.len() as f32 / having as f32).ln()))
        })
        .collect();
    // The first line with the most of the question, if any has some of it.
    let mut best: Option<(usize, f32)> = None;
    for (at, line) in terms.iter().enumerate() {
        let score: f32 = weights
            .iter()
            .filter(|(term, _)| line.contains(*term))
            .map(|(_, weight)| weight)
            .sum();
        if score > 0.0 && best.is_none_or(|(_, most)| score > most) {
            best = Some((at, score));
        }
    }
    let line = match best {
        Some((at, _)) => lines.get(at),
        None => lines
            .iter()
            .find(|(_, line)| !line.starts_with('#'))
            .or_else(|| lines.first()),
    }?;
    Some((line.0, line.1.to_string()))
}

/// The line to show for a note found: its best line, or for a drawing, whose
/// file is a scene and not lines to read, the best of the words in it.
pub(super) fn shown_line(
    rel: &str,
    content: &str,
    start: u32,
    end: u32,
    question: &str,
) -> Option<(usize, String)> {
    if emeraldian_core::excalidraw::is_drawing(rel) {
        let words = text::passages(rel, content, "").into_iter().next()?.text;
        let count = words.lines().count() as u32;
        return best_line(&words, 1, count, question).map(|(_, word)| (start as usize, word));
    }
    best_line(content, start, end, question)
}

/// The answer as a list: each note, its score, and its best line, with the
/// links between them and the notes that bridge them.
fn list(ctx: &mut Ctx, question: &str, answer: &Answer) -> Result<(String, Value)> {
    let root = ctx.root()?;
    let mut text = Vec::new();
    let mut notes = Vec::new();
    for hit in &answer.notes {
        let content = std::fs::read_to_string(root.join(&hit.rel)).unwrap_or_default();
        let line = shown_line(&hit.rel, &content, hit.start, hit.end, question);
        let linked: Vec<&str> = answer
            .links
            .iter()
            .filter(|(from, _)| from == &hit.rel)
            .map(|(_, to)| display(to))
            .collect();
        let mut head = format!("{}  {:.2}", display(&hit.rel), hit.score);
        if !linked.is_empty() {
            head.push_str(&format!("  links {}", linked.join(", ")));
        }
        if hit.by_graph {
            head.push_str("  (via links)");
        }
        text.push(head);
        if let Some((n, line)) = &line {
            text.push(format!("  {n}: {}", clip(line, 160)));
        }
        notes.push(json!({
            "note": hit.rel,
            "title": hit.title,
            "score": hit.score,
            "lines": [hit.start, hit.end],
            "line": line.as_ref().map(|(n, t)| json!({ "line": n, "text": t })),
            "links": linked,
            "via_links": hit.by_graph,
        }));
    }
    if !answer.bridges.is_empty() {
        let names: Vec<&str> = answer.bridges.iter().map(|rel| display(rel)).collect();
        text.push(format!("bridging these: {}", names.join(", ")));
    }
    Ok((
        text.join("\n"),
        json!({
            "notes": notes,
            "bridges": answer.bridges,
            "semantic": answer.semantic,
        }),
    ))
}

/// The answer as context: its passages, a round at a time as the answer
/// deals them, until `budget` tokens are spent, each headed by where it is
/// from.
///
/// A passage is given as the index cut it, which isn't always whole lines: a
/// paragraph on one line too long for a passage is several, and a drawing's
/// passage is the words in it, not the scene its file mostly holds. So each
/// note is cut again as the index cuts it, and each passage taken from there.
/// One too long for what is left of the budget is cut short after a word,
/// rather than left out and everything after it with it.
pub(super) fn pack(ctx: &mut Ctx, answer: &Answer, budget: usize) -> Result<(String, Value)> {
    let root = ctx.root()?;
    let mut text = String::new();
    let mut spent = 0;
    let mut used = Vec::new();
    // Each note's passages, cut once however many of them are given.
    let mut notes: HashMap<&str, Vec<text::Passage>> = HashMap::new();
    // The passages given so far, by note and place: of a note changed since
    // it was indexed, two found may be the same one now.
    let mut given: HashSet<(&str, usize)> = HashSet::new();
    for hit in &answer.passages {
        let passages = notes.entry(hit.rel.as_str()).or_insert_with(|| {
            std::fs::read_to_string(root.join(&hit.rel))
                .map(|content| text::passages(&hit.rel, &content, &hit.title))
                .unwrap_or_default()
        });
        let Some(place) = place_of(passages, hit) else {
            continue;
        };
        if !given.insert((hit.rel.as_str(), place)) {
            continue;
        }
        let passage = &passages[place];
        let body = without_project_lines(&passage.text);
        if body.is_empty() {
            continue;
        }
        let head = format!(
            "### {}:{}-{}\n",
            display(&hit.rel),
            passage.start,
            passage.end
        );
        // Its text's line end and the blank line after it cost a token at
        // most, and so does a `…` with them.
        let room = budget.saturating_sub(spent + text::tokens(&head) + 1);
        let (shown, short) = if text::tokens(&body) <= room {
            (format!("{body}\n"), false)
        } else {
            let kept = shortened(&body, room);
            if kept.is_empty() {
                break;
            }
            (format!("{kept}…\n"), true)
        };
        let block = format!("{head}{shown}\n");
        spent += text::tokens(&block);
        text.push_str(&block);
        used.push(json!({ "note": hit.rel, "lines": [passage.start, passage.end], "text": shown }));
        if short {
            break;
        }
    }
    // The links between the answers, counted with the rest, and left out
    // where they would take the answer past its budget.
    let mut linked: &[(String, String)] = &[];
    if !answer.links.is_empty() {
        let links: Vec<String> = answer
            .links
            .iter()
            .map(|(from, to)| format!("{} -> {}", display(from), display(to)))
            .collect();
        let line = format!("links: {}\n", links.join("; "));
        let cost = text::tokens(&line);
        if spent + cost <= budget {
            spent += cost;
            text.push_str(&line);
            linked = &answer.links;
        }
    }
    Ok((
        text.trim_end().to_string(),
        json!({ "passages": used, "tokens": spent, "links": linked, "semantic": answer.semantic }),
    ))
}

/// Where `hit` is among its note's passages as they are cut now: at its
/// place, if the passage there has its lines, as it does unless the note has
/// changed since it was indexed; else the passage with the most of its lines,
/// the nearest there now is to what was found.
fn place_of(passages: &[text::Passage], hit: &PassageHit) -> Option<usize> {
    let (start, end) = (hit.start as usize, hit.end as usize);
    let place = hit.place as usize;
    if passages
        .get(place)
        .is_some_and(|passage| passage.start == start && passage.end == end)
    {
        return Some(place);
    }
    let mut best: Option<(usize, usize)> = None;
    for (at, passage) in passages.iter().enumerate() {
        let shared = (passage.end.min(end) + 1).saturating_sub(passage.start.max(start));
        if shared > 0 && best.is_none_or(|(_, most)| shared > most) {
            best = Some((at, shared));
        }
    }
    best.map(|(at, _)| at)
}

/// A passage's text without the lines the commands write around a note, its
/// `Project:` links, and without the blank lines that leaves at either end,
/// which are only tokens spent.
fn without_project_lines(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.starts_with("Project: [["))
        .collect();
    let first = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    let last = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map_or(first, |at| at + 1);
    lines[first..last].join("\n")
}

/// The most of `body` that costs no more than `tokens`: cut after a word
/// where that keeps more than half of what fits, and otherwise between two
/// characters, as a run without spaces, a hash or a line of Chinese, has to
/// be.
fn shortened(body: &str, tokens: usize) -> &str {
    let ends: Vec<usize> = body
        .char_indices()
        .map(|(at, c)| at + c.len_utf8())
        .collect();
    let fits = ends.partition_point(|&end| text::tokens(&body[..end]) <= tokens);
    let Some(&end) = fits.checked_sub(1).and_then(|last| ends.get(last)) else {
        return "";
    };
    let head = &body[..end];
    if end == body.len() || body[end..].starts_with(char::is_whitespace) {
        return head.trim_end();
    }
    match head.rfind(char::is_whitespace) {
        Some(space) if space > head.len() / 2 => head[..space].trim_end(),
        _ => head,
    }
}

/// The folder a command was asked about, made safe.
fn folder_of(ctx: &mut Ctx, args: &Args) -> Result<String> {
    match args.value("folder") {
        Some(folder) => Ok(ctx.folder(folder)?.0),
        None => Ok(String::new()),
    }
}

/// `--budget`, in tokens.
pub(super) fn budget_of(args: &Args) -> Result<Option<usize>> {
    match args.value("budget") {
        None => Ok(None),
        Some(text) => match text.trim().parse::<usize>() {
            Ok(n) if n >= 50 => Ok(Some(n)),
            _ => Err(Failure::Usage(format!(
                "--budget takes a number of tokens, 50 or more, not `{text}`"
            ))),
        },
    }
}

/// `emeraldian recall`.
pub(super) fn recall(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let question = args.at(0);
    let limit = args.limit(8)?;
    let budget = budget_of(args)?;
    let folder = folder_of(ctx, args)?;
    let asked = answer(ctx, question, &folder, &|_| 0.0, limit, FRESHEN_VAULT)?;
    let (text, json) = if asked.answer.notes.is_empty() {
        (
            format!("nothing in the vault answers `{question}`"),
            json!({ "notes": [], "semantic": asked.answer.semantic }),
        )
    } else if let Some(budget) = budget {
        pack(ctx, &asked.answer, budget)?
    } else {
        list(ctx, question, &asked.answer)?
    };
    let output = Output::new(text, json);
    Ok(match asked.notice {
        Some(notice) => output.notice(notice),
        None => output,
    })
}

/// `emeraldian similar <NOTE>`: the notes closest to one note, by its meaning,
/// its words and its links, and which of them it is linked to yet.
pub(super) fn similar(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let limit = args.limit(10)?;
    let content = super::read(&found)?;
    // The note's own title and opening as the question: what it is about.
    let title = found
        .name()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let opening: String = text::passages(&found.rel, &content, &title)
        .into_iter()
        .take(2)
        .map(|p| p.text)
        .collect::<Vec<_>>()
        .join("\n");
    let question = clip(&format!("{title}\n{opening}"), 2_000);
    let rel = found.rel.clone();
    let asked = answer(
        ctx,
        &question,
        "",
        &|other| if other == rel { -10.0 } else { 0.0 },
        limit + 1,
        FRESHEN_VAULT,
    )?;

    let neighbours = search::neighbours(&asked.view, &found.rel);
    let rows: Vec<(String, f32, bool)> = asked
        .answer
        .notes
        .iter()
        .filter(|hit| hit.rel != found.rel)
        .take(limit)
        .map(|hit| (hit.rel.clone(), hit.score, neighbours.contains(&hit.rel)))
        .collect();
    let text = if rows.is_empty() {
        format!("no note is close to {}", found.name())
    } else {
        rows.iter()
            .map(|(rel, score, linked)| {
                format!(
                    "{}  {score:.2}{}",
                    display(rel),
                    if *linked { "  linked" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let json = json!({
        "note": found.rel,
        "similar": rows.iter().map(|(rel, score, linked)| json!({ "note": rel, "score": score, "linked": linked })).collect::<Vec<_>>(),
    });
    let output = Output::new(text, json);
    Ok(match asked.notice {
        Some(notice) => output.notice(notice),
        None => output,
    })
}

/// `emeraldian index`: sets the vault up for recall, or brings it up to date.
pub(super) fn index(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let mut store = ctx.store()?;
    if args.flag("status") {
        return status(ctx, &store);
    }
    if args.flag("rebuild") {
        store.clear().map_err(Failure::Failed)?;
    }
    let notes = ctx.notes_in("")?;

    // The model: checked for, and downloaded if it isn't there yet. Without
    // one the index is made of words and links, and says how to add meaning.
    let mut unavailable = None;
    let mut model: Option<Box<dyn Embed>> = match ctx.embedder.take() {
        Some(model) => Some(model),
        None => {
            let remote = ctx.remote(Some(KEEP_DEFAULT.to_string()))?;
            match prepare(&remote) {
                Ok(()) => Some(Box::new(Remote(remote))),
                Err(why) => {
                    unavailable = Some(why);
                    None
                }
            }
        }
    };

    let prompts = ctx.prompts();
    let signature = ctx.signature();
    let dims = ctx.config.embeddings.dimensions;
    let started = Instant::now();
    let mut meter = Meter::new("embedding", "passages");
    let embedding = model.as_mut().map(|model| Embedding {
        model: &mut **model,
        prompts: &prompts,
        signature: &signature,
        dims,
        limit: usize::MAX,
    });
    let report = store.refresh(&notes, &|_| true, embedding, &mut |progress| {
        meter.show(progress.done as u64, progress.total as u64);
    });
    meter.finish();
    let report = report.map_err(Failure::Failed)?;
    let view = store.take_view()?;
    let embedded = view.notes.len() - view.pending(&|_| true);

    let mut text = format!(
        "{} notes, {} passages indexed by their words and links",
        view.notes.len(),
        view.passage_count()
    );
    if report.indexed > 0 || report.embedded > 0 || report.removed > 0 {
        text.push_str(&format!(
            "; {} read, {} embedded, {} removed in {}s",
            report.indexed,
            report.embedded,
            report.removed,
            started.elapsed().as_secs()
        ));
    }
    if model.is_some() && report.model_error.is_none() {
        text.push_str(&format!(
            "\nmeaning: {embedded} of {} notes, with {}",
            view.notes.len(),
            ctx.config.embeddings.model
        ));
    }
    if let Some(why) = report.model_error.or(unavailable) {
        text.push_str(&format!(
            "\nmeaning is not set up: {why}\nrecall works by words and links meanwhile"
        ));
    }
    text.push_str(
        "\nask with `emeraldian recall <QUESTION>`; `--budget N` returns the passages themselves",
    );
    Ok(Output::new(
        text,
        json!({
            "notes": view.notes.len(),
            "passages": view.passage_count(),
            "embedded": embedded,
            "read": report.indexed,
            "removed": report.removed,
            "index": store.dir(),
        }),
    ))
}

/// Makes sure the server is there and has the model, downloading it if not.
/// `Err` says, in a sentence or a few, what to do about it.
fn prepare(remote: &Embedder) -> std::result::Result<(), String> {
    if let Err(err) = remote.reachable() {
        return Err(unreachable(remote, &err.to_string()));
    }
    match remote.installed() {
        Ok(true) => Ok(()),
        Ok(false) => {
            eprintln!(
                "emeraldian: downloading {} through Ollama, once",
                remote.model
            );
            let mut meter = Meter::new(&format!("downloading {}", remote.model), "bytes");
            let pulled = remote.pull(&mut |status: &PullStatus| {
                if status.total > 0 {
                    meter.show(status.completed, status.total);
                }
            });
            meter.finish();
            pulled.map_err(|err| {
                let err = err.to_string();
                if err.contains("newer version of Ollama") {
                    format!(
                        "{} needs a newer Ollama than this one: update it from \
                         https://ollama.com/download (or `brew upgrade --cask ollama-app`), \
                         then run `emeraldian index` again",
                        remote.model
                    )
                } else {
                    format!("the download of {} failed: {err}", remote.model)
                }
            })
        }
        Err(err) => Err(unreachable(remote, &err.to_string())),
    }
}

/// How to get a server going.
fn unreachable(remote: &Embedder, why: &str) -> String {
    match remote.api {
        Api::Ollama => format!(
            "can't reach Ollama at {} ({why}). Meaning comes from EmbeddingGemma 2, run by \
             Ollama: install it from https://ollama.com/download (macOS: \
             `brew install --cask ollama-app`), start it, then run `emeraldian index` \
             again; it downloads the model, about 300 MB",
            remote.base_url
        ),
        Api::OpenAi => format!(
            "can't reach the embeddings server at {} ({why}); check base_url under [embeddings]",
            remote.base_url
        ),
    }
}

/// `index --status`.
fn status(ctx: &mut Ctx, store: &Store) -> Result<Output> {
    let notes = ctx.notes_in("")?;
    let config = ctx.config.embeddings.clone();
    let mut lines = Vec::new();
    let (count, stale, embedded) = if store.exists() {
        let view = store.view()?;
        let stale = view.stale(&notes).len() + view.gone(&notes, &|_| true);
        let embedded = view.notes.len() - view.pending(&|_| true);
        lines.push(format!(
            "index: {} notes, {} passages, {embedded} with meaning; {stale} changed since",
            view.notes.len(),
            view.passage_count()
        ));
        (view.notes.len(), stale, embedded)
    } else {
        lines.push("index: none yet; `emeraldian index` makes one".into());
        (0, notes.len(), 0)
    };
    lines.push(format!("kept in: {}", store.dir().display()));
    let server = if ctx.embedder.is_some() {
        "ready".to_string()
    } else {
        let remote = ctx.remote(None)?;
        match remote.reachable() {
            Err(err) => format!("not reachable ({err})"),
            Ok(()) => match remote.installed() {
                Ok(true) => "ready".into(),
                Ok(false) => "running, without the model; `emeraldian index` downloads it".into(),
                Err(err) => {
                    format!("running, but it couldn't say whether it has the model ({err})")
                }
            },
        }
    };
    lines.push(format!(
        "model: {} at {}: {server}",
        config.model, config.base_url
    ));
    Ok(Output::new(
        lines.join("\n"),
        json!({
            "notes": count,
            "stale": stale,
            "embedded": embedded,
            "index": store.dir(),
            "model": config.model,
            "server": server,
        }),
    ))
}

/// Progress on stderr, kept cheap to read: a line that rewrites itself in a
/// terminal, and a line a quarter of the way at a time when an agent or a log
/// is reading.
struct Meter {
    what: String,
    unit: &'static str,
    terminal: bool,
    shown: Option<u64>,
    last: Instant,
    drew: bool,
}

impl Meter {
    fn new(what: &str, unit: &'static str) -> Self {
        Self {
            what: what.to_string(),
            unit,
            terminal: std::io::stderr().is_terminal(),
            shown: None,
            last: Instant::now(),
            drew: false,
        }
    }

    fn show(&mut self, done: u64, total: u64) {
        if total == 0 {
            return;
        }
        let percent = done.min(total) * 100 / total;
        let size = if self.unit == "bytes" {
            format!("{} MB", total / 1_000_000)
        } else {
            format!("{total} {}", self.unit)
        };
        if self.terminal {
            if self.last.elapsed() >= Duration::from_millis(200) || done >= total {
                eprint!("\r{}: {percent}% of {size}", self.what);
                self.last = Instant::now();
                self.drew = true;
            }
        } else {
            let quarter = percent / 25;
            if self.shown != Some(quarter) {
                eprintln!("emeraldian: {}: {percent}% of {size}", self.what);
                self.shown = Some(quarter);
            }
        }
    }

    fn finish(&mut self) {
        if self.drew {
            eprintln!();
            self.drew = false;
        }
    }
}
