//! Entity relationship diagrams: `erDiagram`.
//!
//! Each entity is a box with its name, and under a rule its attributes in
//! columns: the type, the name, any keys, and any comment. Each relationship
//! is a line, dotted when it doesn't confer identity, with its label and the
//! cardinality at each end written on it in UML's notation, so
//! `CUSTOMER ||--o{ ORDER : places` reads `1 places 0..*`. Both Mermaid's
//! marks, as `}|..|{`, and its words, as `only one to zero or more`, are
//! understood. A relationship from an entity to itself is written in its
//! box, after a `↻`.
//!
//! Laid out by [`super::layered`], as flowcharts are. Showing the entity's
//! attributes as a compartment of its box follows beautiful-mermaid (MIT),
//! the renderer prismantis draws these with. The code is our own.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::layered::{self, Direction, Graph, Link};
use super::{Diagram, Ink, Role, key, label_lines, truncate, widest};

const MAX_ENTITIES: usize = 150;
const MAX_ATTRIBUTES: usize = 60;
const MAX_RELATIONSHIPS: usize = 300;
/// Columns of an attribute's comment before it is cut short.
const COMMENT_WIDTH: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    pub kind: String,
    pub name: String,
    pub keys: String,
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// The name relationships use for it, which its colour is picked by.
    pub id: String,
    pub name: Vec<String>,
    pub attributes: Vec<Attribute>,
    /// Relationships from it to itself, as they are written on its box.
    pub loops: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub from: usize,
    pub to: usize,
    /// The label, with the cardinality at each end.
    pub words: Vec<String>,
    /// Identifying relationships are solid, others dotted.
    pub stroke: Stroke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErDiagram {
    pub direction: Direction,
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
}

// ---------------------------------------------------------------- parsing

/// Statements about how entities look, which a drawing in text has no use
/// for.
const IGNORED: &[&str] = &[
    "style", "classDef", "class", "accTitle", "accDescr", "title",
];

/// Mermaid's marks for each cardinality, as written on the left of the line
/// and on its right, and how it is shown.
const MARKS: &[(&str, &str, &str)] = &[
    ("|o", "o|", "0..1"),
    ("||", "||", "1"),
    ("}o", "o{", "0..*"),
    ("}|", "|{", "1..*"),
];

/// Mermaid's words for each cardinality, longest first.
const WORDS: &[(&str, &str)] = &[
    ("zero or more", "0..*"),
    ("zero or many", "0..*"),
    ("one or more", "1..*"),
    ("one or many", "1..*"),
    ("one or zero", "0..1"),
    ("zero or one", "0..1"),
    ("only one", "1"),
    ("many(0)", "0..*"),
    ("many(1)", "1..*"),
    ("0+", "0..*"),
    ("1+", "1..*"),
    ("1", "1"),
];

struct Reader {
    diagram: ErDiagram,
    ids: HashMap<String, usize>,
    /// The entity whose attributes are being read.
    open: Option<usize>,
}

/// Reads the lines after `erDiagram`.
pub fn parse(lines: &[&str]) -> Option<ErDiagram> {
    let mut reader = Reader {
        diagram: ErDiagram {
            direction: Direction::Down,
            entities: Vec::new(),
            relationships: Vec::new(),
        },
        ids: HashMap::new(),
        open: None,
    };
    for line in lines {
        reader.line(line.trim())?;
    }
    if reader.open.is_some() || reader.diagram.entities.is_empty() {
        return None;
    }
    Some(reader.diagram)
}

impl Reader {
    fn line(&mut self, line: &str) -> Option<()> {
        if line.is_empty() {
            return Some(());
        }
        if let Some(entity) = self.open {
            if line == "}" {
                self.open = None;
                return Some(());
            }
            let (line, closed) = match line.strip_suffix('}') {
                Some(line) => (line.trim(), true),
                None => (line, false),
            };
            if !line.is_empty() {
                self.attribute(entity, line)?;
            }
            if closed {
                self.open = None;
            }
            return Some(());
        }
        let first = line.split_whitespace().next().unwrap_or("");
        if IGNORED.contains(&first) {
            return Some(());
        }
        if first == "direction" {
            self.diagram.direction = match line["direction".len()..].trim() {
                "TB" | "TD" => Direction::Down,
                "BT" => Direction::Up,
                "LR" => Direction::Right,
                "RL" => Direction::Left,
                _ => return None,
            };
            return Some(());
        }
        if let Some(parts) = relationship(line) {
            return self.relationship(parts);
        }
        // An entity on its own, perhaps opening its attributes.
        let (name, opens) = match line.strip_suffix('{') {
            Some(name) => (name.trim(), true),
            None => (line, false),
        };
        let (name, rest) = match name.find('{') {
            Some(open) => (name[..open].trim(), Some(&name[open + 1..])),
            None => (name, None),
        };
        let entity = self.entity(name)?;
        if opens || rest.is_some() {
            self.open = Some(entity);
            if let Some(rest) = rest {
                return self.line(rest.trim());
            }
        }
        Some(())
    }

