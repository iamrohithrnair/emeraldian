//! Extraction of wikilinks, tags and headings from note bodies.
//!
//! This is what turns a folder of Markdown files into a graph. All three
//! extractors share one rule: **code is not content**. A `#[derive(Debug)]` in a
//! Rust snippet is not a tag, and `[[i]]` in a C array index is not a link, so
//! every scanner here skips fenced blocks and inline code spans.

/// A `[[wikilink]]` or `[text](url)` found in a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRef {
    /// The link target with any `#heading` / `^block` suffix removed. For a
    /// wikilink this is a note name or vault-relative path; for a Markdown link
    /// it is the URL.
    pub target: String,
    /// A `#heading` or `#^block` suffix, without the `#`.
    pub anchor: Option<String>,
    /// The display text after `|` (wikilink) or before `(` (Markdown link).
    pub alias: Option<String>,
    /// `![[...]]` transclusion rather than a plain link.
    pub embed: bool,
    /// `true` for `[text](url)` Markdown links, which never resolve to notes
    /// unless the URL is a relative path.
    pub markdown: bool,
    /// 0-based line within the text that was scanned.
    pub line: usize,
}

/// A Markdown heading, used for the outline pane and `[[note#heading]]` links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    /// 0-based line within the text that was scanned.
    pub line: usize,
}

/// Tracks whether the scanner is currently inside a fenced code block.
///
/// Fences are matched by their opening character and length, so a ```` ``` ````
/// block containing a shorter fence, or a `~~~` block containing backticks,
/// stays open — the same rule CommonMark uses.
#[derive(Default)]
pub(crate) struct FenceState {
    open: Option<(char, usize)>,
}

impl FenceState {
    /// The line that would close the block left open, if one is.
    pub(crate) fn closing(&self) -> Option<String> {
        self.open
            .map(|(ch, len)| std::iter::repeat_n(ch, len).collect())
    }

    /// Feeds one line and reports whether that line's *content* should be
    /// scanned. Fence delimiter lines themselves are never scanned.
    pub(crate) fn accept(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start();
        let fence = trimmed
            .strip_prefix("```")
            .map(|rest| ('`', 3 + rest.chars().take_while(|&c| c == '`').count()))
            .or_else(|| {
                trimmed
                    .strip_prefix("~~~")
                    .map(|rest| ('~', 3 + rest.chars().take_while(|&c| c == '~').count()))
            });

        match (self.open, fence) {
            (None, Some(f)) => {
                self.open = Some(f);
                false
            }
            (Some((open_ch, open_len)), Some((ch, len))) => {
                // A closing fence must use the same character and be at least
                // as long, and carry no info string.
                if ch == open_ch && len >= open_len && trimmed[len..].trim().is_empty() {
                    self.open = None;
                }
                false
            }
            (Some(_), None) => false,
            (None, None) => true,
        }
    }
}

