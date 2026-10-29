//! Mermaid diagrams drawn in text.
//!
//! A ```` ```mermaid ```` block in reading mode is drawn as boxes and lines
//! made of box-drawing characters, which every terminal can show, rather
//! than as its source. Flowcharts (`graph` and `flowchart`), sequence, class,
//! state and entity relationship diagrams, C4 diagrams of all five kinds,
//! pie charts, Gantt charts, XY charts, mind maps, timelines and user
//! journeys are understood, each in a module of its own, and those that are
//! graphs share one layout, [`layered`]'s. Anything else, and anything this
//! can't make sense of, returns `None`, and the block is shown as code the
//! way it always was: a wrong picture is worse than the source.
//!
//! Nothing here runs a browser or another program. Each diagram is drawn the
//! first time a note that holds it is laid out, and kept in a small cache
//! keyed by its source, so laying the note out again (a resize, an edit
//! elsewhere in it, coming back to it) costs a lookup. The cache holds a
//! fixed number of diagrams, and each has a size limit, so memory stays the
//! same however many diagrams a note or a vault has.
//!
//! Each node, and each participant in a sequence, has a colour of its own,
//! which its outline, the edges it sends and their arrowheads and words are
//! drawn in, so the flow can be followed by colour. So does each slice of a
//! pie, each series of a chart and each section or branch of a timeline,
//! journey or mind map. The words inside a box stay in the body colour,
//! where they read best. The drawing only records whose each run is; the
//! colours are the theme's own hues, picked when the note is laid out (see
//! [`Diagram::colours`]) and kept with the laid-out lines, so a frame costs
//! what it did.
//!
//! Turning a sideways chart that doesn't fit to run downwards, the limit on
//! source size, keeping drawings by their source and giving each box a colour
//! from the theme are ideas taken from prismantis (MIT), which draws Mermaid
//! for Claude Code, and so is the choice of kinds to draw beyond those two:
//! C4, class, state and entity relationship diagrams, pies, Gantt charts and
//! XY charts, each of whose modules says what it took. Arrowheads in their
//! edge's colour, and words on edges kept to a stronger contrast than the
//! lines, come from beautiful-mermaid (MIT), the renderer prismantis builds
//! on. The code is our own.

mod c4;
mod canvas;
mod class;
mod er;
mod flowchart;
mod gantt;
mod journey;
mod layered;
mod mindmap;
mod pie;
mod sequence;
mod state;
mod timeline;
mod xychart;

// Copying a drawn diagram out, as text or as a picture.
pub mod export;
mod glyphs;
mod picture;

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

/// How a run of a diagram's characters is coloured: what it is, and whose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ink {
    pub role: Role,
    /// The node or participant whose colour it is drawn in, by its place in
    /// [`Diagram::keys`]. `None` for what is no one's: the words inside a
    /// box, notes, and the rules of a `loop` or `alt` block.
    pub owner: Option<u16>,
}

impl Ink {
    /// No one's.
    #[must_use]
    pub const fn plain(role: Role) -> Self {
        Self { role, owner: None }
    }

    /// In the colour of the node or participant at `owner`.
    #[must_use]
    pub fn of(role: Role, owner: usize) -> Self {
        Self {
            role,
            owner: u16::try_from(owner).ok(),
        }
    }
}

/// A drawn diagram: rows of text runs, each with its ink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagram {
    pub rows: Vec<Vec<(String, Ink)>>,
    /// Columns in the widest row.
    pub width: usize,
    /// A key for each node or participant, made from its id, so the same one
    /// is given the same colour every time it is drawn.
    pub keys: Vec<u64>,
    /// Owners drawn joined, by an edge or a message or as lifelines side by
    /// side, which are given different colours where there are enough.
    pub links: Vec<(u16, u16)>,
}

