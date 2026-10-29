//! Class diagrams: `classDiagram`.
//!
//! Each class is a box of up to three compartments: its annotation and name,
//! its attributes, and its methods, with generics written `List~int~` shown
//! as `List<int>`. Relationships carry UML's marks where they meet a class:
//! a hollow triangle for inheritance and realisation, a filled diamond for
//! composition, a hollow one for aggregation, an arrowhead for association
//! and a circle for a lollipop interface, on a dotted line for a dependency
//! or a realisation. A relationship's label and the multiplicity at each end
//! are written on it, as `1 lays many`. Namespaces are frames around their
//! classes, and notes are boxes joined by a dotted line to their class.
//!
//! Laid out by [`super::layered`], so the parents in a hierarchy sit above
//! their children, as Mermaid draws them. The compartments, and the marks
//! for each kind of relationship, follow beautiful-mermaid (MIT), the
//! renderer prismantis draws class diagrams with. The code is our own.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::layered::{self, Direction, Facing, Graph, Link};
use super::{Diagram, Ink, Role, key, label_lines, truncate, widest};

const MAX_CLASSES: usize = 150;
const MAX_MEMBERS: usize = 60;
const MAX_RELATIONS: usize = 300;
const MAX_NAMESPACES: usize = 60;
/// Columns of a member before it is cut short.
const MEMBER_WIDTH: usize = 40;

