//! Rewriting a note's text: properties, sections and tasks.
//!
//! Each function takes a note's whole content and returns new content that
//! differs only in what was asked for. Nothing here touches a file — the caller
//! decides how the result is stored — and nothing outside the change is
//! normalised, so a note edited this way and then in Obsidian shows a diff of
//! exactly the edit.
//!
//! Lines are 0-based *file* lines throughout, frontmatter included, matching
//! what [`crate::search::ContentHit`] reports.

use crate::links;
use crate::note::{self, FmValue};

/// Sets one frontmatter property, or removes it when `value` is `None`.
///
/// Only that property's lines change. Other keys, comments and blank lines
/// keep their places and their formatting, so a property written here and one
/// written by Obsidian's property editor can sit in the same block. A note
/// without frontmatter gains a block when a value is set; removing the last
/// property removes the block.
///
/// Keys match case-insensitively, as [`note::Frontmatter::get`] reads them.
/// Lists are written as block lists, the way Obsidian writes them.
#[must_use]
pub fn set_property(content: &str, key: &str, value: Option<&FmValue>) -> String {
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let (yaml, body, _) = note::split_frontmatter(content);
    let Some(yaml) = yaml else {
        return match value {
            Some(value) => {
                let entry = render_property(key, value, newline);
                format!("---{newline}{entry}---{newline}{content}")
            }
            None => content.to_string(),
        };
    };

    // Both are slices of `content`, so where they start in it is where they
    // start in memory, measured from `content`'s own start.
    let yaml_start = yaml.as_ptr() as usize - content.as_ptr() as usize;
    let yaml_end = yaml_start + yaml.len();
    let body_start = body.as_ptr() as usize - content.as_ptr() as usize;

    let lines: Vec<&str> = yaml.split_inclusive('\n').collect();
    let mut updated = String::with_capacity(yaml.len() + 32);
    match lines.iter().position(|line| is_key_line(line, key)) {
        Some(start) => {
            let end = entry_end(&lines, start);
            updated.extend(lines[..start].iter().copied());
            if let Some(value) = value {
                // The key keeps the spelling it already had: asking for
                // `Status` changes `status`, it doesn't add a second one.
                let existing = lines[start].split_once(':').map_or(key, |(k, _)| k);
                updated.push_str(&render_property(existing, value, newline));
            }
            updated.extend(lines[end..].iter().copied());
        }
        None => {
            updated.push_str(yaml);
            if let Some(value) = value {
                if !updated.is_empty() && !updated.ends_with('\n') {
                    updated.push_str(newline);
                }
                updated.push_str(&render_property(key, value, newline));
            }
        }
    }

    if updated.trim().is_empty() {
        // Nothing left to keep: an empty `---` pair is noise at the top of a
        // note, and Obsidian shows it as an empty properties box.
        return content[body_start..].to_string();
    }
    format!(
        "{}{updated}{}",
        &content[..yaml_start],
        &content[yaml_end..]
    )
}

/// Whether a frontmatter line opens `key`'s entry.
fn is_key_line(line: &str, key: &str) -> bool {
    if line.starts_with([' ', '\t', '-', '#']) {
        return false;
    }
    line.split_once(':')
        .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key.trim()))
}

/// The line after the last one belonging to the entry opened at `start`.
///
/// An entry continues over indented lines and `- item` lines, which is how a
/// block list is written whether or not its items are indented. Blank lines
/// belong to it only when more of it follows.
fn entry_end(lines: &[&str], start: usize) -> usize {
    let mut end = start + 1;
    for (index, line) in lines.iter().enumerate().skip(start + 1) {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t', '-']) {
            end = index + 1;
            continue;
        }
        break;
    }
    end
}

/// A property as frontmatter lines, each ending in `newline`.
fn render_property(key: &str, value: &FmValue, newline: &str) -> String {
    let key = key.trim();
    match value {
        FmValue::Scalar(text) => format!("{key}: {}{newline}", yaml_scalar(text)),
        FmValue::List(items) if items.is_empty() => format!("{key}: []{newline}"),
        FmValue::List(items) => {
            let mut out = format!("{key}:{newline}");
            for item in items {
                out.push_str(&format!("  - {}{newline}", yaml_scalar(item)));
            }
            out
        }
    }
}

