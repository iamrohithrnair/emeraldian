//! Commands that read or change one note.

use std::fs::{self, OpenOptions};
use std::io::Write;

use emeraldian_core::edit;
use emeraldian_core::excalidraw;
use emeraldian_core::index::LinkTarget;
use emeraldian_core::links;
use emeraldian_core::note::{self, FmValue};
use emeraldian_core::ops::validate_name;
use serde_json::{Value, json};

use super::{
    Args, Ctx, Failure, Located, Output, Result, append_file, clip, display, fail, read as load,
    write_file,
};
use crate::clock::{self, Utc};

/// How much of a note `read` prints unless told to print all of it: about
/// five thousand tokens. More than any note written by hand; a note that
/// reaches it is one to read a section or a range of lines at a time.
const MAX_READ_CHARS: usize = 20_000;

pub(super) fn read(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let content = load(&found)?;

    let whole =
        !args.flag("full") && args.value("lines").is_none() && args.value("section").is_none();
    if whole
        && excalidraw::is_drawing(&found.rel)
        && let Some(drawing) = excalidraw::parse(&content)
    {
        return Ok(drawing_words(&found, &drawing));
    }

    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();

    let (mut start, mut end) = (0, total);
    if let Some(heading) = args.value("section") {
        let Some(range) = edit::section(&content, heading) else {
            return fail(missing_section(&found, &content, heading));
        };
        (start, end) = range;
    }
    if let Some(range) = args.value("lines") {
        let (from, to) = line_range(range, total, found.name())?;
        start = start.max(from);
        end = end.min(to);
    }
    if args.flag("body") {
        let (_, _, offset) = note::split_frontmatter(&content);
        start = start.max(offset);
    }
    let start = start.min(end);

    let numbered = args.flag("numbered");
    let mut text = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if numbered {
                format!("{}: {line}", start + i + 1)
            } else {
                (*line).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut shown_to = end;
    let mut notice = None;
    if !args.flag("full")
        && let Some((cut, _)) = text.char_indices().nth(MAX_READ_CHARS)
    {
        // Back to the end of a line, so nothing is cut mid-sentence.
        let cut = text[..cut].rfind('\n').unwrap_or(cut);
        text.truncate(cut);
        shown_to = start + text.lines().count();
        let rest = if end < total {
            format!("{}:{end}", shown_to + 1)
        } else {
            format!("{}:", shown_to + 1)
        };
        notice = Some(format!(
            "emeraldian: cut at line {shown_to} of {total}; --lines {rest} for the rest, or --full"
        ));
    }

    let json = json!({
        "note": found.rel,
        "from": start + 1,
        "to": shown_to,
        "lines": total,
        "truncated": notice.is_some(),
        "content": text,
    });
    let output = Output::new(text, json);
    Ok(match notice {
        Some(notice) => output.notice(notice),
        None => output,
    })
}

/// What can be read of a drawing: the words in it.
///
/// An Excalidraw note is stored as compressed base64, often tens of thousands
/// of characters of it, which says nothing to whoever reads it and would fill
/// an agent's context for no return. The app draws the scene instead of showing
/// that; here, its labels stand in for it.
fn drawing_words(found: &Located, drawing: &excalidraw::Drawing) -> Output {
    let words: Vec<&str> = drawing
        .elements
        .iter()
        .map(|element| element.text.trim())
        .filter(|text| !text.is_empty())
        .collect();
    let shapes = drawing.elements.len();
    let mut text = format!(
        "{} is an Excalidraw drawing of {shapes} shape{}",
        found.name(),
        if shapes == 1 { "" } else { "s" }
    );
    if words.is_empty() {
        text.push_str(", with no text in it");
    } else {
        text.push_str(" with this text:");
        for word in &words {
            text.push_str(&format!("\n- {}", word.replace('\n', " ")));
        }
    }
    Output::new(
        text,
        json!({ "note": found.rel, "drawing": true, "shapes": shapes, "text": words }),
    )
    .notice("emeraldian: the scene itself is compressed; --full prints the file as it is")
}

/// A `--lines` range, `A:B` from 1, as 0-based `[from, to)`. A range that
/// starts past the end is a failure rather than an empty answer, which would
/// read as an empty note.
fn line_range(range: &str, total: usize, name: &str) -> Result<(usize, usize)> {
    let bad = || Failure::Usage(format!("--lines takes A:B, A: or :B, not `{range}`"));
    let number = |text: &str| -> Result<Option<usize>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        match text.parse::<usize>() {
            Ok(0) | Err(_) => Err(bad()),
            Ok(n) => Ok(Some(n)),
        }
    };
    let (from, to) = match range.split_once(':') {
        Some((from, to)) => (number(from)?, number(to)?),
        None => {
            let line = number(range)?.ok_or_else(bad)?;
            (Some(line), Some(line))
        }
    };
    if let (Some(from), Some(to)) = (from, to)
        && to < from
    {
        return Err(bad());
    }
    let from = from.map_or(0, |n| n - 1);
    if from > 0 && from >= total {
        return fail(format!(
            "{name} has {total} line{}",
            if total == 1 { "" } else { "s" }
        ));
    }
    let to = to.map_or(total, |n| n.min(total));
    if from > to {
        return Err(bad());
    }
    Ok((from, to))
}