/// What is drawn where a relationship meets a class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `<|` or `|>`: inheritance, or realisation on a dotted line.
    Triangle,
    /// `*`: composition.
    Filled,
    /// `o`: aggregation.
    Hollow,
    /// `<` or `>`: association, or dependency on a dotted line.
    Arrow,
    /// `()`: a lollipop interface.
    Lollipop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Class {
    /// The name relationships use for it, which its colour is picked by.
    pub id: String,
    pub name: Vec<String>,
    pub annotation: Option<String>,
    pub attributes: Vec<String>,
    pub methods: Vec<String>,
    /// The namespace it is drawn in.
    pub frame: Option<usize>,
    /// Whether a relationship leads from it back to itself.
    pub looped: bool,
    /// A note rather than a class.
    pub note: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub from: usize,
    pub to: usize,
    pub stroke: Stroke,
    /// The marks at the `from` and `to` ends.
    pub tail: Option<Mark>,
    pub head: Option<Mark>,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    pub label: String,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDiagram {
    pub direction: Direction,
    pub classes: Vec<Class>,
    pub relations: Vec<Relation>,
    pub namespaces: Vec<Namespace>,
}

// ---------------------------------------------------------------- parsing

/// What a `{` opened.
enum Open {
    Body(usize),
    Namespace(usize),
}

struct Reader {
    diagram: ClassDiagram,
    ids: HashMap<String, usize>,
    namespace_ids: HashMap<String, usize>,
    open: Vec<Open>,
}

/// Statements about how things look, or what happens on a click, which a
/// drawing in text has no use for.
const IGNORED: &[&str] = &[
    "style", "classDef", "cssClass", "click", "link", "callback", "accTitle", "accDescr", "title",
];

/// Reads the lines after `classDiagram`.
pub fn parse(lines: &[&str]) -> Option<ClassDiagram> {
    let mut reader = Reader {
        diagram: ClassDiagram {
            direction: Direction::Down,
            classes: Vec::new(),
            relations: Vec::new(),
            namespaces: Vec::new(),
        },
        ids: HashMap::new(),
        namespace_ids: HashMap::new(),
        open: Vec::new(),
    };
    for line in lines {
        reader.line(line.trim())?;
    }
    if !reader.open.is_empty() || reader.diagram.classes.is_empty() {
        return None;
    }
    Some(reader.diagram)
}

impl Reader {
    fn line(&mut self, line: &str) -> Option<()> {
        if line.is_empty() {
            return Some(());
        }
        if let Some(Open::Body(class)) = self.open.last() {
            let class = *class;
            if line == "}" {
                self.open.pop();
                return Some(());
            }
            let (member, closed) = match line.strip_suffix('}') {
                Some(member) => (member.trim(), true),
                None => (line, false),
            };
            self.member(class, member)?;
            if closed {
                self.open.pop();
            }
            return Some(());
        }
        if line == "}" {
            self.open.pop()?;
            return Some(());
        }
        let first = line
            .split(|c: char| c.is_whitespace() || c == ':')
            .next()
            .unwrap_or("");
        if IGNORED.contains(&first) {
            return Some(());
        }
        match first {
            "direction" => {
                let name = line["direction".len()..].trim();
                self.diagram.direction = match name {
                    "TB" | "TD" => Direction::Down,
                    "BT" => Direction::Up,
                    "LR" => Direction::Right,
                    "RL" => Direction::Left,
                    _ => return None,
                };
                Some(())
            }
            "namespace" => self.namespace(line["namespace".len()..].trim()),
            "class" => self.class_statement(line["class".len()..].trim()),
            "note" => self.note(line["note".len()..].trim()),
            _ if line.starts_with("<<") => {
                let close = line.find(">>")?;
                let annotation = line[2..close].trim().to_string();
                let class = self.class(line[close + 2..].trim())?;
                self.diagram.classes[class].annotation = Some(annotation);
                Some(())
            }
            _ => match relation(line) {
                Some(parts) => self.relation(&parts),
                None => self.member_line(line),
            },
        }
    }

    fn frame(&self) -> Option<usize> {
        self.open.iter().rev().find_map(|open| match open {
            Open::Namespace(at) => Some(*at),
            Open::Body(_) => None,
        })
    }

    /// The class named `name`, made the first time it is named.
    fn class(&mut self, name: &str) -> Option<usize> {
        let id = class_id(name);
        if id.is_empty() {
            return None;
        }
        if let Some(&at) = self.ids.get(&id) {
            return Some(at);
        }
        if self.diagram.classes.len() >= MAX_CLASSES {
            return None;
        }
        self.diagram.classes.push(Class {
            name: label_lines(&display_name(name))
                .iter()
                .map(|line| generics(line))
                .collect(),
            id: id.clone(),
            annotation: None,
            attributes: Vec::new(),
            methods: Vec::new(),
            frame: self.frame(),
            looped: false,
            note: false,
        });
        self.ids.insert(id, self.diagram.classes.len() - 1);
        Some(self.diagram.classes.len() - 1)
    }

    /// `class Name`, with a label, generics, an annotation or a body.
    fn class_statement(&mut self, rest: &str) -> Option<()> {
        let (head, body) = match rest.find('{') {
            Some(open) => (rest[..open].trim(), Some(rest[open + 1..].trim())),
            None => (rest, None),
        };
        let head = head.split(":::").next().unwrap_or(head).trim();
        let (head, annotation) = match head.find("<<") {
            Some(open) => {
                let close = head[open..].find(">>")? + open;
                (
                    head[..open].trim(),
                    Some(head[open + 2..close].trim().to_string()),
                )
            }
            None => (head, None),
        };
        let (name, label) = match head.find('[') {
            Some(open) if head.ends_with(']') => {
                (&head[..open], Some(&head[open + 1..head.len() - 1]))
            }
            Some(_) => return None,
            None => (head, None),
        };
        let class = self.class(name.trim())?;
        // Declared inside a namespace, it is drawn there.
        if self.frame().is_some() {
            self.diagram.classes[class].frame = self.frame();
        }
        if let Some(label) = label {
            self.diagram.classes[class].name = label_lines(label);
        }
        if annotation.is_some() {
            self.diagram.classes[class].annotation = annotation;
        }
        if let Some(body) = body {
            self.open.push(Open::Body(class));
            if !body.is_empty() {
                return self.line(body);
            }
        }
        Some(())
    }

    /// `Name : member`, or a member written on its own inside a body.
    fn member_line(&mut self, line: &str) -> Option<()> {
        let (name, member) = line.split_once(':')?;
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) && !name.starts_with('`') {
            return None;
        }
        let class = self.class(name)?;
        self.member(class, member.trim())
    }

    fn member(&mut self, class: usize, member: &str) -> Option<()> {
        if member.is_empty() {
            return Some(());
        }
        let class = &mut self.diagram.classes[class];
        if let Some(inner) = member.strip_prefix("<<").and_then(|m| m.strip_suffix(">>")) {
            class.annotation = Some(inner.trim().to_string());
            return Some(());
        }
        if class.attributes.len() + class.methods.len() >= MAX_MEMBERS {
            return None;
        }
        let shown = truncate(&generics(member), MEMBER_WIDTH);
        if member.contains('(') {
            class.methods.push(shown);
        } else {
            class.attributes.push(shown);
        }
        Some(())
    }

    /// `namespace Name {`, which may be dotted, as `A.B`, for one in another.
    fn namespace(&mut self, rest: &str) -> Option<()> {
        let rest = rest.strip_suffix('{')?.trim();
        let (name, label) = match rest.find('[') {
            Some(open) if rest.ends_with(']') => {
                (rest[..open].trim(), Some(&rest[open + 1..rest.len() - 1]))
            }
            Some(_) => return None,
            None => (rest, None),
        };
        if name.is_empty() {
            return None;
        }
        // Each part of a dotted name is a namespace of its own, inside the
        // one before it.
        let mut parent = self.frame();
        let mut path = String::new();
        let parts: Vec<&str> = name.split('.').collect();
        for (at, part) in parts.iter().enumerate() {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(part);
            let full = match parent {
                Some(p) if at == 0 => format!("{}.{path}", p),
                _ => path.clone(),
            };
            let found = self.namespace_ids.get(&full).copied();
            let frame = match found {
                Some(frame) => frame,
                None => {
                    if self.diagram.namespaces.len() >= MAX_NAMESPACES {
                        return None;
                    }
                    let last = at + 1 == parts.len();
                    let shown = match label {
                        Some(label) if last => label_lines(label).join(" "),
                        _ => (*part).to_string(),
                    };
                    self.diagram.namespaces.push(Namespace {
                        label: shown,
                        parent,
                    });
                    let frame = self.diagram.namespaces.len() - 1;
                    self.namespace_ids.insert(full, frame);
                    frame
                }
            };
            parent = Some(frame);
        }
        self.open.push(Open::Namespace(parent?));
        if self.open.len() > layered::MAX_DEPTH {
            return None;
        }
        Some(())
    }

    /// `note for Name "text"`, or a `note "text"` of its own.
    fn note(&mut self, rest: &str) -> Option<()> {
        let (owner, text) = match rest.strip_prefix("for ") {
            Some(rest) => {
                let rest = rest.trim();
                let split = rest.find(char::is_whitespace)?;
                (Some(&rest[..split]), rest[split..].trim())
            }
            None => (None, rest),
        };
        let text = text.strip_prefix('"')?.strip_suffix('"')?;
        let owner = match owner {
            Some(name) => Some(self.class(name)?),
            None => None,
        };
        if self.diagram.classes.len() >= MAX_CLASSES {
            return None;
        }
        let at = self.diagram.classes.len();
        self.diagram.classes.push(Class {
            id: format!("note {at}"),
            name: label_lines(text),
            annotation: None,
            attributes: Vec::new(),
            methods: Vec::new(),
            frame: owner.and_then(|owner| self.diagram.classes[owner].frame),
            looped: false,
            note: true,
        });
        if let Some(owner) = owner {
            self.push(Relation {
                from: owner,
                to: at,
                stroke: Stroke::Dotted,
                tail: None,
                head: None,
                words: Vec::new(),
            })?;
        }
        Some(())
    }

    fn relation(&mut self, parts: &Parts) -> Option<()> {
        let from = self.class(parts.from)?;
        let to = self.class(parts.to)?;
        if from == to {
            self.diagram.classes[from].looped = true;
            return Some(());
        }
        let label = label_lines(parts.label).join(" ");
        let text = match (parts.tail_count, label.is_empty(), parts.head_count) {
            (None, _, None) => label,
            (Some(a), true, Some(b)) => format!("{a} to {b}"),
            (a, _, b) => [a.unwrap_or(""), &label, b.unwrap_or("")]
                .iter()
                .filter(|part| !part.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        };
        let words = if text.is_empty() {
            Vec::new()
        } else {
            label_lines(&text)
        };
        self.push(Relation {
            from,
            to,
            stroke: parts.stroke,
            tail: parts.tail,
            head: parts.head,
            words,
        })
    }

    fn push(&mut self, relation: Relation) -> Option<()> {
        if self.diagram.relations.len() >= MAX_RELATIONS {
            return None;
        }
        self.diagram.relations.push(relation);
        Some(())
    }
}

/// A class's id from how it is written: without backticks, a label or
/// generics.
fn class_id(name: &str) -> String {
    let name = name.trim();
    if let Some(inner) = name.strip_prefix('`').and_then(|n| n.strip_suffix('`')) {
        return inner.to_string();
    }
    let name = name.split('[').next().unwrap_or(name);
    name.split('~').next().unwrap_or(name).trim().to_string()
}

/// How a class's name is shown: as written, without backticks.
fn display_name(name: &str) -> String {
    let name = name.trim();
    name.strip_prefix('`')
        .and_then(|n| n.strip_suffix('`'))
        .unwrap_or(name)
        .to_string()
}

/// `List~int~` as `List<int>`, leaving a leading `~` that marks a member as
/// package-private as it is.
fn generics(text: &str) -> String {
    let (lead, rest) = match text.strip_prefix('~') {
        Some(rest) => ("~", rest),
        None => ("", text),
    };
    let chars: Vec<char> = rest.chars().collect();
    let mut out = String::from(lead);
    let mut depth = 0usize;
    for (at, &c) in chars.iter().enumerate() {
        if c != '~' {
            out.push(c);
            continue;
        }
        // A `~` before a name opens a generic, unless one is open and there
        // aren't tildes enough left to close another, as in `Map~K~ x~y~`;
        // otherwise it closes one, as the two at the end of
        // `List~List~int~~` do.
        let names = chars
            .get(at + 1)
            .is_some_and(|&n| n.is_alphanumeric() || n == '_');
        let left = chars[at..].iter().filter(|&&c| c == '~').count();
        if names && (depth == 0 || left > depth) {
            out.push('<');
            depth += 1;
        } else if depth > 0 {
            out.push('>');
            depth -= 1;
        } else {
            out.push('~');
        }
    }
    out
}

/// The parts of a relationship statement.
struct Parts<'a> {
    from: &'a str,
    to: &'a str,
    stroke: Stroke,
    tail: Option<Mark>,
    head: Option<Mark>,
    tail_count: Option<&'a str>,
    head_count: Option<&'a str>,
    label: &'a str,
}

