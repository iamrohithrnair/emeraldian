//! Mind maps: `mindmap`.
//!
//! A mind map is written as an outline, each line a node under the nearest
//! line above it that is indented less, and it is drawn as one: the root in a
//! box, and every other node on a line of its own under its parent, joined to
//! it by the lines of a tree, `├──` and `└──`, the way `tree` draws folders.
//! Each branch from the root has a colour of its own, which its lines and its
//! first node are drawn in, so a branch can be followed down the page, while
//! the nodes further out keep the body colour, where they read best.
//!
//! The shapes Mermaid draws nodes in, `[square]`, `(rounded)`, `((circle))`,
//! `))bang((`, `)cloud(` and `{{hexagon}}`, are read but not drawn, since the
//! tree already shows how the nodes hang together. Icons and classes only
//! change how a node looks in a browser, so they are passed over. A long
//! label wraps onto more lines under its node, and a markdown string may run
//! over several lines of the source, as Mermaid allows.

use super::canvas::Canvas;
use super::label::{self, widest};
use super::{Diagram, Ink, ROUND, Role, key, pairs, syntax};

/// Past these a map is too big to read in a terminal.
const MAX_NODES: usize = 500;
const MAX_DEPTH: usize = 30;
/// Source lines a markdown string may run over.
const MAX_STRING_LINES: usize = 50;
/// Columns each level of the tree is indented by.
const STEP: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// What its colour is picked by: its id, or its words when it has none.
    pub id: String,
    pub label: Vec<String>,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mindmap {
    /// Every node, the root first.
    pub nodes: Vec<Node>,
}

/// Reads the lines after `mindmap`. `None` for a map with no root, or with
/// two, which Mermaid doesn't draw either.
pub fn parse(lines: &[&str]) -> Option<Mindmap> {
    let mut nodes: Vec<Node> = Vec::new();
    // The nodes a later line may hang from: each one's indent, place and
    // depth, deepest last.
    let mut open: Vec<(usize, usize, usize)> = Vec::new();
    let mut at = 0;
    while let Some(&raw) = lines.get(at) {
        at += 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with("::icon(") || text.starts_with(":::") {
            continue;
        }
        // A quoted label left open carries on over the lines after it.
        let mut statement = text.to_string();
        let mut joined = 0;
        while statement.matches('"').count() % 2 == 1 {
            let next = lines.get(at)?;
            at += 1;
            joined += 1;
            if joined > MAX_STRING_LINES {
                return None;
            }
            statement.push('\n');
            statement.push_str(next.trim());
        }
        let (id, label) = node(&statement)?;
        let indent = indent_of(raw);
        while open.last().is_some_and(|&(other, _, _)| other >= indent) {
            open.pop();
        }
        let depth = match open.last() {
            Some(&(_, _, depth)) => depth + 1,
            None if nodes.is_empty() => 0,
            None => return None,
        };
        if depth > MAX_DEPTH || nodes.len() >= MAX_NODES {
            return None;
        }
        let place = nodes.len();
        nodes.push(Node {
            id,
            label,
            children: Vec::new(),
        });
        if let Some(&(_, parent, _)) = open.last() {
            nodes[parent].children.push(place);
        }
        open.push((indent, place, depth));
    }
    if nodes.is_empty() {
        return None;
    }
    Some(Mindmap { nodes })
}

