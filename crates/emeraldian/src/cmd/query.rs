//! Commands that find notes and report on them.

use std::fs;

use emeraldian_core::index::LinkTarget;
use emeraldian_core::links;
use emeraldian_core::note::{self, NoteMeta};
use emeraldian_core::search::{self, SearchLimits};
use emeraldian_core::sort::SortOrder;
use serde_json::{Value, json};

use super::{Args, Ctx, Failure, Output, Result, cap, clip, display, fail, lines};
use crate::clock::Utc;

/// The longest line of a note shown as context, in characters.
const CONTEXT_CHARS: usize = 160;

/// A note as `list` and `tasks` see it.
struct Row {
    meta: NoteMeta,
    tags: Vec<String>,
    words: usize,
}

/// The notes under `folder` (all of them for `""`), from a scan of the
/// folders when nothing more than names and dates is wanted, which is far
/// cheaper than indexing every note's links.
fn rows(ctx: &mut Ctx, folder: &str, indexed: bool) -> Result<Vec<Row>> {
    let prefix = if folder.is_empty() {
        String::new()
    } else {
        format!("{folder}/")
    };
    let inside = |rel: &str| prefix.is_empty() || rel.starts_with(&prefix);

    let rows: Vec<Row> = if indexed {
        ctx.index()?
            .notes()
            .iter()
            .filter(|note| inside(&note.meta.rel))
            .map(|note| Row {
                meta: note.meta.clone(),
                tags: note.tags.clone(),
                words: note.words,
            })
            .collect()
    } else {
        ctx.scan()?
            .notes
            .iter()
            .filter(|meta| inside(&meta.rel))
            .map(|meta| Row {
                meta: meta.clone(),
                tags: Vec::new(),
                words: 0,
            })
            .collect()
    };

    if rows.is_empty() && !folder.is_empty() {
        let (_, path) = ctx.folder(folder)?;
        if !path.is_dir() {
            return fail(format!("no folder `{folder}`"));
        }
    }
    Ok(rows)
}

/// Whether a note's frontmatter has `key` set to `value`, or has `key` at all
/// when `value` is `None`. A list matches when any of its items does.
fn has_property(content: &str, key: &str, value: Option<&str>) -> bool {
    let (yaml, _, _) = note::split_frontmatter(content);
    let Some(found) = yaml.map(note::parse_frontmatter) else {
        return false;
    };
    let Some(property) = found.get(key) else {
        return false;
    };
    match value {
        None => true,
        Some(value) => property
            .as_list()
            .iter()
            .any(|item| item.eq_ignore_ascii_case(value.trim())),
    }
}

/// Whether a note carries `tag` or a tag nested under it.
fn tagged(tags: &[String], tag: &str) -> bool {
    tags.iter()
        .any(|t| t == tag || t.starts_with(&format!("{tag}/")))
}

