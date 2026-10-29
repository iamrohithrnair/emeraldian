//! The words in a diagram: a label's markup read into plain lines, wrapped
//! and cut short to fit, measured, and written out.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::canvas::Canvas;
use super::{Ink, Role};

/// Widest a line of a box's or an edge's words may be before it wraps.
pub const WIDTH: usize = 28;

/// A label as the lines it is shown on: `<br>` breaks a line, other HTML tags
/// and Markdown markers are dropped, the common entities are decoded, and
/// long lines are wrapped.
#[must_use]
pub fn lines(raw: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text(raw).lines() {
        wrap(line, WIDTH, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A label's [`lines`] on one line.
#[must_use]
pub fn joined(raw: &str) -> String {
    lines(raw).join(" ")
}

/// A label as the lines `<br>` breaks it into, as [`lines`] reads them but
/// left unwrapped, to be wrapped to the pane, and with no blank ones.
#[must_use]
pub fn unwrapped(raw: &str) -> Vec<String> {
    text(raw)
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

/// A label's text without its quotes and markup, with a line break for each
/// `<br>` and a space for anything else that isn't drawn.
fn text(raw: &str) -> String {
    let mut text = raw.trim();
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text = &text[1..text.len() - 1];
    }
    let text = text.trim_matches('`');
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        plain.push_str(&rest[..open]);
        let tag = &rest[open..];
        match tag.find('>') {
            Some(close) if is_tag(&tag[1..close]) => {
                if tag[1..close].trim().to_ascii_lowercase().starts_with("br") {
                    plain.push('\n');
                }
                rest = &tag[close + 1..];
            }
            _ => {
                plain.push('<');
                rest = &tag[1..];
            }
        }
    }
    plain.push_str(rest);
    plain
        .replace("**", "")
        .replace("&quot;", "\"")
        .replace("#quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("\\n", "\n")
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

/// Whether the inside of `<…>` looks like an HTML tag rather than a `<` in
/// prose, such as `a < b > c`.
fn is_tag(inside: &str) -> bool {
    let name = inside.trim_start_matches('/');
    name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/')
            .all(|c| c.is_ascii_alphanumeric())
}

/// Breaks text at spaces into lines no wider than `width`, splitting a word
/// that is wider on its own.
pub fn wrap(text: &str, width: usize, out: &mut Vec<String>) {
    let mut line = Line::default();
    for word in text.split_whitespace() {
        let needed = line.width + usize::from(!line.text.is_empty()) + word.width();
        if !line.text.is_empty() && needed > width {
            out.push(line.take());
        }
        if !line.text.is_empty() {
            line.push(' ');
        }
        // Each character as a string of its own.
        for piece in word.split_inclusive(|_: char| true) {
            if line.width + piece.width() > width && !line.text.is_empty() {
                out.push(line.take());
            }
            line.push_str(piece);
        }
    }
    if !line.text.is_empty() || out.is_empty() {
        out.push(line.take());
    }
}

/// [`wrap`] into lines of their own.
#[must_use]
pub fn wrapped(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    wrap(text, width, &mut lines);
    lines
}

/// A line being wrapped and the columns it takes, kept up to date as it
/// grows rather than measured again for each character, which took a label
/// of a few thousand accents a tenth of a second.
///
/// A string isn't as wide as its characters added up: `unicode-width`
/// measures it from its end, and a character can change the ones before it,
/// as a variation selector widens an emoji, joined emoji or a pair of flags
/// count once, and an alef makes the lam before it part of one ligature. So
/// most characters are added as they are, and the rest are measured again
/// with the stretch of the line they could change.
#[derive(Default)]
struct Line {
    text: String,
    width: usize,
    /// Where the stretch measured last starts, and its columns.
    stretch: Option<(usize, usize)>,
}

impl Line {
    fn take(&mut self) -> String {
        self.width = 0;
        self.stretch = None;
        std::mem::take(&mut self.text)
    }

    fn push_str(&mut self, piece: &str) {
        piece.chars().for_each(|c| self.push(c));
    }

    fn push(&mut self, c: char) {
        let alone = c.encode_utf8(&mut [0; 4]).width();
        if self.adds(c) {
            // A stretch keeps its columns past a character of no width, and
            // past any other may no longer be the one to measure.
            if alone > 0 {
                self.stretch = None;
            }
            self.width += alone;
            self.text.push(c);
            return;
        }
        let from = reach(&self.text);
        let before = match self.stretch {
            Some((at, columns)) if at == from => columns,
            _ => self.text[from..].width(),
        };
        self.text.push(c);
        let after = self.text[from..].width();
        self.width = self.width - before + after;
        self.stretch = Some((from, after));
    }

    /// Whether `c` adds its own width and leaves the line before it as it
    /// was: ASCII but a line break, a letter that is never part of a rule,
    /// anything of no width but a variation selector, a selector after
    /// anything of no width, and a flag after two others.
    fn adds(&self, c: char) -> bool {
        let mut before = self.text.chars().rev();
        let last = before.next();
        if c.is_ascii() || is_plain(c) {
            c != '\n'
        } else if c.width() == Some(0) {
            !matches!(c, '\u{FE01}' | '\u{FE0E}' | '\u{FE0F}')
                || last.is_some_and(|last| last.width() == Some(0))
        } else {
            is_flag(c) && last.is_some_and(is_flag) && before.next().is_some_and(is_flag)
        }
    }
}

/// Characters that whatever follows them leaves as they are: ASCII but the
/// digits, `#` and `*`, which a selector turns into keycap emoji, and the
/// letters of the scripts written without spaces.
fn is_plain(c: char) -> bool {
    matches!(c, ' ' | '!'..='"' | '$'..=')' | '+'..='/' | ':'..='~'
        | '\u{3041}'..='\u{3096}' | '\u{30A1}'..='\u{30FA}' | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}' | '\u{AC00}'..='\u{D7A3}' | '\u{FF01}'..='\u{FF60}')
}

fn is_flag(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Where the part of `text` a character added to it could change begins:
/// after the last plain character, or sixteen characters with width back,
/// further than any of the width rules reach. A run of flags counts as one,
/// and Tifinagh and Kirat Rai, whose rules carry along a whole run, as
/// none; their runs take a column for every few characters, so they never
/// reach past the line.
fn reach(text: &str) -> usize {
    let mut steps = 0;
    let mut flags = false;
    for (at, c) in text.char_indices().rev() {
        if is_plain(c) {
            return at + c.len_utf8();
        }
        let runs = matches!(c, '\u{2D30}'..='\u{2D7F}' | '\u{16D40}'..='\u{16D7F}');
        if c.width() != Some(0) && !runs && !(flags && is_flag(c)) {
            steps += 1;
            if steps > 16 {
                return at;
            }
        }
        flags = is_flag(c);
    }
    0
}

/// Columns the widest of some lines takes.
#[must_use]
pub fn widest(lines: &[String]) -> usize {
    lines.iter().map(|line| line.width()).max().unwrap_or(0)
}

/// Text cut short to fit in `width` columns, ending in `…` where it was cut.
#[must_use]
pub fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let wide = c.width().unwrap_or(0);
        if used + wide + 1 > width {
            break;
        }
        out.push(c);
        used += wide;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

/// Writes lines of text centred in a band `width` columns wide.
pub fn centred(canvas: &mut Canvas, x: usize, y: usize, width: usize, lines: &[String], ink: Ink) {
    for (row, line) in lines.iter().enumerate() {
        let indent = width.saturating_sub(line.width()) / 2;
        canvas.text(x + indent, y + row, line, ink);
    }
}

/// A chart's title wrapped to `width`, which is drawn at its top left.
#[must_use]
pub fn title(text: Option<&str>, width: usize) -> Vec<String> {
    text.map(|text| wrapped(text, width)).unwrap_or_default()
}

/// The rows a title takes, with a blank one under it.
#[must_use]
pub fn above(title: &[String]) -> usize {
    if title.is_empty() { 0 } else { title.len() + 1 }
}

/// Writes a title at the top left.
pub fn draw_title(canvas: &mut Canvas, title: &[String]) {
    for (row, line) in title.iter().enumerate() {
        canvas.text(0, row, line, Ink::plain(Role::Text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_lose_markup_and_break_where_asked() {
        assert_eq!(lines("\"one<br/>two\""), ["one", "two"]);
        assert_eq!(lines("<b>bold</b> &amp; **strong**"), ["bold & strong"]);
        assert_eq!(lines("a < b > c"), ["a < b > c"], "not a tag");
        assert_eq!(lines(""), [""]);
    }

    #[test]
    fn long_labels_wrap_and_long_words_split() {
        let wrapped = lines(&"word ".repeat(20));
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= WIDTH));
        let split = lines(&"x".repeat(70));
        assert_eq!(split.len(), 3);
        let wide = lines(&"漢".repeat(30));
        assert!(wide.iter().all(|line| line.width() <= WIDTH));
    }
}