    /// The entity written `name`, made the first time it is named. A name
    /// may be quoted, carry a class as `NAME:::hot`, and have an alias shown
    /// in its place, as `p[Person]`.
    fn entity(&mut self, name: &str) -> Option<usize> {
        let name = name.split(":::").next().unwrap_or(name).trim();
        let (id, shown) = match name.find('[') {
            Some(open) if name.ends_with(']') => {
                (name[..open].trim(), Some(&name[open + 1..name.len() - 1]))
            }
            Some(_) => return None,
            None => (name, None),
        };
        let id = unquote(id);
        if id.is_empty() || (id.contains(char::is_whitespace) && !name.starts_with('"')) {
            return None;
        }
        let at = match self.ids.get(id) {
            Some(&at) => at,
            None => {
                if self.diagram.entities.len() >= MAX_ENTITIES {
                    return None;
                }
                self.diagram.entities.push(Entity {
                    id: id.to_string(),
                    name: label_lines(id),
                    attributes: Vec::new(),
                    loops: Vec::new(),
                });
                self.ids
                    .insert(id.to_string(), self.diagram.entities.len() - 1);
                self.diagram.entities.len() - 1
            }
        };
        if let Some(shown) = shown {
            self.diagram.entities[at].name = label_lines(unquote(shown));
        }
        Some(at)
    }

    /// `type name PK, FK "comment"`.
    fn attribute(&mut self, entity: usize, line: &str) -> Option<()> {
        let (line, comment) = match line.find('"') {
            Some(open) => {
                let close = line[open + 1..].find('"')? + open + 1;
                (line[..open].trim(), line[open + 1..close].trim())
            }
            None => (line, ""),
        };
        let mut words = line.split_whitespace();
        let kind = words.next()?;
        let name = words.next()?;
        let keys: Vec<&str> = words
            .flat_map(|word| word.split(','))
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .collect();
        if !keys.iter().all(|key| matches!(*key, "PK" | "FK" | "UK")) {
            return None;
        }
        let attributes = &mut self.diagram.entities[entity].attributes;
        if attributes.len() >= MAX_ATTRIBUTES {
            return None;
        }
        attributes.push(Attribute {
            kind: kind.to_string(),
            name: name.to_string(),
            keys: keys.join(", "),
            comment: truncate(comment, COMMENT_WIDTH),
        });
        Some(())
    }

    fn relationship(&mut self, parts: Parts) -> Option<()> {
        let from = self.entity(parts.from)?;
        let to = self.entity(parts.to)?;
        let label = label_lines(parts.label).join(" ");
        let text = if label.is_empty() {
            format!("{} to {}", parts.left, parts.right)
        } else {
            format!("{} {label} {}", parts.left, parts.right)
        };
        if from == to {
            let loops = &mut self.diagram.entities[from].loops;
            if loops.len() < MAX_ATTRIBUTES {
                loops.push(format!("↻ {text}"));
            }
            return Some(());
        }
        if self.diagram.relationships.len() >= MAX_RELATIONSHIPS {
            return None;
        }
        self.diagram.relationships.push(Relationship {
            from,
            to,
            words: label_lines(&text),
            stroke: parts.stroke,
        });
        Some(())
    }
}

fn unquote(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(text)
}

/// The parts of a relationship statement.
struct Parts<'a> {
    from: &'a str,
    to: &'a str,
    /// The cardinality at each end, as shown.
    left: &'static str,
    right: &'static str,
    stroke: Stroke,
    label: &'a str,
}