/// Blanks out inline `code spans` so their contents aren't scanned, preserving
/// byte length so column positions stay meaningful.
fn mask_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.char_indices().peekable();
    let mut in_code = false;

    while let Some((_, c)) = chars.next() {
        if c == '`' {
            in_code = !in_code;
            out.push(' ');
            continue;
        }
        if c == '\\' {
            // An escaped character can't open a construct; blank both.
            out.push(' ');
            if let Some((_, next)) = chars.next() {
                for _ in 0..next.len_utf8() {
                    out.push(' ');
                }
            }
            continue;
        }
        if in_code {
            for _ in 0..c.len_utf8() {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Extracts every link in a note body, in document order.
#[must_use]
pub fn extract_links(body: &str) -> Vec<LinkRef> {
    let mut links = Vec::new();
    let mut fence = FenceState::default();

    for (line_no, raw) in body.lines().enumerate() {
        if !fence.accept(raw) {
            continue;
        }
        let line = mask_inline_code(raw);
        scan_wikilinks(&line, line_no, &mut links);
        scan_markdown_links(&line, line_no, &mut links);
    }

    links
}

fn scan_wikilinks(line: &str, line_no: usize, out: &mut Vec<LinkRef>) {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i + 1 < bytes.len() {
        if bytes[i] != b'[' || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let Some(end) = line[i + 2..].find("]]") else {
            break;
        };
        let inner = &line[i + 2..i + 2 + end];
        let embed = i > 0 && bytes[i - 1] == b'!';

        if let Some(link) = parse_wikilink(inner, embed, line_no) {
            out.push(link);
        }
        i += 2 + end + 2;
    }
}

fn parse_wikilink(inner: &str, embed: bool, line_no: usize) -> Option<LinkRef> {
    // Order matters: `[[note#heading|alias]]` puts the alias last.
    let (target_part, alias) = match inner.split_once('|') {
        Some((t, a)) => (t, Some(a.trim().to_string())),
        None => (inner, None),
    };
    let (target, anchor) = match target_part.split_once('#') {
        Some((t, a)) => (t, Some(a.trim().to_string())),
        None => (target_part, None),
    };

    let target = target.trim();
    // `[[#heading]]` jumps within the current note, so there is no edge to
    // record — the graph only cares about links between notes.
    if target.is_empty() {
        return None;
    }

    Some(LinkRef {
        target: target.to_string(),
        anchor,
        alias,
        embed,
        markdown: false,
        line: line_no,
    })
}

fn scan_markdown_links(line: &str, line_no: usize, out: &mut Vec<LinkRef>) {
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        // A `[[` here belongs to a wikilink, already handled.
        if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            continue;
        }
        let Some(text_end) = line[i + 1..].find(']') else {
            break;
        };
        let text_end = i + 1 + text_end;
        if line[text_end + 1..].starts_with('(')
            && let Some(url_end) = line[text_end + 2..].find(')')
        {
            let text = &line[i + 1..text_end];
            let url = line[text_end + 2..text_end + 2 + url_end].trim();
            if !url.is_empty() {
                let (target, anchor) = match url.split_once('#') {
                    Some((t, a)) if !t.is_empty() => (t, Some(a.to_string())),
                    _ => (url, None),
                };
                out.push(LinkRef {
                    target: target.to_string(),
                    anchor,
                    alias: (!text.is_empty()).then(|| text.to_string()),
                    embed: i > 0 && bytes[i - 1] == b'!',
                    markdown: true,
                    line: line_no,
                });
            }
            i = text_end + 2 + url_end + 1;
            continue;
        }
        i = text_end + 1;
    }
}

/// Extracts inline `#tags`, in document order and deduplicated.
///
/// A tag must follow a non-word character (so `C#` and a URL fragment aren't
/// tags), must not be all digits (so `#1` reads as a number, matching
/// Obsidian), and may nest with `/`.
#[must_use]
pub fn extract_tags(body: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    let mut fence = FenceState::default();

    for raw in body.lines() {
        if !fence.accept(raw) {
            continue;
        }
        // An ATX heading's `#` characters are structure, not tags.
        let line = mask_inline_code(raw);
        let scan = line.trim_start();
        if scan.starts_with('#') && scan.trim_start_matches('#').starts_with(' ') {
            continue;
        }

        // Scanning by character, not by byte: casting a byte to `char` turns
        // the lead byte of a multi-byte character into a letter (`0xC2` becomes
        // `Â`), which walks the cursor into the middle of that character and
        // panics on the slice. Real vaults are full of non-breaking spaces and
        // smart quotes, so this path has to be UTF-8 aware.
        let chars: Vec<(usize, char)> = line.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].1 != '#' {
                i += 1;
                continue;
            }
            let preceded_by_word = i > 0 && is_tag_char(chars[i - 1].1);
            if preceded_by_word {
                i += 1;
                continue;
            }
            let start = i + 1;
            let mut end = start;
            while end < chars.len() && is_tag_char(chars[end].1) {
                end += 1;
            }
            if end > start {
                let start_byte = chars[start].0;
                let end_byte = chars.get(end).map_or(line.len(), |(byte, _)| *byte);
                let tag = &line[start_byte..end_byte];
                let tag = tag.trim_end_matches('/');
                if !tag.is_empty() && !tag.bytes().all(|b| b.is_ascii_digit()) {
                    let tag = tag.to_string();
                    if !tags.contains(&tag) {
                        tags.push(tag);
                    }
                }
            }
            i = end.max(i + 1);
        }
    }

    tags
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '/')
}

/// Extracts ATX headings (`# Heading`) for the outline pane.
///
/// Setext headings (underlined with `===`) are not extracted: they're vanishingly
/// rare in Obsidian vaults, which use `#` throughout.
#[must_use]
pub fn extract_headings(body: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut fence = FenceState::default();

    for (line_no, raw) in body.lines().enumerate() {
        if !fence.accept(raw) {
            continue;
        }
        let trimmed = raw.trim_start();
        let level = trimmed.bytes().take_while(|&b| b == b'#').count();
        if level == 0 || level > 6 {
            continue;
        }
        let rest = &trimmed[level..];
        // `#tag` at line start is a tag, not a heading — a space is required.
        if !rest.starts_with(' ') {
            continue;
        }
        let text = heading_text(rest).to_string();
        if text.is_empty() {
            continue;
        }
        headings.push(Heading {
            level: level as u8,
            text,
            line: line_no,
        });
    }

    headings
}

/// A heading's text, without the closing `#`s it may end with.
///
/// CommonMark counts a closing run of `#`s only when a space separates it from
/// the text, so `## Middle ##` is "Middle" but `## C#` is "C#", a language
/// rather than a heading with a stray mark.
pub(crate) fn heading_text(text: &str) -> &str {
    let text = text.trim();
    let without = text.trim_end_matches('#');
    if without.is_empty() || without.ends_with([' ', '\t']) {
        without.trim_end()
    } else {
        text
    }
}

