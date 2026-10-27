//! Flowcharts: Mermaid's `graph` and `flowchart`.
//!
//! Laid out the way most graph drawing tools lay out a flow, in four steps.
//! Edges that close a loop are turned round, so the graph flows one way.
//! Each node is given a rank, its distance along the flow, and an edge that
//! spans several ranks is broken into one bend per rank it passes. The nodes
//! in each rank are put in the order that crosses fewest edges, by moving
//! each towards the average place of its neighbours a few times over. Then
//! each is slid sideways to sit near those neighbours without overlapping.
//!
//! Edges run straight down the gaps between ranks, turning once in a lane
//! of their own when they have to move sideways, so two edges never share a
//! stretch of line. The words on an edge take a place in a rank of their
//! own, which keeps them clear of everything else.
//!
//! Everything is worked out along two axes, the way the chart flows and
//! across it, and only turned into rows and columns at the end, so one
//! layout serves all four directions.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::{Diagram, Ink, Role, key, label_lines, widest};

/// Past these a chart is too big to read in a terminal, and the layout would
/// do a lot of work to show it badly.
const MAX_NODES: usize = 300;
const MAX_EDGES: usize = 600;
/// Nodes, bends and edge words together.
const MAX_PLACES: usize = 4000;
/// Rounds of reordering. The improvement after a handful is small.
const SWEEPS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    fn across(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

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
        "" | "TD" | "TB" => Direction::Down,
        "BT" => Direction::Up,
        "LR" => Direction::Right,
        "RL" => Direction::Left,
        _ => return None,
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
        let first = statement
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("");
        if IGNORED.contains(&first) {
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
        let at = match self.ids.get(id) {
            Some(&at) => at,
            None => {
                if self.chart.nodes.len() >= MAX_NODES {
                    return None;
                }
                self.chart.nodes.push(Node {
                    id: id.to_string(),
                    label: label_lines(id),
                    shape: Shape::Rect,
                    looped: false,
                });
                self.ids.insert(id.to_string(), self.chart.nodes.len() - 1);
                self.chart.nodes.len() - 1
            }
        };
        if let Some((label, shape)) = shaped {
            self.chart.nodes[at].label = label_lines(&label);
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
        label = label_lines(&scan.rest()[..end]);
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
        label = label_lines(&scan.rest()[..end]);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Node(usize),
    /// Where a long edge passes through a rank.
    Bend,
    /// The words of an edge.
    Words(usize),
}

/// Something that takes room in a rank.
#[derive(Debug, Clone)]
struct Place {
    kind: Kind,
    rank: usize,
    /// Its extent along the flow and across it.
    along: usize,
    across: usize,
    /// Where it starts across the flow.
    at: i64,
}

impl Place {
    fn centre(&self) -> i64 {
        self.at + self.across as i64 / 2
    }
}

/// One rank's worth of an edge, from a place to one in the next rank.
#[derive(Debug, Clone)]
struct Segment {
    upper: usize,
    lower: usize,
    edge: usize,
    /// Where it leaves the upper place and meets the lower one, across the
    /// flow.
    from: i64,
    to: i64,
    /// Which lane of the gap it turns in, if it turns.
    lane: usize,
}

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

#[allow(clippy::too_many_lines)]
fn layout(chart: &Chart, direction: Direction) -> Option<Diagram> {
    let across_flow = direction.across();
    let labels: Vec<Vec<String>> = chart.nodes.iter().map(node_label).collect();

    // Turn round the edges that close a loop.
    let reversed = back_edges(chart);
    let ends = |edge: usize| {
        let e = &chart.edges[edge];
        if reversed[edge] {
            (e.to, e.from)
        } else {
            (e.from, e.to)
        }
    };
    let rank = ranks(chart, &reversed);

    // Places: the nodes, then the bends and words of each edge.
    let mut places: Vec<Place> = chart
        .nodes
        .iter()
        .enumerate()
        .map(|(at, _)| {
            let (columns, rows) = (widest(&labels[at]) + 4, labels[at].len() + 2);
            let (along, across) = if across_flow {
                (columns, rows)
            } else {
                (rows, columns)
            };
            Place {
                kind: Kind::Node(at),
                rank: rank[at],
                along,
                across,
                at: 0,
            }
        })
        .collect();
    let mut segments = Vec::new();
    for edge in 0..chart.edges.len() {
        let (top, bottom) = ends(edge);
        let words = &chart.edges[edge].label;
        let middle = rank[top] + (rank[bottom] - rank[top]) / 2;
        let mut previous = top;
        for r in rank[top] + 1..rank[bottom] {
            let place = if !words.is_empty() && r == middle {
                let (columns, rows) = (widest(words).max(1), words.len());
                let (along, across) = if across_flow {
                    (columns, rows)
                } else {
                    (rows, columns)
                };
                Place {
                    kind: Kind::Words(edge),
                    rank: r,
                    along,
                    across,
                    at: 0,
                }
            } else {
                Place {
                    kind: Kind::Bend,
                    rank: r,
                    along: 1,
                    across: 1,
                    at: 0,
                }
            };
            places.push(place);
            if places.len() > MAX_PLACES {
                return None;
            }
            segments.push(segment(previous, places.len() - 1, edge));
            previous = places.len() - 1;
        }
        segments.push(segment(previous, bottom, edge));
    }

    // A box going down the page widens to give each of its edges a column
    // of its own, rather than several sharing one and merging.
    if !across_flow {
        let mut ends = vec![(0usize, 0usize); chart.nodes.len()];
        for s in &segments {
            if let Kind::Node(at) = places[s.upper].kind {
                ends[at].0 += 1;
            }
            if let Kind::Node(at) = places[s.lower].kind {
                ends[at].1 += 1;
            }
        }
        for (at, (out, into)) in ends.into_iter().enumerate() {
            places[at].across = places[at].across.max(2 * out.max(into) + 1);
        }
    }

    let ranks_count = places.iter().map(|p| p.rank).max().unwrap_or(0) + 1;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); ranks_count];
    for (at, place) in places.iter().enumerate() {
        layers[place.rank].push(at);
    }
    let mut above: Vec<Vec<usize>> = vec![Vec::new(); places.len()];
    let mut below: Vec<Vec<usize>> = vec![Vec::new(); places.len()];
    for s in &segments {
        below[s.upper].push(s.lower);
        above[s.lower].push(s.upper);
    }

    order(&mut layers, &above, &below, places.len());
    position(&mut places, &layers, &above, &below, across_flow);
    let lanes = route(&places, &mut segments, &layers);

    // Along the flow: each rank as deep as its deepest place, and each gap
    // deep enough for its lanes.
    let depth: Vec<usize> = layers
        .iter()
        .map(|layer| layer.iter().map(|&p| places[p].along).max().unwrap_or(1))
        .collect();
    let least_gap = if across_flow { 3 } else { 2 };
    let mut start = vec![0usize; ranks_count];
    for r in 1..ranks_count {
        let gap = (lanes[r - 1] + 2).max(least_gap);
        start[r] = start[r - 1] + depth[r - 1] + gap;
    }
    let total_along = start[ranks_count - 1] + depth[ranks_count - 1];
    let total_across = places
        .iter()
        .map(|p| p.at + p.across as i64)
        .max()
        .unwrap_or(1);
    let total_across = usize::try_from(total_across).ok()?;
    // Where each place begins along the flow: centred in its rank, except
    // bends, which run the whole depth of it.
    let first = |p: &Place| match p.kind {
        Kind::Bend => start[p.rank],
        _ => start[p.rank] + (depth[p.rank] - p.along) / 2,
    };
    let last = |p: &Place| match p.kind {
        Kind::Bend => start[p.rank] + depth[p.rank] - 1,
        _ => first(p) + p.along - 1,
    };

    let view = View {
        direction,
        along: total_along,
    };
    let (columns, rows) = if across_flow {
        (total_along, total_across)
    } else {
        (total_across, total_along)
    };
    let mut canvas = Canvas::new(columns, rows)?;

    // Boxes and words first, so lines stop at them.
    for place in &places {
        let across = usize::try_from(place.at).ok()?;
        let (x, y, w, h) = view.rect(first(place), place.along, across, place.across);
        match place.kind {
            Kind::Node(at) => {
                let shape = chart.nodes[at].shape;
                let corners = match shape {
                    Shape::Rect => ['┌', '┐', '└', '┘'],
                    Shape::Round => ['╭', '╮', '╰', '╯'],
                    Shape::Subroutine => ['╔', '╗', '╚', '╝'],
                    Shape::Diamond => ['╱', '╲', '╲', '╱'],
                };
                let double = shape == Shape::Subroutine;
                canvas.outline(x, y, w, h, corners, double, Ink::of(Role::Border, at));
                let words = Ink::plain(Role::Text);
                centred(&mut canvas, x + 1, y + 1, w - 2, &labels[at], words);
            }
            Kind::Words(edge) => {
                let edge = &chart.edges[edge];
                let ink = Ink::of(Role::Label, edge.from);
                centred(&mut canvas, x, y, w, &edge.label, ink);
            }
            Kind::Bend => {}
        }
    }

    // Each edge is drawn in the colour of the node it leaves, which its
    // arrowhead carries to the node it reaches.
    for s in &segments {
        let (upper, lower) = (&places[s.upper], &places[s.lower]);
        let edge = &chart.edges[s.edge];
        let owner = Ink::of(Role::Line, edge.from).owner;
        let (from, to) = (s.from as usize, s.to as usize);
        let leave = last(upper);
        let arrive = first(lower);
        let gap = start[upper.rank] + depth[upper.rank];
        let mut points = vec![(leave, from)];
        if from != to {
            let lane = gap + 1 + s.lane;
            points.push((lane, from));
            points.push((lane, to));
        }
        points.push((arrive, to));
        let screen: Vec<(usize, usize)> = points.iter().map(|&(a, c)| view.point(a, c)).collect();
        canvas.path(&screen, edge.stroke, owner);
        if upper.kind == Kind::Bend {
            canvas.line(
                view.point(first(upper), from),
                view.point(leave, from),
                edge.stroke,
                owner,
            );
        }

        let forward = !reversed[s.edge];
        let head_below = if forward { edge.head } else { edge.tail };
        let head_above = if forward { edge.tail } else { edge.head };
        let double = |place: &Place| matches!(place.kind, Kind::Node(at) if chart.nodes[at].shape == Shape::Subroutine);
        let arrow = Ink::of(Role::Arrow, edge.from);
        // Where an edge meets a box, the join is part of the box's outline.
        if let Kind::Node(at) = upper.kind {
            if !double(upper) {
                let (x, y) = view.point(leave, from);
                canvas.put(x, y, view.join(true), Ink::of(Role::Border, at));
            }
            if head_above {
                let (x, y) = view.point(leave + 1, from);
                canvas.put(x, y, view.arrow(false), arrow);
            }
        }
        if let Kind::Node(at) = lower.kind {
            if head_below {
                let (x, y) = view.point(arrive - 1, to);
                canvas.put(x, y, view.arrow(true), arrow);
            } else if !double(lower) {
                let (x, y) = view.point(arrive, to);
                canvas.put(x, y, view.join(false), Ink::of(Role::Border, at));
            }
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

fn segment(upper: usize, lower: usize, edge: usize) -> Segment {
    Segment {
        upper,
        lower,
        edge,
        from: 0,
        to: 0,
        lane: 0,
    }
}

/// Writes lines of text centred in a band `width` columns wide.
fn centred(canvas: &mut Canvas, x: usize, y: usize, width: usize, lines: &[String], ink: Ink) {
    for (row, line) in lines.iter().enumerate() {
        let indent = width.saturating_sub(line.width()) / 2;
        canvas.text(x + indent, y + row, line, ink);
    }
}

/// Which edges close a loop, found walking the graph depth first from each
/// node in the order they were written. Turning those round leaves a graph
/// with no loops, which is what ranking needs.
fn back_edges(chart: &Chart) -> Vec<bool> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); chart.nodes.len()];
    for (at, edge) in chart.edges.iter().enumerate() {
        out[edge.from].push(at);
    }
    // 0 not seen, 1 on the current path, 2 done.
    let mut state = vec![0u8; chart.nodes.len()];
    let mut reversed = vec![false; chart.edges.len()];
    for root in 0..chart.nodes.len() {
        if state[root] != 0 {
            continue;
        }
        // Walked with a stack of its own rather than by recursion, so a long
        // chain can't run out of stack.
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some((node, next)) = stack.last_mut() {
            let node = *node;
            if let Some(&edge) = out[node].get(*next) {
                *next += 1;
                let to = chart.edges[edge].to;
                match state[to] {
                    0 => {
                        state[to] = 1;
                        stack.push((to, 0));
                    }
                    1 => reversed[edge] = true,
                    _ => {}
                }
            } else {
                state[node] = 2;
                stack.pop();
            }
        }
    }
    reversed
}

