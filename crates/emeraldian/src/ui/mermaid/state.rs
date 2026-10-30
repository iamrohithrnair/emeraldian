//! State diagrams: `stateDiagram` and `stateDiagram-v2`.
//!
//! Each state is a box with round corners holding its name, and under a rule
//! its description, if it has one. The start is a dot and the end a ringed
//! one, a choice is a diamond on a short bar and a fork or join a thick bar,
//! and each transition is an arrow with its words on it. A transition from a
//! state back to itself is written in its box, after a `↻`.
//!
//! A composite state is a frame around the states inside it, with its own
//! start and end, and a transition into it arrives at its start, as UML
//! reads it; out of it, it leaves from its end. Without those, the first and
//! last of the states written inside it stand in. Concurrent regions,
//! parted by `--`, share their state's frame, each with a start and end of
//! its own. Notes are boxes joined by a dotted line to their state.
//!
//! Laid out by [`super::layered`], as flowcharts are.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::label::{self, widest};
use super::layered::{self, Direction, Facing, Graph, Link};
use super::{Diagram, Ink, ROUND, Role, SQUARE, key, pairs, syntax};

const MAX_STATES: usize = 200;
const MAX_TRANSITIONS: usize = 400;
const MAX_COMPOSITES: usize = 60;
const MAX_DESCRIPTION: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    State,
    Start,
    End,
    Choice,
    /// A fork or a join, both drawn as a bar.
    Fork,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// The name transitions use for it, which its colour is picked by.
    pub id: String,
    pub kind: Kind,
    pub name: Vec<String>,
    pub description: Vec<String>,
    /// The composite state it is drawn inside.
    pub frame: Option<usize>,
    /// The composite it is, if it holds states of its own.
    pub composite: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composite {
    pub label: String,
    pub parent: Option<usize>,
    /// The state it is.
    pub state: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub from: usize,
    pub to: usize,
    pub words: Vec<String>,
    /// A dotted line with no arrowhead, joining a note to its state.
    pub note: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateDiagram {
    pub direction: Direction,
    pub states: Vec<State>,
    pub composites: Vec<Composite>,
    pub transitions: Vec<Transition>,
}

// ---------------------------------------------------------------- parsing

/// Statements about how states look, which a drawing in text has no use for.
const IGNORED: &[&str] = &[
    "classDef", "class", "style", "click", "scale", "hide", "title",
];

struct Reader {
    diagram: StateDiagram,
    ids: HashMap<String, usize>,
    /// The composites opened and not yet closed, innermost last.
    open: Vec<usize>,
    /// Which concurrent region of each open composite is being read.
    region: HashMap<Option<usize>, usize>,
    /// A note being read up to its `end note`: its state, and its lines.
    note: Option<(usize, Vec<String>)>,
    described: syntax::Descriptions,
}

/// Reads the lines after `stateDiagram`.
pub fn parse(lines: &[&str]) -> Option<StateDiagram> {
    let mut reader = Reader {
        diagram: StateDiagram {
            direction: Direction::Down,
            states: Vec::new(),
            composites: Vec::new(),
            transitions: Vec::new(),
        },
        ids: HashMap::new(),
        open: Vec::new(),
        region: HashMap::new(),
        note: None,
        described: syntax::Descriptions::default(),
    };
    for line in lines {
        reader.line(line.trim())?;
    }
    if !reader.open.is_empty() || reader.note.is_some() || reader.diagram.states.is_empty() {
        return None;
    }
    Some(reader.diagram)
}

impl Reader {
    fn frame(&self) -> Option<usize> {
        self.open.last().copied()
    }

    fn line(&mut self, line: &str) -> Option<()> {
        if let Some((owner, lines)) = &mut self.note {
            if line == "end note" || line == "endnote" {
                let (owner, lines) = (*owner, std::mem::take(lines));
                self.note = None;
                return self.add_note(owner, &lines.join("\n"));
            }
            lines.push(line.to_string());
            return Some(());
        }
        if line.is_empty() || self.described.skip(line) {
            return Some(());
        }
        if line == "}" {
            self.open.pop()?;
            return Some(());
        }
        if line == "--" {
            // Another concurrent region of the composite, with a start and an
            // end of its own.
            *self.region.entry(self.frame()).or_insert(0) += 1;
            return Some(());
        }
        let first = syntax::first_word(line);
        if IGNORED.contains(&first) {
            return Some(());
        }
        if first == "direction" {
            if self.open.is_empty() {
                self.diagram.direction = syntax::direction(line["direction".len()..].trim())?;
            }
            return Some(());
        }
        if let Some(rest) = line.strip_prefix("note ") {
            return self.note(rest.trim());
        }
        if let Some(rest) = line.strip_prefix("state ") {
            return self.state_statement(rest.trim());
        }
        if let Some(at) = syntax::outside_quotes(line, "-->", &['"']) {
            let (link, label) = match line[at + 3..].split_once(':') {
                Some((to, label)) => (&line[..at + 3 + to.len()], label.trim()),
                None => (line, ""),
            };
            let from = self.end(link[..at].trim(), true)?;
            let to = self.end(link[at + 3..].trim(), false)?;
            return self.transition(from, to, label);
        }
        if let Some((id, description)) = line.split_once(':') {
            let state = self.state(id.trim())?;
            return self.describe(state, description.trim());
        }
        // A state named on a line of its own.
        self.state(line).map(|_| ())
    }

    /// The state with `id`, made the first time it is named.
    fn state(&mut self, id: &str) -> Option<usize> {
        let id = id.split(":::").next().unwrap_or(id).trim();
        if id.is_empty() || id.contains(char::is_whitespace) || id.starts_with('[') {
            return None;
        }
        let frame = self.frame();
        self.named(id, || State {
            id: id.to_string(),
            kind: Kind::State,
            name: label::lines(id),
            description: Vec::new(),
            frame,
            composite: None,
        })
    }

    /// The state with `id`, made by `make` the first time it is named.
    fn named(&mut self, id: &str, make: impl FnOnce() -> State) -> Option<usize> {
        syntax::named(
            &mut self.ids,
            &mut self.diagram.states,
            id,
            MAX_STATES,
            make,
        )
    }

    /// A transition's end: a state, or `[*]`, which is the start of the
    /// composite or region it is written in when it leads somewhere, and its
    /// end when something leads to it.
    fn end(&mut self, text: &str, from: bool) -> Option<usize> {
        if text != "[*]" {
            return self.state(text);
        }
        let frame = self.frame();
        let region = self.region.get(&frame).copied().unwrap_or(0);
        let kind = if from { Kind::Start } else { Kind::End };
        let scope = frame.map_or_else(|| "root".to_string(), |f| f.to_string());
        let id = format!(
            "[*] {} {scope} {region}",
            if from { "start" } else { "end" }
        );
        self.named(&id, || State {
            id: id.clone(),
            kind,
            name: Vec::new(),
            description: Vec::new(),
            frame,
            composite: None,
        })
    }

    fn transition(&mut self, from: usize, to: usize, label: &str) -> Option<()> {
        if from == to {
            let words = label::joined(label);
            let line = if words.is_empty() {
                "↻".to_string()
            } else {
                format!("↻ {words}")
            };
            return self.describe(from, &line);
        }
        if self.diagram.transitions.len() >= MAX_TRANSITIONS {
            return None;
        }
        let words = if label.is_empty() {
            Vec::new()
        } else {
            label::lines(label)
        };
        self.diagram.transitions.push(Transition {
            from,
            to,
            words,
            note: false,
        });
        Some(())
    }

    fn describe(&mut self, state: usize, text: &str) -> Option<()> {
        let description = &mut self.diagram.states[state].description;
        if description.len() >= MAX_DESCRIPTION {
            return Some(());
        }
        description.extend(label::lines(text));
        Some(())
    }

    /// `state "Description" as id`, `state id <<choice>>`, `state id {`.
    fn state_statement(&mut self, rest: &str) -> Option<()> {
        let (rest, opens) = match rest.strip_suffix('{') {
            Some(rest) => (rest.trim(), true),
            None => (rest, false),
        };
        let (id, name) = if let Some(quoted) = rest.strip_prefix('"') {
            let close = quoted.find('"')?;
            let id = quoted[close + 1..].trim().strip_prefix("as")?.trim();
            (id, Some(&quoted[..close]))
        } else {
            match rest.split_once(" as ") {
                Some((name, id)) => (id.trim(), Some(name.trim().trim_matches('"'))),
                None => (rest, None),
            }
        };
        let (id, kind) = syntax::annotated(id)?;
        let (id, description) = match id.split_once(':') {
            Some((id, description)) => (id.trim(), Some(description.trim())),
            None => (id, None),
        };
        let state = self.state(id)?;
        if let Some(name) = name {
            self.diagram.states[state].name = label::lines(name);
        }
        if let Some(description) = description {
            self.describe(state, description)?;
        }
        match kind.map(str::to_ascii_lowercase).as_deref() {
            Some("choice") => self.diagram.states[state].kind = Kind::Choice,
            Some("fork" | "join") => self.diagram.states[state].kind = Kind::Fork,
            Some(_) | None => {}
        }
        if opens {
            let composite = match self.diagram.states[state].composite {
                Some(composite) => composite,
                None => {
                    if self.diagram.composites.len() >= MAX_COMPOSITES {
                        return None;
                    }
                    self.diagram.composites.push(Composite {
                        label: self.diagram.states[state].name.join(" "),
                        parent: self.frame(),
                        state,
                    });
                    let composite = self.diagram.composites.len() - 1;
                    self.diagram.states[state].composite = Some(composite);
                    composite
                }
            };
            self.open.push(composite);
            if self.open.len() > layered::MAX_DEPTH {
                return None;
            }
        }
        Some(())
    }

    /// `note left of id : text`, or the first line of a note that runs to an
    /// `end note`.
    fn note(&mut self, rest: &str) -> Option<()> {
        let rest = rest
            .strip_prefix("left of")
            .or_else(|| rest.strip_prefix("right of"))?
            .trim();
        match rest.split_once(':') {
            Some((id, text)) => {
                let owner = self.state(id.trim())?;
                self.add_note(owner, text.trim())
            }
            None => {
                let owner = self.state(rest)?;
                self.note = Some((owner, Vec::new()));
                Some(())
            }
        }
    }

    fn add_note(&mut self, owner: usize, text: &str) -> Option<()> {
        let id = format!("note {}", self.diagram.states.len());
        let frame = self.diagram.states[owner].frame;
        let at = self.named(&id, || State {
            id: id.clone(),
            kind: Kind::Note,
            name: label::lines(text),
            description: Vec::new(),
            frame,
            composite: None,
        })?;
        if self.diagram.transitions.len() >= MAX_TRANSITIONS {
            return None;
        }
        self.diagram.transitions.push(Transition {
            from: owner,
            to: at,
            words: Vec::new(),
            note: true,
        });
        Some(())
    }
}

// ---------------------------------------------------------------- drawing

impl State {
    fn is_mark(&self) -> bool {
        matches!(
            self.kind,
            Kind::Start | Kind::End | Kind::Choice | Kind::Fork
        )
    }

    /// Columns and rows of its box, laid out as if the diagram ran down the
    /// page.
    fn size(&self, across: bool) -> (usize, usize) {
        let along = |(columns, rows): (usize, usize)| {
            if across {
                (rows, columns)
            } else {
                (columns, rows)
            }
        };
        match self.kind {
            Kind::Start | Kind::End => (1, 1),
            Kind::Choice => along((3, 1)),
            Kind::Fork => along((7, 1)),
            Kind::State | Kind::Note => {
                let inside = widest(&self.name).max(widest(&self.description));
                let rule = usize::from(!self.description.is_empty());
                (
                    inside + 4,
                    self.name.len() + rule + self.description.len() + 2,
                )
            }
        }
    }
}

impl StateDiagram {
    /// The state a transition into `state` arrives at: itself, or for a
    /// composite, its start, or else the first state inside it.
    fn entry(&self, state: usize, depth: usize) -> usize {
        self.stand_in(state, depth, Kind::Start, false)
    }

    /// The state a transition out of `state` leaves from.
    fn exit(&self, state: usize, depth: usize) -> usize {
        self.stand_in(state, depth, Kind::End, true)
    }

    fn stand_in(&self, state: usize, depth: usize, kind: Kind, last: bool) -> usize {
        let Some(composite) = self.states[state].composite else {
            return state;
        };
        if depth > layered::MAX_DEPTH {
            return state;
        }
        let inside = |s: &&State| s.frame == Some(composite);
        let mut held = self.states.iter().enumerate().filter(|(_, s)| inside(s));
        let found = held.clone().find(|(_, s)| s.kind == kind).map(|(at, _)| at);
        let ordinary = |(at, s): (usize, &State)| (s.kind != Kind::Note).then_some(at);
        let fallback = if last {
            held.filter_map(ordinary).next_back()
        } else {
            held.find_map(ordinary)
        };
        match found.or(fallback) {
            Some(inner) => self.stand_in(inner, depth + 1, kind, last),
            None => state,
        }
    }
}

/// Draws a state diagram, or `None` when it is too big.
pub fn draw(diagram: &StateDiagram, width: usize) -> Option<Diagram> {
    let states = &diagram.states;
    let across = diagram.direction.across();
    // A composite that holds states is drawn as a frame around them rather
    // than as a box, so the boxes are the other states.
    let framed = |s: &State| {
        s.composite
            .is_some_and(|c| states.iter().any(|t| t.frame == Some(c)))
    };
    let mut boxes: Vec<usize> = Vec::new();
    let mut box_of: Vec<Option<usize>> = vec![None; states.len()];
    for (at, state) in states.iter().enumerate() {
        if !framed(state) {
            box_of[at] = Some(boxes.len());
            boxes.push(at);
        }
    }
    let mut graph = Graph::new(diagram.direction);
    graph.boxes = boxes.iter().map(|&s| states[s].size(across)).collect();
    let mut transitions = Vec::with_capacity(diagram.transitions.len());
    for transition in &diagram.transitions {
        let from = box_of[diagram.exit(transition.from, 0)]?;
        let to = box_of[diagram.entry(transition.to, 0)]?;
        transitions.push((from, to, transition));
    }
    graph.links = transitions
        .iter()
        .map(|&(from, to, t)| Link::new(from, to, &t.words))
        .collect();
    graph.frames = diagram.composites.iter().map(|c| c.parent).collect();
    graph.framed = boxes.iter().map(|&s| states[s].frame).collect();
    graph.labels = diagram.composites.iter().map(|c| c.label.width()).collect();
    graph.room = width;
    graph.spread = true;
    graph.points = boxes
        .iter()
        .map(|&s| matches!(states[s].kind, Kind::Start | Kind::End))
        .collect();
    let placed = layered::layout(&graph)?;
    let mut canvas = Canvas::new(placed.width, placed.height)?;

    // A state's colour is its own; the start, end, choices and forks are no
    // one's, and nor are notes.
    let ink = |role: Role, b: usize| {
        let state = &states[boxes[b]];
        match state.kind {
            Kind::Note => Ink::plain(Role::Note),
            _ if state.is_mark() => Ink::plain(role),
            _ => Ink::of(role, b),
        }
    };
    placed.draw_frames(&mut canvas, Stroke::Solid);
    for (b, rect) in placed.boxes.iter().enumerate() {
        draw_state(
            &mut canvas,
            &states[boxes[b]],
            *rect,
            ink(Role::Border, b),
            across,
        );
    }
    placed.draw_words(&mut canvas, |link| {
        let (from, _, t) = transitions[link];
        (&t.words, ink(Role::Label, from))
    });
    placed.draw_lines(&mut canvas, |link| {
        let (from, _, t) = transitions[link];
        let stroke = if t.note {
            Stroke::Dotted
        } else {
            Stroke::Solid
        };
        (stroke, ink(Role::Line, from).owner)
    });
    for end in &placed.ends {
        let (from, _, t) = transitions[end.link];
        let state = &states[boxes[end.node]];
        let arrow = end.to && !t.note;
        // A start or end has no outline to join, and a bar is joined below.
        let joins = !matches!(state.kind, Kind::Start | Kind::End)
            && joinable(state, end.border, placed.boxes[end.node], across);
        if joins && end.joined(arrow) {
            let glyph = join(state.kind, &canvas, end);
            canvas.put(
                end.border.0,
                end.border.1,
                glyph,
                ink(Role::Border, end.node),
            );
        }
        if arrow {
            canvas.put(
                end.outside.0,
                end.outside.1,
                end.facing.arrow(),
                ink(Role::Arrow, from),
            );
        }
    }
    for (at, composite) in diagram.composites.iter().enumerate() {
        placed.label_frame(&mut canvas, at, &[&composite.label]);
    }

    let mut drawn = canvas.finish();
    drawn.keys = boxes.iter().map(|&s| key(&states[s].id)).collect();
    drawn.links = pairs(transitions.iter().map(|&(from, to, _)| (from, to)));
    Some(drawn)
}

/// Whether a line may be joined to a state's outline at `cell`: not at the
/// diamond in the middle of a choice's bar.
fn joinable(state: &State, cell: (usize, usize), rect: layered::Rect, across: bool) -> bool {
    if state.kind != Kind::Choice {
        return true;
    }
    let middle = if across {
        (rect.x, rect.y + rect.height / 2)
    } else {
        (rect.x + rect.width / 2, rect.y)
    };
    cell != middle
}

/// The piece of a state's outline, or its bar, where a line meets it.
fn join(kind: Kind, canvas: &Canvas, end: &layered::End) -> char {
    if kind != Kind::Fork {
        return layered::join(canvas, end);
    }
    match end.facing {
        Facing::Down => '┻',
        Facing::Up => '┳',
        Facing::Right => '┫',
        Facing::Left => '┣',
    }
}

fn draw_state(canvas: &mut Canvas, state: &State, rect: layered::Rect, outline: Ink, across: bool) {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    match state.kind {
        Kind::Start | Kind::End => {
            let glyph = if state.kind == Kind::Start {
                '●'
            } else {
                '◉'
            };
            let (cx, cy) = if across {
                (x, y + h / 2)
            } else {
                (x + w / 2, y)
            };
            canvas.put(cx, cy, glyph, outline);
        }
        Kind::Choice | Kind::Fork => {
            let bar = match (state.kind, across) {
                (Kind::Fork, false) => '━',
                (Kind::Fork, true) => '┃',
                (_, false) => '─',
                (_, true) => '│',
            };
            if across {
                for row in y..y + h {
                    canvas.put(x, row, bar, outline);
                }
            } else {
                for column in x..x + w {
                    canvas.put(column, y, bar, outline);
                }
            }
            if state.kind == Kind::Choice {
                let (cx, cy) = if across {
                    (x, y + h / 2)
                } else {
                    (x + w / 2, y)
                };
                canvas.put(cx, cy, '◇', outline);
            }
        }
        Kind::State | Kind::Note => {
            // A note's words are written as they are, from the left.
            if state.kind == Kind::Note {
                canvas.outline(x, y, w, h, SQUARE, false, outline);
                for (row, line) in state.name.iter().enumerate() {
                    canvas.text(x + 2, y + 1 + row, line, outline);
                }
            } else {
                canvas.outline(x, y, w, h, ROUND, false, outline);
                label::centred(
                    canvas,
                    x + 1,
                    y + 1,
                    w - 2,
                    &state.name,
                    Ink::plain(Role::Text),
                );
            }
            if !state.description.is_empty() {
                let rule = y + 1 + state.name.len();
                layered::rule(canvas, x, rule, w, outline);
                for (row, line) in state.description.iter().enumerate() {
                    canvas.text(x + 2, rule + 1 + row, line, Ink::plain(Role::Text));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn diagram(source: &str) -> Option<StateDiagram> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&diagram(source).expect("parses"), width).expect("draws"))
    }

    const REVIEW: &str = "[*] --> Draft
Draft --> Submitted : submit
state Review {
  [*] --> Screening
  Screening --> Decision
  Decision --> [*]
}
Submitted --> Review
Review --> Published : approved
Review --> Draft : rejected
Published --> [*]";

    #[test]
    fn states_transitions_and_starts_are_read() {
        let d = diagram(REVIEW).unwrap();
        let ids: Vec<&str> = d.states.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "[*] start root 0",
                "Draft",
                "Submitted",
                "Review",
                "[*] start 0 0",
                "Screening",
                "Decision",
                "[*] end 0 0",
                "Published",
                "[*] end root 0"
            ]
        );
        assert_eq!(d.composites.len(), 1);
        assert_eq!(d.states[5].frame, Some(0));
        assert_eq!(d.transitions[1].words, ["submit"]);
        // Into the composite is into its start; out of it, out of its end.
        assert_eq!(d.entry(3, 0), 4);
        assert_eq!(d.exit(3, 0), 7);
    }

    #[test]
    fn descriptions_kinds_notes_and_regions_are_read() {
        let d = diagram(
            "state \"Moving fast\" as Moving\nMoving : going places\nstate if_state <<choice>>\nstate fork_state <<fork>>\nnote right of Moving : a note\nnote left of Moving\n  two lines\n  of note\nend note\nstate Active {\n  [*] --> On\n  --\n  [*] --> Caps\n}\nMoving --> Moving : again\nclassDef hot fill:#f00\nclass Moving hot\nMoving:::hot --> if_state",
        )
        .unwrap();
        let moving = &d.states[0];
        assert_eq!(moving.name, ["Moving fast"]);
        assert_eq!(moving.description, ["going places", "↻ again"]);
        assert_eq!(d.states[1].kind, Kind::Choice);
        assert_eq!(d.states[2].kind, Kind::Fork);
        let notes: Vec<&State> = d.states.iter().filter(|s| s.kind == Kind::Note).collect();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[1].name, ["two lines", "of note"]);
        let starts = d.states.iter().filter(|s| s.kind == Kind::Start).count();
        assert_eq!(starts, 2, "each region has its own start");
    }

    #[test]
    fn broken_diagrams_are_not_drawn() {
        for source in [
            "",
            "state A {\n  B --> C",
            "}",
            "direction XY",
            "A -->",
            "--> B",
            "this is not a state diagram",
            "note right of A",
            "note sideways of A : x",
            "state \"unclosed as A",
        ] {
            assert!(diagram(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_review_flow_draws_with_its_composite_framed() {
        let out = drawn(REVIEW, 100);
        for words in [
            "Draft",
            "Submitted",
            "Review",
            "Screening",
            "Decision",
            "Published",
            "submit",
            "approved",
            "rejected",
        ] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(out.contains('●') && out.contains('◉'), "{out}");
        assert!(out.contains('▼') && out.contains('▲'), "{out}");
        let rows: Vec<&str> = out.lines().collect();
        let draft = rows.iter().position(|r| r.contains("Draft")).unwrap();
        let screening = rows.iter().position(|r| r.contains("Screening")).unwrap();
        assert!(draft < screening, "{out}");
        assert!(
            rows[screening].matches('│').count() >= 2,
            "the frame's sides run past it:\n{out}"
        );
    }

    #[test]
    fn choices_forks_and_sideways_diagrams_draw() {
        let out = drawn(
            "direction LR\nstate fork_state <<fork>>\n[*] --> fork_state\nfork_state --> A\nfork_state --> B\nstate join_state <<join>>\nA --> join_state\nB --> join_state\njoin_state --> C\nstate pick <<choice>>\nC --> pick\npick --> D : yes\npick --> E : no",
            160,
        );
        assert!(out.contains('┃') && out.contains('◇'), "{out}");
        assert!(out.contains('▶'), "{out}");
        for words in ["A", "B", "C", "D", "E", "yes", "no"] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
    }

    #[test]
    fn states_have_colours_and_the_marks_are_no_ones() {
        let d = diagram("[*] --> A\nA --> B : go\nB --> [*]").unwrap();
        let drawn = draw(&d, 80).unwrap();
        let ink_of = |needle: &str| super::super::ink_of(&drawn, needle);
        assert_eq!(ink_of("●"), Some(Ink::plain(Role::Border)));
        assert_eq!(ink_of("go"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(drawn.keys.len(), 4);
    }

    #[test]
    fn big_deep_and_looping_diagrams_stay_bounded() {
        let mut deep = String::new();
        for at in 0..20 {
            deep.push_str(&format!("state S{at} {{\n"));
        }
        deep.push_str("[*] --> X\n");
        deep.push_str(&"}\n".repeat(20));
        assert!(diagram(&deep).is_none(), "nested too deep");

        let mut many = String::new();
        for at in 0..MAX_STATES + 1 {
            many.push_str(&format!("S{at}\n"));
        }
        assert!(diagram(&many).is_none());

        let mut web = String::from("[*] --> S0\n");
        for a in 0..60 {
            web.push_str(&format!(
                "S{a} --> S{} : e{a}\nS{a} --> S{}\nS{a} --> S{a}\n",
                (a + 1) % 60,
                (a * 7) % 60
            ));
        }
        let d = diagram(&web).unwrap();
        if let Some(drawn) = draw(&d, 100) {
            assert!(drawn.rows.len() * drawn.width <= super::super::canvas::MAX_CELLS);
        }
    }
}
