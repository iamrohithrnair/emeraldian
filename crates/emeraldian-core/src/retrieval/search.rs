//! Answering a question from the index: recall by words and by meaning, a
//! score for each note found, and a walk over the links. See the
//! [module](super) for the shape.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;

use super::graph::Links;
use super::store::{View, fit};
use super::text;

/// How many passages recall by words brings back, best first.
const WORDS_RECALL: usize = 200;
/// How many passages recall by meaning brings back. More than by words: the
/// first pass compares only the vectors' first components, and a passage it
/// ranks a few hundred places down is now and then the best by the whole
/// vector.
const MEANING_RECALL: usize = 500;
/// BM25's term saturation and length normalisation, at their usual values.
const K1: f32 = 1.2;
const B: f32 = 0.75;
/// The most a note's words add to its cosine, at the best BM25 of the notes
/// found: enough to order notes the model scores alike, and to lift the one
/// that names what was asked about, not enough to overrule the model.
const WORDS_WEIGHT: f32 = 0.05;
/// Without meaning, the least share of a question some passage must have for
/// the vault to answer it at all: of its terms, each weighted by how rare it
/// is in the vault, a term that no note has weighing the most. Below it, the
/// best passage shares a common word or two with the question and nothing
/// more. A note the model hasn't read yet needs as much to answer beside
/// notes it has.
const WORDS_COVER: f32 = 0.4;
/// The most a note's links add to its score.
const LINKS_WEIGHT: f32 = 0.15;
/// The least share of the walk's start a note that answers has: one just at
/// the floor, or one the model hasn't read, still starts it, if barely.
const LEAST_WEIGHT: f32 = 1e-6;
/// A note whose links add at least this to its score is marked as found
/// through them.
const BY_LINKS: f32 = 0.01;
/// Notes the walk starts from: the best few. Starting from more brings in
/// the links of notes that only nearly answer.
const SEEDS: usize = 5;
/// The chance of a walk returning to its start at each step: HippoRAG's 0.5,
/// which keeps the walk near the notes found.
const RESTART: f32 = 0.5;
/// Steps of the walk. Each keeps half of what the one before added, so past
/// a few the scores hardly move.
const WALK_STEPS: usize = 8;
/// How far out from its start the walk may go, in links, and how many notes
/// it may touch: a bound on the work, however connected the vault.
const HOPS: usize = 2;
const MAX_NODES: usize = 5_000;

/// A question, and how to answer it.
pub struct Question<'a> {
    pub text: &'a str,
    /// The question's embedding, or `None` to answer by words and links.
    pub vector: Option<&'a [f32]>,
    pub in_scope: &'a dyn Fn(&str) -> bool,
    /// Added to a note's score: what kind of note it is, or how recent, where
    /// the caller has a view on that. With meaning, scores are cosines, a few
    /// hundredths apart where the model can't tell notes apart.
    pub prior: &'a dyn Fn(&str) -> f32,
    /// Below this cosine, a passage is taken to have nothing to do with the
    /// question.
    pub floor: f32,
    /// How many notes to return.
    pub limit: usize,
}

/// A note that answers.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteHit {
    pub rel: String,
    pub title: String,
    /// Its best passage's cosine, with what its words, its links and the
    /// prior add; without meaning, its words' BM25 as a share of the best
    /// note's, with the same.
    pub score: f32,
    /// Its best passage's lines.
    pub start: u32,
    pub end: u32,
    /// Lifted by its links: they add a hundredth or more to its score.
    pub by_graph: bool,
}

/// A passage that answers.
#[derive(Debug, Clone, PartialEq)]
pub struct PassageHit {
    pub rel: String,
    pub title: String,
    pub start: u32,
    pub end: u32,
    /// Which of its note's passages it is, from 0, in the order
    /// [`text::passages`] cuts the note into them: a paragraph on one line
    /// too long for a passage is several, all with the same lines.
    pub place: u32,
    pub score: f32,
}

/// What the index knows about a question.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answer {
    /// Best first, one each.
    pub notes: Vec<NoteHit>,
    /// The answering notes' passages a round at a time: each note's best, in
    /// the notes' order, then each one's second best, and so on. What a
    /// budgeted answer is assembled from, so it is spread over the answers
    /// rather than spent on the first.
    pub passages: Vec<PassageHit>,
    /// Notes outside the answer that several answers link to, of those the
    /// question may be answered from.
    pub bridges: Vec<String>,
    /// Links between answering notes, from and to.
    pub links: Vec<(String, String)>,
    /// Whether meaning took part, or only words and links.
    pub semantic: bool,
}

type Key = (usize, usize);

/// What a question's words found.
#[derive(Default)]
struct Lexical {
    /// Every passage that may answer and has a term of the question, by its
    /// global number: its BM25 score, and how much of the question it
    /// covers, as [`WORDS_COVER`] counts it.
    passages: HashMap<u32, (f32, f32)>,
    /// The best [`WORDS_RECALL`] of them, best first.
    best: Vec<u32>,
}

/// A note found, and the passages it is judged by.
struct Found {
    key: Key,
    /// Global number, cosine where there is one, BM25 score and cover.
    passages: Vec<(u32, Option<f32>, f32, f32)>,
    /// The best of those passages' cosines.
    cosine: Option<f32>,
    /// The best BM25 score of any of its passages.
    words: f32,
}

/// A note that answers, before the walk.
struct Kept {
    key: Key,
    /// What its cosine and its words say.
    base: f32,
    /// Its share of the walk's start.
    weight: f32,
    /// Its passages that answer, as (score, global number), best first.
    passages: Vec<(f32, u32)>,
}