/// A `- [ ]` task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    /// 0-based line within the text that was scanned.
    pub line: usize,
    /// Anything other than a space between the brackets counts as ticked, as
    /// it does in Obsidian, where `[-]` and `[>]` are themes' ways of saying
    /// cancelled or deferred: in either case, not still to do.
    pub done: bool,
    pub text: String,
}

/// Finds a task's checkbox on a line: the byte offset of the character
/// between the brackets, and whether the task is ticked.
///
/// A bulleted (`-`, `*`, `+`) or numbered (`1.`, `1)`) list item whose text
/// opens with `[ ]`, at any indent. The offset is what lets a task be ticked by
/// changing one character and nothing else on the line.
#[must_use]
pub fn task_marker(line: &str) -> Option<(usize, bool)> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();

    let after_marker = if let Some(rest) = trimmed
        .strip_prefix(['-', '*', '+'])
        .and_then(|rest| rest.strip_prefix(' '))
    {
        rest
    } else {
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        trimmed[digits..]
            .strip_prefix(". ")
            .or_else(|| trimmed[digits..].strip_prefix(") "))?
    };
    let rest = after_marker.trim_start_matches(' ');

    let mut chars = rest.chars();
    if chars.next() != Some('[') {
        return None;
    }
    let state = chars.next()?;
    if chars.next() != Some(']') {
        return None;
    }
    // `- [x]` must be followed by a space or nothing: `- [x]y` is a link-ish
    // run of text, not a task.
    if !matches!(chars.next(), None | Some(' ' | '\t')) {
        return None;
    }

    let offset = indent + (trimmed.len() - rest.len()) + 1;
    Some((offset, state != ' '))
}

/// Extracts every `- [ ]` and `- [x]` task, in document order, skipping fenced
/// code.
#[must_use]
pub fn extract_tasks(body: &str) -> Vec<Task> {
    let mut tasks = Vec::new();
    let mut fence = FenceState::default();

    for (line_no, raw) in body.lines().enumerate() {
        if !fence.accept(raw) {
            continue;
        }
        let Some((offset, done)) = task_marker(raw) else {
            continue;
        };
        // The checkbox is three bytes, `[`, its state and `]`, and the state
        // may itself be wider than a byte.
        let state_len = raw[offset..].chars().next().map_or(1, char::len_utf8);
        let text = raw[offset + state_len + 1..].trim().to_string();
        tasks.push(Task {
            line: line_no,
            done,
            text,
        });
    }

    tasks
}