/// `A ||--o{ B : label`, or the same in words, as its parts; `None` when
/// the line holds no relationship.
fn relationship(line: &str) -> Option<Parts<'_>> {
    let (link, label) = split_label(line);
    marks(link)
        .or_else(|| words(link))
        .map(|(from, left, stroke, right, to)| Parts {
            from,
            to,
            left,
            right,
            stroke,
            label: unquote(label),
        })
}

/// Splits at the first colon outside quotes.
fn split_label(line: &str) -> (&str, &str) {
    let mut quoted = false;
    for (at, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return (&line[..at], line[at + 1..].trim()),
            _ => {}
        }
    }
    (line, "")
}

type Link4<'a> = (&'a str, &'static str, Stroke, &'static str, &'a str);

/// `A ||--o{ B`: two marks either side of `--` or `..`.
fn marks(link: &str) -> Option<Link4<'_>> {
    let bytes = link.as_bytes();
    let mut quoted = false;
    for at in 0..bytes.len() {
        if bytes[at] == b'"' {
            quoted = !quoted;
        }
        if quoted || at < 2 || at + 4 > bytes.len() || !matches!(&bytes[at..at + 2], b"--" | b"..")
        {
            continue;
        }
        let (Some(before), Some(after)) = (link.get(at - 2..at), link.get(at + 2..at + 4)) else {
            continue;
        };
        let left = MARKS.iter().find(|m| m.0 == before).map(|m| m.2);
        let right = MARKS.iter().find(|m| m.1 == after).map(|m| m.2);
        if let (Some(left), Some(right)) = (left, right) {
            let stroke = if bytes[at] == b'.' {
                Stroke::Dotted
            } else {
                Stroke::Solid
            };
            let from = link[..at - 2].trim();
            let to = link[at + 4..].trim();
            if from.is_empty() || to.is_empty() {
                return None;
            }
            return Some((from, left, stroke, right, to));
        }
    }
    None
}

/// `A only one to zero or more B`: the cardinalities in words, either side
/// of `to` or `optionally to`.
fn words(link: &str) -> Option<Link4<'_>> {
    let (at, len, stroke) = match link.find(" optionally to ") {
        Some(at) => (at, " optionally to ".len(), Stroke::Dotted),
        None => (link.find(" to ")?, " to ".len(), Stroke::Solid),
    };
    let before = link[..at].trim_end();
    let after = link[at + len..].trim_start();
    let (from, left) = WORDS.iter().find_map(|&(words, shown)| {
        let rest = before.strip_suffix(words)?;
        rest.ends_with(char::is_whitespace)
            .then(|| (rest.trim(), shown))
    })?;
    let (to, right) = WORDS.iter().find_map(|&(words, shown)| {
        let rest = after.strip_prefix(words)?;
        rest.starts_with(char::is_whitespace)
            .then(|| (rest.trim(), shown))
    })?;
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some((from, left, stroke, right, to))
}

// ---------------------------------------------------------------- drawing

impl Entity {
    /// Its attributes in four columns, each as wide as its widest entry.
    fn columns(&self) -> [usize; 4] {
        let mut widths = [0; 4];
        for attribute in &self.attributes {
            for (width, part) in widths.iter_mut().zip([
                &attribute.kind,
                &attribute.name,
                &attribute.keys,
                &attribute.comment,
            ]) {
                *width = (*width).max(part.width());
            }
        }
        widths
    }

    /// The columns an attribute row takes.
    fn row_width(&self) -> usize {
        let columns = self.columns();
        let used: Vec<usize> = columns.iter().copied().filter(|&w| w > 0).collect();
        used.iter().sum::<usize>() + 2 * used.len().saturating_sub(1)
    }

    fn header(&self) -> Vec<String> {
        let mut header = self.name.clone();
        header.extend(self.loops.iter().cloned());
        header
    }

    fn size(&self) -> (usize, usize) {
        let header = self.header();
        let inside = widest(&header).max(self.row_width());
        let rows = if self.attributes.is_empty() {
            0
        } else {
            self.attributes.len() + 1
        };
        (inside + 4, header.len() + rows + 2)
    }
}