/// Answers a question.
pub fn ask(view: &View, question: &Question<'_>) -> io::Result<Answer> {
    // Which notes count, and which may answer.
    let live: Vec<Vec<bool>> = view.parts.iter().map(|part| part.live.clone()).collect();
    let answerable: Vec<Vec<bool>> = view
        .parts
        .iter()
        .map(|part| {
            part.notes
                .iter()
                .zip(&part.live)
                .map(|(note, &live)| live && (question.in_scope)(&note.rel))
                .collect()
        })
        .collect();
    let note_of = |global: u32| -> Key {
        let (p, local) = view.locate(global);
        (p, view.parts[p].passages[local as usize].note as usize)
    };

    // 1. Recall by words, over every passage that has one of them.
    let lexical = bm25(view, question.text, &live, &answerable)?;

    // 2. Recall by meaning, with the vectors' first components.
    let query = question
        .vector
        .filter(|_| view.dims > 0)
        .map(|vector| fit(vector, view.dims));
    let mut dense: Vec<(u32, f32)> = Vec::new();
    if let Some(query) = &query {
        for (p, part) in view.parts.iter().enumerate() {
            let prefix = part.segment.prefix();
            if prefix == 0 || part.segment.dims() != view.dims {
                continue;
            }
            let head = fit(&query[..prefix], prefix);
            let keep = |local: u32| answerable[p][part.passages[local as usize].note as usize];
            for (local, score) in part.segment.coarse(&head, &keep, MEANING_RECALL)? {
                dense.push((part.base + local, score));
            }
        }
        dense.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        dense.truncate(MEANING_RECALL);
    }
    // With no vector in reach of the question, it is answered by words.
    let semantic = query.is_some() && !dense.is_empty();
    let query = query.filter(|_| semantic);

    // 3. The notes found, each judged by the passages found of it and by its
    // first, which says what it is about.
    let mut judged: HashMap<Key, Vec<u32>> = HashMap::new();
    for &global in lexical
        .best
        .iter()
        .chain(dense.iter().map(|(global, _)| global))
    {
        judged.entry(note_of(global)).or_default().push(global);
    }
    for (&(p, n), globals) in &mut judged {
        let info = &view.parts[p].notes[n];
        if info.count > 0 {
            globals.push(view.parts[p].base + info.first);
        }
        globals.sort_unstable();
        globals.dedup();
    }
    let cosines = match &query {
        Some(query) => cosines(view, &judged, query)?,
        None => HashMap::new(),
    };
    let mut keys: Vec<Key> = judged.keys().copied().collect();
    keys.sort_unstable();
    let found: Vec<Found> = keys
        .into_iter()
        .map(|key| {
            let part = &view.parts[key.0];
            let info = &part.notes[key.1];
            let words = (info.first..info.first + info.count)
                .filter_map(|local| lexical.passages.get(&(part.base + local)))
                .fold(0.0f32, |best, &(score, _)| best.max(score));
            let passages: Vec<(u32, Option<f32>, f32, f32)> = judged[&key]
                .iter()
                .map(|&global| {
                    let (score, cover) = lexical.passages.get(&global).copied().unwrap_or_default();
                    (global, cosines.get(&global).copied(), score, cover)
                })
                .collect();
            let cosine = passages
                .iter()
                .filter_map(|&(_, cosine, _, _)| cosine)
                .reduce(f32::max);
            Found {
                key,
                passages,
                cosine,
                words,
            }
        })
        .collect();

    // 4. Each note's score, from its cosine and its words. A passage answers
    // if its cosine reaches the floor, and so does a note; a note the model
    // hasn't read yet answers if its words cover the question, as though its
    // meaning reached the floor and no further. Without meaning, words are
    // all there is.
    let top_words = found.iter().fold(0.0f32, |best, note| best.max(note.words));
    let share = |words: f32| {
        if top_words > 0.0 {
            words / top_words
        } else {
            0.0
        }
    };
    let floor = question.floor;
    // Without meaning, a question some passage covers is answered by every
    // note that shares its words, best first: the note asked about often
    // covers only part of a question. One that nothing covers is a question
    // the vault can't answer.
    let covered = found.iter().any(|note| {
        note.passages
            .iter()
            .any(|&(_, _, _, cover)| cover >= WORDS_COVER)
    });
    if !semantic && !covered {
        return Ok(Answer {
            semantic,
            ..Answer::default()
        });
    }
    let mut kept: Vec<Kept> = Vec::new();
    for note in &found {
        let mut passages: Vec<(f32, u32)> = note
            .passages
            .iter()
            .filter_map(|&(global, cosine, words, cover)| {
                let score = match (semantic, cosine) {
                    (true, Some(cosine)) => {
                        (cosine >= floor).then(|| cosine + WORDS_WEIGHT * share(words))
                    }
                    (true, None) => {
                        (cover >= WORDS_COVER).then(|| floor + WORDS_WEIGHT * share(words))
                    }
                    (false, _) => (words > 0.0).then(|| share(words)),
                };
                score.map(|score| (score, global))
            })
            .collect();
        if passages.is_empty() {
            continue;
        }
        passages.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        // A note whose best cosine is under the floor answers only by a
        // passage the model hasn't read, which covers the question: it
        // scores as a note the model hasn't read at all does, not by a
        // cosine that says it has nothing to do with the question.
        let (base, weight) = match (semantic, note.cosine) {
            (true, Some(cosine)) if cosine >= floor => (
                cosine + WORDS_WEIGHT * share(note.words),
                cosine - floor + LEAST_WEIGHT,
            ),
            (true, _) => (floor + WORDS_WEIGHT * share(note.words), LEAST_WEIGHT),
            (false, _) => (share(note.words), share(note.words)),
        };
        kept.push(Kept {
            key: note.key,
            base,
            weight: weight.max(LEAST_WEIGHT),
            passages,
        });
    }
    if kept.is_empty() {
        return Ok(Answer {
            semantic,
            ..Answer::default()
        });
    }

    // 5. The links: a walk from the best few notes, weighted by how far past
    // the floor each one is, or without meaning by its words. Notes alike
    // are taken in the order of their paths, not of where the index keeps
    // them, which a note written again as it was changes.
    let path = |key: Key| view.note(key).rel.as_str();
    let graph = Graph::new(view);
    kept.sort_by(|a, b| {
        b.base
            .total_cmp(&a.base)
            .then_with(|| path(a.key).cmp(path(b.key)))
    });
    let seeds: Vec<(Key, f32)> = kept
        .iter()
        .take(SEEDS)
        .map(|note| (note.key, note.weight))
        .collect();
    let mut walked = graph.walk(&seeds);
    let lifts: HashMap<Key, f32> = walked.iter().map(|w| (w.key, w.lift)).collect();

    let mut ranked: Vec<(f32, f32, &Kept)> = kept
        .iter()
        .map(|note| {
            let links = LINKS_WEIGHT * lifts.get(&note.key).copied().unwrap_or(0.0);
            let prior = (question.prior)(path(note.key));
            let score = note.base + links + prior;
            debug_assert!(score.is_finite(), "{} scores {score}", path(note.key));
            (score, links, note)
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| path(a.2.key).cmp(path(b.2.key)))
    });
    ranked.truncate(question.limit);

    let passage_info = |global: u32| {
        let (p, local) = view.locate(global);
        view.parts[p].passages[local as usize]
    };
    let notes: Vec<NoteHit> = ranked
        .iter()
        .map(|&(score, links, note)| {
            let info = view.note(note.key);
            let best = passage_info(note.passages[0].1);
            NoteHit {
                rel: info.rel.clone(),
                title: info.title.clone(),
                score,
                start: best.start,
                end: best.end,
                by_graph: links >= BY_LINKS,
            }
        })
        .collect();

    // 6. The passages to read, dealt a round at a time.
    let wanted = question.limit.saturating_mul(3);
    let mut passages: Vec<PassageHit> = Vec::new();
    let mut round = 0;
    while passages.len() < wanted {
        let mut dealt = false;
        for &(_, _, note) in &ranked {
            let Some(&(score, global)) = note.passages.get(round) else {
                continue;
            };
            dealt = true;
            if passages.len() == wanted {
                break;
            }
            let info = view.note(note.key);
            let passage = passage_info(global);
            let (_, local) = view.locate(global);
            passages.push(PassageHit {
                rel: info.rel.clone(),
                title: info.title.clone(),
                start: passage.start,
                end: passage.end,
                place: local - info.first,
                score,
            });
        }
        if !dealt {
            break;
        }
        round += 1;
    }

    // What the graph says about the answer: its links, and what bridges it.
    let answer_keys: Vec<Key> = ranked.iter().map(|&(_, _, note)| note.key).collect();
    let answer_set: HashSet<Key> = answer_keys.iter().copied().collect();
    let mut links = Vec::new();
    for &from in &answer_keys {
        for to in graph.outgoing(from) {
            if to != from && answer_set.contains(&to) {
                links.push((view.note(from).rel.clone(), view.note(to).rel.clone()));
            }
        }
    }
    links.sort();
    links.dedup();
    walked.sort_by(|a, b| {
        b.rank
            .total_cmp(&a.rank)
            .then_with(|| path(a.key).cmp(path(b.key)))
    });
    // The walk goes where the links go, but only a note the question may be
    // answered from is named: a bridge out of the folder asked about is
    // somewhere the asker didn't ask to look.
    let mut bridges: Vec<(Key, usize, f32)> = walked
        .iter()
        .filter(|walk| !answer_set.contains(&walk.key) && (question.in_scope)(path(walk.key)))
        .filter_map(|walk| {
            let touching = graph
                .neighbours(walk.key)
                .into_iter()
                .filter(|n| answer_set.contains(n))
                .count();
            (touching >= 2).then_some((walk.key, touching, walk.rank))
        })
        .collect();
    bridges.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.total_cmp(&a.2))
            .then_with(|| path(a.0).cmp(path(b.0)))
    });
    let bridges = bridges
        .into_iter()
        .take(3)
        .map(|(key, _, _)| view.note(key).rel.clone())
        .collect();

    Ok(Answer {
        notes,
        passages,
        bridges,
        links,
        semantic,
    })
}