/// `A "1" <|-- "many" B : label`, as its parts, or `None` if the line has
/// no relationship in it.
fn relation(line: &str) -> Option<Parts<'_>> {
    let (link, label) = split_label(line);
    // The line of the arrow, `--` or `..`, outside quotes.
    let bytes = link.as_bytes();
    let mut quoted = false;
    let mut found = None;
    for at in 0..bytes.len() {
        match bytes[at] {
            b'"' => quoted = !quoted,
            b'-' | b'.' if !quoted && bytes.get(at + 1) == Some(&bytes[at]) => {
                found = Some(at);
                break;
            }
            _ => {}
        }
    }
    let start = found?;
    let line_char = bytes[start];
    let mut end = start;
    while bytes.get(end) == Some(&line_char) {
        end += 1;
    }
    let stroke = if line_char == b'.' {
        Stroke::Dotted
    } else {
        Stroke::Solid
    };
    let before = &link[..start];
    let after = &link[end..];
    let (before, tail) = left_mark(before);
    let (after, head) = right_mark(after);
    let (from, tail_count) = count_at_end(before.trim_end());
    let (to, head_count) = count_at_start(after.trim_start());
    let (from, to) = (from.trim(), to.trim());
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some(Parts {
        from,
        to,
        stroke,
        tail,
        head,
        tail_count,
        head_count,
        label,
    })
}

