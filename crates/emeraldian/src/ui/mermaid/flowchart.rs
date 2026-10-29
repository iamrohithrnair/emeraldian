//! Flowcharts: Mermaid's `graph` and `flowchart`.
//!
//! Laid out by [`super::layered`], which also lays out the other diagrams
//! that are graphs. Each node is a box in the shape its brackets ask for,
//! and each edge runs from the box it leaves, in that box's colour, with an
//! arrowhead where Mermaid draws one.

use std::collections::HashMap;

use super::canvas::{Canvas, Stroke};
use super::label::{self, widest};
use super::layered::{self, Direction, Graph};
use super::{Diagram, Ink, Role, key, syntax};

/// Past these a chart is too big to read in a terminal, and the layout would
/// do a lot of work to show it badly.
const MAX_NODES: usize = 300;
const MAX_EDGES: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Rect,
    Round,
    Subroutine,
    Diamond,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub label: Vec<String>,
    pub shape: Shape,
    /// Whether an edge leads from this node back to itself.
    pub looped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Empty when the edge has no words.
    pub label: Vec<String>,
    pub stroke: Stroke,
    /// An arrowhead at the `to` end.
    pub head: bool,
    /// An arrowhead at the `from` end too, as in `<-->`.
    pub tail: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

// ---------------------------------------------------------------- parsing

/// Reads a chart from what follows `graph` or `flowchart` on its first line
/// and the lines after it.
pub fn parse(header: &str, lines: &[&str]) -> Option<Chart> {
    let (direction, first) = match header.split_once(';') {
        Some((direction, rest)) => (direction, Some(rest)),
        None => (header, None),
    };
    let direction = match direction.trim() {
        "" => Direction::Down,
        named => syntax::direction(named)?,
    };
    let mut builder = Builder {
        chart: Chart {
            direction,
            nodes: Vec::new(),
            edges: Vec::new(),
        },
        ids: HashMap::new(),
    };
    for line in first.into_iter().chain(lines.iter().copied()) {
        for statement in statements(line) {
            builder.statement(statement)?;
        }
    }
    let chart = builder.chart;
    if chart.nodes.is_empty() {
        return None;
    }
    Some(chart)
}