/// The notes linked to `rel` either way, by path.
#[must_use]
pub fn neighbours(view: &View, rel: &str) -> Vec<String> {
    let Some(key) = view.find(rel) else {
        return Vec::new();
    };
    Graph::new(view)
        .neighbours(key)
        .into_iter()
        .map(|other| view.note(other).rel.clone())
        .collect()
}

/// BM25 over every segment, with statistics from the live passages, of
/// every passage that may answer and has a term of the question; and how
/// much of the question each covers: the rarity, by BM25's measure, of the
/// terms it has, as a share of all the question's, those no passage has
/// counted as the rarest.
fn bm25(
    view: &View,
    question: &str,
    live: &[Vec<bool>],
    answerable: &[Vec<bool>],
) -> io::Result<Lexical> {
    let mut terms = text::question_terms(question);
    let mut seen = HashSet::new();
    terms.retain(|term| seen.insert(term.clone()));
    if terms.is_empty() {
        return Ok(Lexical::default());
    }

    let mut passages = 0f32;
    let mut length = 0f64;
    for (p, part) in view.parts.iter().enumerate() {
        for info in &part.passages {
            if live[p][info.note as usize] {
                passages += 1.0;
                length += f64::from(info.length);
            }
        }
    }
    if passages == 0.0 {
        return Ok(Lexical::default());
    }
    let average = (length / f64::from(passages)) as f32;

    let mut scores: HashMap<u32, (f32, f32)> = HashMap::new();
    let mut whole = 0f32;
    for term in &terms {
        // Each segment's postings, live passages only; their count is the
        // term's true document frequency.
        let mut hits: Vec<(u32, u32, u32)> = Vec::new();
        for (p, part) in view.parts.iter().enumerate() {
            let Some((_, list)) = part.segment.postings(term)? else {
                continue;
            };
            for (local, tf) in list {
                let info = part.passages[local as usize];
                if live[p][info.note as usize] {
                    hits.push((part.base + local, tf, info.length));
                }
            }
        }
        let df = hits.len() as f32;
        let idf = (1.0 + (passages - df + 0.5) / (df + 0.5)).ln();
        whole += idf;
        for (global, tf, len) in hits {
            let (p, local) = view.locate(global);
            if !answerable[p][view.parts[p].passages[local as usize].note as usize] {
                continue;
            }
            let tf = tf as f32;
            let norm = K1 * (1.0 - B + B * len as f32 / average.max(1.0));
            let entry = scores.entry(global).or_default();
            entry.0 += idf * tf * (K1 + 1.0) / (tf + norm);
            entry.1 += idf;
        }
    }
    for (_, cover) in scores.values_mut() {
        *cover /= whole;
    }

    let order = |a: &(u32, f32), b: &(u32, f32)| -> Ordering {
        b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0))
    };
    let mut best: Vec<(u32, f32)> = scores
        .iter()
        .map(|(&global, &(score, _))| (global, score))
        .collect();
    if best.len() > WORDS_RECALL {
        best.select_nth_unstable_by(WORDS_RECALL - 1, order);
        best.truncate(WORDS_RECALL);
    }
    best.sort_by(order);
    Ok(Lexical {
        passages: scores,
        best: best.into_iter().map(|(global, _)| global).collect(),
    })
}