pub(super) fn list(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let folder = args.get(0).unwrap_or_default().trim().trim_matches('/');
    let order: SortOrder = match args.value("sort") {
        Some(name) => name.parse().map_err(|_| {
            Failure::Usage(format!(
                "--sort takes modified, modified-asc, created, created-asc, name or name-desc, not `{name}`"
            ))
        })?,
        None => SortOrder::ModifiedDesc,
    };
    let tag = args.value("tag").map(|t| t.trim().trim_start_matches('#'));
    let long = args.flag("long");
    let wheres: Vec<(&str, Option<&str>)> = args
        .values("where")
        .into_iter()
        .map(|w| match w.split_once('=') {
            Some((key, value)) => (key.trim(), Some(value)),
            None => (w.trim(), None),
        })
        .collect();
    let limit = args.limit(50)?;

    let mut rows = rows(ctx, folder, tag.is_some() || long)?;
    if let Some(tag) = tag {
        rows.retain(|row| tagged(&row.tags, tag));
    }
    if !wheres.is_empty() {
        rows.retain(|row| {
            fs::read_to_string(&row.meta.path).is_ok_and(|content| {
                wheres
                    .iter()
                    .all(|(key, value)| has_property(&content, key, *value))
            })
        });
    }
    rows.sort_by(|a, b| order.compare(&a.meta, &b.meta));
    let cut = cap(&mut rows, limit);

    let text = if rows.is_empty() {
        "no notes".to_string()
    } else {
        let shown = rows
            .iter()
            .map(|row| {
                let name = display(&row.meta.rel);
                if !long {
                    return name.to_string();
                }
                let tags = if row.tags.is_empty() {
                    String::new()
                } else {
                    format!("  #{}", row.tags.join(" #"))
                };
                format!(
                    "{}  {:>5}w  {name}{tags}",
                    Utc::at(row.meta.modified).format("%Y-%m-%d"),
                    row.words
                )
            })
            .collect();
        lines(shown, cut)
    };
    let json: Vec<Value> = rows
        .iter()
        .map(|row| {
            let mut entry = json!({
                "note": row.meta.rel,
                "title": row.meta.title,
                "modified": Utc::at(row.meta.modified).iso(),
                "created": Utc::at(row.meta.created).iso(),
            });
            if tag.is_some() || long {
                entry["tags"] = json!(row.tags);
                entry["words"] = json!(row.words);
            }
            entry
        })
        .collect();
    Ok(Output::new(text, json!({ "notes": json, "more": cut })))
}

pub(super) fn find(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let query = args.at(0);
    let limit = args.limit(10)?;
    let index = ctx.index()?;
    let mut matches = search::search_notes(index, query, usize::MAX);
    let cut = cap(&mut matches, limit);

    let names: Vec<String> = matches
        .iter()
        .filter_map(|m| index.note(m.id))
        .map(|note| display(&note.meta.rel).to_string())
        .collect();
    let text = if names.is_empty() {
        format!("no note is named like `{query}`")
    } else {
        lines(names.clone(), cut)
    };
    Ok(Output::new(text, json!({ "notes": names, "more": cut })))
}

pub(super) fn search(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let query = args.at(0);
    let limit = args.limit(10)?;
    let hits = args.number("hits", 3)?.max(1);
    literal(ctx, query, args.value("folder"), limit, hits)
}

/// The notes containing `query` as written, with up to `hits` lines of each.
fn literal(
    ctx: &mut Ctx,
    query: &str,
    folder: Option<&str>,
    limit: usize,
    hits: usize,
) -> Result<Output> {
    let folder = folder.map(|f| f.trim().trim_matches('/').to_string());

    // Searched from a listing rather than an index: a search reads every note
    // in any case, and indexing first would read each of them twice.
    let notes: Vec<_> = match &folder {
        Some(folder) => {
            let prefix = format!("{folder}/");
            ctx.scan()?
                .notes
                .iter()
                .filter(|meta| meta.rel.starts_with(&prefix))
                .cloned()
                .collect()
        }
        None => ctx.scan()?.notes.clone(),
    };
    let mut results = search::search_files(
        &notes,
        query,
        // A ceiling for a query that matches everything, high enough that the
        // ranking is over every note that matters.
        SearchLimits {
            max_notes: 2_000,
            max_hits_per_note: hits,
        },
    );
    let cut = cap(&mut results, limit);

    let mut text = Vec::new();
    let mut json = Vec::new();
    for result in &results {
        let Some(note) = notes.get(result.id) else {
            continue;
        };
        text.push(display(&note.rel).to_string());
        for hit in &result.hits {
            text.push(format!(
                "  {}: {}",
                hit.line + 1,
                clip(&hit.text, CONTEXT_CHARS)
            ));
        }
        json.push(json!({
            "note": note.rel,
            "hits": result.hits.iter().map(|h| json!({ "line": h.line + 1, "text": h.text })).collect::<Vec<_>>(),
        }));
    }
    let text = if results.is_empty() {
        format!("no note contains `{query}`")
    } else {
        lines(text, cut)
    };
    Ok(Output::new(text, json!({ "results": json, "more": cut })))
}