/// Splits `link : label` at the first colon outside quotes and backticks.
fn split_label(line: &str) -> (&str, &str) {
    let mut quoted = false;
    for (at, c) in line.char_indices() {
        match c {
            '"' | '`' => quoted = !quoted,
            ':' if !quoted => return (&line[..at], line[at + 1..].trim()),
            _ => {}
        }
    }
    (line, "")
}

fn left_mark(text: &str) -> (&str, Option<Mark>) {
    for (token, mark) in [
        ("<|", Mark::Triangle),
        ("()", Mark::Lollipop),
        ("*", Mark::Filled),
        ("o", Mark::Hollow),
        ("<", Mark::Arrow),
    ] {
        if let Some(rest) = text.strip_suffix(token) {
            // An `o` is a mark only where it isn't the end of a name.
            if token == "o" && !rest.ends_with([' ', '"']) {
                continue;
            }
            return (rest, Some(mark));
        }
    }
    (text, None)
}

fn right_mark(text: &str) -> (&str, Option<Mark>) {
    for (token, mark) in [
        ("|>", Mark::Triangle),
        ("()", Mark::Lollipop),
        ("*", Mark::Filled),
        ("o", Mark::Hollow),
        (">", Mark::Arrow),
    ] {
        if let Some(rest) = text.strip_prefix(token) {
            if token == "o" && !rest.starts_with([' ', '"']) {
                continue;
            }
            return (rest, Some(mark));
        }
    }
    (text, None)
}