/// Draws an entity relationship diagram, or `None` when it is too big.
pub fn draw(diagram: &ErDiagram, width: usize) -> Option<Diagram> {
    let entities = &diagram.entities;
    let mut graph = Graph::new(diagram.direction);
    graph.boxes = entities.iter().map(Entity::size).collect();
    graph.links = diagram
        .relationships
        .iter()
        .map(|r| Link {
            from: r.from,
            to: r.to,
            words: (!r.words.is_empty()).then(|| (widest(&r.words), r.words.len())),
        })
        .collect();
    graph.room = width;
    graph.spread = true;
    let placed = layered::layout(&graph)?;
    let mut canvas = Canvas::new(placed.width, placed.height)?;

    for (at, (entity, rect)) in entities.iter().zip(&placed.boxes).enumerate() {
        draw_entity(&mut canvas, entity, at, *rect);
    }
    for (r, rect) in diagram.relationships.iter().zip(&placed.words) {
        if let Some(rect) = rect {
            let ink = Ink::of(Role::Label, r.from);
            layered::centred(&mut canvas, rect.x, rect.y, rect.width, &r.words, ink);
        }
    }
    placed.draw_lines(&mut canvas, |at| {
        let r = &diagram.relationships[at];
        (r.stroke, Ink::of(Role::Line, r.from).owner)
    });
    for end in &placed.ends {
        let join = Ink::of(Role::Border, end.node);
        let glyph = layered::join(&canvas, end);
        canvas.put(end.border.0, end.border.1, glyph, join);
    }

    let mut drawn = canvas.finish();
    drawn.keys = entities.iter().map(|e| key(&e.id)).collect();
    drawn.links = diagram
        .relationships
        .iter()
        .filter_map(|r| Some((u16::try_from(r.from).ok()?, u16::try_from(r.to).ok()?)))
        .collect();
    Some(drawn)
}

fn draw_entity(canvas: &mut Canvas, entity: &Entity, at: usize, rect: layered::Rect) {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    let outline = Ink::of(Role::Border, at);
    canvas.outline(x, y, w, h, ['┌', '┐', '└', '┘'], false, outline);
    layered::centred(
        canvas,
        x + 1,
        y + 1,
        w - 2,
        &entity.name,
        Ink::plain(Role::Text),
    );
    let loops = y + 1 + entity.name.len();
    layered::centred(
        canvas,
        x + 1,
        loops,
        w - 2,
        &entity.loops,
        Ink::of(Role::Label, at),
    );
    if entity.attributes.is_empty() {
        return;
    }
    let rule = loops + entity.loops.len();
    canvas.put(x, rule, '├', outline);
    canvas.put(x + w - 1, rule, '┤', outline);
    for column in x + 1..x + w - 1 {
        canvas.put(column, rule, '─', outline);
    }
    let widths = entity.columns();
    let quiet = Ink::plain(Role::Label);
    for (row, attribute) in entity.attributes.iter().enumerate() {
        let mut column = x + 2;
        let parts = [
            (&attribute.kind, quiet),
            (&attribute.name, Ink::plain(Role::Text)),
            (&attribute.keys, Ink::of(Role::Label, at)),
            (&attribute.comment, quiet),
        ];
        for ((text, ink), &width) in parts.into_iter().zip(&widths) {
            if width == 0 {
                continue;
            }
            canvas.text(column, rule + 1 + row, text, ink);
            column += width + 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn diagram(source: &str) -> Option<ErDiagram> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&diagram(source).expect("parses"), width).expect("draws"))
    }

    const ORDERS: &str = "CUSTOMER ||--o{ ORDER : places