/// Splits a line at `;`, except inside quotes or brackets, where it is part
/// of a label.
fn statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut start = 0;
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => depth += 1,
            ']' | ')' | '}' if !quoted => depth -= 1,
            ';' if !quoted && depth <= 0 => {
                out.push(&line[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

struct Builder {
    chart: Chart,
    ids: HashMap<String, usize>,
}

/// Statements about how things look, or how the chart is grouped, which a
/// text drawing has no use for.
const IGNORED: &[&str] = &[
    "classDef",
    "class",
    "style",
    "linkStyle",
    "click",
    "direction",
    "subgraph",
    "end",
    "accTitle",
    "accDescr",
    "title",
];

impl Builder {
    fn statement(&mut self, statement: &str) -> Option<()> {
        if IGNORED.contains(&syntax::first_word(statement)) {
            return Some(());
        }
        let mut scan = Scan {
            text: statement,
            at: 0,
        };
        let mut previous = self.group(&mut scan)?;
        loop {
            scan.skip_space();
            if scan.done() {
                return Some(());
            }
            let link = link(&mut scan)?;
            scan.skip_space();
            let next = self.group(&mut scan)?;
            if let Some(link) = link {
                for &from in &previous {
                    for &to in &next {
                        if from == to {
                            self.chart.nodes[from].looped = true;
                            continue;
                        }
                        if self.chart.edges.len() >= MAX_EDGES {
                            return None;
                        }
                        self.chart.edges.push(Edge {
                            from,
                            to,
                            label: link.label.clone(),
                            stroke: link.stroke,
                            head: link.head,
                            tail: link.tail,
                        });
                    }
                }
            }
            previous = next;
        }
    }

    /// One node, or several joined by `&`.
    fn group(&mut self, scan: &mut Scan) -> Option<Vec<usize>> {
        let mut nodes = vec![self.node(scan)?];
        loop {
            let at = scan.at;
            scan.skip_space();
            if scan.eat("&") {
                scan.skip_space();
                nodes.push(self.node(scan)?);
            } else {
                scan.at = at;
                return Some(nodes);
            }
        }
    }

    fn node(&mut self, scan: &mut Scan) -> Option<usize> {
        let id = scan.id();
        if id.is_empty() {
            return None;
        }
        let shaped = shape(scan)?;
        // Classes and the newer `@{ … }` options only change how a node
        // looks in a browser.
        if scan.eat(":::") {
            scan.id();
        }
        if scan.eat("@{") {
            let close = scan.rest().find('}')?;
            scan.at += close + 1;
        }
        let at = syntax::named(&mut self.ids, &mut self.chart.nodes, id, MAX_NODES, || {
            Node {
                id: id.to_string(),
                label: label::lines(id),
                shape: Shape::Rect,
                looped: false,
            }
        })?;
        if let Some((label, shape)) = shaped {
            self.chart.nodes[at].label = label::lines(&label);
            self.chart.nodes[at].shape = shape;
        }
        Some(at)
    }
}

struct Scan<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scan<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn done(&self) -> bool {
        self.at >= self.text.len()
    }

    fn skip_space(&mut self) {
        let rest = self.rest();
        self.at += rest.len() - rest.trim_start().len();
    }

    fn eat(&mut self, token: &str) -> bool {
        if self.rest().starts_with(token) {
            self.at += token.len();
            true
        } else {
            false
        }
    }

    /// A node's id: everything up to a space, a bracket or the start of a
    /// link.
    fn id(&mut self) -> &'a str {
        let start = self.at;
        let rest = self.rest();
        for (at, c) in rest.char_indices() {
            let tail = &rest[at..];
            if c.is_whitespace()
                || "[](){}<>|&;\"".contains(c)
                || tail.starts_with("--")
                || tail.starts_with("==")
                || tail.starts_with("-.")
                || tail.starts_with("~~~")
                || tail.starts_with(":::")
                || tail.starts_with("@{")
            {
                self.at = start + at;
                return &self.text[start..self.at];
            }
        }
        self.at = self.text.len();
        &self.text[start..]
    }
}

/// Mermaid's node brackets and the shape each is drawn as, longest first so
/// `((` is not read as `(`.
const SHAPES: &[(&str, &str, Shape)] = &[
    ("(((", ")))", Shape::Round),
    ("([", "])", Shape::Round),
    ("((", "))", Shape::Round),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Round),
    ("[/", "/]", Shape::Rect),
    ("[/", "\\]", Shape::Rect),
    ("[\\", "\\]", Shape::Rect),
    ("[\\", "/]", Shape::Rect),
    ("{{", "}}", Shape::Diamond),
    ("(", ")", Shape::Round),
    ("[", "]", Shape::Rect),
    ("{", "}", Shape::Diamond),
    (">", "]", Shape::Rect),
];

/// The bracketed label after a node's id, if it has one. `None` when the
/// brackets don't close.
#[allow(clippy::option_option)]
fn shape(scan: &mut Scan) -> Option<Option<(String, Shape)>> {
    let rest = scan.rest();
    for &(open, close, shape) in SHAPES {
        let Some(inside) = rest.strip_prefix(open) else {
            continue;
        };
        // A quoted label may hold the closing bracket.
        let skip = inside
            .strip_prefix('"')
            .and_then(|quoted| quoted.find('"'))
            .map_or(0, |end| end + 2);
        if let Some(end) = inside[skip..].find(close) {
            let end = skip + end;
            scan.at += open.len() + end + close.len();
            return Some(Some((inside[..end].to_string(), shape)));
        }
    }
    if rest.starts_with(['[', '(', '{', '>']) {
        return None;
    }
    Some(None)
}

struct Link {
    label: Vec<String>,
    stroke: Stroke,
    head: bool,
    tail: bool,
}