/// A name with a multiplicity in quotes after it: `Customer "1"`.
fn count_at_end(text: &str) -> (&str, Option<&str>) {
    if let Some(inner) = text.strip_suffix('"')
        && let Some(open) = inner.rfind('"')
    {
        return (&inner[..open], Some(inner[open + 1..].trim()));
    }
    (text, None)
}

/// A name with a multiplicity in quotes before it: `"many" Egg`.
fn count_at_start(text: &str) -> (&str, Option<&str>) {
    if let Some(inner) = text.strip_prefix('"')
        && let Some(close) = inner.find('"')
    {
        return (&inner[close + 1..], Some(inner[..close].trim()));
    }
    (text, None)
}

// ---------------------------------------------------------------- drawing

impl Class {
    fn header(&self) -> Vec<String> {
        let mut header = Vec::new();
        if let Some(annotation) = &self.annotation {
            header.push(format!("<<{annotation}>>"));
        }
        header.extend(self.name.iter().cloned());
        if self.looped
            && let Some(last) = header.last_mut()
        {
            last.push_str(" ↻");
        }
        header
    }

    /// Its compartments after the header: attributes, then methods, each
    /// left out when it has nothing in it.
    fn compartments(&self) -> Vec<&[String]> {
        [&self.attributes[..], &self.methods[..]]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect()
    }

    fn size(&self) -> (usize, usize) {
        let header = self.header();
        let parts = self.compartments();
        let inside = parts
            .iter()
            .map(|part| widest(part))
            .chain([widest(&header)])
            .max()
            .unwrap_or(0);
        let rows: usize = parts.iter().map(|part| part.len() + 1).sum();
        (inside + 4, header.len() + rows + 2)
    }
}

