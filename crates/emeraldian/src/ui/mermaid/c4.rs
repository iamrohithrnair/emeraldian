//! C4 diagrams: `C4Context`, `C4Container`, `C4Component`, `C4Dynamic` and
//! `C4Deployment`.
//!
//! Each person, system, container and component is a box holding its name,
//! its kind in brackets, with its technology for a container or component,
//! and its description. People have round corners, databases a rim under
//! their lid and queues double sides, and the external ones are drawn in a
//! quiet grey, as C4 draws them. Boundaries and deployment nodes are dashed
//! frames around what they hold, one inside another, and each relationship
//! is an arrow with its label and technology on it. A dynamic diagram
//! numbers its relationships in the order they are written.
//!
//! The boxes are laid out by [`super::layered`], as a flowchart's are, so
//! the relationships read down the page, and boxes with no relationships are
//! wrapped into rows the width of the pane. Statements that only style a
//! diagram in a browser, such as `UpdateElementStyle` and
//! `UpdateLayoutConfig`, are skipped. A relationship to something that isn't
//! there, or to a boundary rather than a box, leaves the diagram as code.
//!
//! What goes in each box, and passing over the styling statements rather
//! than giving up on them, follow prismantis (MIT), which draws C4 for Claude
//! Code. The code is our own.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::label::{self, truncate, widest};
use super::layered::{self, Direction, Graph, Link};
use super::{Diagram, Ink, ROUND, Role, SQUARE, key, pairs, syntax};

const MAX_ELEMENTS: usize = 120;
const MAX_RELS: usize = 240;
const MAX_FRAMES: usize = 60;
/// Lines of a box's name and of its description, at most.
const NAME_LINES: usize = 2;
const DESCRIPTION_LINES: usize = 4;
/// Columns of a box's kind and technology before they wrap, and lines of
/// them at most.
const KIND_WIDTH: usize = 32;
const KIND_LINES: usize = 2;
/// Columns of the title before it wraps.
const TITLE_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Person,
    Box,
    Database,
    Queue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The name relationships use for it, which its colour is picked by.
    pub alias: String,
    pub shape: Shape,
    pub external: bool,
    pub name: Vec<String>,
    /// Its kind and technology, as `[Container: Go]`.
    pub kind: Vec<String>,
    pub description: Vec<String>,
    /// The boundary it is drawn in.
    pub frame: Option<usize>,
    /// Whether a relationship leads from it back to itself.
    pub looped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub label: String,
    /// Its kind, such as a deployment node's operating system, shown after
    /// its label where there is room.
    pub kind: Option<String>,
    pub parent: Option<usize>,
}