pub(super) fn tags(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let limit = args.limit(50)?;
    let index = ctx.index()?;

    if let Some(tag) = args.get(0) {
        let tag = tag.trim().trim_start_matches('#');
        let Some(ids) = index.tags().get(tag) else {
            return fail(format!("no note is tagged #{tag}"));
        };
        let mut names: Vec<String> = ids
            .iter()
            .filter_map(|&id| index.note(id))
            .map(|note| display(&note.meta.rel).to_string())
            .collect();
        let cut = cap(&mut names, limit);
        return Ok(Output::new(
            lines(names.clone(), cut),
            json!({ "tag": tag, "notes": names, "more": cut }),
        ));
    }

    let mut counted: Vec<(&String, usize)> = index
        .tags()
        .iter()
        .map(|(tag, ids)| (tag, ids.len()))
        .collect();
    counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let cut = cap(&mut counted, limit);

    let text = if counted.is_empty() {
        "no tags".to_string()
    } else {
        lines(
            counted
                .iter()
                .map(|(tag, count)| format!("#{tag} {count}"))
                .collect(),
            cut,
        )
    };
    let json: Vec<Value> = counted
        .iter()
        .map(|(tag, count)| json!({ "tag": tag, "notes": count }))
        .collect();
    Ok(Output::new(text, json!({ "tags": json, "more": cut })))
}

pub(super) fn tasks(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let all = args.flag("all");
    let want_done = args.flag("done");
    let limit = args.limit(50)?;

    // A folder if there is one by that name, else a note.
    let notes: Vec<(String, std::path::PathBuf)> = match args.get(0) {
        Some(scope) => {
            let (folder, path) = ctx.folder(scope)?;
            if path.is_dir() {
                let mut rows = rows(ctx, &folder, false)?;
                rows.sort_by(|a, b| SortOrder::ModifiedDesc.compare(&a.meta, &b.meta));
                rows.into_iter()
                    .map(|row| (row.meta.rel, row.meta.path))
                    .collect()
            } else {
                let found = ctx.locate(scope)?;
                vec![(found.rel, found.path)]
            }
        }
        None => {
            let mut rows = rows(ctx, "", false)?;
            rows.sort_by(|a, b| SortOrder::ModifiedDesc.compare(&a.meta, &b.meta));
            rows.into_iter()
                .map(|row| (row.meta.rel, row.meta.path))
                .collect()
        }
    };

    let mut found = Vec::new();
    for (rel, path) in notes {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let (_, body, offset) = note::split_frontmatter(&content);
        for task in links::extract_tasks(body) {
            if all || task.done == want_done {
                found.push((rel.clone(), offset + task.line + 1, task));
            }
        }
    }
    let cut = cap(&mut found, limit);

    let text = if found.is_empty() {
        match (all, want_done) {
            (true, _) => "no tasks".to_string(),
            (false, true) => "no ticked tasks".to_string(),
            (false, false) => "no open tasks".to_string(),
        }
    } else {
        lines(
            found
                .iter()
                .map(|(rel, line, task)| {
                    let mark = match (all, task.done) {
                        (false, _) => "",
                        (true, true) => "[x] ",
                        (true, false) => "[ ] ",
                    };
                    format!(
                        "{}:{line} {mark}{}",
                        display(rel),
                        clip(&task.text, CONTEXT_CHARS)
                    )
                })
                .collect(),
            cut,
        )
    };
    let json: Vec<Value> = found
        .iter()
        .map(|(rel, line, task)| {
            json!({ "note": rel, "line": line, "done": task.done, "text": task.text })
        })
        .collect();
    Ok(Output::new(text, json!({ "tasks": json, "more": cut })))
}

