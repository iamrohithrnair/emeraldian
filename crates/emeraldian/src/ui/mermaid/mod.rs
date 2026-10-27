//! Mermaid diagrams drawn in text.
//!
//! A ```` ```mermaid ```` block in reading mode is drawn as boxes and lines
//! made of box-drawing characters, which every terminal can show, rather
//! than as its source. Flowcharts (`graph` and `flowchart`) and sequence
//! diagrams are understood. Anything else, and anything this can't make
//! sense of, returns `None`, and the block is shown as code the way it always
//! was: a wrong picture is worse than the source.
//!
//! Nothing here runs a browser or another program. Each diagram is drawn the
//! first time a note that holds it is laid out, and kept in a small cache
//! keyed by its source, so laying the note out again (a resize, an edit
//! elsewhere in it, coming back to it) costs a lookup. The cache holds a
//! fixed number of diagrams, and each has a size limit, so memory stays the
//! same however many diagrams a note or a vault has.
//!
//! Turning a sideways chart that doesn't fit to run downwards, the limit on
//! source size and keeping drawings by their source are ideas taken from
//! prismantis (MIT), which draws Mermaid for Claude Code. The code is our own.

mod canvas;
mod flowchart;
mod sequence;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use unicode_width::UnicodeWidthStr;

/// What a run of a diagram's characters is, so the renderer can colour it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Blank space and the words inside a box.
    Text,
    /// A box's outline.
    Border,
    /// An edge, or a lifeline in a sequence diagram.
    Line,
    /// The head of an edge.
    Arrow,
    /// The words on an edge or a message.
    Label,
    /// A note, or the frame of a `loop` or `alt` block.
    Note,
}

/// A drawn diagram: rows of text runs, each with its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    pub rows: Vec<Vec<(String, Role)>>,
    /// Columns in the widest row.
    pub width: usize,
}

/// Longest source drawn, in bytes. A diagram past this is not something
/// anyone reads in a terminal, and the limit bounds the work of trying.
const MAX_SOURCE: usize = 16 * 1024;

/// How many diagrams are kept drawn.
const CACHE_SIZE: usize = 64;

/// How many cells the kept diagrams may take between them, a few megabytes
/// at most, so a vault of big diagrams can't grow the cache past it.
const CACHE_CELLS: usize = 1_000_000;

/// Draws a diagram from the lines of a `mermaid` block, or `None` when it
/// isn't a kind this understands or doesn't parse.
///
/// `width` is the room the pane has. A left-to-right flowchart that doesn't
/// fit is tried top to bottom, which is usually narrower.
#[must_use]
pub fn render(lines: &[String], width: usize) -> Option<Rc<Diagram>> {
    let mut hasher = DefaultHasher::new();
    lines.hash(&mut hasher);
    let key = hasher.finish();
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(found) = cache.get(key, width) {
            return found;
        }
        let drawn = draw(lines, width).map(Rc::new);
        cache.put(key, width, drawn.clone());
        drawn
    })
}

fn draw(lines: &[String], width: usize) -> Option<Diagram> {
    let size: usize = lines.iter().map(|line| line.len() + 1).sum();
    if size > MAX_SOURCE {
        return None;
    }
    let body = without_preamble(lines);
    let (header, rest) = body.split_first()?;
    let header = header.trim();
    let kind = header
        .split(|c: char| c.is_whitespace() || c == ';')
        .next()?;
    match kind {
        "graph" | "flowchart" => {
            let chart = flowchart::parse(&header[kind.len()..], rest)?;
            flowchart::draw(&chart, width)
        }
        "sequenceDiagram" => sequence::draw(&sequence::parse(rest)?),
        _ => None,
    }
}

/// The lines from the one naming the diagram on, past blank lines, `%%`
/// comments and a `---` front matter block.
fn without_preamble(lines: &[String]) -> Vec<&str> {
    let mut rest: Vec<&str> = lines.iter().map(String::as_str).collect();
    if rest.first().is_some_and(|line| line.trim() == "---")
        && let Some(end) = rest.iter().skip(1).position(|line| line.trim() == "---")
    {
        rest.drain(..end + 2);
    }
    let start = rest
        .iter()
        .position(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with("%%")
        })
        .unwrap_or(rest.len());
    rest.drain(..start);
    rest.retain(|line| !line.trim_start().starts_with("%%"));
    rest
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// The most recently drawn diagrams, newest first. A failure is kept too, so
/// a block that doesn't parse isn't parsed again on every layout.
#[derive(Default)]
struct Cache {
    entries: VecDeque<(u64, usize, Option<Rc<Diagram>>)>,
}

impl Cache {
    fn get(&mut self, key: u64, width: usize) -> Option<Option<Rc<Diagram>>> {
        let at = self
            .entries
            .iter()
            .position(|(k, w, _)| *k == key && *w == width)?;
        let entry = self.entries.remove(at)?;
        let found = entry.2.clone();
        self.entries.push_front(entry);
        Some(found)
    }

    fn put(&mut self, key: u64, width: usize, diagram: Option<Rc<Diagram>>) {
        self.entries.push_front((key, width, diagram));
        self.entries.truncate(CACHE_SIZE);
        while self.entries.len() > 1 && self.cells() > CACHE_CELLS {
            self.entries.pop_back();
        }
    }

    fn cells(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|(_, _, diagram)| diagram.as_ref())
            .map(|diagram| diagram.rows.len() * diagram.width)
            .sum()
    }
}

/// Widest a line of a box's or an edge's words may be before it wraps.
const LABEL_WIDTH: usize = 28;

