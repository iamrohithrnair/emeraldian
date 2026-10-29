//! Sequence diagrams: `sequenceDiagram`.
//!
//! Each participant is a box at the top and the bottom with a lifeline
//! between, and each message is a row of words over a row with the arrow.
//! The participants are spaced so that every message's words fit between the
//! two lifelines it joins.

use std::collections::HashMap;

use super::canvas::{Canvas, Stroke};
use super::label::{self, widest};
use super::{Diagram, Ink, Role, key, syntax};

const MAX_ACTORS: usize = 40;
const MAX_EVENTS: usize = 400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    /// The name messages use for it, which its colour is picked by.
    pub id: String,
    pub label: Vec<String>,
    /// Drawn with round corners, for an `actor` rather than a `participant`.
    pub person: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    Arrow,
    Cross,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Message {
        from: usize,
        to: usize,
        label: String,
        stroke: Stroke,
        head: Head,
    },
    Note {
        first: usize,
        last: usize,
        side: Side,
        label: Vec<String>,
    },
    /// The start of a `loop`, `alt` or the like, an `else` inside one, or an
    /// `end`, shown as a dotted rule across the diagram.
    Rule(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequence {
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
}

/// Arrows, longest first so `-->>` isn't read as `-->`.
const ARROWS: &[(&str, Stroke, Head)] = &[
    ("<<-->>", Stroke::Dotted, Head::Arrow),
    ("<<->>", Stroke::Solid, Head::Arrow),
    ("-->>", Stroke::Dotted, Head::Arrow),
    ("->>", Stroke::Solid, Head::Arrow),
    ("--x", Stroke::Dotted, Head::Cross),
    ("-x", Stroke::Solid, Head::Cross),
    ("--)", Stroke::Dotted, Head::Arrow),
    ("-)", Stroke::Solid, Head::Arrow),
    ("-->", Stroke::Dotted, Head::Open),
    ("->", Stroke::Solid, Head::Open),
];

/// Blocks that open a framed section, and whether it is drawn: a `rect` or
/// `box` only colours things in a browser.
const BLOCKS: &[(&str, bool)] = &[
    ("loop", true),
    ("alt", true),
    ("opt", true),
    ("par", true),
    ("critical", true),
    ("break", true),
    ("rect", false),
    ("box", false),
];

struct Builder {
    sequence: Sequence,
    ids: HashMap<String, usize>,
}

impl Builder {
    fn actor(&mut self, id: &str) -> Option<usize> {
        if id.is_empty() {
            return None;
        }
        let actors = &mut self.sequence.actors;
        syntax::named(&mut self.ids, actors, id, MAX_ACTORS, || Actor {
            id: id.to_string(),
            label: label::lines(id),
            person: false,
        })
    }
}

/// Reads the lines after `sequenceDiagram`.
pub fn parse(lines: &[&str]) -> Option<Sequence> {
    let mut builder = Builder {
        sequence: Sequence {
            actors: Vec::new(),
            events: Vec::new(),
        },
        ids: HashMap::new(),
    };
    let mut open: Vec<bool> = Vec::new();
    let mut numbered = false;
    let mut number = 0;
    for line in lines {
        let line = line.trim();
        let line = line.strip_prefix("create ").unwrap_or(line).trim();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match word.to_ascii_lowercase().as_str() {
            "participant" | "actor" => {
                let (id, label) = match rest.split_once(" as ") {
                    Some((id, label)) => (id.trim(), Some(label)),
                    None => (rest, None),
                };
                let at = builder.actor(id)?;
                let actor = &mut builder.sequence.actors[at];
                actor.person = word.eq_ignore_ascii_case("actor");
                if let Some(label) = label {
                    actor.label = label::lines(label);
                }
            }
            "autonumber" => numbered = true,
            "activate" | "deactivate" | "destroy" | "title" | "link" | "links" | "properties"
            | "details" => {}
            w if w.starts_with("acctitle")
                || w.starts_with("accdescr")
                || w.starts_with("title:") => {}
            "note" => {
                let (place, text) = rest.split_once(':')?;
                let place = place.trim();
                let (side, who) = if let Some(who) = place.strip_prefix("left of") {
                    (Side::Left, who)
                } else if let Some(who) = place.strip_prefix("right of") {
                    (Side::Right, who)
                } else {
                    let who = place.strip_prefix("over")?;
                    (Side::Over, who)
                };
                let mut who = who.split(',');
                let first = builder.actor(who.next()?.trim())?;
                let last = match who.next() {
                    Some(other) => builder.actor(other.trim())?,
                    None => first,
                };
                builder.sequence.events.push(Event::Note {
                    first: first.min(last),
                    last: first.max(last),
                    side,
                    label: label::lines(text),
                });
            }
            "end" => {
                if open.pop()? {
                    builder.sequence.events.push(Event::Rule(String::new()));
                }
            }
            "else" | "and" | "option" => {
                if open.is_empty() {
                    return None;
                }
                builder
                    .sequence
                    .events
                    .push(Event::Rule(rule_text(word, rest)));
            }
            w if BLOCKS.iter().any(|(name, _)| *name == w) => {
                let shown = BLOCKS.iter().any(|(name, shown)| *name == w && *shown);
                open.push(shown);
                if shown {
                    builder
                        .sequence
                        .events
                        .push(Event::Rule(rule_text(word, rest)));
                }
            }
            _ => {
                let (from, to, stroke, head, text) = message(line)?;
                let from = builder.actor(from)?;
                let to = builder.actor(to)?;
                let mut label = label::joined(text);
                if numbered {
                    number += 1;
                    label = format!("{number}. {label}").trim_end().to_string();
                }
                builder.sequence.events.push(Event::Message {
                    from,
                    to,
                    label,
                    stroke,
                    head,
                });
            }
        }
        if builder.sequence.events.len() > MAX_EVENTS {
            return None;
        }
    }
    let sequence = builder.sequence;
    if sequence.actors.is_empty() {
        return None;
    }
    Some(sequence)
}

fn rule_text(word: &str, rest: &str) -> String {
    let text = label::joined(rest);
    if text.is_empty() {
        word.to_string()
    } else {
        format!("{word} {text}")
    }
}

/// `Alice->>+Bob: words`, as its two ends, the arrow and the words.
fn message(line: &str) -> Option<(&str, &str, Stroke, Head, &str)> {
    let (link, text) = line.split_once(':').unwrap_or((line, ""));
    let (at, arrow) = ARROWS
        .iter()
        .filter_map(|arrow| link.find(arrow.0).map(|at| (at, arrow)))
        .min_by_key(|(at, arrow)| (*at, std::cmp::Reverse(arrow.0.len())))?;
    let from = link[..at].trim();
    let to = link[at + arrow.0.len()..]
        .trim()
        .trim_start_matches(['+', '-'])
        .trim();
    if from.is_empty()
        || to.is_empty()
        || from.contains(char::is_whitespace)
        || to.contains(char::is_whitespace)
    {
        return None;
    }
    Some((from, to, arrow.1, arrow.2, text.trim()))
}

/// Draws a sequence, or `None` when it is too big.
pub fn draw(sequence: &Sequence) -> Option<Diagram> {
    use unicode_width::UnicodeWidthStr;

    let actors = &sequence.actors;
    let count = actors.len();
    let boxes: Vec<usize> = actors.iter().map(|a| widest(&a.label) + 4).collect();
    let tall = actors.iter().map(|a| a.label.len() + 2).max().unwrap_or(3);

    // How far apart each pair of lifelines must be, at least.
    let mut needs: Vec<(usize, usize, i64)> = (1..count)
        .map(|k| (k - 1, k, (boxes[k - 1] / 2 + boxes[k] / 2 + 3) as i64))
        .collect();
    let mut events_rows = 0;
    for event in &sequence.events {
        match event {
            Event::Message {
                from, to, label, ..
            } => {
                let width = label.width() as i64;
                if from == to {
                    events_rows += 3;
                    if *from + 1 < count {
                        needs.push((*from, *from + 1, width + 6));
                    }
                } else {
                    events_rows += 2;
                    needs.push(((*from).min(*to), (*from).max(*to), width + 4));
                }
            }
            Event::Note {
                first,
                last,
                side,
                label,
            } => {
                events_rows += label.len() + 2;
                let width = widest(label) as i64;
                match side {
                    Side::Right if *last + 1 < count => needs.push((*last, *last + 1, width + 8)),
                    Side::Left if *first > 0 => needs.push((*first - 1, *first, width + 8)),
                    Side::Over if first != last => needs.push((*first, *last, width - 1)),
                    _ => {}
                }
            }
            Event::Rule(_) => events_rows += 1,
        }
    }
    let mut centre = vec![0i64; count];
    for k in 0..count {
        centre[k] = needs
            .iter()
            .filter(|(_, hi, _)| *hi == k)
            .map(|(lo, _, need)| centre[*lo] + need)
            .max()
            .unwrap_or(0);
    }

    // Everything's left and right edge, to size the canvas and shift it so
    // nothing starts left of the first column.
    let mut left = 0i64;
    let mut right = 0i64;
    let mut reach = |from: i64, to: i64| {
        left = left.min(from);
        right = right.max(to);
    };
    for k in 0..count {
        let half = boxes[k] as i64 / 2;
        reach(centre[k] - half, centre[k] - half + boxes[k] as i64);
    }
    let mut notes = Vec::new();
    for event in &sequence.events {
        match event {
            Event::Message {
                from, to, label, ..
            } if from == to => {
                reach(centre[*from], centre[*from] + 5 + label.width() as i64);
            }
            Event::Note {
                first,
                last,
                side,
                label,
            } => {
                let width = widest(label) as i64 + 4;
                let (from, to) = match side {
                    Side::Right => (centre[*last] + 2, centre[*last] + 2 + width),
                    Side::Left => (centre[*first] - 2 - width, centre[*first] - 2),
                    Side::Over => {
                        let span = (centre[*last] - centre[*first] + 5).max(width);
                        let middle = (centre[*first] + centre[*last]) / 2;
                        (middle - span / 2, middle - span / 2 + span)
                    }
                };
                reach(from, to);
                notes.push((from, to));
            }
            Event::Rule(text) => reach(0, centre[0] + text.width() as i64 + 6),
            Event::Message { .. } => {}
        }
    }
    let shift = -left;
    let x = |at: i64| usize::try_from(at + shift).unwrap_or(0);
    let width = usize::try_from(right - left).ok()?;
    let height = tall * 2 + events_rows;
    let mut canvas = Canvas::new(width, height)?;

    // Boxes, notes and words first, so the lines stop at them. Each
    // participant has a colour, which its boxes, its lifeline and the
    // messages it sends are drawn in.
    for (k, actor) in actors.iter().enumerate() {
        let column = x(centre[k] - boxes[k] as i64 / 2);
        let corners = if actor.person {
            ['╭', '╮', '╰', '╯']
        } else {
            ['┌', '┐', '└', '┘']
        };
        let outline = Ink::of(Role::Border, k);
        for top in [0, height - tall] {
            canvas.outline(column, top, boxes[k], tall, corners, false, outline);
            for (row, line) in actor.label.iter().enumerate() {
                let indent = (boxes[k] - 2 - line.width()) / 2;
                let words = Ink::plain(Role::Text);
                canvas.text(column + 1 + indent, top + 1 + row, line, words);
            }
        }
    }
    let mut row = tall;
    let mut notes = notes.into_iter();
    let mut arrows = Vec::new();
    let mut lines = Vec::new();
    for event in &sequence.events {
        match event {
            Event::Message {
                from,
                to,
                label,
                stroke,
                head,
            } => {
                let (a, b) = (x(centre[*from]), x(centre[*to]));
                let sender = Ink::of(Role::Line, *from).owner;
                if from == to {
                    canvas.text(a + 2, row, label, Ink::of(Role::Label, *from));
                    lines.push((
                        vec![
                            (a, row + 1),
                            (a + 3, row + 1),
                            (a + 3, row + 2),
                            (a + 1, row + 2),
                        ],
                        *stroke,
                        sender,
                    ));
                    arrows.push((a + 1, row + 2, glyph(*head, false), *from));
                    row += 3;
                } else {
                    let (low, high) = (a.min(b), a.max(b));
                    let indent = (high - low).saturating_sub(label.width()) / 2;
                    let words = Ink::of(Role::Label, *from);
                    canvas.text(low + indent.max(1), row, label, words);
                    let toward = b > a;
                    let end = if *head == Head::Open {
                        b
                    } else if toward {
                        b - 1
                    } else {
                        b + 1
                    };
                    lines.push((vec![(a, row + 1), (end, row + 1)], *stroke, sender));
                    if *head != Head::Open {
                        arrows.push((end, row + 1, glyph(*head, toward), *from));
                    }
                    row += 2;
                }
            }
            Event::Note { label, .. } => {
                let (from, to) = notes.next()?;
                let (column, wide) = (x(from), usize::try_from(to - from).ok()?);
                let note = Ink::plain(Role::Note);
                canvas.outline(
                    column,
                    row,
                    wide,
                    label.len() + 2,
                    ['┌', '┐', '└', '┘'],
                    false,
                    note,
                );
                // Filled with blanks first, so the lifelines don't run
                // through it.
                for inside in row + 1..row + 1 + label.len() {
                    for blank in column + 1..column + wide - 1 {
                        canvas.put(blank, inside, ' ', note);
                    }
                }
                for (line_at, line) in label.iter().enumerate() {
                    canvas.text(column + 2, row + 1 + line_at, line, note);
                }
                row += label.len() + 2;
            }
            Event::Rule(text) => {
                lines.push((vec![(0, row), (width - 1, row)], Stroke::Dotted, None));
                // Just right of the first lifeline, so it isn't cut.
                if !text.is_empty() {
                    let words = Ink::plain(Role::Note);
                    canvas.text(x(centre[0]) + 2, row, &format!(" {text} "), words);
                }
                row += 1;
            }
        }
    }
    // Lifelines before messages, so a message keeps its colour where it
    // crosses one.
    for (k, &at) in centre.iter().enumerate() {
        let column = x(at);
        let own = Ink::of(Role::Line, k).owner;
        canvas.line(
            (column, tall - 1),
            (column, height - tall),
            Stroke::Solid,
            own,
        );
        canvas.put(column, tall - 1, '┬', Ink::of(Role::Border, k));
        canvas.put(column, height - tall, '┴', Ink::of(Role::Border, k));
    }
    for (points, stroke, owner) in lines {
        canvas.path(&points, stroke, owner);
    }
    for (column, row, arrow, owner) in arrows {
        canvas.put(column, row, arrow, Ink::of(Role::Arrow, owner));
    }
    let mut diagram = canvas.finish();
    diagram.keys = actors.iter().map(|actor| key(&actor.id)).collect();
    diagram.links = links(sequence);
    Some(diagram)
}

/// Participants side by side, and those that message each other, which
/// should differ in colour.
fn links(sequence: &Sequence) -> Vec<(u16, u16)> {
    let pair =
        |a: usize, b: usize| Some((u16::try_from(a.min(b)).ok()?, u16::try_from(a.max(b)).ok()?));
    let mut links: Vec<(u16, u16)> = (1..sequence.actors.len())
        .filter_map(|k| pair(k - 1, k))
        .collect();
    for event in &sequence.events {
        if let Event::Message { from, to, .. } = event
            && from != to
            && let Some(link) = pair(*from, *to)
        {
            links.push(link);
        }
    }
    links.sort_unstable();
    links.dedup();
    links
}

fn glyph(head: Head, rightwards: bool) -> char {
    match (head, rightwards) {
        (Head::Cross, _) => '×',
        (_, true) => '▶',
        (_, false) => '◀',
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn sequence(source: &str) -> Option<Sequence> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    #[test]
    fn participants_messages_and_notes_are_read() {
        let s = sequence(
            "participant A as Alice\nactor B as Bob\nA->>+B: Hello\nB-->>-A: Hi back\nA-xB: lost\nA-)B: async\nNote right of B: thinking\nNote over A,B: both",
        )
        .unwrap();
        assert_eq!(s.actors.len(), 2);
        assert_eq!(s.actors[0].label, ["Alice"]);
        assert!(s.actors[1].person);
        assert_eq!(s.events.len(), 6);
        assert!(matches!(
            &s.events[1],
            Event::Message { from: 1, to: 0, stroke: Stroke::Dotted, head: Head::Arrow, label } if label == "Hi back"
        ));
        assert!(matches!(
            &s.events[2],
            Event::Message {
                head: Head::Cross,
                ..
            }
        ));
        assert!(matches!(
            &s.events[5],
            Event::Note {
                first: 0,
                last: 1,
                side: Side::Over,
                ..
            }
        ));
    }

    #[test]
    fn blocks_become_rules_and_must_close() {
        let s = sequence("loop Every minute\nA->>B: ping\nend\nrect rgb(0,0,0)\nA->>B: x\nend")
            .unwrap();
        let rules = s
            .events
            .iter()
            .filter(|e| matches!(e, Event::Rule(_)))
            .count();
        assert_eq!(rules, 2, "a rect is not drawn");
        assert!(sequence("A->>B: x\nend").is_none());
        assert!(sequence("A->>B: x\nelse").is_none());
        assert!(sequence("this is not a message").is_none());
        assert!(sequence("Note sideways of A: x").is_none());
        assert!(sequence("").is_none());
    }

    #[test]
    fn autonumber_numbers_the_messages() {
        let s = sequence("autonumber\nA->>B: one\nB->>A: two").unwrap();
        assert!(matches!(&s.events[1], Event::Message { label, .. } if label == "2. two"));
    }

    #[test]
    fn a_conversation_draws_with_lifelines_and_arrows() {
        let s = sequence("A->>B: Hello\nB-->>A: Hi").unwrap();
        let out = plain(&draw(&s).unwrap());
        let rows: Vec<&str> = out.lines().collect();
        assert!(rows[1].contains('A') && rows[1].contains('B'));
        assert!(out.contains("Hello") && out.contains("Hi"));
        assert!(out.contains('▶') && out.contains('◀'), "{out}");
        assert_eq!(out.matches('A').count(), 2, "header and footer:\n{out}");
    }

    #[test]
    fn words_fit_between_lifelines() {
        let s = sequence("A->>B: a much longer message than the boxes are wide").unwrap();
        let out = plain(&draw(&s).unwrap());
        assert!(
            out.contains("a much longer message than the boxes are wide"),
            "{out}"
        );
    }

    #[test]
    fn self_messages_notes_and_unicode_draw() {
        let s = sequence(
            "participant 甲\n甲->>甲: 自分\nNote left of 甲: 左 🙂\nNote right of 甲: right\nloop retry\n甲->>B: again\nend",
        )
        .unwrap();
        let out = plain(&draw(&s).unwrap());
        assert!(
            out.contains("自分") && out.contains("左 🙂") && out.contains("loop retry"),
            "{out}"
        );
    }

    #[test]
    fn each_participant_colours_its_lifeline_and_what_it_sends() {
        let s = sequence("A->>B: hi\nB-->>A: back\nNote over A: aside\nC->>C: me").unwrap();
        let diagram = draw(&s).unwrap();
        let ink_of = |needle: &str| {
            diagram
                .rows
                .iter()
                .flatten()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, ink)| *ink)
        };
        assert_eq!(ink_of("hi"), Some(Ink::of(Role::Label, 0)));
        assert_eq!(ink_of("back"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("▶"), Some(Ink::of(Role::Arrow, 0)));
        assert_eq!(ink_of("◀"), Some(Ink::of(Role::Arrow, 1)));
        assert_eq!(ink_of("aside"), Some(Ink::plain(Role::Note)));
        assert_eq!(ink_of("me"), Some(Ink::of(Role::Label, 2)));
        // Below the boxes, each lifeline is its participant's.
        let mut lifelines: Vec<Option<u16>> = diagram.rows[3]
            .iter()
            .filter(|(text, ink)| ink.role == Role::Line && text.contains('│'))
            .map(|(_, ink)| ink.owner)
            .collect();
        lifelines.dedup();
        assert_eq!(lifelines, [Some(0), Some(1), Some(2)]);
        assert_eq!(diagram.keys, [key("A"), key("B"), key("C")]);
        assert_eq!(diagram.links, [(0, 1), (1, 2)], "side by side, and talking");
    }

    #[test]
    fn many_participants_and_messages_stay_bounded() {
        let mut source = String::new();
        for at in 0..39 {
            source.push_str(&format!("p{at}->>p{}: step {at}\n", at + 1));
        }
        assert!(draw(&sequence(&source).unwrap()).is_some());
        let mut crowd = String::new();
        for at in 0..60 {
            crowd.push_str(&format!("participant p{at}\n"));
        }
        assert!(sequence(&crowd).is_none());
    }
}