/// Counts words in a note body the way Obsidian's status bar does: runs of
/// non-whitespace, with fenced code included.
#[must_use]
pub fn word_count(body: &str) -> usize {
    body.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wikilink_variants() {
        let links =
            extract_links("See [[Note A]], [[b/c|Alias]], [[Page#Section]] and ![[Img.png]]");
        assert_eq!(links.len(), 4);

        assert_eq!(links[0].target, "Note A");
        assert!(links[0].alias.is_none());

        assert_eq!(links[1].target, "b/c");
        assert_eq!(links[1].alias.as_deref(), Some("Alias"));

        assert_eq!(links[2].target, "Page");
        assert_eq!(links[2].anchor.as_deref(), Some("Section"));

        assert_eq!(links[3].target, "Img.png");
        assert!(links[3].embed, "![[...]] is an embed");
    }

    #[test]
    fn alias_and_anchor_combine() {
        let links = extract_links("[[Page#Section|Nice Name]]");
        assert_eq!(links[0].target, "Page");
        assert_eq!(links[0].anchor.as_deref(), Some("Section"));
        assert_eq!(links[0].alias.as_deref(), Some("Nice Name"));
    }

    #[test]
    fn parses_markdown_links() {
        let links = extract_links("[docs](https://example.com) and [rel](notes/a.md)");
        assert_eq!(links.len(), 2);
        assert!(links[0].markdown);
        assert_eq!(links[0].target, "https://example.com");
        assert_eq!(links[0].alias.as_deref(), Some("docs"));
        assert_eq!(links[1].target, "notes/a.md");
    }

    #[test]
    fn skips_links_and_tags_in_fenced_code() {
        let body =
            "before #real\n```rust\n#[derive(Debug)]\nlet x = [[not a link]];\n```\nafter [[Real]]";
        let links = extract_links(body);
        assert_eq!(links.len(), 1, "only the link outside the fence counts");
        assert_eq!(links[0].target, "Real");

        let tags = extract_tags(body);
        assert_eq!(tags, vec!["real"], "#[derive] inside a fence is not a tag");
    }

    #[test]
    fn tilde_fence_can_contain_backticks() {
        let body = "~~~\n```\n#nope\n~~~\n#yes";
        assert_eq!(extract_tags(body), vec!["yes"]);
    }

    #[test]
    fn skips_inline_code() {
        assert_eq!(extract_links("use `[[literal]]` here").len(), 0);
        assert_eq!(extract_tags("the `#hash` operator").len(), 0);
    }

    #[test]
    fn tag_rules_match_obsidian() {
        assert_eq!(extract_tags("#project/alpha done"), vec!["project/alpha"]);
        assert_eq!(extract_tags("scored 100#1"), Vec::<String>::new());
        assert_eq!(extract_tags("issue #1 filed"), Vec::<String>::new());
        assert_eq!(extract_tags("I write C# daily"), Vec::<String>::new());
        assert_eq!(extract_tags("#a and #a again"), vec!["a"], "deduplicated");
    }

    #[test]
    fn non_ascii_after_a_hash_does_not_panic() {
        // A non-breaking space after `#` — common in notes pasted from the web,
        // and what crashed a real vault. `0xC2` looks like a letter when a byte
        // is cast to `char`, so a byte-wise scanner walks into the character.
        let line = "#\u{a0}Find all Python processes using port 8000";
        assert_eq!(extract_tags(line), Vec::<String>::new());
        assert!(extract_links(line).is_empty());
        assert!(extract_headings(line).is_empty());
    }

    #[test]
    fn tags_handle_unicode_text_around_them() {
        // Smart quotes, CJK, emoji and accented letters all sit next to tags in
        // real notes.
        assert_eq!(extract_tags("“quoted” #real"), vec!["real"]);
        assert_eq!(extract_tags("日本語 #tag です"), vec!["tag"]);
        assert_eq!(extract_tags("🎉 #party time"), vec!["party"]);
        assert_eq!(extract_tags("#café au lait"), vec!["café"]);
        assert_eq!(
            extract_tags("naïve#notatag"),
            Vec::<String>::new(),
            "a tag still can't start inside a word"
        );
    }

    #[test]
    fn a_lone_hash_at_end_of_line_is_safe() {
        assert_eq!(extract_tags("trailing #"), Vec::<String>::new());
        assert_eq!(extract_tags("#"), Vec::<String>::new());
    }

    #[test]
    fn headings_need_a_space_and_report_lines() {
        let body = "# Title\nbody\n### Deep\n#nottag-heading\n####### too deep";
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].level, 1);
        assert_eq!(headings[0].text, "Title");
        assert_eq!(headings[0].line, 0);
        assert_eq!(headings[1].level, 3);
        assert_eq!(headings[1].line, 2);
    }

    #[test]
    fn heading_closing_hashes_are_trimmed() {
        assert_eq!(extract_headings("## Middle ##")[0].text, "Middle");
    }

    #[test]
    fn a_hash_that_belongs_to_a_word_stays_in_the_heading() {
        assert_eq!(extract_headings("## C#")[0].text, "C#");
        assert_eq!(extract_headings("## F# and C# ##")[0].text, "F# and C#");
        assert!(extract_headings("## ##").is_empty());
    }

    #[test]
    fn self_anchor_links_are_ignored() {
        assert!(extract_links("jump to [[#Section]]").is_empty());
    }

    #[test]
    fn tasks_are_found_in_every_list_style() {
        let body =
            "- [ ] open\n* [x] done\n  + [-] cancelled\n1. [ ] numbered\n2) [X] closed\n- [ ]\n";
        let tasks = extract_tasks(body);
        let summary: Vec<(usize, bool, &str)> = tasks
            .iter()
            .map(|t| (t.line, t.done, t.text.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                (0, false, "open"),
                (1, true, "done"),
                (2, true, "cancelled"),
                (3, false, "numbered"),
                (4, true, "closed"),
                (5, false, ""),
            ]
        );
    }

    #[test]
    fn things_that_only_look_like_tasks_are_not() {
        let body = "- [[Link]] here\n- [x](https://example.com)\n[ ] no bullet\n- plain\n-[ ] no space\n```\n- [ ] in code\n```\n";
        assert!(extract_tasks(body).is_empty(), "{:?}", extract_tasks(body));
    }

    #[test]
    fn the_task_marker_points_at_the_checkbox() {
        let line = "    - [ ] indented";
        let (offset, done) = task_marker(line).expect("a task");
        assert!(!done);
        assert_eq!(&line[offset..=offset], " ");
        assert_eq!(&line[offset - 1..offset], "[");

        let wide = "- [✓] ticked with a wide character";
        let (offset, done) = task_marker(wide).expect("a task");
        assert!(done);
        assert!(wide[offset..].starts_with('✓'));
        assert_eq!(extract_tasks(wide)[0].text, "ticked with a wide character");
    }
}