impl Frame {
    /// Its label and kind together, as `bigbank-db01 [Ubuntu]`.
    fn full(&self) -> String {
        match &self.kind {
            Some(kind) => format!("{} [{kind}]", self.label),
            None => self.label.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rel {
    pub from: usize,
    pub to: usize,
    /// Its label and, under it, its technology.
    pub words: Vec<String>,
    /// An arrowhead at the `from` end too, for a `BiRel`.
    pub both: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct C4 {
    pub title: Option<String>,
    pub elements: Vec<Element>,
    pub frames: Vec<Frame>,
    pub rels: Vec<Rel>,
}

/// Whether `kind`, the first word of a diagram, names a kind of C4 diagram.
#[must_use]
pub fn is_kind(kind: &str) -> bool {
    matches!(
        kind,
        "C4Context" | "C4Container" | "C4Component" | "C4Dynamic" | "C4Deployment"
    )
}

// ---------------------------------------------------------------- parsing

/// Reads the lines after the line naming the diagram. `numbered` numbers the
/// relationships, as a dynamic diagram does.
pub fn parse(lines: &[&str], numbered: bool) -> Option<C4> {
    let mut reader = Reader {
        c4: C4 {
            title: None,
            elements: Vec::new(),
            frames: Vec::new(),
            rels: Vec::new(),
        },
        aliases: HashMap::new(),
        frame_aliases: HashMap::new(),
        open: Vec::new(),
        pending: None,
        rels: Vec::new(),
    };
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(title) = line.strip_prefix("title ") {
            reader.c4.title = Some(label::joined(title));
            continue;
        }
        if line.starts_with("accTitle") || line.starts_with("accDescr") || line == "title" {
            continue;
        }
        for piece in pieces(line) {
            reader.piece(piece)?;
        }
    }
    if !reader.open.is_empty() || reader.c4.elements.is_empty() {
        return None;
    }
    let mut number = 0;
    for (name, args) in std::mem::take(&mut reader.rels) {
        reader.rel(
            &name,
            &args,
            numbered.then(|| {
                number += 1;
                number
            }),
        )?;
    }
    Some(reader.c4)
}

struct Reader {
    c4: C4,
    aliases: HashMap<String, usize>,
    frame_aliases: HashMap<String, usize>,
    /// The boundaries opened by a `{` and not yet closed, innermost last;
    /// `None` for a `{` that opened no boundary.
    open: Vec<Option<usize>>,
    /// A boundary waiting for the `{` that opens it.
    pending: Option<usize>,
    /// Relationships, read once every box is known, so one may name a box
    /// written after it.
    rels: Vec<(String, Args)>,
}

impl Reader {
    fn frame(&self) -> Option<usize> {
        self.open.iter().rev().find_map(|f| *f)
    }

    fn piece(&mut self, piece: &str) -> Option<()> {
        let pending = self.pending.take();
        match piece {
            "{" => {
                self.open.push(pending);
                if self.open.len() > layered::MAX_DEPTH {
                    return None;
                }
                return Some(());
            }
            "}" => {
                self.open.pop()?;
                return Some(());
            }
            _ => {}
        }
        let (name, args) = call(piece)?;
        let lower = name.to_ascii_lowercase();
        if let Some((shape, external, kind)) = element(&lower) {
            self.element(shape, external, kind, &args)
        } else if is_boundary(&lower) {
            self.boundary(&lower, &args)
        } else if is_rel(&lower) {
            if self.rels.len() >= MAX_RELS {
                return None;
            }
            self.rels.push((lower, args));
            Some(())
        } else {
            // Styling and layout, which only matter in a browser.
            Some(())
        }
    }

    fn element(&mut self, shape: Shape, external: bool, kind: &str, args: &Args) -> Option<()> {
        let technical = matches!(kind, "container" | "component");
        let slots: &[&str] = if technical {
            &["alias", "label", "techn", "descr"]
        } else {
            &["alias", "label", "descr"]
        };
        let alias = args.get(slots, "alias")?;
        let label = args.get(slots, "label").unwrap_or_else(|| alias.clone());
        let named = match kind {
            "person" => "Person",
            "system" => "Software System",
            "container" => "Container",
            _ => "Component",
        };
        let mut kind = format!("{}{named}", if external { "External " } else { "" });
        if technical && let Some(techn) = args.get(slots, "techn") {
            kind = format!("{kind}: {}", label::joined(&techn));
        }
        let element = Element {
            alias: alias.clone(),
            shape,
            external,
            name: clip(label::lines(&label), NAME_LINES),
            kind: clip(label::wrapped(&format!("[{kind}]"), KIND_WIDTH), KIND_LINES),
            description: args
                .get(slots, "descr")
                .map(|text| clip(label::lines(&text), DESCRIPTION_LINES))
                .unwrap_or_default(),
            frame: self.frame(),
            looped: false,
        };
        // Written again, as Mermaid allows, it is the last that counts.
        if let Some(&at) = self.aliases.get(&alias) {
            self.c4.elements[at] = element;
            return Some(());
        }
        if self.c4.elements.len() >= MAX_ELEMENTS {
            return None;
        }
        self.aliases.insert(alias, self.c4.elements.len());
        self.c4.elements.push(element);
        Some(())
    }

    fn boundary(&mut self, name: &str, args: &Args) -> Option<()> {
        let typed = matches!(name, "boundary")
            || name.starts_with("deployment_node")
            || name.starts_with("node");
        let slots: &[&str] = &["alias", "label", "type", "descr"];
        let alias = args.get(slots, "alias")?;
        let label = label::joined(&args.get(slots, "label").unwrap_or_else(|| alias.clone()));
        let kind = args
            .get(slots, "type")
            .filter(|_| typed)
            .map(|kind| label::joined(&kind));
        if self.c4.frames.len() >= MAX_FRAMES {
            return None;
        }
        self.c4.frames.push(Frame {
            label,
            kind,
            parent: self.frame(),
        });
        let at = self.c4.frames.len() - 1;
        self.frame_aliases.insert(alias, at);
        self.pending = Some(at);
        Some(())
    }

    fn rel(&mut self, name: &str, args: &Args, number: Option<usize>) -> Option<()> {
        let slots: &[&str] = if name == "relindex" {
            &["index", "from", "to", "label", "techn", "descr"]
        } else {
            &["from", "to", "label", "techn", "descr"]
        };
        let mut from = *self.aliases.get(&args.get(slots, "from")?)?;
        let mut to = *self.aliases.get(&args.get(slots, "to")?)?;
        if name.contains("back") {
            std::mem::swap(&mut from, &mut to);
        }
        if from == to {
            self.c4.elements[from].looped = true;
            return Some(());
        }
        let label = args.get(slots, "label").unwrap_or_default();
        let label = match number {
            Some(number) => format!("{number}. {label}"),
            None => label,
        };
        let mut words: Vec<String> = label::lines(&label)
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect();
        if let Some(techn) = args.get(slots, "techn") {
            let techn = format!("[{}]", label::joined(&techn));
            words.extend(clip(label::wrapped(&techn, KIND_WIDTH), KIND_LINES));
        }
        self.c4.rels.push(Rel {
            from,
            to,
            words,
            both: name.starts_with("birel"),
        });
        Some(())
    }
}

/// The shape of an element's box, whether it is external, and its kind,
/// from the name of the statement that makes it, such as `ContainerDb_Ext`.
fn element(name: &str) -> Option<(Shape, bool, &'static str)> {
    let (name, external) = match name.strip_suffix("_ext") {
        Some(name) => (name, true),
        None => (name, false),
    };
    for kind in ["person", "system", "container", "component"] {
        let Some(rest) = name.strip_prefix(kind) else {
            continue;
        };
        let shape = match rest {
            "" if kind == "person" => Shape::Person,
            "" => Shape::Box,
            "db" if kind != "person" => Shape::Database,
            "queue" if kind != "person" => Shape::Queue,
            _ => return None,
        };
        return Some((shape, external, kind));
    }
    None
}

fn is_boundary(name: &str) -> bool {
    matches!(
        name,
        "boundary"
            | "enterprise_boundary"
            | "system_boundary"
            | "container_boundary"
            | "deployment_node"
            | "deployment_node_l"
            | "deployment_node_r"
            | "node"
            | "node_l"
            | "node_r"
    )
}

fn is_rel(name: &str) -> bool {
    matches!(
        name,
        "rel"
            | "birel"
            | "relindex"
            | "rel_u"
            | "rel_up"
            | "rel_d"
            | "rel_down"
            | "rel_l"
            | "rel_left"
            | "rel_r"
            | "rel_right"
            | "rel_back"
            | "rel_neighbor"
            | "rel_back_neighbor"
    )
}

/// A line split into its statements and the braces between them, outside
/// quotes, up to a `%%` comment.
fn pieces(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => quoted = !quoted,
            '%' if !quoted && chars.peek().is_some_and(|&(_, next)| next == '%') => {
                out.push(&line[start..at]);
                start = line.len();
                break;
            }
            '{' | '}' if !quoted => {
                out.push(&line[start..at]);
                out.push(&line[at..at + 1]);
                start = at + 1;
            }
            _ => {}
        }
    }
    out.push(&line[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// A statement's arguments: those given in order, and those given by name
/// as `$descr="…"`.
#[derive(Debug, Clone, Default)]
struct Args {
    given: Vec<String>,
    named: Vec<(String, String)>,
}

impl Args {
    /// The argument for `slot`, by name or by its place among `slots`.
    fn get(&self, slots: &[&str], slot: &str) -> Option<String> {
        let named = self
            .named
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(slot))
            .map(|(_, value)| value);
        let given = slots
            .iter()
            .position(|s| *s == slot)
            .and_then(|at| self.given.get(at));
        named
            .or(given)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }
}

/// `Name(arguments)`, as its name and arguments.
fn call(piece: &str) -> Option<(String, Args)> {
    let open = piece.find('(')?;
    let name = piece[..open].trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let inside = piece[open + 1..].strip_suffix(')')?;
    let mut args = Args::default();
    for part in syntax::split_outside_quotes(inside, ',')? {
        let part = part.trim();
        if let Some(named) = part.strip_prefix('$')
            && let Some((name, value)) = named.split_once('=')
        {
            let value = syntax::unquote(value).to_string();
            args.named.push((name.trim().to_string(), value));
        } else {
            args.given.push(syntax::unquote(part).to_string());
        }
    }
    Some((name.to_string(), args))
}

/// At most `most` lines, the last ending in `…` if any were cut.
fn clip(mut lines: Vec<String>, most: usize) -> Vec<String> {
    if lines.len() > most {
        lines.truncate(most);
        if let Some(last) = lines.last_mut() {
            *last = truncate(&format!("{last}…"), label::WIDTH);
            if !last.ends_with('…') {
                last.push('…');
            }
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

// ---------------------------------------------------------------- drawing

impl Element {
    /// Its name, with a mark for a relationship to itself.
    fn name(&self) -> Vec<String> {
        let mut name = self.name.clone();
        if self.looped
            && let Some(first) = name.first_mut()
        {
            first.push_str(" ↻");
        }
        name
    }

    /// Columns and rows of its box.
    fn size(&self) -> (usize, usize) {
        let name = self.name();
        let inside = widest(&name)
            .max(widest(&self.kind))
            .max(widest(&self.description));
        let rim = usize::from(self.shape == Shape::Database);
        let rows = name.len() + self.kind.len() + self.description.len();
        (inside + 4, rows + 2 + rim)
    }

    /// In its own colour, or in no one's when it is external.
    fn ink(&self, role: Role, at: usize) -> Ink {
        if self.external {
            Ink::plain(role)
        } else {
            Ink::of(role, at)
        }
    }
}

/// Draws a C4 diagram, or `None` when it is too big. `width` is the room the
/// pane has, which boxes with no relationships are wrapped to.
pub fn draw(c4: &C4, width: usize) -> Option<Diagram> {
    let mut graph = Graph::new(Direction::Down);
    graph.boxes = c4.elements.iter().map(Element::size).collect();
    graph.links = c4
        .rels
        .iter()
        .map(|rel| Link::new(rel.from, rel.to, &rel.words))
        .collect();
    graph.frames = c4.frames.iter().map(|frame| frame.parent).collect();
    graph.framed = c4.elements.iter().map(|element| element.frame).collect();
    graph.labels = c4.frames.iter().map(|frame| frame.label.width()).collect();
    graph.room = width;
    graph.spread = true;
    let mut placed = layered::layout(&graph)?;
    let title = label::title(c4.title.as_deref(), TITLE_WIDTH);
    placed.moved(0, label::above(&title));
    let mut canvas = Canvas::new(placed.width.max(widest(&title)), placed.height)?;
    label::draw_title(&mut canvas, &title);

    placed.draw_frames(&mut canvas, Stroke::Dotted);
    for (at, (element, rect)) in c4.elements.iter().zip(&placed.boxes).enumerate() {
        draw_element(&mut canvas, element, at, *rect);
    }
    placed.draw_words(&mut canvas, |rel| {
        let rel = &c4.rels[rel];
        (&rel.words, c4.elements[rel.from].ink(Role::Label, rel.from))
    });
    placed.draw_lines(&mut canvas, |rel| {
        let from = c4.rels[rel].from;
        (Stroke::Solid, c4.elements[from].ink(Role::Line, from).owner)
    });
    for end in &placed.ends {
        let rel = &c4.rels[end.link];
        let arrow = end.to || rel.both;
        let head = c4.elements[rel.from].ink(Role::Arrow, rel.from);
        let join = c4.elements[end.node].ink(Role::Border, end.node);
        if end.joined(arrow) {
            let glyph = layered::join(&canvas, end);
            canvas.put(end.border.0, end.border.1, glyph, join);
        }
        if arrow {
            canvas.put(end.outside.0, end.outside.1, end.facing.arrow(), head);
        }
    }
    for (at, frame) in c4.frames.iter().enumerate() {
        placed.label_frame(&mut canvas, at, &[&frame.full(), &frame.label]);
    }

    let mut diagram = canvas.finish();
    diagram.keys = c4
        .elements
        .iter()
        .map(|element| key(&element.alias))
        .collect();
    diagram.links = pairs(c4.rels.iter().map(|rel| (rel.from, rel.to)));
    Some(diagram)
}

fn draw_element(canvas: &mut Canvas, element: &Element, at: usize, rect: layered::Rect) {
    let (x, y, w, h) = (rect.x, rect.y, rect.width, rect.height);
    let outline = element.ink(Role::Border, at);
    match element.shape {
        Shape::Person => canvas.outline(x, y, w, h, ROUND, false, outline),
        Shape::Box => canvas.outline(x, y, w, h, SQUARE, false, outline),
        Shape::Database => {
            canvas.outline(x, y, w, h, ROUND, false, outline);
            layered::rule(canvas, x, y + 1, w, outline);
        }
        Shape::Queue => {
            canvas.outline(x, y, w, h, ['╓', '╖', '╙', '╜'], false, outline);
            for row in y + 1..y + h - 1 {
                canvas.put(x, row, '║', outline);
                canvas.put(x + w - 1, row, '║', outline);
            }
        }
    }
    let mut row = y + 1 + usize::from(element.shape == Shape::Database);
    let inside = w - 2;
    let name = element.name();
    label::centred(canvas, x + 1, row, inside, &name, Ink::plain(Role::Text));
    row += name.len();
    let kind = element.ink(Role::Label, at);
    label::centred(canvas, x + 1, row, inside, &element.kind, kind);
    row += element.kind.len();
    let quiet = Ink::plain(Role::Label);
    label::centred(canvas, x + 1, row, inside, &element.description, quiet);
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn c4(source: &str) -> Option<C4> {
        let mut lines = source.lines();
        let header = lines.next()?.trim();
        assert!(is_kind(header), "{header}");
        let rest: Vec<&str> = lines.collect();
        parse(&rest, header == "C4Dynamic")
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&c4(source).expect("parses"), width).expect("draws"))
    }

    const CONTAINER: &str = r#"C4Container
    title Container diagram for Internet Banking System
    System_Ext(email_system, "E-Mail System", "The internal Microsoft Exchange system", $tags="v1.0")
    Person(customer, Customer, "A customer of the bank, with personal bank accounts", $tags="v1.0")
    Container_Boundary(c1, "Internet Banking") {
        Container(spa, "Single-Page App", "JavaScript, Angular", "Provides all the Internet banking functionality to customers via their web browser")
        Container_Ext(mobile_app, "Mobile App", "C#, Xamarin", "Provides a limited subset of the Internet banking functionality to customers via their mobile device")
        ContainerDb(database, "Database", "SQL Database", "Stores user registration information, hashed auth credentials, access logs, etc.")
        ContainerQueue(queue, "Events", "Kafka")
    }
    Rel(customer, spa, "Uses", "HTTPS")
    UpdateRelStyle(customer, spa, $offsetY="-40")
    Rel(customer, mobile_app, "Uses")
    Rel_Back(database, spa, "Reads from and writes to", "sync, JDBC")
    BiRel(spa, queue, "Streams")
    Rel(email_system, customer, "Sends e-mails to")
    UpdateLayoutConfig($c4ShapeInRow="3", $c4BoundaryInRow="1")"#;

    #[test]
    fn elements_boundaries_and_relationships_are_read() {
        let c4 = c4(CONTAINER).unwrap();
        assert_eq!(
            c4.title.as_deref(),
            Some("Container diagram for Internet Banking System")
        );
        let names: Vec<&str> = c4.elements.iter().map(|e| e.name[0].as_str()).collect();
        assert_eq!(
            names,
            [
                "E-Mail System",
                "Customer",
                "Single-Page App",
                "Mobile App",
                "Database",
                "Events"
            ]
        );
        let kinds: Vec<String> = c4.elements.iter().map(|e| e.kind.join(" ")).collect();
        assert_eq!(
            kinds,
            [
                "[External Software System]",
                "[Person]",
                "[Container: JavaScript, Angular]",
                "[External Container: C#, Xamarin]",
                "[Container: SQL Database]",
                "[Container: Kafka]"
            ]
        );
        let shapes: Vec<Shape> = c4.elements.iter().map(|e| e.shape).collect();
        assert_eq!(
            shapes,
            [
                Shape::Box,
                Shape::Person,
                Shape::Box,
                Shape::Box,
                Shape::Database,
                Shape::Queue
            ]
        );
        assert!(c4.elements[0].external && c4.elements[3].external);
        assert_eq!(c4.frames.len(), 1);
        assert_eq!(
            (c4.frames[0].full(), c4.frames[0].parent),
            ("Internet Banking".into(), None)
        );
        let framed: Vec<Option<usize>> = c4.elements.iter().map(|e| e.frame).collect();
        assert_eq!(framed, [None, None, Some(0), Some(0), Some(0), Some(0)]);

        assert_eq!(c4.rels.len(), 5);
        assert_eq!(c4.rels[0].words, ["Uses", "[HTTPS]"]);
        // `Rel_Back` points the other way.
        assert_eq!((c4.rels[2].from, c4.rels[2].to), (2, 4));
        assert!(c4.rels[3].both && !c4.rels[0].both);
    }

    #[test]
    fn a_dynamic_diagram_numbers_its_steps() {
        let c4 = c4(
            "C4Dynamic\n  Container(a, \"A\")\n  Container(b, \"B\")\n  Rel(a, b, \"Asks\")\n  RelIndex(9, b, a, \"Answers\", \"JSON\")",
        )
        .unwrap();
        assert_eq!(c4.rels[0].words, ["1. Asks"]);
        assert_eq!(c4.rels[1].words, ["2. Answers", "[JSON]"]);
        assert_eq!((c4.rels[1].from, c4.rels[1].to), (1, 0));
    }

    #[test]
    fn deployment_nodes_nest_and_show_their_type() {
        let c4 = c4(r#"C4Deployment
    Deployment_Node(plc, "Big Bank plc", "Big Bank plc data center"){
        Deployment_Node(dn, "bigbank-api x8", "Ubuntu 16.04 LTS")
        {
            Container(api, "API Application", "Java and Spring MVC")
        }
        Node_R(db01, "bigbank-db01") { ContainerDb(db, "Database") }
    }
    Rel(api, db, "Reads from", "JDBC")"#)
        .unwrap();
        let labels: Vec<String> = c4.frames.iter().map(Frame::full).collect();
        assert_eq!(
            labels,
            [
                "Big Bank plc [Big Bank plc data center]",
                "bigbank-api x8 [Ubuntu 16.04 LTS]",
                "bigbank-db01"
            ]
        );
        assert_eq!(c4.frames[1].parent, Some(0));
        assert_eq!(c4.elements[0].frame, Some(1));
        assert_eq!(c4.elements[1].frame, Some(2));
    }

    #[test]
    fn broken_diagrams_are_not_drawn() {
        for source in [
            "C4Context",
            "C4Context\n  title Only a title",
            "C4Context\n  Person(a, \"A\")\n  }",
            "C4Context\n  Boundary(b, \"B\") {\n  Person(a, \"A\")",
            "C4Context\n  Person(a, \"A\"\n",
            "C4Context\n  Person(a, \"unclosed)",
            "C4Context\n  Person(a, \"A\")\n  Rel(a, nobody, \"Uses\")",
            "C4Context\n  Boundary(b, \"B\") {\n  Person(a, \"A\")\n  }\n  Rel(a, b, \"Into a boundary\")",
            "C4Context\n  Person(a, \"A\")\n  this is not a statement",
        ] {
            assert!(c4(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn styling_comments_and_a_brace_on_its_own_line_are_handled() {
        let c4 = c4(
            "C4Context\n  Boundary(b, \"B\")\n  {\n    Person(a, \"A\") %% a comment\n  }\n  UpdateElementStyle(a, $fontColor=\"red\")\n  SHOW_LEGEND()",
        )
        .unwrap();
        assert_eq!(c4.elements.len(), 1);
        assert_eq!(c4.elements[0].frame, Some(0));
    }

    #[test]
    fn a_container_diagram_draws_with_its_boundary_around_its_containers() {
        let c4 = c4(CONTAINER).unwrap();
        let diagram = draw(&c4, 160).unwrap();
        let out = plain(&diagram);
        for words in [
            "Container diagram for Internet Banking System",
            "Single-Page App",
            "[Container: JavaScript, Angular]",
            "[Person]",
            "Uses",
            "[HTTPS]",
            "Streams",
            "Internet Banking",
        ] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(
            out.contains('┆') && out.contains('┄'),
            "a dashed frame:\n{out}"
        );
        assert!(out.contains('▼'), "{out}");
        assert!(out.contains('║'), "a queue's double sides:\n{out}");

        // Everything in the boundary is inside its frame, and nothing else.
        let mut graph = Graph::new(Direction::Down);
        graph.boxes = c4.elements.iter().map(Element::size).collect();
        graph.frames = c4.frames.iter().map(|f| f.parent).collect();
        graph.framed = c4.elements.iter().map(|e| e.frame).collect();
        graph.links = c4
            .rels
            .iter()
            .map(|rel| Link {
                from: rel.from,
                to: rel.to,
                words: Some((widest(&rel.words), rel.words.len())),
            })
            .collect();
        graph.room = 160;
        let placed = layered::layout(&graph).unwrap();
        let frame = placed.frames[0].unwrap();
        for (element, rect) in c4.elements.iter().zip(&placed.boxes) {
            let inside = rect.x > frame.x
                && rect.y > frame.y
                && rect.x + rect.width < frame.x + frame.width
                && rect.y + rect.height < frame.y + frame.height;
            let apart = rect.x + rect.width <= frame.x
                || frame.x + frame.width <= rect.x
                || rect.y + rect.height <= frame.y
                || frame.y + frame.height <= rect.y;
            if element.frame.is_some() {
                assert!(inside, "{} is in the frame", element.alias);
            } else {
                assert!(apart, "{} is clear of the frame", element.alias);
            }
        }
    }

    #[test]
    fn external_elements_are_quiet_and_others_have_their_own_colours() {
        let c4 = c4(CONTAINER).unwrap();
        let diagram = draw(&c4, 160).unwrap();
        let ink_of = |needle: &str| {
            diagram
                .rows
                .iter()
                .flatten()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, ink)| *ink)
        };
        assert_eq!(
            ink_of("[External Software System]"),
            Some(Ink::plain(Role::Label))
        );
        assert_eq!(ink_of("[Person]"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("Customer"), Some(Ink::plain(Role::Text)));
        assert_eq!(diagram.keys.len(), 6);
        assert_eq!(diagram.keys[1], key("customer"));
    }

    #[test]
    fn unlinked_boxes_wrap_to_the_width_of_the_pane() {
        let mut source = String::from("C4Context\n");
        for at in 0..12 {
            source.push_str(&format!("  System(s{at}, \"System number {at}\")\n"));
        }
        let narrow = draw(&c4(&source).unwrap(), 80).unwrap();
        let wide = draw(&c4(&source).unwrap(), 400).unwrap();
        assert!(narrow.width <= 80, "{}", plain(&narrow));
        assert!(narrow.rows.len() > wide.rows.len());
    }

    #[test]
    fn nested_boundaries_draw_as_frames_in_frames() {
        let out = drawn(
            "C4Context\n  Enterprise_Boundary(e, \"Enterprise\") {\n    System_Boundary(s, \"Inner\") {\n      System(a, \"Alpha\")\n    }\n    System(b, \"Beta\")\n  }\n  Person(p, \"User\")\n  Rel(p, a, \"Uses\")\n  Rel(a, b, \"Calls\")",
            120,
        );
        let row_of = |needle: &str| out.lines().position(|row| row.contains(needle));
        let (outer, inner) = (row_of("Enterprise"), row_of("Inner"));
        assert!(outer.is_some() && inner.is_some(), "{out}");
        assert!(
            outer < inner || outer > row_of("Alpha"),
            "the outer frame's label is outside the inner one:\n{out}"
        );
        let alpha = out.lines().find(|row| row.contains("Alpha")).unwrap();
        assert_eq!(
            alpha.matches('┆').count(),
            4,
            "two frames either side:\n{out}"
        );
    }

    #[test]
    fn huge_deep_and_looping_diagrams_stay_bounded() {
        let mut deep = String::from("C4Context\n");
        for at in 0..20 {
            deep.push_str(&format!("Boundary(b{at}, \"B{at}\") {{\n"));
        }
        deep.push_str("Person(p, \"P\")\n");
        deep.push_str(&"}\n".repeat(20));
        assert!(c4(&deep).is_none(), "nested too deep");

        let mut many = String::from("C4Context\n");
        for at in 0..MAX_ELEMENTS + 1 {
            many.push_str(&format!("System(s{at}, \"S\")\n"));
        }
        assert!(c4(&many).is_none(), "too many boxes");

        let mut ring = String::from("C4Container\n");
        for at in 0..30 {
            ring.push_str(&format!(
                "Container(c{at}, \"Container {at}\", \"Rust\", \"Does a thing\")\n"
            ));
        }
        for at in 0..30 {
            ring.push_str(&format!(
                "Rel(c{at}, c{}, \"Next\", \"gRPC\")\nRel(c{at}, c{at}, \"Self\")\n",
                (at + 1) % 30
            ));
        }
        if let Some(diagram) = draw(&c4(&ring).unwrap(), 100) {
            assert!(diagram.rows.len() * diagram.width <= super::super::canvas::MAX_CELLS);
            assert!(plain(&diagram).contains('↻'));
        }
    }
}
