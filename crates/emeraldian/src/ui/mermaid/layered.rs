//! Laying out a graph of boxes and the lines between them, for every kind of
//! diagram that is one: flowcharts, class, state, entity relationship and C4
//! diagrams.
//!
//! Laid out the way most graph drawing tools lay out a flow, in four steps.
//! Edges that close a loop are turned round, so the graph flows one way.
//! Each box is given a rank, its distance along the flow, and an edge that
//! spans several ranks is broken into one bend per rank it passes. The boxes
//! in each rank are put in the order that crosses fewest edges, by moving
//! each towards the average place of its neighbours a few times over. Then
//! each is slid sideways to sit near those neighbours without overlapping.
//!
//! Edges run straight down the gaps between ranks, turning once in a lane
//! of their own when they have to move sideways, so two edges never share a
//! stretch of line. The words on an edge take a place in a rank of their
//! own, which keeps them clear of everything else.
//!
//! Boxes may sit in frames, such as a C4 boundary or a composite state, and
//! frames in frames. A frame's boxes are kept together in each rank it spans,
//! and frames are kept in the same order in every rank, so each can be drawn
//! as one rectangle; then anything not in a frame that its sides would run
//! into is pushed clear of them. Boxes with no edges at all are wrapped onto
//! further ranks rather than run off past the width of the pane, the way
//! Mermaid lays out the shapes of a C4 diagram in rows.
//!
//! Everything is worked out along two axes, the way the graph flows and
//! across it, and only turned into rows and columns at the end, so one
//! layout serves all four directions. What is drawn at the ends of the lines,
//! and inside the boxes, is left to each kind of diagram.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::canvas::{Canvas, Stroke};
use super::label::{self, widest};
use super::{Ink, Role};

/// Boxes, bends, edge words and the places frames keep in ranks they would
/// otherwise have nothing in, together.
const MAX_PLACES: usize = 4000;
/// Rounds of reordering. The improvement after a handful is small.
const SWEEPS: usize = 8;
/// Frames inside frames, at most.
pub const MAX_DEPTH: usize = 16;
/// Columns of a frame's label it is widened for, at most. A longer one is
/// cut short.
const LABEL_ROOM: usize = 48;

/// Which way a graph flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    Down,
    Up,
    Right,
    Left,
}

impl Direction {
    #[must_use]
    pub fn across(self) -> bool {
        matches!(self, Self::Right | Self::Left)
    }
}

/// Which way a line runs where it meets a box: into the box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Down,
    Up,
    Right,
    Left,
}

impl Facing {
    /// An arrowhead pointing into the box.
    #[must_use]
    pub fn arrow(self) -> char {
        match self {
            Self::Down => '▼',
            Self::Up => '▲',
            Self::Right => '▶',
            Self::Left => '◀',
        }
    }

    /// A hollow arrowhead, the way UML marks inheritance.
    #[must_use]
    pub fn hollow(self) -> char {
        match self {
            Self::Down => '▽',
            Self::Up => '△',
            Self::Right => '▷',
            Self::Left => '◁',
        }
    }

    /// The piece of a box's outline where the line meets it.
    #[must_use]
    pub fn join(self) -> char {
        match self {
            Self::Down => '┴',
            Self::Up => '┬',
            Self::Right => '┤',
            Self::Left => '├',
        }
    }
}

/// An edge between two boxes, by their place in [`Graph::boxes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    /// The columns and rows of its words, which get a place of their own
    /// between its ends.
    pub words: Option<(usize, usize)>,
}

impl Link {
    /// A link with `words` on it, or none.
    #[must_use]
    pub fn new(from: usize, to: usize, words: &[String]) -> Self {
        Self {
            from,
            to,
            words: (!words.is_empty()).then(|| (widest(words), words.len())),
        }
    }
}

/// What to lay out.
#[derive(Debug, Clone)]
pub struct Graph {
    pub direction: Direction,
    /// Each box's columns and rows.
    pub boxes: Vec<(usize, usize)>,
    /// A link from a box to itself has no line, so is left out: mark it on
    /// the box instead.
    pub links: Vec<Link>,
    /// The frame each frame sits in, if any. A frame comes after the one it
    /// sits in.
    pub frames: Vec<Option<usize>>,
    /// The frame each box sits in, if any, or nothing when no box does.
    pub framed: Vec<Option<usize>>,
    /// Columns each frame's label takes. When the graph runs down or up, a
    /// frame is made wide enough to show it, up to a point.
    pub labels: Vec<usize>,
    /// Columns the pane has. When the graph runs down or up, boxes with no
    /// links are wrapped onto more ranks rather than run past it.
    pub room: usize,
    /// Whether boxes are made tall enough to give each of their links a row
    /// of its own when the graph runs sideways, as they are always made wide
    /// enough to give each a column when it runs down or up.
    pub spread: bool,
    /// Boxes drawn as a single mark, such as a state diagram's start, which
    /// every line meets in the middle rather than at a place of its own.
    pub points: Vec<bool>,
}