pub(super) fn links(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    // Lower than other lists: each link in comes with the line it is on.
    let limit = args.limit(20)?;
    let index = ctx.index()?;
    let Some(id) = index.id_of_rel(&found.rel) else {
        return fail(format!(
            "{} isn't indexed: it's in a hidden folder (see `ui.show_hidden`)",
            found.name()
        ));
    };
    let note = index.note(id).expect("indexed");

    let mut out = Vec::new();
    let mut unresolved = Vec::new();
    let mut files = Vec::new();
    for link in &note.links {
        let name = match &link.target {
            LinkTarget::Note(target) => index
                .note(*target)
                .map(|n| (&mut out, display(&n.meta.rel).to_string())),
            LinkTarget::Unresolved(name) => Some((&mut unresolved, name.clone())),
            LinkTarget::Attachment(path) => Some((&mut files, path.clone())),
            LinkTarget::External(_) => None,
        };
        if let Some((list, name)) = name
            && !list.contains(&name)
        {
            list.push(name);
        }
    }

    let mut incoming: Vec<(String, usize, String)> = index
        .backlinks(id)
        .iter()
        .filter_map(|backlink| {
            let source = index.note(backlink.source)?;
            Some((
                display(&source.meta.rel).to_string(),
                source.body_offset + backlink.line + 1,
                backlink.context.to_string(),
            ))
        })
        .collect();

    let out_total = out.len();
    let in_total = incoming.len();
    let out_cut = cap(&mut out, limit);
    let in_cut = cap(&mut incoming, limit);

    let mut text = vec![format!("out ({out_total}): {}", or_none(&out, out_cut))];
    if incoming.is_empty() {
        text.push("in (0): none".to_string());
    } else {
        text.push(format!("in ({in_total}):"));
        for (source, line, context) in &incoming {
            text.push(format!(
                "  {source}:{line} {}",
                clip(context, CONTEXT_CHARS)
            ));
        }
        if in_cut > 0 {
            text.push(format!("  {}", super::more(in_cut)));
        }
    }
    let unresolved_total = unresolved.len();
    let unresolved_cut = cap(&mut unresolved, limit);
    if !unresolved.is_empty() {
        text.push(format!(
            "unresolved ({unresolved_total}): {}",
            or_none(&unresolved, unresolved_cut)
        ));
    }
    let files_total = files.len();
    let files_cut = cap(&mut files, limit);
    if !files.is_empty() {
        text.push(format!(
            "files ({files_total}): {}",
            or_none(&files, files_cut)
        ));
    }

    let json = json!({
        "note": found.rel,
        "out": out,
        "in": incoming.iter().map(|(source, line, context)| json!({ "note": source, "line": line, "context": context })).collect::<Vec<_>>(),
        "unresolved": unresolved,
        "files": files,
        "more": { "out": out_cut, "in": in_cut, "unresolved": unresolved_cut, "files": files_cut },
    });
    Ok(Output::new(text.join("\n"), json))
}

/// A comma-separated list, `none` when empty, ending in a count of what was
/// left out.
fn or_none(items: &[String], cut: usize) -> String {
    if items.is_empty() {
        return "none".to_string();
    }
    let mut text = items.join(", ");
    if cut > 0 {
        text.push_str(&format!(", … {cut} more"));
    }
    text
}

pub(super) fn stats(ctx: &mut Ctx, _args: &Args) -> Result<Output> {
    let stats = ctx.index()?.stats();
    Ok(Output::new(
        format!(
            "{} notes, {} folders, {} attachments, {} tags, {} links, {} unresolved, {} words",
            stats.notes,
            stats.folders,
            stats.attachments,
            stats.tags,
            stats.links,
            stats.unresolved,
            stats.words
        ),
        json!({
            "notes": stats.notes,
            "folders": stats.folders,
            "attachments": stats.attachments,
            "tags": stats.tags,
            "links": stats.links,
            "unresolved": stats.unresolved,
            "words": stats.words,
        }),
    ))
}

pub(super) fn vaults(_ctx: &mut Ctx, _args: &Args) -> Result<Output> {
    let vaults = emeraldian_core::vault::discover();
    let text = if vaults.is_empty() {
        "Obsidian has no vaults registered on this machine".to_string()
    } else {
        vaults
            .iter()
            .map(|v| {
                format!(
                    "{}  {}{}",
                    v.name,
                    v.path.display(),
                    if v.open { "  (open)" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let json: Vec<Value> = vaults
        .iter()
        .map(|v| json!({ "name": v.name, "path": v.path, "open": v.open }))
        .collect();
    Ok(Output::new(text, json!({ "vaults": json })))
}