impl Diagram {
    /// Which of `hues` colours each owner is drawn in, by its place in
    /// [`Self::keys`]. Empty when there are no colours to give.
    ///
    /// Each owner's key picks the colour it starts from, so the same diagram
    /// always looks the same, in this note or any other, and an owner keeps
    /// its colour as others are added unless one of them takes the colour it
    /// would have had. One already taken passes to the next along: first to
    /// one no one has, so a diagram with no more owners than colours has no
    /// two alike, and once every colour is in use, to the least used one that
    /// none of its neighbours has.
    #[must_use]
    pub fn colours(&self, hues: usize) -> Vec<u8> {
        // The neighbours' colours are kept as bits of a `u64`.
        let hues = hues.min(64);
        if hues == 0 {
            return Vec::new();
        }
        let mut used = [0usize; 64];
        let mut picked: Vec<u8> = Vec::with_capacity(self.keys.len());
        for (owner, key) in self.keys.iter().enumerate() {
            let mut near = 0u64;
            for &(a, b) in &self.links {
                let other = match (usize::from(a) == owner, usize::from(b) == owner) {
                    (true, false) => b,
                    (false, true) => a,
                    _ => continue,
                };
                if let Some(&hue) = picked.get(usize::from(other)) {
                    near |= 1 << hue;
                }
            }
            let first = (key % hues as u64) as usize;
            let hue = (0..hues)
                .map(|step| (first + step) % hues)
                .min_by_key(|&hue| ((near >> hue) & 1, used[hue]))
                .unwrap_or(first);
            used[hue] += 1;
            picked.push(hue as u8);
        }
        picked
    }
}

/// A key for a node's or participant's id, the same on every run and every
/// machine: FNV-1a, then mixed with SplitMix64's finaliser so ids that differ
/// only in their last letter, as `A`, `B` and `C` do, still land far apart.
#[must_use]
pub fn key(id: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // Most diagrams name their nodes `A`, `B`, `C` and `D`, as Mermaid's own
    // examples do. This salt was picked so those start out in different
    // colours, `A` in the first, a theme's blue, and none of the first five
    // in purple or red where a theme has eight.
    hash ^= SALT;
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^ (hash >> 31)
}

/// See [`key`].
const SALT: u64 = 231_170;

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
        kind if c4::is_kind(kind) => c4::draw(&c4::parse(rest, kind == "C4Dynamic")?, width),
        "classDiagram" | "classDiagram-v2" => class::draw(&class::parse(rest)?),
        "stateDiagram" | "stateDiagram-v2" => state::draw(&state::parse(rest)?, width),
        "erDiagram" => er::draw(&er::parse(rest)?, width),
        "pie" => pie::draw(&pie::parse(&header[kind.len()..], rest)?, width),
        "gantt" => gantt::draw(&gantt::parse(rest)?, width),
        "xychart-beta" | "xychart" => {
            xychart::draw(&xychart::parse(&header[kind.len()..], rest)?, width)
        }
        "mindmap" => mindmap::draw(&mindmap::parse(rest)?),
        "timeline" => timeline::draw(&timeline::parse(rest)?, width),
        "journey" => journey::draw(&journey::parse(rest)?, width),
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