CUSTOMER {
    string name
    string custNumber PK \"the number on the card\"
    string sector
}
ORDER ||--|{ LINE-ITEM : contains
ORDER {
    int orderNumber PK
    string deliveryAddress
}
LINE-ITEM {
    string productCode PK, FK
    int quantity
}
CUSTOMER }|..|{ DELIVERY-ADDRESS : uses";

    #[test]
    fn entities_attributes_and_relationships_are_read() {
        let d = diagram(ORDERS).unwrap();
        let ids: Vec<&str> = d.entities.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["CUSTOMER", "ORDER", "LINE-ITEM", "DELIVERY-ADDRESS"]);
        let number = &d.entities[0].attributes[1];
        assert_eq!(
            (
                number.kind.as_str(),
                number.name.as_str(),
                number.keys.as_str(),
                number.comment.as_str()
            ),
            ("string", "custNumber", "PK", "the number on the card")
        );
        assert_eq!(d.entities[2].attributes[0].keys, "PK, FK");
        assert_eq!(d.relationships[0].words, ["1 places 0..*"]);
        assert_eq!(d.relationships[1].words, ["1 contains 1..*"]);
        assert_eq!(d.relationships[2].words, ["1..* uses 1..*"]);
        assert_eq!(d.relationships[2].stroke, Stroke::Dotted);
    }

    #[test]
    fn words_aliases_and_self_relationships_are_read() {
        let d = diagram(
            "CAR 1 to zero or more NAMED-DRIVER : allows\nPERSON many(0) optionally to 0+ NAMED-DRIVER : is\np[Person] {\n  string firstName\n}\na[\"Customer Account\"] {\n  string email\n}\np ||--o| a : has\nEMPLOYEE ||--o{ EMPLOYEE : manages\n\"Big Thing\" ||--|| CAR : owns\nMANUFACTURER only one to zero or more CAR : \"makes\"",
        )
        .unwrap();
        assert_eq!(d.relationships[0].words, ["1 allows 0..*"]);
        assert_eq!(d.relationships[1].words, ["0..* is 0..*"]);
        assert_eq!(d.relationships[1].stroke, Stroke::Dotted);
        let person = d.entities.iter().find(|e| e.id == "p").unwrap();
        assert_eq!(person.name, ["Person"]);
        let account = d.entities.iter().find(|e| e.id == "a").unwrap();
        assert_eq!(account.name, ["Customer Account"]);
        let employee = d.entities.iter().find(|e| e.id == "EMPLOYEE").unwrap();
        assert_eq!(employee.loops, ["↻ 1 manages 0..*"]);
        assert!(d.entities.iter().any(|e| e.id == "Big Thing"));
        assert!(d.relationships.iter().any(|r| r.words == ["1 makes 0..*"]));
    }

    #[test]
    fn broken_diagrams_are_not_drawn() {
        for source in [
            "",
            "CUSTOMER {\n  string name",
            "CUSTOMER {\n  onlyonething\n}",
            "CUSTOMER {\n  string name XK\n}",
            "direction XY",
            "A ||--o{ : places",
            "this is not an entity",
            "A }|--o{",
            "p[Person",
        ] {
            assert!(diagram(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn an_order_model_draws_with_its_attributes_and_cardinalities() {
        let out = drawn(ORDERS, 120);
        for words in [
            "CUSTOMER",
            "custNumber",
            "PK",
            "the number on the card",
            "1 places 0..*",
            "1 contains 1..*",
            "1..* uses 1..*",
            "LINE-ITEM",
        ] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(
            out.contains('┆') || out.contains('┄'),
            "a dotted line:\n{out}"
        );
        assert!(
            !out.contains('▼'),
            "relationships have no arrowheads:\n{out}"
        );
        // Attributes line up in columns.
        let rows: Vec<&str> = out.lines().filter(|r| r.contains("string")).collect();
        let starts: Vec<usize> = rows
            .iter()
            .filter(|r| r.contains("name") || r.contains("sector"))
            .map(|r| r.find("name").or_else(|| r.find("sector")).unwrap())
            .collect();
        assert!(starts.windows(2).all(|w| w[0] == w[1]), "{out}");
    }

    #[test]
    fn keys_are_in_the_entitys_colour_and_types_are_quiet() {
        let d = diagram("A {\n  int id PK\n}\nA ||--o{ B : has").unwrap();
        let drawn = draw(&d, 80).unwrap();
        let ink_of = |needle: &str| {
            drawn
                .rows
                .iter()
                .flatten()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, ink)| *ink)
        };
        assert_eq!(ink_of("PK"), Some(Ink::of(Role::Label, 0)));
        assert_eq!(ink_of("int"), Some(Ink::plain(Role::Label)));
        assert_eq!(ink_of("has"), Some(Ink::of(Role::Label, 0)));
    }

    #[test]
    fn big_and_tangled_diagrams_stay_bounded() {
        let mut many = String::new();
        for at in 0..MAX_ENTITIES + 1 {
            many.push_str(&format!("E{at}\n"));
        }
        assert!(diagram(&many).is_none());

        let mut web = String::new();
        for a in 0..50 {
            web.push_str(&format!(
                "E{a} ||--o{{ E{} : r{a}\nE{a} }}|..|{{ E{} : s\n",
                (a + 1) % 50,
                (a * 3) % 50
            ));
        }
        let d = diagram(&web).unwrap();
        if let Some(drawn) = draw(&d, 100) {
            assert!(drawn.rows.len() * drawn.width <= super::super::canvas::MAX_CELLS);
        }
    }
}