/// Says a section wasn't found, and which ones there are.
fn missing_section(found: &Located, content: &str, heading: &str) -> String {
    let (_, body, _) = note::split_frontmatter(content);
    let headings: Vec<String> = links::extract_headings(body)
        .into_iter()
        .map(|h| h.text)
        .collect();
    if headings.is_empty() {
        format!("{} has no headings", found.name())
    } else {
        format!(
            "{} has no section `{heading}`; it has {}",
            found.name(),
            headings.join(", ")
        )
    }
}

pub(super) fn info(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let content = load(&found)?;
    let (_, body, offset) = note::split_frontmatter(&content);
    let tasks = links::extract_tasks(body);
    let open = tasks.iter().filter(|t| !t.done).count();

    let index = ctx.index()?;
    let Some(note) = index.id_of_rel(&found.rel).and_then(|id| index.note(id)) else {
        return fail(format!(
            "{} isn't indexed: it's in a hidden folder (see `ui.show_hidden`)",
            found.name()
        ));
    };
    let id = index.id_of_rel(&found.rel).expect("just found");

    let outgoing = note.outgoing().len();
    let mut sources: Vec<usize> = index.backlinks(id).iter().map(|b| b.source).collect();
    sources.dedup();
    let mut unresolved: Vec<&str> = note
        .links
        .iter()
        .filter_map(|link| match &link.target {
            LinkTarget::Unresolved(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    unresolved.sort_unstable();
    unresolved.dedup();

    let now = clock::now();
    let when = |secs: u64| {
        format!(
            "{} ({})",
            Utc::at(secs).format("%Y-%m-%d %H:%MZ"),
            clock::ago(secs, now)
        )
    };

    let mut lines = vec![format!("path: {}", found.rel)];
    if note.meta.title != note.meta.stem {
        lines.push(format!("title: {}", note.meta.title));
    }
    if !note.aliases.is_empty() {
        lines.push(format!("aliases: {}", note.aliases.join(", ")));
    }
    if !note.tags.is_empty() {
        lines.push(format!("tags: #{}", note.tags.join(" #")));
    }
    lines.push(format!("words: {}", note.words));
    lines.push(format!("modified: {}", when(note.meta.modified)));
    lines.push(format!("created: {}", when(note.meta.created)));
    lines.push(format!(
        "links: {outgoing} out, {} in, {} unresolved",
        sources.len(),
        unresolved.len()
    ));
    if !note.headings.is_empty() {
        lines.push(format!("headings: {}", note.headings.len()));
    }
    if !tasks.is_empty() {
        lines.push(format!("tasks: {open} open, {} done", tasks.len() - open));
    }

    let json = json!({
        "note": found.rel,
        "file": found.path,
        "title": note.meta.title,
        "aliases": note.aliases,
        "tags": note.tags,
        "words": note.words,
        "modified": Utc::at(note.meta.modified).iso(),
        "created": Utc::at(note.meta.created).iso(),
        "links": { "out": outgoing, "in": sources.len(), "unresolved": unresolved },
        "headings": note.headings.len(),
        "body_line": offset + 1,
        "tasks": { "open": open, "done": tasks.len() - open },
    });
    Ok(Output::new(lines.join("\n"), json))
}

pub(super) fn path(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let file = found.path.display().to_string();
    Ok(Output::new(
        file.clone(),
        json!({ "note": found.rel, "file": file }),
    ))
}

pub(super) fn outline(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let content = load(&found)?;
    let (_, body, offset) = note::split_frontmatter(&content);
    let headings = links::extract_headings(body);

    let text = if headings.is_empty() {
        format!("{} has no headings", found.name())
    } else {
        headings
            .iter()
            .map(|h| {
                format!(
                    "{}: {} {}",
                    offset + h.line + 1,
                    "#".repeat(usize::from(h.level)),
                    h.text
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let json: Vec<Value> = headings
        .iter()
        .map(|h| json!({ "line": offset + h.line + 1, "level": h.level, "text": h.text }))
        .collect();
    Ok(Output::new(
        text,
        json!({ "note": found.rel, "headings": json }),
    ))
}

pub(super) fn props(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let content = load(&found)?;
    let sets = args.values("set");
    let unsets = args.values("unset");

    if !sets.is_empty() || !unsets.is_empty() {
        if args.get(1).is_some() {
            return Err(Failure::Usage(
                "give a KEY to read, or --set and --unset to change, not both".into(),
            ));
        }
        let mut updated = content.clone();
        let mut changed = Vec::new();
        for assignment in sets {
            let Some((key, value)) = assignment.split_once('=') else {
                return Err(Failure::Usage(format!(
                    "--set takes KEY=VALUE, not `{assignment}`"
                )));
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(Failure::Usage(format!("--set `{assignment}` has no key")));
            }
            updated = edit::set_property(&updated, key, Some(&property_value(value)));
            changed.push(key.to_string());
        }
        for key in unsets {
            updated = edit::set_property(&updated, key.trim(), None);
            changed.push(format!("-{}", key.trim()));
        }
        write_file(&found.path, &updated)?;
        return Ok(Output::new(
            format!("updated {}: {}", found.name(), changed.join(", ")),
            json!({ "note": found.rel, "changed": changed }),
        ));
    }

    let (yaml, _, _) = note::split_frontmatter(&content);
    let frontmatter = yaml.map(note::parse_frontmatter).unwrap_or_default();

    if let Some(key) = args.get(1) {
        let Some(value) = frontmatter.get(key) else {
            return fail(format!("{} has no property `{key}`", found.name()));
        };
        let text = match value {
            FmValue::Scalar(text) => text.clone(),
            FmValue::List(items) => items.join("\n"),
        };
        return Ok(Output::new(
            text,
            json!({ "note": found.rel, "key": key, "value": fm_json(value) }),
        ));
    }

    let mut map = serde_json::Map::new();
    let lines: Vec<String> = frontmatter
        .entries()
        .iter()
        .map(|(key, value)| {
            map.insert(key.clone(), fm_json(value));
            match value {
                FmValue::Scalar(text) => format!("{key}: {text}"),
                FmValue::List(items) => format!("{key}: [{}]", items.join(", ")),
            }
        })
        .collect();
    let text = if lines.is_empty() {
        format!("{} has no properties", found.name())
    } else {
        lines.join("\n")
    };
    Ok(Output::new(
        text,
        json!({ "note": found.rel, "properties": map }),
    ))
}

/// `[a, b]` is a list, anything else one value, a `[[link]]` included: that is
/// how a property points at a note, and it would otherwise read as a list
/// holding `[link]`.
fn property_value(text: &str) -> FmValue {
    let text = text.trim();
    let one_link =
        text.starts_with("[[") && text.ends_with("]]") && text.matches("[[").count() == 1;
    match text
        .strip_prefix('[')
        .and_then(|t| t.strip_suffix(']'))
        .filter(|_| !one_link)
    {
        Some(inner) => FmValue::List(
            inner
                .split(',')
                .map(|item| item.trim().trim_matches(['"', '\'']).to_string())
                .filter(|item| !item.is_empty())
                .collect(),
        ),
        None => FmValue::Scalar(text.to_string()),
    }
}

fn fm_json(value: &FmValue) -> Value {
    match value {
        FmValue::Scalar(text) => json!(text),
        FmValue::List(items) => json!(items),
    }
}

/// Text from the command line, given the final newline a file should have.
fn with_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// What a new note holds when nothing was given for it: its name, as a
/// heading, as the app writes one.
fn starter(place: &Located) -> String {
    let title = place.name().rsplit('/').next().unwrap_or_default();
    format!("# {title}\n\n")
}

/// Creates a file that must not already exist. Refusing at the moment of
/// creation, rather than checking first, means two agents creating the same
/// note can't both succeed with one silently replacing the other.
fn create(place: &Located, content: &str) -> Result<bool> {
    if let Some(parent) = place.path.parent() {
        fs::create_dir_all(parent)?;
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&place.path)
    {
        Ok(mut file) => {
            file.write_all(content.as_bytes())?;
            Ok(true)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(err) => Err(err.into()),
    }
}

pub(super) fn new(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let place = ctx.place(args.at(0))?;
    let content = match args.text(1)? {
        Some(text) => with_newline(text),
        None => starter(&place),
    };

    let mut target = place.clone();
    let mut n = 1;
    loop {
        if create(&target, &content)? {
            break;
        }
        if !args.flag("unique") {
            return fail(format!(
                "{} already exists; `write` replaces it, --unique picks a free name",
                target.name()
            ));
        }
        // ` 1`, ` 2`, … as Obsidian numbers a name that is taken.
        let stem = place.rel.strip_suffix(".md").unwrap_or(&place.rel);
        target = ctx.place(&format!("{stem} {n}"))?;
        n += 1;
    }
    Ok(Output::new(
        format!("created {}", target.name()),
        json!({ "created": target.rel }),
    ))
}

pub(super) fn write(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let name = args.at(0);
    let text = args.text(1)?.unwrap_or_default();
    // An existing note under any name it answers to, else a new one at
    // exactly the name given.
    let (found, existed) = match ctx.locate(name) {
        Ok(found) => (found, true),
        Err(_) => (ctx.place(name)?, false),
    };

    let content = match args.value("section") {
        Some(heading) => {
            let current = if existed {
                load(&found)?
            } else {
                starter(&found)
            };
            edit::write_section(&current, heading, &text, None)
        }
        None => with_newline(text),
    };
    write_file(&found.path, &content)?;

    let verb = if existed { "wrote" } else { "created" };
    Ok(Output::new(
        format!("{verb} {}", found.name()),
        json!({ "note": found.rel, "created": !existed }),
    ))
}

pub(super) fn append(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let name = args.at(0);
    let text = args.text(1)?.unwrap_or_default();
    let found = match ctx.locate(name) {
        Ok(found) => found,
        Err(err) if !args.flag("create") => return Err(err),
        Err(_) => {
            let place = ctx.place(name)?;
            create(&place, "")?;
            place
        }
    };

    match args.value("section") {
        Some(heading) => {
            let content = load(&found)?;
            let updated = edit::append_to_section(&content, heading, &text)
                .unwrap_or_else(|| edit::write_section(&content, heading, &text, None));
            write_file(&found.path, &updated)?;
        }
        None => append_file(&found.path, &text)?,
    }
    Ok(Output::new(
        format!("appended to {}", found.name()),
        json!({ "note": found.rel }),
    ))
}

pub(super) fn edit(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let (Some(old), Some(new)) = (args.value("old"), args.value("new")) else {
        return Err(Failure::Usage(
            "edit needs --old <TEXT> and --new <TEXT>".into(),
        ));
    };
    if old.is_empty() {
        return Err(Failure::Usage("--old can't be empty".into()));
    }
    let found = ctx.locate(args.at(0))?;
    let content = load(&found)?;

    let count = content.matches(old).count();
    if count == 0 {
        return fail(format!("`{}` isn't in {}", clip(old, 60), found.name()));
    }
    if count > 1 && !args.flag("all") {
        return fail(format!(
            "`{}` is in {} {count} times; include more of the text around it, or pass --all",
            clip(old, 60),
            found.name()
        ));
    }
    let updated = if args.flag("all") {
        content.replace(old, new)
    } else {
        content.replacen(old, new, 1)
    };
    write_file(&found.path, &updated)?;

    let changes = if count == 1 {
        "1 change".to_string()
    } else {
        format!("{count} changes")
    };
    Ok(Output::new(
        format!("edited {} ({changes})", found.name()),
        json!({ "note": found.rel, "changes": count }),
    ))
}

/// The id of a note that `locate` found, which a command changing the vault's
/// shape needs.
fn indexed(ctx: &mut Ctx, found: &Located) -> Result<usize> {
    let index = ctx.index()?;
    index.id_of_rel(&found.rel).ok_or_else(|| {
        Failure::Failed(format!(
            "{} isn't indexed: it's in a hidden folder (see `ui.show_hidden`)",
            found.name()
        ))
    })
}

pub(super) fn rename(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let new_name = args.at(1).trim();
    if new_name.contains('/') {
        return Err(Failure::Usage(
            "rename takes a new name, not a path; `move` changes the folder".into(),
        ));
    }
    let found = ctx.locate(args.at(0))?;
    let id = indexed(ctx, &found)?;
    let index = ctx.index()?;

    let mut sources: Vec<usize> = index.backlinks(id).iter().map(|b| b.source).collect();
    sources.sort_unstable();
    sources.dedup();
    let new_id = index.rename_note(id, new_name)?;
    let rel = index
        .note(new_id)
        .map(|n| n.meta.rel.clone())
        .unwrap_or_default();

    let mut text = format!("renamed {} to {}", found.name(), display(&rel));
    if !sources.is_empty() {
        text.push_str(&format!(
            "; updated the links in {} note{}",
            sources.len(),
            if sources.len() == 1 { "" } else { "s" }
        ));
    }
    Ok(Output::new(
        text,
        json!({ "from": found.rel, "to": rel, "linking_notes": sources.len() }),
    ))
}

pub(super) fn move_note(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let id = indexed(ctx, &found)?;
    let (folder, _) = ctx.folder(args.at(1))?;
    let index = ctx.index()?;
    let new_id = index.move_note(id, &folder)?;
    let rel = index
        .note(new_id)
        .map(|n| n.meta.rel.clone())
        .unwrap_or_default();
    Ok(Output::new(
        format!("moved {} to {}", found.name(), display(&rel)),
        json!({ "from": found.rel, "to": rel }),
    ))
}

pub(super) fn delete(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let id = indexed(ctx, &found)?;
    let root = ctx.root()?;
    let trashed = ctx.index()?.delete_note(id)?;
    let trashed = trashed
        .strip_prefix(&root)
        .unwrap_or(&trashed)
        .to_string_lossy()
        .replace('\\', "/");
    Ok(Output::new(
        format!(
            "deleted {}; it can be restored from {trashed}",
            found.name()
        ),
        json!({ "deleted": found.rel, "trash": trashed }),
    ))
}

pub(super) fn link(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let from = ctx.locate(args.at(0))?;
    let to = args.at(1).trim();
    let index = ctx.index()?;

    // The shortest name that still means this note: its bare name, unless
    // another note answers to that first, in which case its path.
    let target = match index.resolve(to).and_then(|id| Some((id, index.note(id)?))) {
        Some((id, note)) if index.resolve(&note.meta.stem) == Some(id) => note.meta.stem.clone(),
        Some((_, note)) => display(&note.meta.rel).to_string(),
        // A note not written yet: linked as named, as Obsidian plans one.
        None => display(to).to_string(),
    };
    let line = match args.value("text") {
        Some(text) if text.contains("[[") => text.to_string(),
        Some(text) => format!("{} [[{target}]]", text.trim_end()),
        None => format!("[[{target}]]"),
    };
    append_file(&from.path, &line)?;
    Ok(Output::new(
        format!("linked {} to {target}", from.name()),
        json!({ "from": from.rel, "to": target, "line": line }),
    ))
}

pub(super) fn task(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let found = ctx.locate(args.at(0))?;
    let line: usize = match args.at(1).trim().parse() {
        Ok(n) if n > 0 => n,
        _ => {
            return Err(Failure::Usage(format!(
                "LINE is a line number, as `tasks` prints it, not `{}`",
                args.at(1)
            )));
        }
    };
    let done = !args.flag("undo");
    let content = load(&found)?;
    let Some(updated) = edit::set_task(&content, line - 1, done) else {
        return fail(format!("line {line} of {} isn't a task", found.name()));
    };
    if updated != content {
        write_file(&found.path, &updated)?;
    }

    let text = task_text(&updated, line - 1);
    let verb = if done { "ticked" } else { "unticked" };
    Ok(Output::new(
        format!("{verb} {}:{line} {text}", found.name()),
        json!({ "note": found.rel, "line": line, "done": done, "text": text }),
    ))
}

/// The words of the task on a 0-based file line.
pub(super) fn task_text(content: &str, line: usize) -> String {
    let (_, body, offset) = note::split_frontmatter(content);
    links::extract_tasks(body)
        .into_iter()
        .find(|task| offset + task.line == line)
        .map(|task| task.text)
        .unwrap_or_default()
}

pub(super) fn daily(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let name = ctx.config.editor.daily_note_name();
    let (found, created) = match ctx.locate(&name) {
        Ok(found) => (found, false),
        Err(_) => {
            let place = ctx.place(&name)?;
            let created = create(&place, &starter(&place))?;
            (place, created)
        }
    };

    match args.text(0)? {
        Some(text) => {
            append_file(&found.path, &text)?;
            Ok(Output::new(
                format!("appended to {}", found.name()),
                json!({ "note": found.rel, "created": created }),
            ))
        }
        None => Ok(Output::new(
            found.name().to_string(),
            json!({ "note": found.rel, "created": created }),
        )),
    }
}

pub(super) fn mkdir(ctx: &mut Ctx, args: &Args) -> Result<Output> {
    let (rel, path) = ctx.folder(args.at(0))?;
    if rel.is_empty() {
        return Err(Failure::Usage("mkdir needs a folder name".into()));
    }
    for segment in rel.split('/') {
        validate_name(segment)?;
    }
    if path.exists() {
        return fail(format!("{rel} already exists"));
    }
    fs::create_dir_all(&path)?;
    Ok(Output::new(
        format!("created folder {rel}"),
        json!({ "created": rel }),
    ))
}