/// Text cut short to fit in `width` columns, ending in `…` where it was cut.
fn truncate(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

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
        assert!(render(&lines("gitGraph\n  commit\n  branch develop"), 80).is_none());
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
            "C4Container\n  title Bank 日本\n  Person(c, \"Customer\", \"Has accounts\")\n  Container_Boundary(b, \"App\") {\n    Container(w, \"Web\", \"Rust, Axum\", \"Serves pages\")\n    ContainerDb_Ext(d, \"DB\", \"Postgres\")\n  }\n  BiRel(c, w, \"Uses\", \"HTTPS\")\n  Rel_Back(d, w, \"Reads\")\n  Rel(w, w, \"Self\")\n  UpdateElementStyle(c, $fontColor=\"red\")",
            "classDiagram\n  direction LR\n  class Animal~T~ {\n    <<abstract>>\n    +int age\n    +mate() List~int~\n  }\n  namespace Zoo.Pens {\n    class Duck\n  }\n  Animal <|-- Duck\n  Duck \"1\" *-- \"many\" Egg : lays 🥚\n  Duck ..|> Swims\n  Animal --> Animal\n  note for Duck \"quacks\"",
            "stateDiagram-v2\n  [*] --> Still\n  state Moving {\n    [*] --> Slow\n    Slow --> Fast : 加速\n    --\n    [*] --> Loud\n  }\n  state pick <<choice>>\n  Still --> pick\n  pick --> Moving : go\n  pick --> [*]\n  Moving --> Moving : again\n  note right of Still : resting",
            "erDiagram\n  direction LR\n  CUSTOMER ||--o{ ORDER : places\n  CUSTOMER {\n    string name PK \"the 名前\"\n  }\n  ORDER }|..|{ ITEM : has\n  p[Person] ||--o| CUSTOMER : is\n  CAR 1 to zero or more ORDER : drives\n  ORDER ||--o{ ORDER : splits",
            "mindmap\n  root((mindmap))\n    Origins\n      Long history\n      ::icon(fa fa-book)\n      Popularisation\n    Research\n      On effectiveness<br/>and features\n      id2[\"`**Bold** and *italic*\nover two lines`\"]\n    Tools{{Tools}}:::urgent\n      Pen and paper",
            "timeline\n  title England's History\n  section Stone Age\n    7600 BC : Britain's oldest known house : Orkney\n    6000 BC : Sea levels rise.<br> People hunt.\n  section Bronze Age\n    2300 BC : People arrive from Europe\n            : New styles of pottery\n  accTitle: Ages",
            "journey\n  title My working day\n  section Go to work\n    Make tea: 5: Me\n    Do work: 1: Me, Cat\n  section Go home\n    Sit down: 4.5: Me\n  accTitle: Day",
            "pie showData title Pets 🐶\n  \"Dogs\" : 386\n  \"Cats\" : 85.5\n  \"Rats\" : .5\n  \"日本\" : 0",
            "gantt\n  dateFormat YYYY-MM-DD\n  title Plan 🙂\n  section A\n  One : done, a1, 2024-01-01, 3d\n  Two : after a1, 12h\n  section 日本\n  Three : until m1\n  Ship : milestone, m1, 2024-01-09, 0d",
            "xychart-beta horizontal\n  title \"Sales 🙂\"\n  x-axis \"Month\" [jan, \"feb x\", mar]\n  y-axis \"Revenue\" -40 --> 110.5\n  bar \"rain\" [5, -6, 7.5]\n  line [+1, .5, 99]",
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

    /// Sources just under the limit, dense with links, loops, nesting and
    /// wide characters, of each kind that is a graph: each is drawn within
    /// the canvas's limit or refused, and none takes long, as the run time
    /// of this test shows.
    #[test]
    fn huge_tangled_sources_of_every_kind_stay_bounded() {
        let mut seed = 7u64;
        let mut next = |below: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % below
        };
        let mut sources = Vec::new();
        let mut c4 = String::from("C4Container\n");
        for at in 0..16 {
            c4.push_str(&format!("Boundary(b{at}, \"境界 {at}\") {{\n"));
        }
        for at in 0..60 {
            c4.push_str(&format!(
                "Container(c{at}, \"コンテナ {at}\", \"Go\", \"Does {at}\")\n"
            ));
        }
        c4.push_str(&"}\n".repeat(16));
        let mut class = String::from("classDiagram\n");
        let mut state = String::from("stateDiagram-v2\n");
        let mut er = String::from("erDiagram\n");
        for _ in 0..400 {
            let (a, b) = (next(60), next(60));
            c4.push_str(&format!("Rel(c{a}, c{b}, \"r\")\n"));
            class.push_str(&format!("K{a} <|-- K{b} : x\n"));
            state.push_str(&format!("S{a} --> S{b} : e\n"));
            er.push_str(&format!("E{a} ||--o{{ E{b} : has\n"));
        }
        for source in [c4, class, state, er] {
            let mut cut = String::new();
            for line in source.lines() {
                if cut.len() + line.len() + 1 >= MAX_SOURCE {
                    break;
                }
                cut.push_str(line);
                cut.push('\n');
            }
            sources.push(cut);
        }
        for source in &sources {
            for width in [1, 80, 400] {
                if let Some(drawn) = draw(&lines(source), width) {
                    assert!(drawn.rows.len() * drawn.width <= canvas::MAX_CELLS);
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

    /// The colour each node or participant is given out of `hues`.
    fn colours_of(source: &str, width: usize, hues: usize) -> Vec<u8> {
        draw(&lines(source), width).expect("draws").colours(hues)
    }

    #[test]
    fn colours_are_the_same_every_time_and_whichever_way_the_chart_runs() {
        let chart = "graph LR\n  A[Start] --> B{Ready?}\n  B -->|yes| C[Go]\n  B -->|no| D[Wait]\n  D --> B";
        let first = colours_of(chart, 200, 8);
        assert_eq!(first.len(), 4);
        assert_eq!(colours_of(chart, 200, 8), first, "drawn again");
        // Too narrow to run sideways, so it is drawn downwards instead, and
        // its nodes keep their colours.
        assert_eq!(colours_of(chart, 20, 8), first);
        let renamed = chart.replace("Start", "Begin");
        assert_eq!(colours_of(&renamed, 200, 8), first, "a node is its id");

        // The key is spelled out so it can't drift between builds or
        // machines, as `std`'s hasher may.
        assert_eq!(key(""), 0xda4a_6948_0291_1fc9);
        assert_eq!(key("A"), 0x8d63_b02d_cc18_cd78);
    }

    #[test]
    fn the_ids_most_diagrams_use_start_out_apart() {
        for hues in [6, 7, 8] {
            let picked = colours_of("graph TD\n  A --> B --> C --> D", 80, hues);
            assert_eq!(picked[0], 0, "A is in the first colour of {hues}");
            let mut seen = picked.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), 4, "{hues}: {picked:?}");
        }
        // The last two of a theme's eight hues are its purple and red.
        let eight = colours_of("graph TD\n  A --> B --> C --> D --> E", 80, 8);
        assert!(eight.iter().all(|&hue| hue < 6), "{eight:?}");
    }

    #[test]
    fn a_node_keeps_its_colour_when_another_is_added() {
        let before = colours_of("graph TD\n  A --> B --> C\n  B --> D", 80, 7);
        let after = colours_of("graph TD\n  A --> B --> C\n  B --> D\n  D --> E", 80, 7);
        assert_eq!(after[..4], before[..], "the first four are as they were");
    }

    #[test]
    fn no_two_alike_until_the_colours_run_out_then_never_neighbours() {
        let few = colours_of("graph TD\n  A --> B & C & D\n  C --> E", 80, 6);
        let mut seen = few.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), few.len(), "five nodes, five colours: {few:?}");

        // A chain of twelve in four colours: some share, but never two that
        // are joined.
        let chain: Vec<String> = (0..12).map(|at| format!("n{at}")).collect();
        let source = format!("graph TD\n  {}", chain.join(" --> "));
        let many = colours_of(&source, 400, 4);
        assert!(many.windows(2).all(|pair| pair[0] != pair[1]), "{many:?}");
        assert!(many.iter().all(|&hue| hue < 4));

        let sequence = "sequenceDiagram\n  A->>B: one\n  B->>C: two\n  C->>A: three";
        let people = colours_of(sequence, 80, 6);
        assert_eq!(people.len(), 3);
        assert!(people[0] != people[1] && people[1] != people[2] && people[0] != people[2]);
        assert!(colours_of(sequence, 80, 0).is_empty(), "no colours to give");
    }

    #[test]
    fn big_diagrams_are_dropped_from_the_cache_to_keep_it_small() {
        let mut cache = Cache::default();
        let big = Rc::new(Diagram {
            rows: vec![Vec::new(); 1000],
            width: 400,
            keys: Vec::new(),
            links: Vec::new(),
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