/// An edge between two nodes: `-->`, `---`, `-.->`, `==>`, `<-->`, `--x`,
/// with its words written `-->|words|` or `-- words -->`. `Some(None)` is the
/// invisible `~~~`, which joins nothing.
#[allow(clippy::option_option)]
fn link(scan: &mut Scan) -> Option<Option<Link>> {
    if scan.eat("~~~") {
        while scan.eat("~") {}
        return Some(None);
    }
    let tail = scan.eat("<");
    let body = body(scan);
    if !body.starts_with(['-', '=']) {
        return None;
    }
    let mut head = arrowhead(scan);
    let mut stroke = stroke_of(body);
    let mut label = Vec::new();
    // `-- words -->`: the words sit inside the link itself.
    if !head && matches!(body, "--" | "==" | "-.") && scan.rest().starts_with(char::is_whitespace) {
        let close = if body == "-." { ".-" } else { body };
        let end = scan.rest().find(close)?;
        label = label::lines(&scan.rest()[..end]);
        scan.at += end;
        let closing = self::body(scan);
        stroke = stroke_of(closing);
        head = arrowhead(scan);
    } else if body.len() < 2 && !head {
        return None;
    }
    let at = scan.at;
    scan.skip_space();
    if scan.eat("|") {
        let end = scan.rest().find('|')?;
        label = label::lines(&scan.rest()[..end]);
        scan.at += end + 1;
    } else {
        scan.at = at;
    }
    if label.iter().all(String::is_empty) {
        label.clear();
    }
    Some(Some(Link {
        label,
        stroke,
        head,
        tail: tail && head,
    }))
}

fn body<'a>(scan: &mut Scan<'a>) -> &'a str {
    let rest = scan.rest();
    let len = rest.len() - rest.trim_start_matches(['-', '.', '=']).len();
    scan.at += len;
    &rest[..len]
}

/// Eats an arrowhead: `>`, or the `x` and `o` ends when a space follows.
fn arrowhead(scan: &mut Scan) -> bool {
    if scan.eat(">") {
        return true;
    }
    let rest = scan.rest();
    if rest.starts_with(['x', 'o'])
        && rest[1..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || c == '|')
    {
        scan.at += 1;
        return true;
    }
    false
}

fn stroke_of(body: &str) -> Stroke {
    if body.contains('=') {
        Stroke::Thick
    } else if body.contains('.') {
        Stroke::Dotted
    } else {
        Stroke::Solid
    }
}

// ----------------------------------------------------------------- layout

/// Draws a chart, or `None` when it is too big. A sideways chart too wide
/// for `width` is drawn downwards instead when that is narrower.
pub fn draw(chart: &Chart, width: usize) -> Option<Diagram> {
    let drawn = layout(chart, chart.direction)?;
    if chart.direction.across()
        && drawn.width > width
        && let Some(down) = layout(chart, Direction::Down)
        && down.width < drawn.width
    {
        return Some(down);
    }
    Some(drawn)
}

/// The lines of a node's label, with a mark for an edge to itself.
fn node_label(node: &Node) -> Vec<String> {
    let mut label = node.label.clone();
    if node.looped
        && let Some(first) = label.first_mut()
    {
        first.push_str(" ↻");
    }
    label
}