/// Draws a class diagram, or `None` when it is too big.
pub fn draw(diagram: &ClassDiagram) -> Option<Diagram> {
    let classes = &diagram.classes;
    let mut graph = Graph::new(diagram.direction);
    graph.boxes = classes.iter().map(Class::size).collect();
    graph.links = diagram
        .relations
        .iter()
        .map(|relation| Link {
            from: relation.from,
            to: relation.to,
            words: (!relation.words.is_empty())
                .then(|| (widest(&relation.words), relation.words.len())),
        })
        .collect();
    graph.frames = diagram.namespaces.iter().map(|n| n.parent).collect();
    graph.framed = classes.iter().map(|class| class.frame).collect();
    graph.labels = diagram.namespaces.iter().map(|n| n.label.width()).collect();
    graph.spread = true;
    let placed = layered::layout(&graph)?;
    let mut canvas = Canvas::new(placed.width, placed.height)?;

    placed.draw_frames(&mut canvas, Stroke::Dotted);
    for (at, (class, rect)) in classes.iter().zip(&placed.boxes).enumerate() {
        draw_class(&mut canvas, class, at, *rect);
    }
    let ink = |role: Role, at: usize| {
        if classes[at].note {
            Ink::plain(Role::Note)
        } else {
            Ink::of(role, at)
        }
    };
    for (relation, rect) in diagram.relations.iter().zip(&placed.words) {
        if let Some(rect) = rect {
            let words = ink(Role::Label, relation.from);
            layered::centred(
                &mut canvas,
                rect.x,
                rect.y,
                rect.width,
                &relation.words,
                words,
            );
        }
    }
    placed.draw_lines(&mut canvas, |at| {
        let relation = &diagram.relations[at];
        (relation.stroke, ink(Role::Line, relation.from).owner)
    });
    for end in &placed.ends {
        let relation = &diagram.relations[end.link];
        let mark = if end.to { relation.head } else { relation.tail };
        let join = ink(Role::Border, end.node);
        if end.leaving || mark.is_none() {
            let glyph = layered::join(&canvas, end);
            canvas.put(end.border.0, end.border.1, glyph, join);
        }
        if let Some(mark) = mark {
            let glyph = glyph(mark, end.facing);
            let head = ink(Role::Arrow, relation.from);
            canvas.put(end.outside.0, end.outside.1, glyph, head);
        }
    }
    for (at, namespace) in diagram.namespaces.iter().enumerate() {
        placed.label_frame(&mut canvas, at, &[&namespace.label]);
    }

    let mut drawn = canvas.finish();
    drawn.keys = classes.iter().map(|class| key(&class.id)).collect();
    drawn.links = diagram
        .relations
        .iter()
        .filter_map(|r| Some((u16::try_from(r.from).ok()?, u16::try_from(r.to).ok()?)))
        .collect();
    Some(drawn)
}

fn glyph(mark: Mark, facing: Facing) -> char {
    match mark {
        Mark::Triangle => facing.hollow(),
        Mark::Arrow => facing.arrow(),
        Mark::Filled => '◆',
        Mark::Hollow => '◇',
        Mark::Lollipop => '○',
    }
}