/// Writes a value so YAML reads it back as the same text.
///
/// Left bare wherever that is safe, so `true`, `3` and `2026-10-22` stay a
/// checkbox, a number and a date to Obsidian rather than becoming strings.
/// Quoted only where YAML would otherwise read something else: a leading
/// indicator, `: ` or ` #` inside, or surrounding space. `[[Note]]` is quoted,
/// which is how Obsidian itself writes a link in a property.
fn yaml_scalar(text: &str) -> String {
    let text = text.replace(['\n', '\r'], " ");
    if text.is_empty() {
        return "\"\"".to_string();
    }
    let indicator = text.starts_with([
        '[', ']', '{', '}', '#', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', ',',
    ]) || ["-", "?", ":"]
        .iter()
        .any(|lead| text == *lead || text.starts_with(&format!("{lead} ")));
    let needs_quotes = indicator
        || text != text.trim()
        || text.contains(": ")
        || text.contains(" #")
        || text.ends_with(':');
    if !needs_quotes {
        return text;
    }
    if !text.contains(['"', '\\']) {
        return format!("\"{text}\"");
    }
    // Single quotes take everything literally, a backslash included; a single
    // quote inside them is written twice.
    format!("'{}'", text.replace('\'', "''"))
}

/// What a heading is asked for by: its text, in any case, and its level when
/// it was given with its `#`s.
fn heading_key(heading: &str) -> (Option<u8>, String) {
    let heading = heading.trim();
    let hashes = heading.bytes().take_while(|&b| b == b'#').count();
    // `## Log` gives a level; `#Log` is a tag, and `Log` says nothing of one.
    let level =
        ((1..=6).contains(&hashes) && heading[hashes..].starts_with(' ')).then_some(hashes as u8);
    (level, heading.trim_start_matches('#').trim().to_lowercase())
}

/// Where a section is: the line of its heading, and the line after its last.
///
/// A section runs from its heading to the next heading of the same or a
/// higher level, or to the end of the note. A heading is matched by its text
/// in any case, so `Log` and `LOG` find the same one; written with its `#`s,
/// as `## Log`, it matches only at that level, which tells a `## Log` section
/// from a note whose title happens to be `# Log`. The first match wins.
#[must_use]
pub fn section(content: &str, heading: &str) -> Option<(usize, usize)> {
    let (_, body, offset) = note::split_frontmatter(content);
    let (wanted_level, wanted) = heading_key(heading);
    let headings = links::extract_headings(body);
    let at = headings.iter().position(|h| {
        h.text.to_lowercase() == wanted && wanted_level.is_none_or(|level| level == h.level)
    })?;
    let level = headings[at].level;
    let end = headings[at + 1..]
        .iter()
        .find(|h| h.level <= level)
        .map_or_else(|| content.lines().count(), |h| offset + h.line);
    Some((offset + headings[at].line, end))
}

/// Adds `text` to the end of a section, after its last line that isn't blank.
///
/// The blank lines that separate it from the next section stay where they
/// are, so the note keeps its spacing however many times this is called. `None`
/// when there is no such section.
#[must_use]
pub fn append_to_section(content: &str, heading: &str, text: &str) -> Option<String> {
    let (start, end) = section(content, heading)?;
    let lines: Vec<&str> = content.split_inclusive('\n').collect();

    let mut at = end;
    while at > start + 1 && lines[at - 1].trim().is_empty() {
        at -= 1;
    }

    let mut block = text.to_string();
    if !block.ends_with('\n') {
        block.push('\n');
    }
    // An empty section: keep the blank line under the heading, as there would
    // be under any heading written by hand, and put one back before whatever
    // follows, since that line was doing both jobs.
    if at == start + 1 && lines.get(at).is_some_and(|line| line.trim().is_empty()) {
        at += 1;
        if lines.get(at).is_some_and(|line| !line.trim().is_empty()) {
            block.push('\n');
        }
    }

    Some(splice(content, &lines, at, at, &block))
}

/// Replaces what is under a heading, keeping the heading itself.
///
/// With no such section, a new one is added, at the level `heading` was
/// written with or else as a `##`: before the heading named by `before` when
/// the note has one, else at the end.
#[must_use]
pub fn write_section(content: &str, heading: &str, text: &str, before: Option<&str>) -> String {
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let text = text.trim_matches('\n');

    if let Some((start, end)) = section(content, heading) {
        let mut block = if text.is_empty() {
            String::new()
        } else {
            format!("\n{text}\n")
        };
        if end < lines.len() {
            block.push('\n');
        }
        return splice(content, &lines, start + 1, end, &block);
    }

    let (level, _) = heading_key(heading);
    let marks = "#".repeat(usize::from(level.unwrap_or(2)));
    let title = heading.trim().trim_start_matches('#').trim();
    match before.and_then(|next| section(content, next)) {
        Some((next, _)) => {
            let block = format!("{marks} {title}\n\n{text}\n\n");
            splice(content, &lines, next, next, &block)
        }
        None => {
            let mut out = content.trim_end_matches('\n').to_string();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&format!("{marks} {title}\n\n{text}\n"));
            out
        }
    }
}