/// Each node's rank: as far along as the longest path to it, with an edge
/// that has words spanning two ranks so they have one to sit in. A node that
/// only leads elsewhere is then moved down to just above the first thing it
/// leads to, so it isn't stranded at the top on a long edge.
fn ranks(chart: &Chart, reversed: &[bool]) -> Vec<usize> {
    let count = chart.nodes.len();
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    let mut incoming = vec![0usize; count];
    for (at, edge) in chart.edges.iter().enumerate() {
        let (from, to) = if reversed[at] {
            (edge.to, edge.from)
        } else {
            (edge.from, edge.to)
        };
        let span = if edge.label.is_empty() { 1 } else { 2 };
        out[from].push((to, span));
        incoming[to] += 1;
    }
    let mut rank = vec![0usize; count];
    let mut waiting = incoming.clone();
    let mut ready: Vec<usize> = (0..count).filter(|&n| waiting[n] == 0).collect();
    let mut sorted = Vec::with_capacity(count);
    while let Some(node) = ready.pop() {
        sorted.push(node);
        for &(to, span) in &out[node] {
            rank[to] = rank[to].max(rank[node] + span);
            waiting[to] -= 1;
            if waiting[to] == 0 {
                ready.push(to);
            }
        }
    }
    for &node in sorted.iter().rev() {
        if incoming[node] == 0
            && let Some(lowest) = out[node].iter().map(|&(to, span)| rank[to] - span).min()
        {
            rank[node] = lowest;
        }
    }
    rank
}