impl Graph {
    #[must_use]
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            boxes: Vec::new(),
            links: Vec::new(),
            frames: Vec::new(),
            framed: Vec::new(),
            labels: Vec::new(),
            room: 0,
            spread: false,
            points: Vec::new(),
        }
    }
}

/// A rectangle of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

/// Where a line meets a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    /// The link, by its place in [`Graph::links`].
    pub link: usize,
    /// Whether this is the link's `to` end rather than its `from` end.
    pub to: bool,
    /// Whether the line leaves the box here, along the flow, rather than
    /// arrives at it. A link that closes a loop runs against the flow.
    pub leaving: bool,
    /// The box.
    pub node: usize,
    /// The cell of the box's outline the line meets.
    pub border: (usize, usize),
    /// The cell just outside it, where an arrowhead goes.
    pub outside: (usize, usize),
    pub facing: Facing,
}

impl End {
    /// Whether the box's outline is joined to the line here: where the line
    /// leaves, and where it arrives without a `head` drawn outside.
    #[must_use]
    pub fn joined(&self, head: bool) -> bool {
        self.leaving || !head
    }
}

/// Where everything went.
#[derive(Debug, Clone)]
pub struct Layout {
    pub width: usize,
    pub height: usize,
    pub boxes: Vec<Rect>,
    /// Where each link's words go, for those that have words.
    pub words: Vec<Option<Rect>>,
    /// Each frame's outline, for those that hold anything.
    pub frames: Vec<Option<Rect>>,
    /// The stretches of line to draw, each with the link it belongs to.
    pub lines: Vec<(usize, Vec<(usize, usize)>)>,
    /// Where the lines meet boxes, in the order the marks there are drawn.
    pub ends: Vec<End>,
}

impl Layout {
    /// Moves everything right and down, to make room above and to the left.
    pub fn moved(&mut self, x: usize, y: usize) {
        let shift = |point: &mut (usize, usize)| {
            point.0 += x;
            point.1 += y;
        };
        let rects = self
            .boxes
            .iter_mut()
            .chain(self.words.iter_mut().flatten())
            .chain(self.frames.iter_mut().flatten());
        for rect in rects {
            rect.x += x;
            rect.y += y;
        }
        for (_, points) in &mut self.lines {
            points.iter_mut().for_each(shift);
        }
        for end in &mut self.ends {
            shift(&mut end.border);
            shift(&mut end.outside);
        }
        self.width += x;
        self.height += y;
    }

    /// Writes each link's words in the place kept for them, centred, as
    /// `words` gives them for the link with their ink.
    pub fn draw_words<'a>(
        &self,
        canvas: &mut Canvas,
        words: impl Fn(usize) -> (&'a [String], Ink),
    ) {
        for (link, rect) in self.words.iter().enumerate() {
            if let Some(rect) = rect {
                let (lines, ink) = words(link);
                label::centred(canvas, rect.x, rect.y, rect.width, lines, ink);
            }
        }
    }

    /// Draws every line, each in the stroke and colour `style` gives its
    /// link.
    pub fn draw_lines(&self, canvas: &mut Canvas, style: impl Fn(usize) -> (Stroke, Option<u16>)) {
        for (link, points) in &self.lines {
            let (stroke, owner) = style(*link);
            canvas.path(points, stroke, owner);
        }
    }

    /// Draws each frame's outline as a line of no one's, so the lines drawn
    /// after it cross it rather than stop at it.
    pub fn draw_frames(&self, canvas: &mut Canvas, stroke: Stroke) {
        for rect in self.frames.iter().flatten() {
            let (left, top) = (rect.x, rect.y);
            let (right, bottom) = (rect.x + rect.width - 1, rect.y + rect.height - 1);
            canvas.path(
                &[
                    (left, top),
                    (right, top),
                    (right, bottom),
                    (left, bottom),
                    (left, top),
                ],
                stroke,
                None,
            );
        }
    }