fn layout(chart: &Chart, direction: Direction) -> Option<Diagram> {
    let labels: Vec<Vec<String>> = chart.nodes.iter().map(node_label).collect();
    let mut graph = Graph::new(direction);
    graph.boxes = labels
        .iter()
        .map(|label| (widest(label) + 4, label.len() + 2))
        .collect();
    graph.links = chart
        .edges
        .iter()
        .map(|edge| layered::Link {
            from: edge.from,
            to: edge.to,
            words: (!edge.label.is_empty()).then(|| (widest(&edge.label), edge.label.len())),
        })
        .collect();
    let placed = layered::layout(&graph)?;
    let mut canvas = Canvas::new(placed.width, placed.height)?;

    // Boxes and words first, so lines stop at them.
    for (at, rect) in placed.boxes.iter().enumerate() {
        let shape = chart.nodes[at].shape;
        let corners = match shape {
            Shape::Rect => ['┌', '┐', '└', '┘'],
            Shape::Round => ['╭', '╮', '╰', '╯'],
            Shape::Subroutine => ['╔', '╗', '╚', '╝'],
            Shape::Diamond => ['╱', '╲', '╲', '╱'],
        };
        let double = shape == Shape::Subroutine;
        let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
        canvas.outline(x, y, w, h, corners, double, Ink::of(Role::Border, at));
        let words = Ink::plain(Role::Text);
        label::centred(&mut canvas, x + 1, y + 1, w - 2, &labels[at], words);
    }
    for (edge, rect) in chart.edges.iter().zip(&placed.words) {
        if let Some(rect) = rect {
            let ink = Ink::of(Role::Label, edge.from);
            label::centred(&mut canvas, rect.x, rect.y, rect.width, &edge.label, ink);
        }
    }

    // Each edge is drawn in the colour of the node it leaves, which its
    // arrowhead carries to the node it reaches.
    placed.draw_lines(&mut canvas, |edge| {
        let edge = &chart.edges[edge];
        (edge.stroke, Ink::of(Role::Line, edge.from).owner)
    });
    for end in &placed.ends {
        let edge = &chart.edges[end.link];
        let double = chart.nodes[end.node].shape == Shape::Subroutine;
        let arrow = if end.to { edge.head } else { edge.tail };
        let head = Ink::of(Role::Arrow, edge.from);
        // Where an edge meets a box, the join is part of the box's outline.
        let join = Ink::of(Role::Border, end.node);
        if end.leaving {
            if !double {
                canvas.put(end.border.0, end.border.1, end.facing.join(), join);
            }
            if arrow {
                canvas.put(end.outside.0, end.outside.1, end.facing.arrow(), head);
            }
        } else if arrow {
            canvas.put(end.outside.0, end.outside.1, end.facing.arrow(), head);
        } else if !double {
            canvas.put(end.border.0, end.border.1, end.facing.join(), join);
        }
    }
    let mut diagram = canvas.finish();
    diagram.keys = chart.nodes.iter().map(|node| key(&node.id)).collect();
    diagram.links = chart
        .edges
        .iter()
        .filter_map(|edge| Some((u16::try_from(edge.from).ok()?, u16::try_from(edge.to).ok()?)))
        .collect();
    Some(diagram)
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::super::plain;
    use super::*;

    fn chart(source: &str) -> Option<Chart> {
        let mut lines = source.lines();
        let header = lines.next()?.trim();
        let header = header
            .strip_prefix("flowchart")
            .or_else(|| header.strip_prefix("graph"))?;
        let rest: Vec<&str> = lines.collect();
        parse(header, &rest)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&chart(source).expect("parses"), width).expect("draws"))
    }

    #[test]
    fn nodes_shapes_and_labels_are_read() {
        let chart = chart(
            "graph LR\n  A[Start] --> B(Round)\n  B --> C{Choice?}\n  C -->|yes| D[[Sub]]\n  C -- no --> E((Circle))",
        )
        .unwrap();
        assert_eq!(chart.direction, Direction::Right);
        let names: Vec<&str> = chart.nodes.iter().map(|n| n.label[0].as_str()).collect();
        assert_eq!(names, ["Start", "Round", "Choice?", "Sub", "Circle"]);
        let shapes: Vec<Shape> = chart.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(
            shapes,
            [
                Shape::Rect,
                Shape::Round,
                Shape::Diamond,
                Shape::Subroutine,
                Shape::Round
            ]
        );
        assert_eq!(chart.edges.len(), 4);
        assert_eq!(chart.edges[2].label, ["yes"]);
        assert_eq!(chart.edges[3].label, ["no"]);
        assert!(chart.edges.iter().all(|e| e.head));
    }

    #[test]
    fn links_of_every_kind_are_read() {
        let chart = chart(
            "flowchart TD\n  a --- b\n  b -.-> c\n  c ==> d\n  d <--> e\n  e --x f\n  f -. maybe .-> g\n  g == strong ==> h\n  h ~~~ i\n  i --> |spaced| j",
        )
        .unwrap();
        let e = &chart.edges;
        assert_eq!(e.len(), 8, "the invisible link joins nothing");
        assert!(!e[0].head);
        assert_eq!(e[1].stroke, Stroke::Dotted);
        assert_eq!(e[2].stroke, Stroke::Thick);
        assert!(e[3].head && e[3].tail);
        assert!(e[4].head);
        assert_eq!(
            (e[5].stroke, e[5].label.as_slice()),
            (Stroke::Dotted, &["maybe".to_string()][..])
        );
        assert_eq!(
            (e[6].stroke, e[6].label.as_slice()),
            (Stroke::Thick, &["strong".to_string()][..])
        );
        assert_eq!(e[7].label, ["spaced"]);
    }

    #[test]
    fn groups_semicolons_and_styling_are_handled() {
        let chart = chart(
            "graph TD; A & B --> C;\n  classDef red fill:#f00\n  class A red\n  style B fill:#0f0\n  subgraph one [The group]\n    C --> D:::red\n  end\n  click D callback",
        )
        .unwrap();
        assert_eq!(chart.nodes.len(), 4);
        assert_eq!(chart.edges.len(), 3);
    }

    #[test]
    fn quoted_labels_may_hold_brackets() {
        let chart = chart("graph TD\n  A[\"a [b] c\"] --> B").unwrap();
        assert_eq!(chart.nodes[0].label, ["a [b] c"]);
    }

    #[test]
    fn broken_charts_are_not_drawn() {
        for source in [
            "graph XY\n  A --> B",
            "graph TD\n  A[unclosed --> B",
            "graph TD\n  A -->",
            "graph TD\n  --> B",
            "graph TD\n  A - B",
            "graph TD\n  A -- words with no end B",
            "graph TD\n  A -->|unclosed B",
            "graph TD",
        ] {
            assert!(chart(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_simple_chart_draws_downwards() {
        let out = drawn("graph TD\n  A[Start] --> B[End]", 80);
        let expected = "\
┌───────┐
│ Start │
└───┬───┘
    │
    ▼
 ┌─────┐
 │ End │
 └─────┘";
        assert_eq!(out, expected);
    }

    #[test]
    fn a_simple_chart_draws_sideways() {
        let out = drawn("graph LR\n  A --> B", 80);
        assert_eq!(out, "┌───┐   ┌───┐\n│ A ├──▶│ B │\n└───┘   └───┘");
    }

    #[test]
    fn upwards_and_leftwards_charts_are_mirrored() {
        let up = drawn("graph BT\n  A --> B", 80);
        assert_eq!(up.lines().next(), Some("┌───┐"));
        assert!(
            up.lines().nth(1).unwrap().contains('B'),
            "B is on top:\n{up}"
        );
        assert!(up.contains('▲'));
        let left = drawn("graph RL\n  A --> B", 80);
        assert!(left.find('B') < left.find('A'), "B is on the left:\n{left}");
        assert!(left.contains('◀'));
    }

    #[test]
    fn edge_words_sit_between_the_nodes() {
        let out = drawn("graph TD\n  A -->|yes| B", 80);
        let rows: Vec<&str> = out.lines().collect();
        let words = rows.iter().position(|r| r.contains("yes")).unwrap();
        let a = rows.iter().position(|r| r.contains('A')).unwrap();
        let b = rows.iter().position(|r| r.contains('B')).unwrap();
        assert!(a < words && words < b, "{out}");
    }

    #[test]
    fn a_branch_fans_out_without_overlapping() {
        let out = drawn("graph TD\n  A --> B\n  A --> C\n  A --> D", 80);
        let row = out.lines().find(|r| r.contains('B')).unwrap();
        assert!(row.contains('C') && row.contains('D'), "{out}");
        assert_eq!(out.matches('▼').count(), 3, "{out}");
    }

    #[test]
    fn cycles_and_self_loops_are_drawn() {
        let out = drawn("graph TD\n  A --> B --> C --> A\n  B --> B", 80);
        assert!(out.contains('↻'), "{out}");
        assert!(out.contains('▲'), "the edge back up points up:\n{out}");
        for name in ["A", "B", "C"] {
            assert!(out.contains(name));
        }
    }

    #[test]
    fn unicode_labels_keep_their_columns() {
        let out = drawn("graph LR\n  A[日本語] --> B[🙂 ok] --> C[café]", 200);
        for row in out.lines() {
            assert!(row.width() <= 200);
        }
        assert!(out.contains("日本語") && out.contains("café"));
        let top = out.lines().next().unwrap().width();
        let bottom = out.lines().last().unwrap().width();
        assert_eq!(top, bottom, "boxes close where they open:\n{out}");
    }

    #[test]
    fn a_wide_sideways_chart_turns_downwards_to_fit() {
        let source = "graph LR\n  A --> B --> C --> D --> E --> F --> G --> H";
        let narrow = draw(&chart(source).unwrap(), 30).unwrap();
        let wide = draw(&chart(source).unwrap(), 500).unwrap();
        assert!(narrow.width < wide.width);
        assert!(narrow.rows.len() > wide.rows.len());
    }

    #[test]
    fn huge_and_dense_charts_stay_bounded() {
        // Every node joined to every other: a lot of edges in few lines.
        let mut source = String::from("graph TD\n");
        for a in 0..30 {
            source.push_str(&format!("  n{a} --> "));
            let all: Vec<String> = (0..30).map(|b| format!("n{b}")).collect();
            source.push_str(&all.join(" & "));
            source.push('\n');
        }
        // Too many edges is refused rather than drawn into a smear.
        assert!(chart(&source).is_none());

        let mut long = String::from("graph TD\n");
        for at in 0..250 {
            long.push_str(&format!("n{at}-->n{}\n", at + 1));
        }
        let parsed = chart(&long).unwrap();
        if let Some(diagram) = draw(&parsed, 80) {
            assert!(diagram.rows.len() * diagram.width.max(1) <= super::super::canvas::MAX_CELLS);
        }

        let mut too_many = String::from("graph TD\n");
        for at in 0..400 {
            too_many.push_str(&format!("n{at}\n"));
        }
        assert!(chart(&too_many).is_none());
    }

    #[test]
    fn a_long_edge_bends_round_what_is_between() {
        let out = drawn("graph TD\n  A --> B --> C --> D\n  A --> D", 80);
        assert_eq!(out.matches('▼').count(), 4, "{out}");
    }

    #[test]
    fn a_box_is_its_nodes_and_an_edge_its_sources() {
        let diagram = draw(&chart("graph LR\n  A -->|go| B").unwrap(), 80).unwrap();
        let ink_of = |needle: char| {
            diagram
                .rows
                .iter()
                .flatten()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, ink)| *ink)
        };
        assert_eq!(ink_of('▶'), Some(Ink::of(Role::Arrow, 0)));
        assert_eq!(ink_of('g'), Some(Ink::of(Role::Label, 0)));
        assert_eq!(
            ink_of('A'),
            Some(Ink::plain(Role::Text)),
            "words stay plain"
        );
        let line = diagram
            .rows
            .iter()
            .flatten()
            .find(|(_, ink)| ink.role == Role::Line);
        assert_eq!(line.map(|(_, ink)| ink.owner), Some(Some(0)));
        let top = diagram.rows[0].last().map(|(_, ink)| *ink);
        assert_eq!(top, Some(Ink::of(Role::Border, 1)), "B's outline");
        assert_eq!(diagram.keys, [key("A"), key("B")]);
        assert_eq!(diagram.links, [(0, 1)]);
    }

    #[test]
    fn many_edges_into_one_node_each_get_an_arrow_or_share_one() {
        let out = drawn("graph TD\n  A & B & C & D --> E", 80);
        assert!(out.matches('▼').count() >= 1);
        let e_row = out.lines().position(|r| r.contains('E')).unwrap();
        assert_eq!(e_row, out.lines().count() - 2, "{out}");
    }
}