/// The cosine with `query`, over whole vectors, of every passage in
/// `judged` that has a vector: each segment's read in one pass, a run of
/// consecutive passages at a time.
fn cosines(
    view: &View,
    judged: &HashMap<Key, Vec<u32>>,
    query: &[f32],
) -> io::Result<HashMap<u32, f32>> {
    let mut wanted: Vec<Vec<u32>> = vec![Vec::new(); view.parts.len()];
    for (&(p, _), globals) in judged {
        let base = view.parts[p].base;
        wanted[p].extend(globals.iter().map(|&global| global - base));
    }
    let mut out = HashMap::new();
    for (part, mut locals) in view.parts.iter().zip(wanted) {
        if locals.is_empty() || part.segment.dims() != view.dims {
            continue;
        }
        locals.sort_unstable();
        locals.dedup();
        for (&local, vector) in locals.iter().zip(part.segment.vectors(&locals)?) {
            if let Some(vector) = vector {
                out.insert(part.base + local, vector.dot(query));
            }
        }
    }
    Ok(out)
}

/// A note the walk reached.
struct Walked {
    key: Key,
    /// Its share of the walk.
    rank: f32,
    /// What its links lift it by, before [`LINKS_WEIGHT`]: see [`lifts`].
    lift: f32,
}

/// The vault's links among the live notes, as the index resolved them when
/// it was written, or resolved here when it hasn't: a path first, then a
/// file name, then an alias.
struct Graph<'a> {
    view: &'a View,
    links: Cow<'a, Links>,
    /// Each part's notes' places in the view's list of live notes; a note
    /// that doesn't count has none.
    places: Vec<Vec<Option<u32>>>,
}

impl<'a> Graph<'a> {
    fn new(view: &'a View) -> Self {
        let links = view
            .graph
            .as_ref()
            .map_or_else(|| Cow::Owned(Links::resolve(view)), Cow::Borrowed);
        let mut places: Vec<Vec<Option<u32>>> = view
            .parts
            .iter()
            .map(|part| vec![None; part.notes.len()])
            .collect();
        for (place, &(p, n)) in view.notes.iter().enumerate() {
            places[p][n] = Some(place as u32);
        }
        Self {
            view,
            links,
            places,
        }
    }