/// Replaces lines `from..to` with `text`, which is given a final newline if it
/// lacks one. A line before the splice that has no newline of its own — the
/// last line of a file that doesn't end in one — is given one.
fn splice(content: &str, lines: &[&str], from: usize, to: usize, text: &str) -> String {
    let mut out = String::with_capacity(content.len() + text.len() + 2);
    out.extend(lines[..from].iter().copied());
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text);
    if !text.is_empty() && !text.ends_with('\n') {
        out.push('\n');
    }
    out.extend(lines[to..].iter().copied());
    out
}

/// Ticks or unticks the task on a 0-based file line.
///
/// `None` when that line holds no task — inside a code block, say — so a line
/// number that has gone stale can't tick something it was never about. Only
/// the character between the brackets changes.
#[must_use]
pub fn set_task(content: &str, line: usize, done: bool) -> Option<String> {
    let (_, body, offset) = note::split_frontmatter(content);
    if !links::extract_tasks(body)
        .iter()
        .any(|task| offset + task.line == line)
    {
        return None;
    }

    let mut start = 0;
    for (index, text) in content.split_inclusive('\n').enumerate() {
        if index == line {
            // Without its line ending, so a task with no words, `- [ ]`,
            // reads as one: the marker wants nothing or a space after `]`.
            let (marker, ticked) = links::task_marker(text.trim_end_matches(['\n', '\r']))?;
            if ticked == done {
                return Some(content.to_string());
            }
            let state_len = text[marker..].chars().next().map_or(1, char::len_utf8);
            let at = start + marker;
            return Some(format!(
                "{}{}{}",
                &content[..at],
                if done { 'x' } else { ' ' },
                &content[at + state_len..]
            ));
        }
        start += text.len();
    }
    None
}