/// Columns of a line's indent, a tab counting as four.
fn indent_of(line: &str) -> usize {
    line.chars()
        .take_while(|c| c.is_whitespace())
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// A node's shapes: the bracket that opens its label, and the one that
/// closes it, longest first so `((` is not read as `(`.
const SHAPES: &[(&str, &str)] = &[
    ("((", "))"),
    ("))", "(("),
    ("{{", "}}"),
    ("(", ")"),
    (")", "("),
    ("[", "]"),
];

/// A node's id and the lines of its label, from how it is written: `id`
/// followed by a label in one of the shapes' brackets, or words of its own.
/// `None` when a shape's brackets are left open.
fn node(text: &str) -> Option<(String, Vec<String>)> {
    // A class only changes how the node looks.
    let text = match syntax::outside_quotes(text, ":::", &['"']) {
        Some(at) => text[..at].trim_end(),
        None => text,
    };
    let opening = text.find(['(', ')', '[', '{']);
    if let Some(at) = opening
        && !text[..at].contains(char::is_whitespace)
    {
        let rest = &text[at..];
        if let Some(&(open, close)) = SHAPES.iter().find(|(open, _)| rest.starts_with(open)) {
            let inside = rest.strip_prefix(open)?.strip_suffix(close)?;
            let id = &text[..at];
            let mut label = words(inside);
            if label.iter().all(String::is_empty) {
                label = words(id);
            }
            let id = if id.is_empty() { inside.trim() } else { id };
            return Some((id.to_string(), label));
        }
    }
    Some((text.to_string(), words(text)))
}

/// A label as the lines it is shown on. A markdown string, in backticks,
/// loses the asterisks that mark its italics as well as its bold.
fn words(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let markdown = raw.starts_with("\"`") || raw.starts_with('`');
    let mut lines = label::lines(raw);
    if markdown {
        for line in &mut lines {
            *line = line.replace('*', "");
        }
    }
    lines
}

/// Draws a mind map, or `None` when it is too big.
pub fn draw(map: &Mindmap) -> Option<Diagram> {
    let root = map.nodes.first()?;
    let branches = &root.children;
    let box_width = widest(&root.label) + 4;
    let box_rows = root.label.len() + 2;

    // Every node below the root, in the order it is drawn: depth first, with
    // its depth, its branch, and whether each node on its way down from the
    // branch, itself last, has a sibling after it.
    let mut rows: Vec<(usize, usize, usize, Vec<bool>)> = Vec::new();
    let mut stack: Vec<(usize, usize, Vec<bool>)> = Vec::new();
    for (branch, &first) in branches.iter().enumerate().rev() {
        stack.push((first, branch, vec![branch + 1 < branches.len()]));
    }
    while let Some((at, branch, more)) = stack.pop() {
        let node = map.nodes.get(at)?;
        let depth = more.len();
        let children = &node.children;
        for (k, &child) in children.iter().enumerate().rev() {
            let mut deeper = more.clone();
            deeper.push(k + 1 < children.len());
            stack.push((child, branch, deeper));
        }
        rows.push((at, depth, branch, more));
    }

    let column = |depth: usize| 2 + STEP * (depth - 1);
    let mut width = box_width;
    let mut height = box_rows;
    for &(at, depth, _, _) in &rows {
        let label = &map.nodes[at].label;
        width = width.max(column(depth) + STEP + widest(label));
        height += label.len().max(1);
    }
    let mut canvas = Canvas::new(width, height)?;

    let quiet = Ink::plain(Role::Border);
    canvas.outline(0, 0, box_width, box_rows, ROUND, false, quiet);
    let words = Ink::plain(Role::Text);
    label::centred(&mut canvas, 1, 1, box_width - 2, &root.label, words);
    if !branches.is_empty() {
        canvas.put(2, box_rows - 1, '┬', quiet);
    }

    let mut row = box_rows;
    for (at, depth, branch, more) in &rows {
        let line = Ink::of(Role::Line, *branch);
        let words = if *depth == 1 {
            Ink::of(Role::Label, *branch)
        } else {
            Ink::plain(Role::Text)
        };
        let label = &map.nodes[*at].label;
        // Whether a sibling comes after it, and after each node above it.
        let (followed, above) = more.split_last()?;
        let x = column(*depth);
        for (k, text) in label.iter().enumerate() {
            for (level, &goes_on) in above.iter().enumerate() {
                if goes_on {
                    canvas.put(column(level + 1), row + k, '│', line);
                }
            }
            if k == 0 {
                canvas.put(x, row, if *followed { '├' } else { '└' }, line);
                canvas.put(x + 1, row, '─', line);
                canvas.put(x + 2, row, '─', line);
            } else if *followed {
                canvas.put(x, row + k, '│', line);
            }
            canvas.text(x + STEP, row + k, text, words);
        }
        row += label.len().max(1);
    }

    let mut diagram = canvas.finish();
    diagram.keys = branches
        .iter()
        .filter_map(|&at| map.nodes.get(at))
        .map(|node| key(&node.id))
        .collect();
    diagram.links = pairs((1..branches.len()).map(|k| (k - 1, k)));
    Some(diagram)
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn map(source: &str) -> Option<Mindmap> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str) -> String {
        plain(&draw(&map(source).expect("parses")).expect("draws"))
    }

    const SAMPLE: &str = "  root((mindmap))
    Origins
      Long history
      ::icon(fa fa-book)
      Popularisation
        British popular psychology author Tony Buzan
    Research
      On effectiveness<br/>and features
      On Automatic creation
        Uses
            Creative techniques
            Strategic planning
            Argument mapping
    Tools
      Pen and paper
      Mermaid";

    #[test]
    fn the_outline_becomes_a_tree() {
        let m = map(SAMPLE).unwrap();
        assert_eq!(m.nodes[0].label, ["mindmap"]);
        assert_eq!(m.nodes[0].id, "root");
        let names = |at: usize| -> Vec<String> {
            m.nodes[at]
                .children
                .iter()
                .map(|&c| m.nodes[c].label.join(" "))
                .collect()
        };
        assert_eq!(names(0), ["Origins", "Research", "Tools"]);
        assert_eq!(
            names(m.nodes[0].children[0]),
            ["Long history", "Popularisation"]
        );
        assert_eq!(
            names(m.nodes[0].children[1]),
            ["On effectiveness and features", "On Automatic creation"]
        );
        assert_eq!(m.nodes.len(), 15, "the icon is not a node");
    }

    #[test]
    fn shapes_classes_and_markdown_strings_are_read() {
        let m = map(
            "mindmap\n  a[Square]\n    b(Rounded)\n    c((Circle)):::urgent\n    d))Bang((\n    e)Cloud(\n    f{{Hexagon}}\n    g[\"`**Bold** and *italic*\nover two lines`\"]\n    Just words (with brackets)",
        )
        .unwrap();
        let labels: Vec<String> = m.nodes.iter().map(|n| n.label.join("|")).collect();
        assert_eq!(
            labels,
            [
                "mindmap",
                "Square",
                "Rounded",
                "Circle",
                "Bang",
                "Cloud",
                "Hexagon",
                "Bold and italic|over two lines",
                "Just words (with brackets)"
            ]
        );
        assert_eq!(m.nodes[1].id, "a");
        // `mindmap` was the root here, so the rest hang from it.
        assert_eq!(m.nodes[0].children, [1]);
        assert_eq!(m.nodes[1].children.len(), 7);
    }

    #[test]
    fn unclear_indentation_hangs_from_the_nearest_shallower_line() {
        let m = map("Root\n    A\n        B\n      C").unwrap();
        assert_eq!(m.nodes[1].children, [2, 3], "C is A's, beside B");
    }

    #[test]
    fn broken_maps_are_not_drawn() {
        for source in [
            "",
            "   \n  ",
            "Root\nSecond root",
            "Root\n  a[unclosed",
            "Root\n  a((half)",
            "Root\n  a[\"`never closed",
            "::icon(fa fa-book)",
        ] {
            assert!(map(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_map_draws_as_a_tree_under_its_root() {
        let out = drawn(SAMPLE);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "╭─────────╮");
        assert_eq!(rows[1], "│ mindmap │");
        assert_eq!(rows[2], "╰─┬───────╯");
        assert_eq!(rows[3], "  ├── Origins");
        assert!(out.contains("  │   ├── Long history"), "{out}");
        assert!(
            out.contains(
                "  │       └── British popular psychology\n  │           author Tony Buzan"
            ),
            "{out}"
        );
        assert!(
            out.contains("  │   ├── On effectiveness\n  │   │   and features"),
            "{out}"
        );
        assert!(out.contains("  └── Tools"), "{out}");
        assert!(out.ends_with("      └── Mermaid"), "{out}");
    }

    #[test]
    fn a_long_label_wraps_under_its_node() {
        let out = drawn(
            "Root\n  A label long enough that it has to wrap onto a second line\n    Child\n  Last",
        );
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[3], "  ├── A label long enough that it");
        assert_eq!(rows[4], "  │   has to wrap onto a second");
        assert_eq!(rows[5], "  │   line");
        assert_eq!(rows[6], "  │   └── Child");
        assert_eq!(rows[7], "  └── Last");
    }

    #[test]
    fn each_branch_has_a_colour_and_deeper_names_are_body_text() {
        let diagram = draw(&map(SAMPLE).unwrap()).unwrap();
        assert_eq!(
            diagram.keys,
            [key("Origins"), key("Research"), key("Tools")]
        );
        assert_eq!(diagram.links, [(0, 1), (1, 2)]);
        let ink_of = |needle: &str| super::super::ink_of(&diagram, needle);
        assert_eq!(ink_of("Research"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("Uses"), Some(Ink::plain(Role::Text)));
        assert_eq!(ink_of("└──"), Some(Ink::of(Role::Line, 0)));
        assert_eq!(ink_of("mindmap"), Some(Ink::plain(Role::Text)));
    }

    #[test]
    fn a_lone_root_and_unicode_labels_draw() {
        assert_eq!(
            drawn("日本語の地図"),
            "╭──────────────╮\n│ 日本語の地図 │\n╰──────────────╯"
        );
        let out = drawn("Root\n  🙂 emoji\n  café");
        assert!(
            out.contains("├── 🙂 emoji") && out.contains("└── café"),
            "{out}"
        );
    }

    #[test]
    fn huge_and_deep_maps_stay_bounded() {
        let mut deep = String::from("Root\n");
        for depth in 1..=MAX_DEPTH + 1 {
            deep.push_str(&format!("{}n{depth}\n", " ".repeat(depth)));
        }
        assert!(map(&deep).is_none(), "too deep");

        let mut wide = String::from("Root\n");
        for at in 0..MAX_NODES {
            wide.push_str(&format!("  node {at}\n"));
        }
        assert!(map(&wide).is_none(), "too many nodes");

        let mut big = String::from("Root\n");
        for at in 0..400 {
            let depth = 1 + at % MAX_DEPTH;
            big.push_str(&format!("{}{}\n", " ".repeat(depth), "word ".repeat(20)));
        }
        let start = std::time::Instant::now();
        if let Some(diagram) = draw(&map(&big).unwrap()) {
            assert!(diagram.rows.len() * diagram.width <= super::super::canvas::MAX_CELLS);
        }
        assert!(start.elapsed().as_secs() < 2);
    }
}