    /// The notes at `places` in the view's list, in key order.
    fn keys(&self, places: impl Iterator<Item = u32>) -> Vec<Key> {
        let mut keys: Vec<Key> = places
            .map(|place| self.view.notes[place as usize])
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    fn place(&self, key: Key) -> Option<usize> {
        self.places
            .get(key.0)?
            .get(key.1)
            .copied()
            .flatten()
            .map(|place| place as usize)
    }

    fn outgoing(&self, key: Key) -> Vec<Key> {
        let Some(place) = self.place(key) else {
            return Vec::new();
        };
        self.keys(self.links.outgoing(place).iter().copied())
    }

    /// Linked either way.
    fn neighbours(&self, key: Key) -> Vec<Key> {
        let Some(place) = self.place(key) else {
            return Vec::new();
        };
        self.keys(self.around(place).into_iter())
    }

    /// The places of the notes linked to the one at `place` either way, in
    /// order, each once.
    fn around(&self, place: usize) -> Vec<u32> {
        let (outgoing, incoming) = (self.links.outgoing(place), self.links.incoming(place));
        let mut all = Vec::with_capacity(outgoing.len() + incoming.len());
        all.extend_from_slice(outgoing);
        all.extend_from_slice(incoming);
        all.sort_unstable();
        all.dedup();
        all
    }

    /// Personalized PageRank from `seeds`, each with its share of the start,
    /// over the notes within [`HOPS`] links of them: every note it reached,
    /// with its share of the walk and what its links lift it by.
    ///
    /// Shares that add up to nothing, or to less, say nowhere to start, and
    /// dividing by their sum would make the ranks infinite or negative: then
    /// there is no walk, and the links lift nothing.
    fn walk(&self, seeds: &[(Key, f32)]) -> Vec<Walked> {
        let total: f32 = seeds.iter().map(|&(_, weight)| weight).sum();
        let positive = total.is_finite() && total > 0.0;
        if !positive {
            return Vec::new();
        }
        let mut index: HashMap<u32, usize> = HashMap::new();
        let mut nodes: Vec<u32> = Vec::new();
        // Each note's neighbours in the whole vault, by place.
        let mut around: Vec<Vec<u32>> = Vec::new();
        let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
        for &(key, _) in seeds {
            let Some(place) = self.place(key) else {
                continue;
            };
            let place = place as u32;
            if let Entry::Vacant(slot) = index.entry(place) {
                slot.insert(nodes.len());
                queue.push_back((nodes.len(), 0));
                nodes.push(place);
                around.push(self.around(place as usize));
            }
        }
        while let Some((at, hops)) = queue.pop_front() {
            if hops == HOPS {
                continue;
            }
            for k in 0..around[at].len() {
                if nodes.len() >= MAX_NODES {
                    break;
                }
                let next = around[at][k];
                if let Entry::Vacant(slot) = index.entry(next) {
                    slot.insert(nodes.len());
                    queue.push_back((nodes.len(), hops + 1));
                    nodes.push(next);
                    around.push(self.around(next as usize));
                }
            }
        }

        let mut restart = vec![0.0f32; nodes.len()];
        for &(key, weight) in seeds {
            if let Some(&at) = self.place(key).and_then(|place| index.get(&(place as u32))) {
                restart[at] += weight / total;
            }
        }
        let neighbours: Vec<Vec<usize>> = around
            .iter()
            .map(|list| {
                list.iter()
                    .filter_map(|place| index.get(place).copied())
                    .collect()
            })
            .collect();
        let degrees: Vec<usize> = around.iter().map(Vec::len).collect();
        let rank = pagerank(&neighbours, &degrees, &restart);
        let lifts = lifts(&rank, &restart, &degrees);
        nodes
            .into_iter()
            .zip(rank)
            .zip(lifts)
            .map(|((place, rank), lift)| Walked {
                key: self.view.notes[place as usize],
                rank,
                lift,
            })
            .collect()
    }
}

/// How readily the walk steps into a note with `degree` links in the whole
/// vault, and how much of what it gains counts: a hub that every note links
/// to is reached through all of them, and would otherwise rise for any
/// question at all.
fn damping(degree: usize) -> f32 {
    1.0 / (2.0 + degree as f32).ln()
}

/// Personalized PageRank over `neighbours`, each note's neighbours in the
/// graph by number, from `restart`, the share of the walk that starts at
/// each note; `degrees` are the notes' links in the whole vault.
///
/// At each step half the walk goes back to the start, and the rest moves on
/// along a link, a note's walk shared among its neighbours by
/// [`damping`]. A note with no neighbours in the graph sends its walk back
/// to the start: summed once a step, not note by note.
fn pagerank(neighbours: &[Vec<usize>], degrees: &[usize], restart: &[f32]) -> Vec<f32> {
    let reach: Vec<f32> = degrees.iter().map(|&degree| damping(degree)).collect();
    let totals: Vec<f32> = neighbours
        .iter()
        .map(|list| list.iter().map(|&to| reach[to]).sum())
        .collect();
    let mut rank = restart.to_vec();
    for _ in 0..WALK_STEPS {
        let stuck: f32 = neighbours
            .iter()
            .zip(&rank)
            .filter(|(list, _)| list.is_empty())
            .map(|(_, &share)| share)
            .sum();
        let back = RESTART + (1.0 - RESTART) * stuck;
        let mut next: Vec<f32> = restart.iter().map(|&start| back * start).collect();
        for (from, list) in neighbours.iter().enumerate() {
            if list.is_empty() {
                continue;
            }
            let share = (1.0 - RESTART) * rank[from] / totals[from];
            for &to in list {
                next[to] += share * reach[to];
            }
        }
        rank = next;
    }
    rank
}

/// What the links lift each note of a walk by: the share of the walk it
/// gained over what it started with, against the walk's best note, damped
/// by its own links as a hub's are. A note several answers link to rises; a
/// note the walk only passes through, or that only gives its share away,
/// doesn't.
fn lifts(rank: &[f32], restart: &[f32], degrees: &[usize]) -> Vec<f32> {
    let top = rank
        .iter()
        .copied()
        .fold(0.0f32, f32::max)
        .max(f32::MIN_POSITIVE);
    rank.iter()
        .zip(restart)
        .zip(degrees)
        .map(|((&share, &start), &degree)| (share - start).max(0.0) / top * damping(degree))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::NoteMeta;
    use crate::retrieval::Prompts;
    use crate::retrieval::fake;
    use crate::retrieval::store::{Embedding, Store};
    use crate::test_support::TempVault;
    use crate::vault::ScanOptions;

    /// Recall's floor, which the stand-in model's cosines are made to suit.
    const FLOOR: f32 = 0.65;

    fn scan(vault: &TempVault) -> Vec<NoteMeta> {
        vault
            .vault()
            .scan(&ScanOptions::default())
            .expect("scan")
            .notes
    }

    fn index(vault: &TempVault, with_model: bool) -> Store {
        let mut store = Store::open(&vault.path().join(".index"));
        let mut model = fake::Words::default();
        let prompts = Prompts::default();
        let embedding = with_model.then_some(Embedding {
            model: &mut model,
            prompts: &prompts,
            signature: "fake",
            dims: 0,
            limit: usize::MAX,
        });
        store
            .refresh(&scan(vault), &|_| true, embedding, &mut |_| {})
            .expect("refresh");
        store
    }

    /// Brings `store` up to date with the model, which embeds at most
    /// `limit` notes.
    fn refresh(store: &mut Store, vault: &TempVault, limit: usize) {
        let mut model = fake::Words::default();
        let prompts = Prompts::default();
        let embedding = Embedding {
            model: &mut model,
            prompts: &prompts,
            signature: "fake",
            dims: 0,
            limit,
        };
        store
            .refresh(&scan(vault), &|_| true, Some(embedding), &mut |_| {})
            .expect("refresh");
    }

    fn ask_with(
        store: &Store,
        text: &str,
        semantic: bool,
        prior: &dyn Fn(&str) -> f32,
        in_scope: &dyn Fn(&str) -> bool,
    ) -> Answer {
        let view = store.view().expect("view");
        let vector = semantic.then(|| fake::vector(&Prompts::default().query(text)));
        ask(
            &view,
            &Question {
                text,
                vector: vector.as_deref(),
                in_scope,
                prior,
                floor: FLOOR,
                limit: 5,
            },
        )
        .expect("ask")
    }

    fn ask_text(store: &Store, text: &str, semantic: bool) -> Answer {
        ask_with(store, text, semantic, &|_| 0.0, &|_| true)
    }

    fn names(answer: &Answer) -> Vec<&str> {
        answer.notes.iter().map(|n| n.rel.as_str()).collect()
    }

    fn vault() -> TempVault {
        let vault = TempVault::new("ask");
        vault.write(
            "Hashing.md",
            "# Hashing\n\nPasswords are hashed with argon2 before storage.\n\nSee [[Security]].\n",
        );
        vault.write(
            "Login.md",
            "# Login\n\nThe login form checks passwords hashed in storage.\n\n[[Hashing]] and [[Security]].\n",
        );
        vault.write(
            "Deploy.md",
            "# Deploy\n\nPush a tag and the release workflow builds.\n",
        );
        vault.write("Security.md", "# Security\n\nThreat model for the vault.\n");
        vault.write("Lonely.md", "# Lonely\n\nNothing to do with anything.\n");
        vault
    }

    #[test]
    fn words_alone_find_the_notes_that_name_the_thing() {
        let vault = vault();
        let store = index(&vault, false);
        let answer = ask_text(&store, "argon2", false);
        assert_eq!(names(&answer), vec!["Hashing.md"]);
        assert!(!answer.semantic);
        assert_eq!(
            (answer.notes[0].start, answer.notes[0].end),
            (3, 5),
            "the passage, its title heading left out"
        );
        let mut found = names(&ask_text(&store, "how are the passwords hashing?", false))
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        found.sort();
        assert_eq!(
            found,
            vec!["Hashing.md", "Login.md"],
            "found by their stems, the question's small words left out"
        );
    }

    #[test]
    fn meaning_and_words_together_rank_and_the_graph_connects() {
        let vault = vault();
        let store = index(&vault, true);
        let answer = ask_text(&store, "passwords hashed storage", true);
        assert!(answer.semantic);
        let found = names(&answer);
        assert!(
            found[..2].contains(&"Hashing.md") && found[..2].contains(&"Login.md"),
            "{found:?}"
        );
        assert!(
            answer
                .links
                .contains(&("Login.md".into(), "Hashing.md".into())),
            "{:?}",
            answer.links
        );
        assert!(
            !found.contains(&"Deploy.md") && !found.contains(&"Lonely.md"),
            "{found:?}"
        );
        assert!(
            answer.bridges.contains(&"Security.md".to_string()) || found.contains(&"Security.md"),
            "the note both answers link to is surfaced: {answer:?}"
        );
    }

    #[test]
    fn a_question_the_vault_cant_answer_gets_no_answer() {
        let vault = vault();
        let store = index(&vault, true);
        let answer = ask_text(&store, "zebra quantum", true);
        assert!(answer.semantic, "the model was asked");
        assert!(
            answer.notes.is_empty() && answer.passages.is_empty() && answer.bridges.is_empty(),
            "nothing reaches the floor: {answer:?}"
        );
        let answer = ask_text(&store, "argon2 zebra quantum unicorn", true);
        assert!(
            answer.notes.is_empty(),
            "a word in common doesn't make an answer when the meaning is elsewhere: {answer:?}"
        );
    }

    #[test]
    fn nothing_relevant_is_no_answer() {
        let vault = vault();
        let store = index(&vault, false);
        assert_eq!(ask_text(&store, "zebra quantum", false), Answer::default());
    }

    #[test]
    fn without_meaning_a_passage_must_cover_the_question_not_share_a_common_word() {
        let vault = TempVault::new("ask-cover");
        for name in ["One", "Two", "Three", "Four", "Five"] {
            vault.write(
                &format!("{name}.md"),
                &format!("The {name} release went out on time.\n"),
            );
        }
        vault.write("Hash.md", "Passwords are hashed with argon2.\n");
        let store = index(&vault, false);
        // `release` is in five passages of six, `argon2` in one: the note
        // with the rare word answers first, the ones sharing only the common
        // word after it.
        let answer = ask_text(&store, "argon2 release", false);
        let found = names(&answer);
        assert_eq!(found[0], "Hash.md", "{found:?}");
        assert!(
            ask_text(&store, "release zebra", false).notes.is_empty(),
            "a word no note has weighs the most"
        );
        assert_eq!(names(&ask_text(&store, "release", false)).len(), 5);
    }

    #[test]
    fn the_scope_limits_the_answers_but_not_the_graph() {
        let vault = vault();
        let store = index(&vault, true);
        let answer = ask_with(&store, "passwords hashed", true, &|_| 0.0, &|rel| {
            rel.starts_with("Login")
        });
        assert_eq!(names(&answer), vec!["Login.md"]);
    }

    #[test]
    fn a_prior_reorders_close_answers() {
        let vault = vault();
        let store = index(&vault, true);
        let answer = ask_with(
            &store,
            "passwords hashed storage",
            true,
            &|rel| if rel == "Login.md" { 1.0 } else { 0.0 },
            &|_| true,
        );
        assert_eq!(answer.notes[0].rel, "Login.md");
    }

    #[test]
    fn a_link_from_an_answer_lifts_a_relevant_note() {
        let vault = TempVault::new("ask-lift");
        vault.write("Main.md", "Rotate signing keys. [[Linked]]\n");
        // The same words, so the same cosine and BM25: without the link,
        // `Alone` would come first, as the first by path.
        vault.write("Linked.md", "Rotate signing keys yearly with care.\n");
        vault.write("Alone.md", "Rotate signing keys yearly with care.\n");
        let store = index(&vault, true);
        let answer = ask_text(&store, "rotate signing keys", true);
        assert_eq!(
            names(&answer),
            vec!["Main.md", "Linked.md", "Alone.md"],
            "{answer:#?}"
        );
        assert!(answer.notes[1].by_graph, "{answer:#?}");
        assert!(!answer.notes[2].by_graph, "{answer:#?}");
        assert_eq!(
            answer.links,
            vec![("Main.md".to_string(), "Linked.md".to_string())]
        );
    }

    #[test]
    fn a_note_kept_by_a_passage_the_model_hasnt_read_scores_at_the_floor() {
        let vault = TempVault::new("ask-partial");
        let pad = "Rotate the signing keys every year with care and record when. ".repeat(5);
        vault.write("X.md", &format!("## Keys\n\n{pad}\n\nSee [[Y]].\n"));
        vault.write("Y.md", "# Y\n\nNothing much here at all.\n");
        vault.write("Z.md", "# Z\n\nSomething else entirely.\n");
        let mut store = Store::open(&vault.path().join(".index"));
        refresh(&mut store, &vault, usize::MAX);
        // X gains a section on something new, past what the model may read:
        // its first passage keeps its vector, and its new one has none.
        vault.write(
            "X.md",
            &format!(
                "## Keys\n\n{pad}\n\nSee [[Y]].\n\n## Zebra\n\nZebra quantum flux capacitor.\n"
            ),
        );
        refresh(&mut store, &vault, 0);
        let view = store.view().expect("view");
        let x = view.find("X.md").expect("X");
        let info = view.note(x);
        let locals: Vec<u32> = (info.first..info.first + info.count).collect();
        let read: Vec<bool> = view.parts[x.0]
            .segment
            .vectors(&locals)
            .expect("vectors")
            .iter()
            .map(Option::is_some)
            .collect();
        assert_eq!(read, [true, false]);

        // Its first passage is far from the question, and its new one
        // covers it: it answers as a note the model hasn't read at all does.
        let answer = ask_text(&store, "zebra quantum flux capacitor", true);
        assert!(answer.semantic);
        assert_eq!(names(&answer), ["X.md"], "{answer:#?}");
        assert!(
            (answer.notes[0].score - (FLOOR + WORDS_WEIGHT)).abs() < 1e-6,
            "{answer:#?}"
        );
        assert!(!answer.notes[0].by_graph, "{answer:#?}");
    }

    #[test]
    fn notes_alike_keep_their_order_when_one_is_written_again() {
        let vault = TempVault::new("ask-ties");
        let body = "# T\n\nRotate the signing keys yearly.\n";
        vault.write("A.md", body);
        vault.write("B.md", body);
        let mut store = Store::open(&vault.path().join(".index"));
        refresh(&mut store, &vault, usize::MAX);
        let before = ask_text(&store, "rotate signing keys", true);
        assert_eq!(names(&before), ["A.md", "B.md"], "by path: {before:#?}");

        std::thread::sleep(std::time::Duration::from_millis(20));
        vault.write("A.md", body);
        refresh(&mut store, &vault, usize::MAX);
        let view = store.view().expect("view");
        assert!(
            view.find("A.md") > view.find("B.md"),
            "A, read again, is kept after B"
        );
        let after = ask_text(&store, "rotate signing keys", true);
        assert_eq!(after, before, "the same answer, in the same order");
    }

    #[test]
    fn only_a_note_in_scope_bridges_the_answers() {
        let vault = TempVault::new("ask-bridge-scope");
        vault.write(
            "In/A.md",
            "# A\n\nRotate the signing keys yearly. [[Hub]] [[Glossary]]\n",
        );
        vault.write(
            "In/B.md",
            "# B\n\nRotate the signing keys with care. [[Hub]] [[Glossary]]\n",
        );
        vault.write(
            "In/Glossary.md",
            "# Glossary\n\nWords used across the project.\n",
        );
        vault.write("Out/Hub.md", "# Hub\n\nUnrelated diary entry.\n");
        let store = index(&vault, true);
        let mut everywhere = ask_text(&store, "rotate signing keys", true).bridges;
        everywhere.sort();
        assert_eq!(everywhere, ["In/Glossary.md", "Out/Hub.md"]);

        let inside = ask_with(&store, "rotate signing keys", true, &|_| 0.0, &|rel| {
            rel.starts_with("In/")
        });
        let mut found = names(&inside);
        found.sort_unstable();
        assert_eq!(found, ["In/A.md", "In/B.md"], "{inside:#?}");
        assert_eq!(inside.bridges, ["In/Glossary.md"], "{inside:#?}");
    }

    #[test]
    fn passages_are_dealt_a_round_at_a_time() {
        let vault = TempVault::new("ask-rounds");
        let section = |words: &str| format!("{} ", words).repeat(8);
        for name in ["One", "Two"] {
            vault.write(
                &format!("{name}.md"),
                &format!(
                    "## First\n\n{}\n\n## Second\n\n{}\n",
                    section("Rotate the signing keys."),
                    section("Rotate the signing keys, then record when.")
                ),
            );
        }
        let store = index(&vault, true);
        let answer = ask_text(&store, "rotate signing keys", true);
        let order: Vec<(&str, u32)> = answer
            .passages
            .iter()
            .map(|p| (p.rel.as_str(), p.start))
            .collect();
        let (first, second) = (names(&answer)[0], names(&answer)[1]);
        assert_eq!(
            order,
            vec![(first, 1), (second, 1), (first, 5), (second, 5)],
            "each note's best, then each one's next: {answer:#?}"
        );
        assert!(answer.passages[0].score >= answer.passages[2].score);
        assert_eq!(
            (answer.notes[0].start, answer.notes[0].end),
            (1, 3),
            "a note points at its best passage"
        );
    }

    #[test]
    fn links_kept_with_the_index_answer_as_resolving_them_again_would() {
        let vault = vault();
        vault.write(
            "Folder/Deep.md",
            "# Deep\n\nPasswords hashed deep in storage.\n\n[[./Login]] [[hashing]] [[Folder/Deep]]\n",
        );
        vault.write(
            "Guard.md",
            "---\naliases: [Shield]\n---\n# Guard\n\nThreat model guard, [[Shield]] and [[security]].\n",
        );
        let store = index(&vault, true);
        let kept = store.view().expect("view");
        assert!(kept.graph.is_some(), "resolved when the index was written");
        let mut fresh = store.view().expect("view");
        fresh.graph = None;
        for text in [
            "passwords hashed storage",
            "threat model",
            "release workflow builds",
        ] {
            let vector = fake::vector(&Prompts::default().query(text));
            let question = Question {
                text,
                vector: Some(&vector),
                in_scope: &|_| true,
                prior: &|_| 0.0,
                floor: FLOOR,
                limit: 5,
            };
            let answer = ask(&kept, &question).expect("ask");
            assert!(!answer.notes.is_empty(), "{text}");
            assert_eq!(answer, ask(&fresh, &question).expect("ask"), "{text}");
        }
        for rel in ["Login.md", "Security.md", "Folder/Deep.md", "Guard.md"] {
            assert_eq!(neighbours(&kept, rel), neighbours(&fresh, rel), "{rel}");
        }
        assert_eq!(
            neighbours(&kept, "Login.md"),
            ["Folder/Deep.md", "Hashing.md", "Security.md"]
        );
    }

    /// A walk over a graph given by its edges, every note's links in the
    /// vault being those it has here.
    fn walk(notes: usize, edges: &[(usize, usize)], restart: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut neighbours = vec![Vec::new(); notes];
        for &(a, b) in edges {
            neighbours[a].push(b);
            neighbours[b].push(a);
        }
        let degrees: Vec<usize> = neighbours.iter().map(Vec::len).collect();
        let rank = pagerank(&neighbours, &degrees, restart);
        let lift = lifts(&rank, restart, &degrees);
        (rank, lift)
    }

    #[test]
    fn a_walk_without_links_changes_nothing() {
        let restart = [0.5, 0.3, 0.2];
        let (rank, lift) = walk(3, &[], &restart);
        for (got, want) in rank.iter().zip(restart) {
            assert!((got - want).abs() < 1e-6, "{rank:?}");
        }
        assert!(lift.iter().all(|&l| l == 0.0), "{lift:?}");
    }

    #[test]
    fn a_walk_keeps_its_whole_share_and_lifts_what_the_start_links_to() {
        // 0 and 1 start the walk; 2 is linked to both, 3 to 2 only.
        let (rank, lift) = walk(4, &[(0, 2), (1, 2), (2, 3)], &[0.5, 0.5, 0.0, 0.0]);
        assert!((rank.iter().sum::<f32>() - 1.0).abs() < 1e-5, "{rank:?}");
        assert!(lift[2] > lift[3] && lift[3] > 0.0, "{lift:?}");
        assert_eq!((lift[0], lift[1]), (0.0, 0.0), "the start gives, not gains");
    }

    #[test]
    fn a_hub_rises_less_than_a_note_linked_for_a_reason() {
        // 0 and 1 start the walk. 2 is linked to both and nothing else; 3 is
        // linked to both and to twenty notes besides, as an index note is.
        let mut edges = vec![(0, 2), (1, 2), (0, 3), (1, 3)];
        edges.extend((4..24).map(|other| (3, other)));
        let mut restart = vec![0.0; 24];
        restart[0] = 0.5;
        restart[1] = 0.5;
        let (_, lift) = walk(24, &edges, &restart);
        assert!(lift[2] > 2.0 * lift[3], "{:?}", &lift[..4]);
    }
}