/// A label as the lines it is shown on: `<br>` breaks a line, other HTML tags
/// and Markdown markers are dropped, the common entities are decoded, and
/// long lines are wrapped.
#[must_use]
pub fn label_lines(raw: &str) -> Vec<String> {
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
    let plain = plain
        .replace("**", "")
        .replace("&quot;", "\"")
        .replace("#quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("\\n", "\n");
    let mut lines = Vec::new();
    for line in plain.lines() {
        let line: String = line
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        wrap(line.trim(), LABEL_WIDTH, &mut lines);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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
fn wrap(text: &str, width: usize, out: &mut Vec<String>) {
    let mut line = String::new();
    for word in text.split_whitespace() {
        let needed = line.width() + usize::from(!line.is_empty()) + word.width();
        if !line.is_empty() && needed > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        for c in word.chars() {
            if line.width() + c.to_string().width() > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
}

/// Columns the widest of some lines takes.
fn widest(lines: &[String]) -> usize {
    lines.iter().map(|line| line.width()).max().unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn plain(diagram: &Diagram) -> String {
    diagram
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(source: &str) -> Vec<String> {
        source.lines().map(str::to_string).collect()
    }

    #[test]
    fn labels_lose_markup_and_break_where_asked() {
        assert_eq!(label_lines("\"one<br/>two\""), ["one", "two"]);
        assert_eq!(
            label_lines("<b>bold</b> &amp; **strong**"),
            ["bold & strong"]
        );
        assert_eq!(label_lines("a < b > c"), ["a < b > c"], "not a tag");
        assert_eq!(label_lines(""), [""]);
    }

    #[test]
    fn long_labels_wrap_and_long_words_split() {
        let wrapped = label_lines(&"word ".repeat(20));
        assert!(wrapped.len() > 1);
        assert!(wrapped.iter().all(|line| line.width() <= LABEL_WIDTH));
        let split = label_lines(&"x".repeat(70));
        assert_eq!(split.len(), 3);
        let wide = label_lines(&"漢".repeat(30));
        assert!(wide.iter().all(|line| line.width() <= LABEL_WIDTH));
    }

    #[test]
    fn unknown_kinds_and_empty_blocks_are_left_as_code() {
        assert!(render(&[], 80).is_none());
        assert!(render(&lines("\n\n"), 80).is_none());
        assert!(render(&lines("pie title Pets\n  \"Dogs\" : 3"), 80).is_none());
        assert!(render(&lines("graph TD\n  A -->"), 80).is_none());
        assert!(render(&lines("not mermaid at all"), 80).is_none());
    }

    #[test]
    fn comments_and_front_matter_before_the_kind_are_skipped() {
        let source = "---\ntitle: Flow\n---\n%% a comment\n\ngraph TD\n  A --> B\n  %% another\n";
        assert!(render(&lines(source), 80).is_some());
    }

    #[test]
    fn huge_sources_are_refused_without_parsing() {
        let mut source = String::from("graph TD\n");
        for at in 0..2000 {
            source.push_str(&format!("  n{at} --> n{}\n", at + 1));
        }
        assert!(render(&lines(&source), 80).is_none());
    }

    /// Every prefix of some real diagrams, and each with one character
    /// dropped: half-typed and mangled input must come back as a drawing or
    /// as `None`, never a panic.
    #[test]
    fn mangled_input_never_panics() {
        let samples = [
            "graph TD\n  A[Christmas] -->|Get money| B(Go shopping)\n  B --> C{Let me think}\n  C -->|One| D[Laptop]\n  C -. Two .-> E[\"iPhone [x]\"]\n  C == Three ==> F((Car))\n  F --> F\n  D & E --- A",
            "flowchart RL; a([x]) <--> b[[y]]; b --x c[(z)]; c ~~~ d{{w}}; d --o a; e>f] --> g[/h/]",
            "sequenceDiagram\n  autonumber\n  participant A as Alice<br>Smith\n  actor B\n  A->>+B: Hello 日本\n  B-->>-A: Hi\n  loop Every minute\n    A-)B: ping\n  else never\n    B--xA: no\n  end\n  Note left of A: 🙂\n  Note over A,B: both\n  A->>A: self",
        ];
        for sample in samples {
            let mut cuts: Vec<String> = sample
                .char_indices()
                .map(|(at, _)| sample[..at].to_string())
                .collect();
            cuts.extend(
                sample
                    .char_indices()
                    .map(|(at, c)| format!("{}{}", &sample[..at], &sample[at + c.len_utf8()..])),
            );
            for cut in cuts {
                for width in [1, 20, 200] {
                    let _ = draw(&lines(&cut), width);
                }
            }
        }
    }

    #[test]
    fn the_cache_returns_the_same_drawing_and_stays_bounded() {
        let source = lines("graph LR\n  A --> B");
        let first = render(&source, 80).unwrap();
        let again = render(&source, 80).unwrap();
        assert!(Rc::ptr_eq(&first, &again), "drawn once, then looked up");
        for at in 0..CACHE_SIZE * 3 {
            let _ = render(&lines(&format!("graph TD\n  A{at} --> B")), 80);
        }
        CACHE.with(|cache| {
            let cache = cache.borrow();
            assert_eq!(cache.entries.len(), CACHE_SIZE);
            assert!(cache.cells() <= CACHE_CELLS);
        });
    }

    #[test]
    fn big_diagrams_are_dropped_from_the_cache_to_keep_it_small() {
        let mut cache = Cache::default();
        let big = Rc::new(Diagram {
            rows: vec![Vec::new(); 1000],
            width: 400,
        });
        for key in 0..10 {
            cache.put(key, 80, Some(big.clone()));
        }
        assert!(cache.cells() <= CACHE_CELLS);
        assert_eq!(cache.entries.len(), 2);
        assert!(cache.get(9, 80).is_some(), "the newest is kept");
        assert!(cache.get(0, 80).is_none(), "the oldest went first");
    }
}