/// Makes text safe to put inside a section of a note.
///
/// A heading in the text would end the section early, and whatever was meant
/// to follow it in that section would land in one of the text's own making;
/// so each heading is escaped with a `\`, which Obsidian shows as the `#` it
/// was. A code fence the text opens and never closes would swallow the rest of
/// the note, so it is closed. A heading inside a code block is left alone,
/// since there it is not a heading.
#[must_use]
pub fn defuse(text: &str) -> String {
    let mut fence = links::FenceState::default();
    let mut out = String::with_capacity(text.len() + 8);
    for line in text.lines() {
        let outside = fence.accept(line);
        if outside && !links::extract_headings(line).is_empty() {
            let indent = line.len() - line.trim_start().len();
            out.push_str(&line[..indent]);
            out.push('\\');
            out.push_str(&line[indent..]);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if let Some(closing) = fence.closing() {
        out.push_str(&closing);
        out.push('\n');
    }
    if !text.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(text: &str) -> FmValue {
        FmValue::Scalar(text.to_string())
    }

    fn list(items: &[&str]) -> FmValue {
        FmValue::List(items.iter().map(|s| (*s).to_string()).collect())
    }

    #[test]
    fn setting_a_property_changes_that_line_alone() {
        let note = "---\ntitle: Plan\n# a comment\nstatus: draft\nweight: 3\n---\n# Plan\n";
        let updated = set_property(note, "status", Some(&scalar("done")));
        assert_eq!(
            updated,
            "---\ntitle: Plan\n# a comment\nstatus: done\nweight: 3\n---\n# Plan\n"
        );
    }

    #[test]
    fn a_new_property_goes_at_the_end_of_the_block() {
        let note = "---\ntitle: Plan\n---\nbody\n";
        assert_eq!(
            set_property(note, "status", Some(&scalar("active"))),
            "---\ntitle: Plan\nstatus: active\n---\nbody\n"
        );
    }

    #[test]
    fn a_note_without_frontmatter_gains_a_block() {
        assert_eq!(
            set_property("# Plan\n", "status", Some(&scalar("active"))),
            "---\nstatus: active\n---\n# Plan\n"
        );
        assert_eq!(set_property("# Plan\n", "status", None), "# Plan\n");
    }

    #[test]
    fn lists_are_written_as_block_lists_and_replace_either_style() {
        let note = "---\ntags: [a, b]\nnext: x\n---\n";
        let updated = set_property(note, "tags", Some(&list(&["c", "d"])));
        assert_eq!(updated, "---\ntags:\n  - c\n  - d\nnext: x\n---\n");

        // A block list, unindented and with a blank line in it, is replaced
        // whole rather than leaving stray items behind.
        let block = "---\ntags:\n- a\n\n- b\nnext: x\n---\n";
        assert_eq!(
            set_property(block, "TAGS", Some(&list(&["c"]))),
            "---\ntags:\n  - c\nnext: x\n---\n",
            "keys match without regard to case"
        );
        let parsed = note::parse_frontmatter(
            note::split_frontmatter(&updated)
                .0
                .expect("still has frontmatter"),
        );
        assert_eq!(parsed.tags(), vec!["c", "d"]);
    }

    #[test]
    fn removing_a_property_takes_its_list_with_it() {
        let note = "---\ntags:\n  - a\n  - b\nstatus: x\n---\nbody\n";
        assert_eq!(
            set_property(note, "tags", None),
            "---\nstatus: x\n---\nbody\n"
        );
        assert_eq!(
            set_property("---\nstatus: x\n---\nbody\n", "status", None),
            "body\n",
            "the last property takes the block with it"
        );
    }

    #[test]
    fn values_that_yaml_would_misread_are_quoted() {
        assert_eq!(yaml_scalar("plain words"), "plain words");
        assert_eq!(yaml_scalar("true"), "true", "a checkbox stays a checkbox");
        assert_eq!(yaml_scalar("2026-10-22"), "2026-10-22");
        assert_eq!(yaml_scalar("-5"), "-5");
        assert_eq!(yaml_scalar("[[Note]]"), "\"[[Note]]\"");
        assert_eq!(yaml_scalar("a: b"), "\"a: b\"");
        assert_eq!(yaml_scalar("x #y"), "\"x #y\"");
        assert_eq!(yaml_scalar("- item"), "\"- item\"");
        assert_eq!(yaml_scalar(""), "\"\"");
        assert_eq!(yaml_scalar("C:\\dir: x"), "'C:\\dir: x'");
        assert_eq!(yaml_scalar("it's: \"q\""), "'it''s: \"q\"'");
        assert_eq!(yaml_scalar("two\nlines"), "two lines");
    }

    #[test]
    fn quoted_values_read_back_as_written() {
        for value in ["[[Note]]", "a: b", "# heading", "plain"] {
            let note = set_property("", "k", Some(&scalar(value)));
            let yaml = note::split_frontmatter(&note).0.expect("frontmatter");
            assert_eq!(
                note::parse_frontmatter(yaml)
                    .get("k")
                    .map(FmValue::as_scalar)
                    .as_deref(),
                Some(value)
            );
        }
    }

    #[test]
    fn windows_line_endings_are_kept() {
        let note = "---\r\ntitle: Plan\r\n---\r\nbody\r\n";
        assert_eq!(
            set_property(note, "status", Some(&scalar("x"))),
            "---\r\ntitle: Plan\r\nstatus: x\r\n---\r\nbody\r\n"
        );
    }

    const SESSION: &str =
        "---\nstatus: active\n---\n# Task\n\n## Log\n\n- 10:00 began\n\n## Notes\n\ntext\n";

    #[test]
    fn a_section_runs_to_the_next_heading_at_its_level() {
        assert_eq!(section(SESSION, "log"), Some((5, 9)));
        assert_eq!(section(SESSION, "## Notes"), Some((9, 12)));
        assert_eq!(
            section(SESSION, "Task"),
            Some((3, 12)),
            "a `#` holds its `##`s"
        );
        assert_eq!(section(SESSION, "Missing"), None);
    }

    #[test]
    fn a_heading_given_with_its_hashes_matches_only_at_that_level() {
        let note = "# Log\n\nintro\n\n## Handoff\n\ns\n\n## Log\n\n- a\n";
        assert_eq!(
            section(note, "Log"),
            Some((0, 11)),
            "the title, by text alone"
        );
        assert_eq!(section(note, "## Log"), Some((8, 11)));
        assert_eq!(section(note, "### Log"), None);
        assert_eq!(
            section(note, "#Log"),
            Some((0, 11)),
            "`#Log` is a tag, not a level"
        );

        let written = write_section("# Log\n", "### Deep", "x", None);
        assert_eq!(written, "# Log\n\n### Deep\n\nx\n");
    }

    #[test]
    fn appending_to_a_section_keeps_the_spacing() {
        let once = append_to_section(SESSION, "Log", "- 10:05 next").expect("has a log");
        assert_eq!(
            once,
            "---\nstatus: active\n---\n# Task\n\n## Log\n\n- 10:00 began\n- 10:05 next\n\n## Notes\n\ntext\n"
        );

        let empty = "# Task\n\n## Log\n\n## Notes\n";
        assert_eq!(
            append_to_section(empty, "Log", "- first").expect("has a log"),
            "# Task\n\n## Log\n\n- first\n\n## Notes\n"
        );

        let unterminated = "## Log\n- a";
        assert_eq!(
            append_to_section(unterminated, "Log", "- b").expect("has a log"),
            "## Log\n- a\n- b\n"
        );
        assert_eq!(append_to_section(SESSION, "Nope", "x"), None);
    }

    #[test]
    fn writing_a_section_replaces_its_body_or_adds_it() {
        let replaced = write_section(SESSION, "Notes", "new text", None);
        assert!(replaced.ends_with("## Notes\n\nnew text\n"), "{replaced}");

        let middle = write_section(SESSION, "Log", "- only", None);
        assert!(
            middle.contains("## Log\n\n- only\n\n## Notes"),
            "the next heading keeps its blank line: {middle}"
        );

        let inserted = write_section(SESSION, "Handoff", "summary", Some("Log"));
        assert!(
            inserted.contains("# Task\n\n## Handoff\n\nsummary\n\n## Log\n"),
            "{inserted}"
        );

        let appended = write_section("# Task\n", "Handoff", "summary", Some("Log"));
        assert_eq!(appended, "# Task\n\n## Handoff\n\nsummary\n");
    }

    #[test]
    fn a_task_with_no_words_can_be_ticked() {
        assert_eq!(set_task("- [ ]\n", 0, true).as_deref(), Some("- [x]\n"));
        assert_eq!(set_task("- [ ]\r\n", 0, true).as_deref(), Some("- [x]\r\n"));
        assert_eq!(set_task("- [ ]", 0, true).as_deref(), Some("- [x]"));
    }

    #[test]
    fn defused_text_cannot_end_a_section_or_swallow_the_note() {
        assert_eq!(defuse("plain\nlines"), "plain\nlines");
        assert_eq!(
            defuse("Findings:\n## Root cause\n  # deep"),
            "Findings:\n\\## Root cause\n  \\# deep"
        );
        assert_eq!(defuse("#tag stays"), "#tag stays", "a tag is not a heading");
        assert_eq!(
            defuse("```\n## in code\n```\n"),
            "```\n## in code\n```\n",
            "a heading in a closed block is code"
        );
        assert_eq!(defuse("````rust\nfn x() {}"), "````rust\nfn x() {}\n````");

        // And the section it is put in stays whole.
        let note = write_section(
            "## Log\n\n- a\n\n## Next\n",
            "Log",
            &defuse("x\n## Next"),
            None,
        );
        assert_eq!(section(&note, "## Next"), Some((5, 6)), "{note}");
        assert_eq!(section(&note, "## Log"), Some((0, 5)), "{note}");
    }

    #[test]
    fn ticking_a_task_changes_one_character() {
        let note = "---\na: b\n---\n- [ ] first\n```\n- [ ] code\n```\n  - [x] second\n";
        let ticked = set_task(note, 3, true).expect("line 3 is a task");
        assert_eq!(
            ticked,
            "---\na: b\n---\n- [x] first\n```\n- [ ] code\n```\n  - [x] second\n"
        );
        let unticked = set_task(note, 7, false).expect("line 7 is a task");
        assert!(unticked.ends_with("  - [ ] second\n"));
        assert_eq!(set_task(note, 5, true), None, "code is not a task");
        assert_eq!(set_task(note, 1, true), None, "frontmatter is not a task");
        assert_eq!(set_task(note, 99, true), None);
        assert_eq!(
            set_task(note, 7, true).as_deref(),
            Some(note),
            "already done"
        );
    }
}