fn draw_class(canvas: &mut Canvas, class: &Class, at: usize, rect: layered::Rect) {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    let (outline, words) = if class.note {
        (Ink::plain(Role::Note), Ink::plain(Role::Note))
    } else {
        (Ink::of(Role::Border, at), Ink::plain(Role::Text))
    };
    let corners = if class.note {
        ['╭', '╮', '╰', '╯']
    } else {
        ['┌', '┐', '└', '┘']
    };
    canvas.outline(x, y, w, h, corners, false, outline);
    let mut row = y + 1;
    let header = class.header();
    if class.annotation.is_some() && !class.note {
        let annotation = std::slice::from_ref(&header[0]);
        layered::centred(
            canvas,
            x + 1,
            row,
            w - 2,
            annotation,
            Ink::of(Role::Label, at),
        );
        layered::centred(canvas, x + 1, row + 1, w - 2, &header[1..], words);
    } else {
        layered::centred(canvas, x + 1, row, w - 2, &header, words);
    }
    row += header.len();
    for part in class.compartments() {
        canvas.put(x, row, '├', outline);
        canvas.put(x + w - 1, row, '┤', outline);
        for column in x + 1..x + w - 1 {
            canvas.put(column, row, '─', outline);
        }
        row += 1;
        for member in part {
            canvas.text(x + 2, row, member, words);
            row += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn diagram(source: &str) -> Option<ClassDiagram> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str) -> String {
        plain(&draw(&diagram(source).expect("parses")).expect("draws"))
    }

    const ANIMALS: &str = "class Animal {
    <<abstract>>
    +int age
    +String gender
    +isMammal() bool
    +mate()
}
class Duck{
    +String beakColor
    +swim()
}
Animal <|-- Duck
Animal <|-- Fish : is a
Duck \"1\" *-- \"many\" Egg : lays
Zoo o-- Animal
Fish : -int sizeInFeet
Fish : -canEat()";

    #[test]
    fn classes_members_and_annotations_are_read() {
        let d = diagram(ANIMALS).unwrap();
        let ids: Vec<&str> = d.classes.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["Animal", "Duck", "Fish", "Egg", "Zoo"]);
        let animal = &d.classes[0];
        assert_eq!(animal.annotation.as_deref(), Some("abstract"));
        assert_eq!(animal.attributes, ["+int age", "+String gender"]);
        assert_eq!(animal.methods, ["+isMammal() bool", "+mate()"]);
        assert_eq!(d.classes[2].attributes, ["-int sizeInFeet"]);
        assert_eq!(d.classes[2].methods, ["-canEat()"]);
    }

    #[test]
    fn every_kind_of_relationship_is_read() {
        let d = diagram(
            "classA <|-- classB\nclassC *-- classD\nclassE o-- classF\nclassG <-- classH\nclassI -- classJ\nclassK <.. classL\nclassM <|.. classN\nclassO .. classP\nclassQ --|> classR\nclassS ..> classT\nclassU --() classV\nAnimal <|--|> Zebra",
        )
        .unwrap();
        let marks: Vec<(Option<Mark>, Stroke, Option<Mark>)> = d
            .relations
            .iter()
            .map(|r| (r.tail, r.stroke, r.head))
            .collect();
        use Mark::*;
        use Stroke::*;
        assert_eq!(
            marks,
            [
                (Some(Triangle), Solid, None),
                (Some(Filled), Solid, None),
                (Some(Hollow), Solid, None),
                (Some(Arrow), Solid, None),
                (None, Solid, None),
                (Some(Arrow), Dotted, None),
                (Some(Triangle), Dotted, None),
                (None, Dotted, None),
                (None, Solid, Some(Triangle)),
                (None, Dotted, Some(Arrow)),
                (None, Solid, Some(Lollipop)),
                (Some(Triangle), Solid, Some(Triangle)),
            ]
        );
    }

    #[test]
    fn labels_multiplicities_generics_and_namespaces_are_read() {
        let d = diagram(
            "class Square~Shape~{\n  List~int~ position\n  getPoints() List~List~int~~\n}\nCustomer \"1\" --> \"*\" Ticket\nGalaxy --> \"many\" Star : Contains\nnamespace Shapes.Flat {\n  class Triangle\n}\nclass Animal[\"Animal with a label\"]\nclass `Odd Name!`\n<<interface>> Triangle",
        )
        .unwrap();
        let square = &d.classes[0];
        assert_eq!(square.id, "Square");
        assert_eq!(square.name, ["Square<Shape>"]);
        assert_eq!(square.attributes, ["List<int> position"]);
        assert_eq!(square.methods, ["getPoints() List<List<int>>"]);
        assert_eq!(d.relations[0].words, ["1 to *"]);
        assert_eq!(d.relations[1].words, ["Contains many"]);
        let labels: Vec<&str> = d.namespaces.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(labels, ["Shapes", "Flat"]);
        assert_eq!(d.namespaces[1].parent, Some(0));
        let triangle = d.classes.iter().find(|c| c.id == "Triangle").unwrap();
        assert_eq!(triangle.frame, Some(1));
        assert_eq!(triangle.annotation.as_deref(), Some("interface"));
        assert!(d.classes.iter().any(|c| c.name == ["Animal with a label"]));
        assert!(d.classes.iter().any(|c| c.id == "Odd Name!"));
    }

    #[test]
    fn broken_diagrams_are_not_drawn() {
        for source in [
            "",
            "class A {\n  +x",
            "}",
            "direction XY",
            "A <|-- ",
            "this is not a class diagram",
            "namespace N {\n class A",
            "note for A",
            "class A[unclosed",
        ] {
            assert!(diagram(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_hierarchy_draws_with_compartments_and_marks() {
        let out = drawn(ANIMALS);
        for words in [
            "<<abstract>>",
            "Animal",
            "+int age",
            "+isMammal() bool",
            "1 lays many",
            "is a",
        ] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(
            out.contains('△'),
            "inheritance points up at the parent:\n{out}"
        );
        assert!(out.contains('◆') && out.contains('◇'), "{out}");
        assert!(
            out.contains('├') && out.contains('┤'),
            "compartments:\n{out}"
        );
        let rows: Vec<&str> = out.lines().collect();
        let animal = rows.iter().position(|r| r.contains("Animal")).unwrap();
        let duck = rows.iter().position(|r| r.contains("Duck")).unwrap();
        assert!(animal < duck, "the parent above the child:\n{out}");
    }

    #[test]
    fn notes_namespaces_and_sideways_diagrams_draw() {
        let out = drawn(
            "direction LR\nnamespace Zoo {\n  class Keeper\n  class Cage\n}\nKeeper --> Cage : cleans\nnote for Keeper \"Works days\"\nnote \"Opens at nine\"",
        );
        for words in [
            "Zoo",
            "Keeper",
            "Cage",
            "cleans",
            "Works days",
            "Opens at nine",
        ] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(out.contains('▶'), "{out}");
    }

    #[test]
    fn each_class_has_its_colour_and_notes_are_no_ones() {
        let d = diagram("A --> B\nnote for B \"hi\"").unwrap();
        let drawn = draw(&d).unwrap();
        assert_eq!(drawn.keys.len(), 3);
        let note = drawn
            .rows
            .iter()
            .flatten()
            .find(|(text, _)| text.contains("hi"))
            .map(|(_, ink)| *ink);
        assert_eq!(note, Some(Ink::plain(Role::Note)));
        let arrow = drawn
            .rows
            .iter()
            .flatten()
            .find(|(text, _)| text.contains('▼'))
            .map(|(_, ink)| *ink);
        assert_eq!(arrow, Some(Ink::of(Role::Arrow, 0)));
    }

    #[test]
    fn big_and_self_referring_diagrams_stay_bounded() {
        let mut many = String::new();
        for at in 0..MAX_CLASSES + 1 {
            many.push_str(&format!("class C{at}\n"));
        }
        assert!(diagram(&many).is_none());

        let mut web = String::new();
        for a in 0..40 {
            for b in 0..5 {
                web.push_str(&format!("C{a} --> C{}\n", (a * 7 + b) % 40));
            }
            web.push_str(&format!("C{a} : +field{a} int\nC{a} --> C{a}\n"));
        }
        let d = diagram(&web).unwrap();
        if let Some(drawn) = draw(&d) {
            assert!(drawn.rows.len() * drawn.width <= super::super::canvas::MAX_CELLS);
        }
    }
}