    /// Writes a frame's label into its top edge, where no line crosses it,
    /// or else its bottom edge, as far left as it fits. Each of `labels` is
    /// tried in turn at each place, longest first, and the last is cut short
    /// with `…` if none fits anywhere.
    pub fn label_frame(&self, canvas: &mut Canvas, frame: usize, labels: &[&str]) {
        use unicode_width::UnicodeWidthStr;

        let Some(Some(rect)) = self.frames.get(frame) else {
            return;
        };
        if rect.width < 6 {
            return;
        }
        // The stretches of plain rule along each edge, leaving a little of it
        // at either end.
        let mut stretches: Vec<(usize, usize, usize)> = Vec::new();
        for y in [rect.y, rect.y + rect.height - 1] {
            let mut start = None;
            for x in rect.x + 2..=rect.x + rect.width - 2 {
                let plain = x < rect.x + rect.width - 2 && canvas.is_rule(x, y);
                match (plain, start) {
                    (true, None) => start = Some(x),
                    (false, Some(from)) => {
                        stretches.push((from, y, x - from));
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        let ink = Ink::plain(Role::Label);
        for &(x, y, length) in &stretches {
            let fits = labels
                .iter()
                .find(|label| !label.is_empty() && label.width() + 2 <= length);
            if let Some(label) = fits {
                canvas.text(x, y, &format!(" {label} "), ink);
                return;
            }
        }
        let longest = stretches.iter().max_by_key(|&&(_, _, length)| length);
        if let (Some(&(x, y, length)), Some(label)) = (longest, labels.last())
            && length >= 4
            && !label.is_empty()
        {
            let cut = label::truncate(label, length - 2);
            canvas.text(x, y, &format!(" {cut} "), ink);
        }
    }
}

/// A rule across a box `width` wide whose left side is at `x`, joined to
/// both sides, as between a box's compartments.
pub fn rule(canvas: &mut Canvas, x: usize, y: usize, width: usize, ink: Ink) {
    canvas.put(x, y, '├', ink);
    canvas.put(x + width - 1, y, '┤', ink);
    for column in x + 1..x + width - 1 {
        canvas.put(column, y, '─', ink);
    }
}

/// The piece of a box's outline to draw where a line meets it: a cross
/// where it meets the end of a rule between two compartments, which a line
/// can when the graph runs sideways.
#[must_use]
pub fn join(canvas: &Canvas, end: &End) -> char {
    let (x, y) = end.border;
    match (canvas.glyph(x, y), end.facing) {
        (Some('├' | '┤'), Facing::Left | Facing::Right) => '┼',
        _ => end.facing.join(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Node(usize),
    /// Where a long edge passes through a rank.
    Bend,
    /// The words of an edge.
    Words(usize),
    /// Where a frame passes through a rank it has nothing else in.
    Hold,
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
    /// The innermost frame it sits in.
    frame: Option<usize>,
}

impl Place {
    /// A place for something `columns` by `rows`, turned to the flow.
    fn new(kind: Kind, rank: usize, (columns, rows): (usize, usize), across_flow: bool) -> Self {
        let (along, across) = if across_flow {
            (columns, rows)
        } else {
            (rows, columns)
        };
        Self {
            kind,
            rank,
            along,
            across,
            at: 0,
            frame: None,
        }
    }

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

/// Lays a graph out, or `None` when it is too big or not one that can be.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn layout(graph: &Graph) -> Option<Layout> {
    let direction = graph.direction;
    let across_flow = direction.across();
    let count = graph.boxes.len();
    let links = &graph.links;
    if count == 0 || links.iter().any(|l| l.from >= count || l.to >= count) {
        return None;
    }

    // Each frame as the frames it sits in, outermost first, then itself.
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(graph.frames.len());
    for (at, parent) in graph.frames.iter().enumerate() {
        let mut chain = match *parent {
            Some(parent) if parent < at => chains[parent].clone(),
            Some(_) => return None,
            None => Vec::new(),
        };
        chain.push(at);
        if chain.len() > MAX_DEPTH {
            return None;
        }
        chains.push(chain);
    }
    let frame_of = |b: usize| {
        graph
            .framed
            .get(b)
            .copied()
            .flatten()
            .filter(|&f| f < chains.len())
    };
    let shared = |a: Option<usize>, b: Option<usize>| -> Option<usize> {
        let (a, b) = (a?, b?);
        chains[a]
            .iter()
            .zip(&chains[b])
            .take_while(|(x, y)| x == y)
            .last()
            .map(|(x, _)| *x)
    };

    // Turn round the edges that close a loop.
    let reversed = back_edges(count, links);
    let ends = |edge: usize| {
        let e = &links[edge];
        if reversed[edge] {
            (e.to, e.from)
        } else {
            (e.from, e.to)
        }
    };
    let mut rank = ranks(count, links, &reversed);
    if !across_flow && graph.room > 0 {
        wrap(&mut rank, graph, &frame_of);
    }

    // Places: the boxes, then the bends and words of each edge.
    let mut places: Vec<Place> = (0..count)
        .map(|at| Place {
            frame: frame_of(at),
            ..Place::new(Kind::Node(at), rank[at], graph.boxes[at], across_flow)
        })
        .collect();
    let mut segments = Vec::new();
    for (edge, link) in links.iter().enumerate() {
        if link.from == link.to {
            continue;
        }
        let (top, bottom) = ends(edge);
        let frame = shared(frame_of(link.from), frame_of(link.to));
        let middle = rank[top] + (rank[bottom] - rank[top]) / 2;
        let mut previous = top;
        for r in rank[top] + 1..rank[bottom] {
            let place = match link.words {
                Some((columns, rows)) if r == middle => {
                    Place::new(Kind::Words(edge), r, (columns.max(1), rows), across_flow)
                }
                _ => Place::new(Kind::Bend, r, (1, 1), across_flow),
            };
            places.push(Place { frame, ..place });
            if places.len() > MAX_PLACES {
                return None;
            }
            segments.push(segment(previous, places.len() - 1, edge));
            previous = places.len() - 1;
        }
        segments.push(segment(previous, bottom, edge));
    }

    // A frame holds a place in each rank it spans, so it can be kept
    // together there, even where none of its boxes are.
    let framed = !chains.is_empty() && places.iter().any(|p| p.frame.is_some());
    let mut span: Vec<Option<(usize, usize)>> = vec![None; chains.len()];
    if framed {
        let mut held: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
        for place in &places {
            for &f in place.frame.map_or(&[][..], |f| &chains[f]) {
                held.insert((f, place.rank));
                span[f] = Some(span[f].map_or((place.rank, place.rank), |(lo, hi)| {
                    (lo.min(place.rank), hi.max(place.rank))
                }));
            }
        }
        let mut deepest: Vec<usize> = (0..chains.len()).collect();
        deepest.sort_by_key(|&f| std::cmp::Reverse(chains[f].len()));
        for f in deepest {
            let Some((lo, hi)) = span[f] else { continue };
            for r in lo..=hi {
                if held.contains(&(f, r)) {
                    continue;
                }
                for &outer in &chains[f] {
                    held.insert((outer, r));
                }
                places.push(Place {
                    frame: Some(f),
                    ..Place::new(Kind::Hold, r, (1, 1), across_flow)
                });
                if places.len() > MAX_PLACES {
                    return None;
                }
            }
        }
    }

    // A box going down the page widens to give each of its edges a column
    // of its own, rather than several sharing one and merging.
    if !across_flow || graph.spread {
        let mut ends = vec![(0usize, 0usize); count];
        for s in &segments {
            if let Kind::Node(at) = places[s.upper].kind {
                ends[at].0 += 1;
            }
            if let Kind::Node(at) = places[s.lower].kind {
                ends[at].1 += 1;
            }
        }
        for (at, (out, into)) in ends.into_iter().enumerate() {
            if !graph.points.get(at).copied().unwrap_or(false) {
                places[at].across = places[at].across.max(2 * out.max(into) + 1);
            }
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

    let chain_of = |p: &Place| -> &[usize] { p.frame.map_or(&[][..], |f| &chains[f]) };
    let place_chains: Vec<&[usize]> = places.iter().map(chain_of).collect();
    order(&mut layers, &above, &below, &place_chains, chains.len());
    let gaps = gaps(&places, &layers, &place_chains, across_flow);
    position(&mut places, &layers, &above, &below, &gaps);
    let mut sides: Vec<Option<(i64, i64)>> = vec![None; chains.len()];
    if framed {
        let labels = label_room(graph, &places, &place_chains, &above, &span, across_flow);
        sides = separate(&mut places, &layers, &place_chains, &gaps, &chains, &labels)?;
    }
    let least = places
        .iter()
        .map(|p| p.at)
        .chain(sides.iter().flatten().map(|&(left, _)| left))
        .min()
        .unwrap_or(0);
    for place in &mut places {
        place.at -= least;
    }
    for (left, right) in sides.iter_mut().flatten() {
        *left -= least;
        *right -= least;
    }
    let lanes = route(&places, &mut segments, &layers, &graph.points);

    // How many frames open just before each rank and close just after it,
    // one inside another, each of which takes two rows: its edge and a
    // blank between that and what it holds.
    let mut opens = vec![0usize; chains.len()];
    let mut closes = vec![0usize; chains.len()];
    for f in (0..chains.len()).rev() {
        let Some((lo, hi)) = span[f] else { continue };
        opens[f] += 1;
        closes[f] += 1;
        if let Some(parent) = graph.frames[f]
            && let Some((parent_lo, parent_hi)) = span[parent]
        {
            if parent_lo == lo {
                opens[parent] = opens[parent].max(opens[f]);
            }
            if parent_hi == hi {
                closes[parent] = closes[parent].max(closes[f]);
            }
        }
    }
    let mut opening = vec![0usize; ranks_count];
    let mut closing = vec![0usize; ranks_count];
    for f in 0..chains.len() {
        if let Some((lo, hi)) = span[f] {
            opening[lo] = opening[lo].max(opens[f]);
            closing[hi] = closing[hi].max(closes[f]);
        }
    }

    // Along the flow: each rank as deep as its deepest place, and each gap
    // deep enough for its lanes and the edges of the frames in it.
    let depth: Vec<usize> = layers
        .iter()
        .map(|layer| layer.iter().map(|&p| places[p].along).max().unwrap_or(1))
        .collect();
    let least_gap = if across_flow { 3 } else { 2 };
    let mut start = vec![0usize; ranks_count];
    start[0] = 2 * opening[0];
    for r in 1..ranks_count {
        let gap = 2 * closing[r - 1] + (lanes[r - 1] + 2).max(least_gap) + 2 * opening[r];
        start[r] = start[r - 1] + depth[r - 1] + gap;
    }
    let total_along =
        start[ranks_count - 1] + depth[ranks_count - 1] + 2 * closing[ranks_count - 1];
    let total_across = places
        .iter()
        .map(|p| p.at + p.across as i64)
        .chain(sides.iter().flatten().map(|&(_, right)| right + 1))
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
    let (width, height) = if across_flow {
        (total_along, total_across)
    } else {
        (total_across, total_along)
    };
    let rect = |along: usize, depth: usize, across: i64, breadth: usize| -> Option<Rect> {
        let (x, y, width, height) = view.rect(along, depth, usize::try_from(across).ok()?, breadth);
        Some(Rect {
            x,
            y,
            width,
            height,
        })
    };

    let mut boxes = Vec::with_capacity(count);
    let mut words = vec![None; links.len()];
    for place in &places {
        match place.kind {
            Kind::Node(_) => boxes.push(rect(first(place), place.along, place.at, place.across)?),
            Kind::Words(edge) => {
                words[edge] = Some(rect(first(place), place.along, place.at, place.across)?);
            }
            Kind::Bend | Kind::Hold => {}
        }
    }
    let mut frames = vec![None; chains.len()];
    for f in 0..chains.len() {
        let (Some((lo, hi)), Some((left, right))) = (span[f], sides[f]) else {
            continue;
        };
        let top = start[lo].checked_sub(2 * opens[f])?;
        let bottom = start[hi] + depth[hi] - 1 + 2 * closes[f];
        let breadth = usize::try_from(right - left + 1).ok()?;
        frames[f] = Some(rect(top, bottom - top + 1, left, breadth)?);
    }

    let mut lines = Vec::with_capacity(segments.len());
    let mut marks = Vec::new();
    for s in &segments {
        let (upper, lower) = (&places[s.upper], &places[s.lower]);
        let (from, to) = (usize::try_from(s.from).ok()?, usize::try_from(s.to).ok()?);
        let leave = last(upper);
        let arrive = first(lower);
        let gap = start[upper.rank] + depth[upper.rank] + 2 * closing[upper.rank];
        let mut points = vec![(leave, from)];
        if from != to {
            let lane = gap + 1 + s.lane;
            points.push((lane, from));
            points.push((lane, to));
        }
        points.push((arrive, to));
        lines.push((
            s.edge,
            points.iter().map(|&(a, c)| view.point(a, c)).collect(),
        ));
        if upper.kind == Kind::Bend {
            lines.push((
                s.edge,
                vec![view.point(first(upper), from), view.point(leave, from)],
            ));
        }

        let forward = !reversed[s.edge];
        if let Kind::Node(node) = upper.kind {
            marks.push(End {
                link: s.edge,
                to: !forward,
                leaving: true,
                node,
                border: view.point(leave, from),
                outside: view.point(leave + 1, from),
                facing: view.facing(false),
            });
        }
        if let Kind::Node(node) = lower.kind {
            marks.push(End {
                link: s.edge,
                to: forward,
                leaving: false,
                node,
                border: view.point(arrive, to),
                outside: view.point(arrive.checked_sub(1)?, to),
                facing: view.facing(true),
            });
        }
    }
    Some(Layout {
        width,
        height,
        boxes,
        words,
        frames,
        lines,
        ends: marks,
    })
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

/// Which edges close a loop, found walking the graph depth first from each
/// box in the order they were written. Turning those round leaves a graph
/// with no loops, which is what ranking needs.
fn back_edges(count: usize, links: &[Link]) -> Vec<bool> {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (at, link) in links.iter().enumerate() {
        if link.from != link.to {
            out[link.from].push(at);
        }
    }
    // 0 not seen, 1 on the current path, 2 done.
    let mut state = vec![0u8; count];
    let mut reversed = vec![false; links.len()];
    for root in 0..count {
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
                let to = links[edge].to;
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

/// Each box's rank: as far along as the longest path to it, with an edge
/// that has words spanning two ranks so they have one to sit in. A box that
/// only leads elsewhere is then moved down to just above the first thing it
/// leads to, so it isn't stranded at the top on a long edge.
fn ranks(count: usize, links: &[Link], reversed: &[bool]) -> Vec<usize> {
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); count];
    let mut incoming = vec![0usize; count];
    for (at, link) in links.iter().enumerate() {
        if link.from == link.to {
            continue;
        }
        let (from, to) = if reversed[at] {
            (link.to, link.from)
        } else {
            (link.from, link.to)
        };
        let span = if link.words.is_none() { 1 } else { 2 };
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

/// Moves boxes with no links at all onto later ranks where a rank would
/// otherwise run past the pane, in the order they were written, starting
/// from where the rest of their frame is.
fn wrap(rank: &mut [usize], graph: &Graph, frame_of: &dyn Fn(usize) -> Option<usize>) {
    let mut linked = vec![false; rank.len()];
    for link in &graph.links {
        if link.from != link.to {
            linked[link.from] = true;
            linked[link.to] = true;
        }
    }
    let width = |b: usize| graph.boxes[b].0 + 4;
    let mut used: Vec<usize> = Vec::new();
    let mut first: HashMap<Option<usize>, usize> = HashMap::new();
    for b in (0..rank.len()).filter(|&b| linked[b]) {
        if used.len() <= rank[b] {
            used.resize(rank[b] + 1, 0);
        }
        used[rank[b]] += width(b);
        let at = first.entry(frame_of(b)).or_insert(rank[b]);
        *at = (*at).min(rank[b]);
    }
    let mut next: HashMap<Option<usize>, usize> = HashMap::new();
    for b in (0..rank.len()).filter(|&b| !linked[b]) {
        let frame = frame_of(b);
        let mut r = next
            .get(&frame)
            .or_else(|| first.get(&frame))
            .copied()
            .unwrap_or(0);
        loop {
            if used.len() <= r {
                used.resize(r + 1, 0);
            }
            if used[r] == 0 || used[r] + width(b) <= graph.room {
                break;
            }
            r += 1;
        }
        used[r] += width(b);
        rank[b] = r;
        next.insert(frame, r);
    }
}

/// Compares two places' sort keys: the key of each frame they are in, from
/// the outermost, then their own.
fn compare(a: &[(f64, usize)], b: &[(f64, usize)]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

/// What each frame's label needs along its top edge, when the graph runs
/// down or up: the columns it takes, and the one place in the frame's first
/// rank, if there is just one and a line comes into it from outside. That
/// line crosses the top edge at the place's middle, so the label has to fit
/// to the left of it.
fn label_room(
    graph: &Graph,
    places: &[Place],
    chains: &[&[usize]],
    above: &[Vec<usize>],
    span: &[Option<(usize, usize)>],
    across_flow: bool,
) -> Vec<(i64, Option<usize>)> {
    let frames = span.len();
    let mut first: Vec<Vec<usize>> = vec![Vec::new(); frames];
    for (p, place) in places.iter().enumerate() {
        for &f in chains[p] {
            if span[f].is_some_and(|(lo, _)| lo == place.rank) {
                first[f].push(p);
            }
        }
    }
    (0..frames)
        .map(|f| {
            let label = graph.labels.get(f).copied().unwrap_or(0).min(LABEL_ROOM);
            if across_flow || label == 0 {
                return (0, None);
            }
            let crossed = match first[f][..] {
                [only] if above[only].iter().any(|&from| !chains[from].contains(&f)) => Some(only),
                _ => None,
            };
            (label as i64, crossed)
        })
        .collect()
}

/// Each frame's key: the average position of everything in it, over all
/// the ranks it spans, so frames sort the same way in every rank.
fn frame_keys(
    layers: &[Vec<usize>],
    index: &[usize],
    chains: &[&[usize]],
    frames: usize,
) -> Vec<f64> {
    let mut sum = vec![0.0f64; frames];
    let mut count = vec![0usize; frames];
    for layer in layers {
        for &p in layer {
            for &f in chains[p] {
                sum[f] += index[p] as f64;
                count[f] += 1;
            }
        }
    }
    sum.iter()
        .zip(&count)
        .map(|(&s, &n)| if n == 0 { 0.0 } else { s / n as f64 })
        .collect()
}

/// Puts the places in each rank in an order that crosses few edges: each is
/// moved to the average position of its neighbours in the rank above, then
/// in the rank below, a few times over. What shares a frame stays together,
/// and frames keep one order in every rank.
fn order(
    layers: &mut [Vec<usize>],
    above: &[Vec<usize>],
    below: &[Vec<usize>],
    chains: &[&[usize]],
    frames: usize,
) {
    let count = chains.len();
    let mut index = vec![0usize; count];
    let number = |layers: &[Vec<usize>], index: &mut Vec<usize>| {
        for layer in layers {
            for (at, &p) in layer.iter().enumerate() {
                index[p] = at;
            }
        }
    };
    number(layers, &mut index);
    let sort =
        |layer: &mut Vec<usize>, own: &dyn Fn(usize) -> f64, index: &[usize], keys: &[f64]| {
            let mut keyed: Vec<(Vec<(f64, usize)>, usize)> = layer
                .iter()
                .map(|&p| {
                    let mut key: Vec<(f64, usize)> =
                        chains[p].iter().map(|&f| (keys[f], f)).collect();
                    key.push((own(p), index[p]));
                    (key, p)
                })
                .collect();
            keyed.sort_by(|a, b| compare(&a.0, &b.0));
            *layer = keyed.into_iter().map(|(_, p)| p).collect();
        };
    for sweep in 0..SWEEPS {
        let downwards = sweep % 2 == 0;
        let keys = frame_keys(layers, &index, chains, frames);
        let ranks: Vec<usize> = if downwards {
            (1..layers.len()).collect()
        } else {
            (0..layers.len().saturating_sub(1)).rev().collect()
        };
        for r in ranks {
            let neighbours = if downwards { above } else { below };
            let own = |p: usize| {
                let near = &neighbours[p];
                if near.is_empty() {
                    index[p] as f64
                } else {
                    near.iter().map(|&n| index[n] as f64).sum::<f64>() / near.len() as f64
                }
            };
            sort(&mut layers[r], &own, &index, &keys);
            for (at, &p) in layers[r].iter().enumerate() {
                index[p] = at;
            }
        }
    }
    // The ranks a sweep left alone were sorted with the frames' keys of the
    // sweep before, so they are sorted once more with one set of keys.
    if frames > 0 {
        let keys = frame_keys(layers, &index, chains, frames);
        for layer in layers.iter_mut() {
            let own = |p: usize| index[p] as f64;
            sort(layer, &own, &index, &keys);
        }
        number(layers, &mut index);
    }
}

/// The least gap across the flow between each place and the one before it
/// in its rank, wide enough for the sides of any frames between them.
fn gaps(
    places: &[Place],
    layers: &[Vec<usize>],
    chains: &[&[usize]],
    across_flow: bool,
) -> Vec<Vec<i64>> {
    layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .enumerate()
                .map(|(i, &p)| {
                    let Some(&q) = i.checked_sub(1).and_then(|i| layer.get(i)) else {
                        return 0;
                    };
                    let shared = chains[q]
                        .iter()
                        .zip(chains[p])
                        .take_while(|(x, y)| x == y)
                        .count();
                    let sides = chains[q].len() + chains[p].len() - 2 * shared;
                    if sides > 0 {
                        return 2 * sides as i64 + 2;
                    }
                    match (places[q].kind, places[p].kind) {
                        _ if across_flow => 1,
                        (Kind::Node(_), Kind::Node(_)) => 4,
                        _ => 2,
                    }
                })
                .collect()
        })
        .collect()
}

/// Slides each place across the flow to sit near the average of its
/// neighbours, keeping the order and a gap between places.
fn position(
    places: &mut [Place],
    layers: &[Vec<usize>],
    above: &[Vec<usize>],
    below: &[Vec<usize>],
    gaps: &[Vec<i64>],
) {
    for (layer, gaps) in layers.iter().zip(gaps) {
        let mut at = 0;
        for (i, &p) in layer.iter().enumerate() {
            if i > 0 {
                at += gaps[i];
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
            let (layer, gaps) = (&layers[r], &gaps[r]);
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
                let least = left[i - 1] + places[layer[i - 1]].across as i64 + gaps[i];
                left[i] = left[i].max(least);
            }
            let mut right = wanted;
            for i in (0..layer.len().saturating_sub(1)).rev() {
                let most = right[i + 1] - places[layer[i]].across as i64 - gaps[i + 1];
                right[i] = right[i].min(most);
            }
            for i in 0..layer.len() {
                let mut at = (left[i] + right[i]).div_euclid(2);
                if i > 0 {
                    let previous = &places[layer[i - 1]];
                    at = at.max(previous.at + previous.across as i64 + gaps[i]);
                }
                places[layer[i]].at = at;
            }
        }
    }
}

/// Pushes places right where a frame's sides, which must line up in every
/// rank it spans, would run into them, and returns where each frame's sides
/// are. `None` if the ranks put frames in orders that can't all hold.
///
/// Each rank is read as a run of places with frames opening and closing
/// between them, and each step of the run is a least distance between two
/// positions; the frames' sides are positions shared by every rank they
/// span. Taking the positions in an order where each comes after everything
/// it must be clear of, each is put as far left as it may go.
#[allow(clippy::type_complexity)]
fn separate(
    places: &mut [Place],
    layers: &[Vec<usize>],
    chains: &[&[usize]],
    gaps: &[Vec<i64>],
    frames: &[Vec<usize>],
    labels: &[(i64, Option<usize>)],
) -> Option<Vec<Option<(i64, i64)>>> {
    let count = places.len();
    let left = |f: usize| count + 2 * f;
    let right = |f: usize| count + 2 * f + 1;
    let total = count + 2 * frames.len();
    let mut after: Vec<Vec<(usize, i64)>> = vec![Vec::new(); total];
    let mut before: Vec<Vec<(usize, i64)>> = vec![Vec::new(); total];
    let mut edge = |from: usize, to: usize, least: i64| {
        after[from].push((to, least));
        before[to].push((from, least));
    };
    // What the run of a rank has just passed: a place, as wide as it is, or
    // a frame's left or right side. A side is a column of its own with a
    // blank column inside it, and two blank columns part a frame from what
    // is beside it, so these agree with `gaps`.
    #[derive(Clone, Copy)]
    enum Last {
        Place(i64),
        Left,
        Right,
    }
    // Steps the run on to `to`, which is `now`, at least as far from what
    // it passed as a right side needs, for no `gap`, or else a left side,
    // which keeps two columns from a place, or a place `gap` from one.
    let mut step = |last: &mut Option<(usize, Last)>, to: usize, now: Last, gap: Option<i64>| {
        if let Some((from, was)) = *last {
            let least = match (was, gap) {
                (Last::Place(across), None) => across + 1,
                (Last::Place(across), Some(gap)) => across + gap,
                (Last::Right, Some(_)) => 3,
                _ => 2,
            };
            edge(from, to, least);
        }
        *last = Some((to, now));
    };
    for (layer, gaps) in layers.iter().zip(gaps) {
        let mut last = None;
        let mut open: &[usize] = &[];
        for (i, &p) in layer.iter().enumerate() {
            let chain = chains[p];
            let shared = open.iter().zip(chain).take_while(|(x, y)| x == y).count();
            for &f in open[shared..].iter().rev() {
                step(&mut last, right(f), Last::Right, None);
            }
            for &f in &chain[shared..] {
                step(&mut last, left(f), Last::Left, Some(2));
            }
            let across = places[p].across as i64;
            step(&mut last, p, Last::Place(across), Some(gaps[i]));
            open = chain;
        }
        for &f in open.iter().rev() {
            step(&mut last, right(f), Last::Right, None);
        }
    }

    // In an order where each position comes after all it must be clear of.
    let mut waiting: Vec<usize> = before.iter().map(Vec::len).collect();
    let mut ready: Vec<usize> = (0..total).filter(|&v| waiting[v] == 0).collect();
    let mut sorted = Vec::with_capacity(total);
    while let Some(v) = ready.pop() {
        sorted.push(v);
        for &(to, _) in &after[v] {
            waiting[to] -= 1;
            if waiting[to] == 0 {
                ready.push(to);
            }
        }
    }
    if sorted.len() < total {
        return None;
    }
    let mut deepest: Vec<usize> = (0..frames.len()).collect();
    deepest.sort_by_key(|&f| std::cmp::Reverse(frames[f].len()));
    // Each position as far left as it may go, at or right of its floor; then
    // each frame's left side moves in to sit just clear of what it holds,
    // innermost frames first.
    // The room a frame keeps inside its left side, past the least, so its
    // label fits along its top edge.
    let mut room = vec![0i64; frames.len()];
    let frame_of_left =
        |v: usize| (v >= count && (v - count).is_multiple_of(2)).then(|| (v - count) / 2);
    let solve = |room: &[i64]| -> Vec<Option<i64>> {
        let mut value: Vec<Option<i64>> = (0..total)
            .map(|v| (v < count).then(|| places[v].at))
            .collect();
        for &v in &sorted {
            let pushed = before[v]
                .iter()
                .filter_map(|&(from, least)| {
                    let extra = frame_of_left(from).map_or(0, |f| room[f]);
                    Some(value[from]? + least + extra)
                })
                .max();
            value[v] = match (value[v], pushed) {
                (Some(own), Some(pushed)) => Some(own.max(pushed)),
                (own, pushed) => own.or(pushed),
            };
        }
        for &f in &deepest {
            let snug = after[left(f)]
                .iter()
                .filter_map(|&(to, least)| Some(value[to]? - least - room[f]))
                .min();
            if let Some(snug) = snug {
                value[left(f)] = Some(value[left(f)].map_or(snug, |own| own.max(snug)));
            }
        }
        value
    };
    let mut value = solve(&room);
    // A frame too narrow for its label, along the whole of its top edge or
    // to the left of the line crossing it, is given more room inside its
    // left side, which moves what it holds, its right side and everything
    // after it along. Widening one can only widen those around it, so a few
    // rounds settle it.
    for _ in 0..4 {
        let mut widened = false;
        for (f, &(label, crossed)) in labels.iter().enumerate() {
            let (Some(l), Some(r)) = (value[left(f)], value[right(f)]) else {
                continue;
            };
            if label == 0 {
                continue;
            }
            // The label is written two columns in, with a blank either side.
            let mut short = (label + 6) - (r - l + 1);
            if let Some(p) = crossed
                && let Some(at) = value[p]
            {
                let crossing = at + places[p].across as i64 / 2;
                short = short.max((l + 2 + label + 2) - crossing);
            }
            if short > 0 {
                room[f] += short;
                widened = true;
            }
        }
        if !widened {
            break;
        }
        value = solve(&room);
    }
    for (p, place) in places.iter_mut().enumerate() {
        if let Some(at) = value[p] {
            place.at = at;
        }
    }
    Some(
        (0..frames.len())
            .map(|f| Some((value[left(f)]?, value[right(f)]?)))
            .collect(),
    )
}

/// Picks where each segment leaves and arrives, spread along the side of a
/// box, and the lane it turns in. Returns how many lanes each gap needs.
fn route(
    places: &[Place],
    segments: &mut [Segment],
    layers: &[Vec<usize>],
    points: &[bool],
) -> Vec<usize> {
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
            Kind::Node(at) if points.get(at).copied().unwrap_or(false) => place.centre(),
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
    /// The graph's whole extent along the flow.
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

    /// The way a line runs into a box: along the flow into the one it
    /// arrives at, back against it into the one it leaves.
    fn facing(&self, forward: bool) -> Facing {
        match (self.direction, forward) {
            (Direction::Down, true) | (Direction::Up, false) => Facing::Down,
            (Direction::Down, false) | (Direction::Up, true) => Facing::Up,
            (Direction::Right, true) | (Direction::Left, false) => Facing::Right,
            (Direction::Right, false) | (Direction::Left, true) => Facing::Left,
        }
    }
}