/// Puts the places in each rank in an order that crosses few edges: each is
/// moved to the average position of its neighbours in the rank above, then
/// in the rank below, a few times over.
fn order(layers: &mut [Vec<usize>], above: &[Vec<usize>], below: &[Vec<usize>], count: usize) {
    let mut index = vec![0usize; count];
    let number = |layers: &[Vec<usize>], index: &mut Vec<usize>| {
        for layer in layers {
            for (at, &p) in layer.iter().enumerate() {
                index[p] = at;
            }
        }
    };
    number(layers, &mut index);
    for sweep in 0..SWEEPS {
        let downwards = sweep % 2 == 0;
        let ranks: Vec<usize> = if downwards {
            (1..layers.len()).collect()
        } else {
            (0..layers.len().saturating_sub(1)).rev().collect()
        };
        for r in ranks {
            let neighbours = if downwards { above } else { below };
            let mut keyed: Vec<(f64, usize)> = layers[r]
                .iter()
                .map(|&p| {
                    let near = &neighbours[p];
                    let key = if near.is_empty() {
                        index[p] as f64
                    } else {
                        near.iter().map(|&n| index[n] as f64).sum::<f64>() / near.len() as f64
                    };
                    (key, p)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            layers[r] = keyed.into_iter().map(|(_, p)| p).collect();
            for (at, &p) in layers[r].iter().enumerate() {
                index[p] = at;
            }
        }
    }
}

/// Slides each place across the flow to sit near the average of its
/// neighbours, keeping the order and a gap between places.
fn position(
    places: &mut [Place],
    layers: &[Vec<usize>],
    above: &[Vec<usize>],
    below: &[Vec<usize>],
    across_flow: bool,
) {
    let gap = |a: &Place, b: &Place| -> i64 {
        match (a.kind, b.kind) {
            _ if across_flow => 1,
            (Kind::Node(_), Kind::Node(_)) => 4,
            _ => 2,
        }
    };
    for layer in layers {
        let mut at = 0;
        for (i, &p) in layer.iter().enumerate() {
            if i > 0 {
                at += gap(&places[layer[i - 1]], &places[p]);
            }
            places[p].at = at;
            at += places[p].across as i64;
        }
    }
    for sweep in 0..SWEEPS {
        let downwards = sweep % 2 == 0;
        let ranks: Vec<usize> = if downwards {
            (1..layers.len()).collect()
        } else {
            (0..layers.len().saturating_sub(1)).rev().collect()
        };
        for r in ranks {
            let layer = &layers[r];
            let neighbours = if downwards { above } else { below };
            let wanted: Vec<i64> = layer
                .iter()
                .map(|&p| {
                    let near = &neighbours[p];
                    if near.is_empty() {
                        places[p].at
                    } else {
                        let sum: i64 = near.iter().map(|&n| places[n].centre()).sum();
                        sum / near.len() as i64 - places[p].across as i64 / 2
                    }
                })
                .collect();
            // As far left as each may go and as far right, then halfway:
            // both keep the order and the gaps, so halfway does too.
            let mut left = wanted.clone();
            for i in 1..layer.len() {
                let least = left[i - 1]
                    + places[layer[i - 1]].across as i64
                    + gap(&places[layer[i - 1]], &places[layer[i]]);
                left[i] = left[i].max(least);
            }
            let mut right = wanted;
            for i in (0..layer.len().saturating_sub(1)).rev() {
                let most = right[i + 1]
                    - places[layer[i]].across as i64
                    - gap(&places[layer[i]], &places[layer[i + 1]]);
                right[i] = right[i].min(most);
            }
            for i in 0..layer.len() {
                let mut at = (left[i] + right[i]).div_euclid(2);
                if i > 0 {
                    let previous = &places[layer[i - 1]];
                    at = at.max(
                        previous.at + previous.across as i64 + gap(previous, &places[layer[i]]),
                    );
                }
                places[layer[i]].at = at;
            }
        }
    }
    let least = places.iter().map(|p| p.at).min().unwrap_or(0);
    for place in places.iter_mut() {
        place.at -= least;
    }
}

/// Picks where each segment leaves and arrives, spread along the side of a
/// box, and the lane it turns in. Returns how many lanes each gap needs.
fn route(places: &[Place], segments: &mut [Segment], layers: &[Vec<usize>]) -> Vec<usize> {
    let mut leaving: Vec<Vec<usize>> = vec![Vec::new(); places.len()];
    let mut arriving: Vec<Vec<usize>> = vec![Vec::new(); places.len()];
    for (at, s) in segments.iter().enumerate() {
        leaving[s.upper].push(at);
        arriving[s.lower].push(at);
    }
    // A box's ports sit inside its corners, in the order of what is at the
    // other end, so edges leaving one box don't cross each other.
    let port = |place: &Place, k: usize, n: usize| -> i64 {
        match place.kind {
            Kind::Node(_) if place.across > 2 => {
                let inside = (place.across - 2) as i64;
                place.at + 1 + (inside * (2 * k as i64 + 1)) / (2 * n as i64)
            }
            _ => place.centre(),
        }
    };
    for (p, place) in places.iter().enumerate() {
        let mut out = leaving[p].clone();
        out.sort_by_key(|&s| places[segments[s].lower].centre());
        for (k, &s) in out.iter().enumerate() {
            segments[s].from = port(place, k, out.len());
        }
        let mut into = arriving[p].clone();
        into.sort_by_key(|&s| places[segments[s].upper].centre());
        for (k, &s) in into.iter().enumerate() {
            segments[s].to = port(place, k, into.len());
        }
    }
    // Lanes: a segment that moves sideways turns in a lane no other segment
    // in the same stretch of that gap uses, with a cell to spare between.
    let mut lanes = vec![0usize; layers.len()];
    let mut turning: Vec<Vec<usize>> = vec![Vec::new(); layers.len()];
    for (at, s) in segments.iter().enumerate() {
        if s.from != s.to {
            turning[places[s.upper].rank].push(at);
        }
    }
    for (rank, list) in turning.iter_mut().enumerate() {
        list.sort_by_key(|&s| {
            (
                segments[s].from.min(segments[s].to),
                segments[s].from.max(segments[s].to),
            )
        });
        let mut ends: Vec<i64> = Vec::new();
        for &s in list.iter() {
            let (low, high) = (
                segments[s].from.min(segments[s].to),
                segments[s].from.max(segments[s].to),
            );
            let lane = match ends.iter().position(|&end| end + 1 < low) {
                Some(lane) => lane,
                None => {
                    ends.push(0);
                    ends.len() - 1
                }
            };
            ends[lane] = high;
            segments[s].lane = lane;
        }
        lanes[rank] = ends.len();
    }
    lanes
}

/// Turns positions along and across the flow into columns and rows.
struct View {
    direction: Direction,
    /// The chart's whole extent along the flow.
    along: usize,
}

impl View {
    fn point(&self, along: usize, across: usize) -> (usize, usize) {
        let flipped = self.along.saturating_sub(1).saturating_sub(along);
        match self.direction {
            Direction::Down => (across, along),
            Direction::Up => (across, flipped),
            Direction::Right => (along, across),
            Direction::Left => (flipped, across),
        }
    }

    /// The column, row, width and height of a rectangle.
    fn rect(
        &self,
        along: usize,
        depth: usize,
        across: usize,
        breadth: usize,
    ) -> (usize, usize, usize, usize) {
        let (a, b) = (
            self.point(along, across),
            self.point(along + depth - 1, across + breadth - 1),
        );
        (
            a.0.min(b.0),
            a.1.min(b.1),
            a.0.abs_diff(b.0) + 1,
            a.1.abs_diff(b.1) + 1,
        )
    }

    /// An arrowhead pointing along the flow, or back against it.
    fn arrow(&self, forward: bool) -> char {
        match (self.direction, forward) {
            (Direction::Down, true) | (Direction::Up, false) => '▼',
            (Direction::Down, false) | (Direction::Up, true) => '▲',
            (Direction::Right, true) | (Direction::Left, false) => '▶',
            (Direction::Right, false) | (Direction::Left, true) => '◀',
        }
    }

    /// Where an edge meets a box's outline: on the side facing along the
    /// flow when it leaves, the side facing back when it arrives.
    fn join(&self, leaving: bool) -> char {
        match (self.direction, leaving) {
            (Direction::Down, true) | (Direction::Up, false) => '┬',
            (Direction::Down, false) | (Direction::Up, true) => '┴',
            (Direction::Right, true) | (Direction::Left, false) => '├',
            (Direction::Right, false) | (Direction::Left, true) => '┤',
        }
    }
}

#[cfg(test)]
mod tests {
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
