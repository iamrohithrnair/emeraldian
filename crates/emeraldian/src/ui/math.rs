//! TeX maths, set in Unicode for the reading pane.
//!
//! Obsidian draws `$…$` and `$$…$$` with MathJax. A terminal has one size of
//! type and nowhere between two rows to put a glyph, but Unicode carries most
//! of what an equation needs: Greek and the operators, super- and subscript
//! forms of the digits and many letters, double-struck and script capitals,
//! and pieces for building a bracket several rows tall.
//!
//! Inline maths is set on one line, since it sits in a line of prose: `x²`,
//! `(a + b)/2`, `√(x + 1)`. Display maths takes the rows it needs, so a
//! fraction stacks over a rule, a sum carries its limits above and below it,
//! and a matrix gets brackets as tall as it is.
//!
//! Spacing follows TeX's rules rather than the source's: a relation or a binary
//! operator gets a space either side, a minus sign with nothing to subtract
//! from gets none, and scripts are set tight. That is what turns `a=b+c` into
//! `a = b + c`, as MathJax sets it, and `-x` into `−x`.
//!
//! The parser is forgiving, because a note is read while it is being written.
//! An unknown command is shown as typed and an unclosed brace closes itself,
//! so a half-finished equation still reads as one.

use emeraldian_core::markdown::Align;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display maths, typeset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// The rows, each `width` columns wide.
    pub rows: Vec<String>,
    pub width: usize,
    /// The row the equation's main line sits on, which a tag lines up with.
    pub baseline: usize,
    /// The equation's number from `\tag`, with its brackets.
    pub tag: Option<String>,
}

/// Sets inline maths on one line.
///
/// Spaces are non-breaking except after a relation or a binary operator,
/// which is where TeX lets a line break inside a formula.
#[must_use]
pub fn inline(tex: &str) -> String {
    let mut parser = Parser::new(tex, 0);
    let nodes = parser.top();
    let mut text = one_line(&layout(&nodes, Ctx::INLINE, None));
    if let Some(tag) = parser.tag {
        text.push_str("\u{a0}\u{a0}");
        text.push_str(&tag);
    }
    text
}

/// Sets display maths on as many rows as it needs.
#[must_use]
pub fn display(tex: &str) -> Display {
    let mut parser = Parser::new(tex, 0);
    let nodes = parser.top();
    // `\\` outside an environment starts a new line, centred like the others.
    let lines: Vec<Boxed> = nodes
        .split(|node| matches!(node, Node::Break))
        .filter(|line| !line.is_empty())
        .map(|line| layout(line, Ctx::DISPLAY, None))
        .collect();

    let width = lines.iter().map(|line| line.width).max().unwrap_or(0);
    let mut rows: Vec<String> = lines.iter().flat_map(|line| centred(line, width)).collect();
    let baseline = match lines.as_slice() {
        [only] => only.baseline,
        _ => rows.len().saturating_sub(1) / 2,
    };
    if rows.is_empty() {
        rows.push(String::new());
    }
    // Scripts are set with non-breaking spaces, which mean nothing on rows
    // that never wrap.
    for row in &mut rows {
        *row = row.replace(NBSP, " ");
    }
    Display {
        rows,
        width,
        baseline,
        tag: parser.tag,
    }
}

const NBSP: char = '\u{a0}';

/// How deeply groups may nest before the rest is shown as typed. Far past
/// anything written by hand, and well short of exhausting the stack.
const MAX_DEPTH: usize = 48;

// ---------------------------------------------------------------------------
// The parsed formula
// ---------------------------------------------------------------------------

/// TeX's classes of atom, which decide the spacing between neighbours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Ord,
    /// A large or named operator: `∑`, `lim`, `sin`.
    Op,
    /// A binary operator: `+`, `×`.
    Bin,
    /// A relation: `=`, `<`, `→`.
    Rel,
    Open,
    Close,
    Punct,
    /// A `\left…\right` group.
    Inner,
}

#[derive(Debug, Clone)]
enum Node {
    Sym(String, Class),
    /// `\text{…}`: words, kept with their spaces.
    Text(String),
    /// `{…}`, which TeX treats as a single ordinary atom.
    Group(Vec<Node>),
    /// `\sum`, `\lim`, `\sin`. With `limits`, display maths puts the scripts
    /// above and below rather than beside.
    Op {
        name: String,
        limits: bool,
    },
    Scripts {
        base: Box<Node>,
        sub: Option<Vec<Node>>,
        sup: Option<Vec<Node>>,
    },
    Frac {
        num: Vec<Node>,
        den: Vec<Node>,
        rule: bool,
    },
    Binom(Vec<Node>, Vec<Node>),
    Root {
        index: Option<Vec<Node>>,
        body: Vec<Node>,
    },
    /// `\hat`, `\vec` and the rest, as a combining mark. `last` puts it on
    /// the last letter of a longer body rather than on every one.
    Accent {
        mark: char,
        last: bool,
        body: Vec<Node>,
    },
    Wide(Wide, Vec<Node>),
    /// `\overbrace` or `\underbrace`, whose script is a label.
    Brace {
        over: bool,
        body: Vec<Node>,
    },
    /// `\overset`, `\underset`, and the arrows that stretch to their labels.
    Stack {
        base: Vec<Node>,
        over: Option<Vec<Node>>,
        under: Option<Vec<Node>>,
        arrow: Option<Arrow>,
    },
    /// `\left( … \right)`.
    Fenced {
        open: String,
        body: Vec<Node>,
        close: String,
    },
    Middle(String),
    Array(Grid),
    Boxed(Vec<Node>),
    Phantom(Vec<Node>),
    Strike(Vec<Node>),
    /// `\mathrel{…}` and its kin: a group spaced as the class it names.
    Classed(Class, Vec<Node>),
    Space(usize),
    /// `\\` outside an environment.
    Break,
}

/// A line across the whole of its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wide {
    Overline,
    Underline,
    RightArrow,
    LeftArrow,
    BothArrow,
}

/// An arrow that grows to fit the labels on it: `\xrightarrow{f}`.
#[derive(Debug, Clone, Copy)]
struct Arrow {
    /// Drawn in a line of text, where it can't grow.
    glyph: &'static str,
    tail: &'static str,
    fill: &'static str,
    head: &'static str,
}

impl Arrow {
    fn stretched(self, width: usize) -> String {
        let ends = self.tail.width() + self.head.width();
        let fill = width.max(ends + 1) - ends;
        format!("{}{}{}", self.tail, self.fill.repeat(fill), self.head)
    }
}

/// A matrix, a set of cases, or lines aligned on their `&`s.
#[derive(Debug, Clone)]
struct Grid {
    rows: Vec<Vec<Vec<Node>>>,
    columns: Columns,
    open: &'static str,
    close: &'static str,
}

#[derive(Debug, Clone)]
enum Columns {
    /// Every column aligned the same way.
    All(Align),
    /// `aligned`: pairs of right- and left-aligned columns that meet at the
    /// `&`, which is how a column of `=` signs lines up.
    Pairs,
    /// An `array` column spec.
    Spec(Vec<Align>),
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// What ended a list of atoms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    End,
    Brace,
    Right,
    Cell,
    Row,
    Environment,
}

/// Which terminators a list answers to. Any other is read where it stands.
#[derive(Debug, Clone, Copy, Default)]
struct Ends {
    brace: bool,
    right: bool,
    cells: bool,
}

struct Parser<'a> {
    src: &'a str,
    at: usize,
    depth: usize,
    tag: Option<String>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, depth: usize) -> Self {
        Self {
            src,
            at: 0,
            depth,
            tag: None,
        }
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.at += ch.len_utf8();
        Some(ch)
    }

    fn eat(&mut self, ch: char) -> bool {
        let found = self.peek() == Some(ch);
        if found {
            self.at += ch.len_utf8();
        }
        found
    }

    /// Skips spaces, which mean nothing in maths, and `%` comments.
    fn skip_space(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.bump();
            } else if ch == '%' {
                while !matches!(self.bump(), None | Some('\n')) {}
            } else {
                break;
            }
        }
    }

    /// Everything left, as typed: what is shown once nesting goes too deep.
    fn rest_as_text(&mut self) -> Node {
        let rest = self.rest();
        self.at = self.src.len();
        Node::Text(rest.to_string())
    }

    fn top(&mut self) -> Vec<Node> {
        self.list(Ends::default()).0
    }

    /// Atoms up to the first of `ends`, or to the end of the input.
    fn list(&mut self, ends: Ends) -> (Vec<Node>, Stop) {
        if self.depth > MAX_DEPTH {
            return (vec![self.rest_as_text()], Stop::End);
        }
        self.depth += 1;
        let list = self.atoms(ends);
        self.depth -= 1;
        list
    }

    fn atoms(&mut self, ends: Ends) -> (Vec<Node>, Stop) {
        let mut nodes = Vec::new();
        // `{a \over b}`: TeX's infix fraction, which takes everything before
        // it in the group as the numerator.
        let mut over: Option<(usize, &str)> = None;

        let stop = loop {
            self.skip_space();
            let Some(ch) = self.peek() else {
                break Stop::End;
            };
            match ch {
                '}' => {
                    self.bump();
                    if ends.brace {
                        break Stop::Brace;
                    }
                }
                '{' => {
                    self.bump();
                    let group = self.group();
                    nodes.push(Node::Group(group));
                }
                '^' | '_' => {
                    self.bump();
                    let script = self.argument();
                    attach(&mut nodes, ch == '^', script);
                }
                '\'' => {
                    let mut primes = 0;
                    while self.eat('\'') {
                        primes += 1;
                    }
                    attach(&mut nodes, true, vec![prime(primes)]);
                }
                '&' => {
                    self.bump();
                    if ends.cells {
                        break Stop::Cell;
                    }
                }
                '~' => {
                    self.bump();
                    nodes.push(Node::Space(1));
                }
                '\\' => match self.command_name() {
                    "\\" | "cr" | "newline" => {
                        self.skip_optional();
                        if ends.cells {
                            break Stop::Row;
                        }
                        nodes.push(Node::Break);
                    }
                    "right" if ends.right => break Stop::Right,
                    "end" if ends.cells => break Stop::Environment,
                    name @ ("over" | "atop" | "choose") => over = Some((nodes.len(), name)),
                    name @ ("limits" | "nolimits") => set_limits(&mut nodes, name == "limits"),
                    name => nodes.extend(self.command(name)),
                },
                ch => {
                    self.bump();
                    nodes.push(char_node(ch));
                }
            }
        };

        if let Some((at, kind)) = over {
            let den = nodes.split_off(at);
            let num = std::mem::take(&mut nodes);
            nodes.push(match kind {
                "choose" => Node::Binom(num, den),
                kind => Node::Frac {
                    num,
                    den,
                    rule: kind == "over",
                },
            });
        }
        (nodes, stop)
    }

    /// The rest of a `{…}` group whose opening brace has been read.
    fn group(&mut self) -> Vec<Node> {
        self.list(Ends {
            brace: true,
            ..Ends::default()
        })
        .0
    }

    /// One argument: a `{…}` group, a command, or a single character.
    fn argument(&mut self) -> Vec<Node> {
        if self.depth > MAX_DEPTH {
            return vec![self.rest_as_text()];
        }
        self.skip_space();
        match self.peek() {
            Some('{') => {
                self.bump();
                self.group()
            }
            Some('\\') => {
                self.depth += 1;
                let name = self.command_name();
                let nodes = self.command(name).into_iter().collect();
                self.depth -= 1;
                nodes
            }
            Some(ch) => {
                self.bump();
                vec![char_node(ch)]
            }
            None => Vec::new(),
        }
    }

    /// Reads a command after its backslash: a run of letters, or the single
    /// character that follows.
    fn command_name(&mut self) -> &'a str {
        self.bump();
        let rest = self.rest();
        let letters = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
        let len = if letters > 0 {
            letters
        } else {
            rest.chars().next().map_or(0, char::len_utf8)
        };
        self.at += len;
        &rest[..len]
    }

    /// An argument as the text it was written with, for `\text` and `\begin`.
    fn raw_group(&mut self) -> &'a str {
        self.skip_space();
        let rest = self.rest();
        if !rest.starts_with('{') {
            // TeX also takes a lone character or command: `\text x`.
            let len = match rest.strip_prefix('\\') {
                Some(after) => {
                    let letters = after.bytes().take_while(u8::is_ascii_alphabetic).count();
                    1 + if letters > 0 {
                        letters
                    } else {
                        after.chars().next().map_or(0, char::len_utf8)
                    }
                }
                None => rest.chars().next().map_or(0, char::len_utf8),
            };
            self.at += len;
            return &rest[..len];
        }

        let mut depth = 0usize;
        let mut escaped = false;
        for (at, ch) in rest.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        self.at += at + 1;
                        return &rest[1..at];
                    }
                }
                _ => {}
            }
        }
        // Never closed: it runs to the end.
        self.at = self.src.len();
        &rest[1..]
    }

    /// `[…]`, as in `\sqrt[3]{x}`, if there is one.
    fn optional(&mut self) -> Option<Vec<Node>> {
        let start = self.at;
        self.skip_space();
        if !self.eat('[') {
            self.at = start;
            return None;
        }
        let rest = self.rest();
        let mut braces = 0usize;
        let end = rest
            .char_indices()
            .find(|&(_, ch)| match ch {
                '{' => {
                    braces += 1;
                    false
                }
                '}' => {
                    braces = braces.saturating_sub(1);
                    false
                }
                ']' => braces == 0,
                _ => false,
            })
            .map_or(rest.len(), |(at, _)| at);
        self.at += (end + 1).min(rest.len());
        Some(Parser::new(&rest[..end], self.depth + 1).top())
    }

    /// The spacing argument `\\[2pt]` may carry, which means nothing here.
    fn skip_optional(&mut self) {
        let start = self.at;
        self.skip_space();
        if self.peek() == Some('[') {
            self.optional();
        } else {
            self.at = start;
        }
    }

    /// The delimiter after `\left`, `\right`, `\middle` or `\big`. A `.` is
    /// none at all.
    fn delimiter(&mut self) -> String {
        self.skip_space();
        match self.peek() {
            Some('\\') => {
                let name = self.command_name();
                match symbol(name) {
                    Some((glyph, _)) => glyph.to_string(),
                    None => format!("\\{name}"),
                }
            }
            Some('.') => {
                self.bump();
                String::new()
            }
            Some(ch) => {
                self.bump();
                match ch {
                    '<' => "⟨".into(),
                    '>' => "⟩".into(),
                    ch => ch.to_string(),
                }
            }
            None => String::new(),
        }
    }

    /// The text of a `\text` argument.
    fn text(&mut self) -> String {
        let raw = self.raw_group();
        text_content(raw, self.depth)
    }

    /// The rows and cells up to `\end`, or to the end of the input.
    fn rows(&mut self) -> Vec<Vec<Vec<Node>>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        loop {
            let (cell, stop) = self.list(Ends {
                cells: true,
                ..Ends::default()
            });
            row.push(cell);
            match stop {
                Stop::Cell => {}
                Stop::Row => rows.push(std::mem::take(&mut row)),
                stop => {
                    if stop == Stop::Environment {
                        self.raw_group();
                    }
                    rows.push(row);
                    break;
                }
            }
        }
        // A `\\` after the last row leaves an empty one that nobody meant.
        while rows
            .last()
            .is_some_and(|row: &Vec<Vec<Node>>| row.iter().all(Vec::is_empty))
        {
            rows.pop();
        }
        rows
    }

    fn environment(&mut self) -> Option<Node> {
        let name = self.raw_group();
        let base = name.trim_end_matches('*');
        let (open, close, columns) = match base {
            "pmatrix" => ("(", ")", Columns::All(Align::Center)),
            "bmatrix" => ("[", "]", Columns::All(Align::Center)),
            "Bmatrix" => ("{", "}", Columns::All(Align::Center)),
            "vmatrix" => ("|", "|", Columns::All(Align::Center)),
            "Vmatrix" => ("‖", "‖", Columns::All(Align::Center)),
            "cases" | "dcases" => ("{", "", Columns::All(Align::Left)),
            "rcases" | "drcases" => ("", "}", Columns::All(Align::Left)),
            "aligned" | "align" | "alignat" | "alignedat" | "split" | "flalign" => {
                ("", "", Columns::Pairs)
            }
            "eqnarray" => (
                "",
                "",
                Columns::Spec(vec![Align::Right, Align::Center, Align::Left]),
            ),
            "array" | "darray" | "subarray" => ("", "", Columns::Spec(spec(self.raw_group()))),
            // `matrix`, `gathered`, `equation`, and anything unknown: centred.
            _ => ("", "", Columns::All(Align::Center)),
        };
        if matches!(base, "alignat" | "alignedat") {
            self.raw_group();
        }

        let mut rows = self.rows();
        // `equation` and its like hold one formula, which needs no grid.
        if open.is_empty() && close.is_empty() && rows.len() == 1 && rows[0].len() == 1 {
            return rows.pop().and_then(|mut row| row.pop()).map(Node::Group);
        }
        Some(Node::Array(Grid {
            rows,
            columns,
            open,
            close,
        }))
    }

    /// Reads whatever a command takes and returns what it stands for, or
    /// nothing for commands that only affect what MathJax draws: colours,
    /// labels, sizes.
    fn command(&mut self, name: &'a str) -> Option<Node> {
        if let Some((glyph, class)) = symbol(name) {
            return Some(Node::Sym(glyph.to_string(), class));
        }
        if let Some((glyph, limits)) = big_operator(name) {
            return Some(Node::Op {
                name: glyph.to_string(),
                limits,
            });
        }
        if let Some((text, limits)) = named_operator(name) {
            return Some(Node::Op {
                name: text.to_string(),
                limits,
            });
        }
        if let Some(width) = space(name) {
            return Some(Node::Space(width));
        }
        if let Some(font) = font(name) {
            let body = self.argument();
            return Some(Node::Group(restyle(body, font)));
        }
        if let Some((mark, last)) = accent(name) {
            let body = self.argument();
            return Some(Node::Accent { mark, last, body });
        }

        Some(match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.argument();
                let den = self.argument();
                Node::Frac {
                    num,
                    den,
                    rule: true,
                }
            }
            "binom" | "dbinom" | "tbinom" => {
                let n = self.argument();
                let k = self.argument();
                Node::Binom(n, k)
            }
            "sqrt" => {
                let index = self.optional();
                let body = self.argument();
                Node::Root { index, body }
            }
            "text" | "textrm" | "textnormal" | "textup" | "textmd" | "textsf" | "texttt"
            | "textit" | "textsl" | "emph" | "mbox" | "hbox" => Node::Text(self.text()),
            "textbf" => Node::Text(
                self.text()
                    .chars()
                    .map(|ch| styled(ch, Font::Bold))
                    .collect(),
            ),
            "TeX" | "LaTeX" => Node::Text(name.to_string()),
            "operatorname" | "operatornamewithlimits" => {
                let limits = self.eat('*') || name == "operatornamewithlimits";
                let body = self.argument();
                Node::Op {
                    name: plain(&body),
                    limits,
                }
            }
            "mathop" => {
                let body = self.argument();
                Node::Op {
                    name: plain(&body),
                    limits: true,
                }
            }
            "mathord" => Node::Classed(Class::Ord, self.argument()),
            "mathbin" => Node::Classed(Class::Bin, self.argument()),
            "mathrel" => Node::Classed(Class::Rel, self.argument()),
            "mathopen" => Node::Classed(Class::Open, self.argument()),
            "mathclose" => Node::Classed(Class::Close, self.argument()),
            "mathpunct" => Node::Classed(Class::Punct, self.argument()),
            "mathinner" => Node::Classed(Class::Inner, self.argument()),
            "left" => {
                let open = self.delimiter();
                let (body, stop) = self.list(Ends {
                    right: true,
                    ..Ends::default()
                });
                let close = if stop == Stop::Right {
                    self.delimiter()
                } else {
                    String::new()
                };
                Node::Fenced { open, body, close }
            }
            // A `\right` without its `\left`.
            "right" => Node::Sym(self.delimiter(), Class::Close),
            "middle" => Node::Middle(self.delimiter()),
            "big" | "Big" | "bigg" | "Bigg" => {
                let delimiter = self.delimiter();
                let class = delimiter_class(&delimiter);
                Node::Sym(delimiter, class)
            }
            "bigl" | "Bigl" | "biggl" | "Biggl" => Node::Sym(self.delimiter(), Class::Open),
            "bigr" | "Bigr" | "biggr" | "Biggr" => Node::Sym(self.delimiter(), Class::Close),
            "bigm" | "Bigm" | "biggm" | "Biggm" => Node::Sym(self.delimiter(), Class::Rel),
            "not" => negate(self.argument()),
            "overline" => Node::Wide(Wide::Overline, self.argument()),
            "underline" => Node::Wide(Wide::Underline, self.argument()),
            "overrightarrow" => Node::Wide(Wide::RightArrow, self.argument()),
            "overleftarrow" => Node::Wide(Wide::LeftArrow, self.argument()),
            "overleftrightarrow" => Node::Wide(Wide::BothArrow, self.argument()),
            "overbrace" | "overbracket" => Node::Brace {
                over: true,
                body: self.argument(),
            },
            "underbrace" | "underbracket" => Node::Brace {
                over: false,
                body: self.argument(),
            },
            "overset" | "stackrel" => {
                let over = self.argument();
                let base = self.argument();
                Node::Stack {
                    base,
                    over: Some(over),
                    under: None,
                    arrow: None,
                }
            }
            "underset" => {
                let under = self.argument();
                let base = self.argument();
                Node::Stack {
                    base,
                    over: None,
                    under: Some(under),
                    arrow: None,
                }
            }
            name if stretchy_arrow(name).is_some() => {
                let under = self.optional();
                let over = self.argument();
                Node::Stack {
                    base: Vec::new(),
                    over: Some(over),
                    under,
                    arrow: stretchy_arrow(name),
                }
            }
            "boxed" => Node::Boxed(self.argument()),
            "fbox" | "framebox" => Node::Boxed(vec![Node::Text(self.text())]),
            "phantom" | "hphantom" => Node::Phantom(self.argument()),
            "smash" => {
                self.optional();
                Node::Group(self.argument())
            }
            "llap" | "rlap" | "clap" | "mathllap" | "mathrlap" | "mathclap" => {
                Node::Group(self.argument())
            }
            "cancel" | "bcancel" | "xcancel" | "sout" => Node::Strike(self.argument()),
            "textcolor" | "colorbox" | "href" | "class" | "cssId" | "style" => {
                self.raw_group();
                Node::Group(self.argument())
            }
            "fcolorbox" => {
                self.raw_group();
                self.raw_group();
                Node::Group(self.argument())
            }
            "tag" => {
                let bare = self.eat('*');
                let tag = self.text();
                self.tag = Some(if bare { tag } else { format!("({tag})") });
                return None;
            }
            "eqref" => Node::Text(format!("({})", self.raw_group())),
            "ref" => Node::Text(self.raw_group().to_string()),
            "pmod" | "pod" => {
                let body = self.argument();
                let mut nodes = vec![Node::Space(1), Node::Sym("(".into(), Class::Open)];
                if name == "pmod" {
                    nodes.push(Node::Op {
                        name: "mod".into(),
                        limits: false,
                    });
                }
                nodes.extend(body);
                nodes.push(Node::Sym(")".into(), Class::Close));
                Node::Group(nodes)
            }
            "bmod" => Node::Sym("mod".into(), Class::Bin),
            "mod" => Node::Classed(
                Class::Op,
                vec![
                    Node::Space(2),
                    Node::Op {
                        name: "mod".into(),
                        limits: false,
                    },
                ],
            ),
            "substack" | "displaylines" => {
                let raw = self.raw_group();
                Node::Array(Grid {
                    rows: Parser::new(raw, self.depth + 1).rows(),
                    columns: Columns::All(Align::Center),
                    open: "",
                    close: "",
                })
            }
            "begin" => return self.environment(),
            "hspace" | "mspace" => {
                self.eat('*');
                self.raw_group();
                Node::Space(1)
            }
            "kern" | "mkern" | "hskip" | "mskip" => {
                self.skip_dimension();
                Node::Space(1)
            }
            "color" | "label" | "end" | "vphantom" | "cline" => {
                self.raw_group();
                return None;
            }
            // What changes how MathJax sizes or numbers a formula, rather
            // than what it says.
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" | "nonumber"
            | "notag" | "allowbreak" | "nobreak" | "hline" | "hdashline" | "mathstrut"
            | "strut" | "\\" | "cr" | "newline" | "limits" | "nolimits" | "over" | "atop"
            | "choose" => return None,
            "" => Node::Sym("\\".into(), Class::Ord),
            // Unknown, or a macro from someone's MathJax preamble: shown as
            // typed, arguments and all, so nothing in the note quietly
            // disappears.
            name => {
                let mut typed = format!("\\{name}");
                loop {
                    self.skip_space();
                    if self.peek() != Some('{') {
                        break;
                    }
                    let argument = self.raw_group();
                    typed.push_str(&format!("{{{argument}}}"));
                }
                // TeX needs a space between a command and a letter after it,
                // and so does what is shown, or `\foo x` reads as `\foox`.
                if self.peek().is_some_and(char::is_alphanumeric) {
                    typed.push(NBSP);
                }
                Node::Sym(typed, Class::Ord)
            }
        })
    }

    /// Skips the dimension after `\kern`: `3mu`, `-1em`, `{2pt}`.
    fn skip_dimension(&mut self) {
        self.skip_space();
        if self.peek() == Some('{') {
            self.raw_group();
            return;
        }
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+'))
        {
            self.bump();
        }
        let unit = self
            .rest()
            .bytes()
            .take_while(u8::is_ascii_alphabetic)
            .count()
            .min(2);
        self.at += unit;
    }
}

/// Hangs a script on the atom before it, or on nothing if there isn't one.
fn attach(nodes: &mut Vec<Node>, is_sup: bool, script: Vec<Node>) {
    let empty = || Box::new(Node::Sym(String::new(), Class::Ord));
    let (base, mut sub, mut sup) = match nodes.pop() {
        Some(Node::Scripts { base, sub, sup }) => (base, sub, sup),
        Some(node @ (Node::Space(_) | Node::Break)) => {
            nodes.push(node);
            (empty(), None, None)
        }
        Some(node) => (Box::new(node), None, None),
        None => (empty(), None, None),
    };
    // A second superscript is an error in TeX. Keeping both is kinder than
    // dropping one.
    let slot = if is_sup { &mut sup } else { &mut sub };
    slot.get_or_insert_with(Vec::new).extend(script);
    nodes.push(Node::Scripts { base, sub, sup });
}

fn prime(count: usize) -> Node {
    let glyph = match count {
        1 => "′".to_string(),
        2 => "″".to_string(),
        3 => "‴".to_string(),
        4 => "⁗".to_string(),
        count => "′".repeat(count),
    };
    Node::Sym(glyph, Class::Ord)
}

/// `\limits` and `\nolimits`, which say where the operator before them puts
/// its scripts.
fn set_limits(nodes: &mut [Node], limits: bool) {
    let op = match nodes.last_mut() {
        Some(Node::Scripts { base, .. }) => base.as_mut(),
        Some(node) => node,
        None => return,
    };
    if let Node::Op {
        limits: current, ..
    } = op
    {
        *current = limits;
    }
}

/// `\not=` and `\not\in`: the slashed form where Unicode has one, and a
/// stroke through the symbol where it doesn't.
fn negate(nodes: Vec<Node>) -> Node {
    let mut nodes = nodes;
    match nodes.as_mut_slice() {
        [Node::Sym(glyph, _)] => {
            let negated = match glyph.as_str() {
                "=" => "≠",
                "<" => "≮",
                ">" => "≯",
                "≤" => "≰",
                "≥" => "≱",
                "∈" => "∉",
                "∋" => "∌",
                "⊂" => "⊄",
                "⊃" => "⊅",
                "⊆" => "⊈",
                "⊇" => "⊉",
                "∼" => "≁",
                "≈" => "≉",
                "≡" => "≢",
                "≅" => "≇",
                "≃" => "≄",
                "∣" => "∤",
                "∥" => "∦",
                "⊢" => "⊬",
                "⊨" => "⊭",
                "∃" => "∄",
                "→" => "↛",
                "←" => "↚",
                "↔" => "↮",
                "⇒" => "⇏",
                "⇐" => "⇍",
                "⇔" => "⇎",
                "≺" => "⊀",
                "≻" => "⊁",
                _ => "",
            };
            if negated.is_empty() {
                glyph.push('\u{338}');
            } else {
                *glyph = negated.to_string();
            }
            nodes.pop().unwrap_or(Node::Space(0))
        }
        _ => Node::Strike(nodes),
    }
}

fn char_node(ch: char) -> Node {
    let class = match ch {
        // TeX sets the hyphen in maths as a true minus sign.
        '-' => return Node::Sym("−".into(), Class::Bin),
        '*' => return Node::Sym("∗".into(), Class::Bin),
        '+' => Class::Bin,
        '=' | '<' | '>' | ':' => Class::Rel,
        ',' | ';' => Class::Punct,
        '(' | '[' => Class::Open,
        ')' | ']' | '!' | '?' => Class::Close,
        '∑' | '∏' | '∐' | '⋃' | '⋂' => {
            return Node::Op {
                name: ch.to_string(),
                limits: true,
            };
        }
        '∫' | '∬' | '∭' | '∮' => {
            return Node::Op {
                name: ch.to_string(),
                limits: false,
            };
        }
        ch => unicode_class(ch),
    };
    Node::Sym(ch.to_string(), class)
}

/// The class of a symbol typed directly rather than as a command.
fn unicode_class(ch: char) -> Class {
    if "≤≥≠≈≡∼≃≅∝∈∉∋⊂⊃⊆⊇→←↔⇒⇐⇔↦⟶⟵⟹⟸⟺∣∥⊥⊢⊨≺≻≪≫≲≳≔".contains(ch)
    {
        Class::Rel
    } else if "±∓×÷·⋅∙∘∗⋆⊕⊖⊗⊘⊙∩∪∧∨∖⊓⊔⊎".contains(ch) {
        Class::Bin
    } else if "⟨⌈⌊⟦".contains(ch) {
        Class::Open
    } else if "⟩⌉⌋⟧".contains(ch) {
        Class::Close
    } else {
        Class::Ord
    }
}

fn delimiter_class(delimiter: &str) -> Class {
    match delimiter {
        "(" | "[" | "{" | "⟨" | "⌈" | "⌊" | "⟦" => Class::Open,
        ")" | "]" | "}" | "⟩" | "⌉" | "⌋" | "⟧" => Class::Close,
        _ => Class::Ord,
    }
}

/// An `array` column spec such as `{lc|r}`. Rules between columns are
/// dropped, and anything that isn't an alignment reads as `l`.
fn spec(raw: &str) -> Vec<Align> {
    let mut columns = Vec::new();
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth > 0 => {}
            'l' | 'p' | 'm' | 'b' => columns.push(Align::Left),
            'c' => columns.push(Align::Center),
            'r' => columns.push(Align::Right),
            _ => {}
        }
    }
    columns
}

/// `\text` content: its words and their spaces, its escapes resolved, and any
/// `$…$` inside it set as maths.
fn text_content(raw: &str, depth: usize) -> String {
    let mut out = String::new();
    let mut chars = raw.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => match chars.peek().map(|&(_, next)| next) {
                Some(next) if next.is_ascii_alphabetic() => {
                    let mut name = String::new();
                    while let Some(&(_, next)) = chars.peek() {
                        if !next.is_ascii_alphabetic() {
                            break;
                        }
                        name.push(next);
                        chars.next();
                    }
                    // A text command's argument is kept; only its name goes.
                    match name.as_str() {
                        "textbackslash" => out.push('\\'),
                        "ldots" | "dots" | "textellipsis" => out.push('…'),
                        "quad" => out.push_str("  "),
                        "qquad" => out.push_str("    "),
                        "space" => out.push(' '),
                        _ => {}
                    }
                }
                Some(next) => {
                    chars.next();
                    out.push(if matches!(next, ',' | ';' | ':' | '!') {
                        ' '
                    } else {
                        next
                    });
                }
                None => out.push('\\'),
            },
            '{' | '}' => {}
            '~' => out.push(' '),
            '$' => {
                let rest = &raw[at + 1..];
                let end = rest.find('$').unwrap_or(rest.len());
                let tex = &rest[..end];
                if depth < MAX_DEPTH {
                    let nodes = Parser::new(tex, depth + 1).top();
                    out.push_str(&one_line(&layout(&nodes, Ctx::INLINE.nested(), None)));
                } else {
                    out.push_str(tex);
                }
                // Past the maths and its closing dollar.
                while chars.peek().is_some_and(|&(next, _)| next <= at + 1 + end) {
                    chars.next();
                }
            }
            ch if ch.is_whitespace() => {
                if !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            ch => out.push(ch),
        }
    }
    out
}

/// Plain text for an operator's name, as in `\operatorname{arg\,max}`.
fn plain(nodes: &[Node]) -> String {
    one_line(&layout(nodes, Ctx::SCRIPT, None)).replace(NBSP, " ")
}

// ---------------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------------

fn symbol(name: &str) -> Option<(&'static str, Class)> {
    use Class::{Bin, Close, Inner, Open, Ord, Punct, Rel};
    Some(match name {
        // Greek, as TeX names it: `\epsilon` and `\phi` are the lunate and
        // closed forms, the `var` ones the curly forms most handwriting uses.
        "alpha" => ("α", Ord),
        "beta" => ("β", Ord),
        "gamma" => ("γ", Ord),
        "delta" => ("δ", Ord),
        "epsilon" => ("ϵ", Ord),
        "varepsilon" => ("ε", Ord),
        "zeta" => ("ζ", Ord),
        "eta" => ("η", Ord),
        "theta" => ("θ", Ord),
        "vartheta" => ("ϑ", Ord),
        "iota" => ("ι", Ord),
        "kappa" => ("κ", Ord),
        "varkappa" => ("ϰ", Ord),
        "lambda" => ("λ", Ord),
        "mu" => ("μ", Ord),
        "nu" => ("ν", Ord),
        "xi" => ("ξ", Ord),
        "omicron" => ("ο", Ord),
        "pi" => ("π", Ord),
        "varpi" => ("ϖ", Ord),
        "rho" => ("ρ", Ord),
        "varrho" => ("ϱ", Ord),
        "sigma" => ("σ", Ord),
        "varsigma" => ("ς", Ord),
        "tau" => ("τ", Ord),
        "upsilon" => ("υ", Ord),
        "phi" => ("ϕ", Ord),
        "varphi" => ("φ", Ord),
        "chi" => ("χ", Ord),
        "psi" => ("ψ", Ord),
        "omega" => ("ω", Ord),
        "digamma" => ("ϝ", Ord),
        "Alpha" => ("Α", Ord),
        "Beta" => ("Β", Ord),
        "Gamma" | "varGamma" => ("Γ", Ord),
        "Delta" | "varDelta" => ("Δ", Ord),
        "Epsilon" => ("Ε", Ord),
        "Zeta" => ("Ζ", Ord),
        "Eta" => ("Η", Ord),
        "Theta" | "varTheta" => ("Θ", Ord),
        "Iota" => ("Ι", Ord),
        "Kappa" => ("Κ", Ord),
        "Lambda" | "varLambda" => ("Λ", Ord),
        "Mu" => ("Μ", Ord),
        "Nu" => ("Ν", Ord),
        "Xi" | "varXi" => ("Ξ", Ord),
        "Omicron" => ("Ο", Ord),
        "Pi" | "varPi" => ("Π", Ord),
        "Rho" => ("Ρ", Ord),
        "Sigma" | "varSigma" => ("Σ", Ord),
        "Tau" => ("Τ", Ord),
        "Upsilon" | "varUpsilon" => ("Υ", Ord),
        "Phi" | "varPhi" => ("Φ", Ord),
        "Chi" => ("Χ", Ord),
        "Psi" | "varPsi" => ("Ψ", Ord),
        "Omega" | "varOmega" => ("Ω", Ord),

        // Letter-like symbols.
        "infty" => ("∞", Ord),
        "partial" => ("∂", Ord),
        "nabla" => ("∇", Ord),
        "hbar" | "hslash" => ("ℏ", Ord),
        "ell" => ("ℓ", Ord),
        "wp" => ("℘", Ord),
        "Re" => ("ℜ", Ord),
        "Im" => ("ℑ", Ord),
        "aleph" => ("ℵ", Ord),
        "beth" => ("ℶ", Ord),
        "gimel" => ("ℷ", Ord),
        "imath" => ("ı", Ord),
        "jmath" => ("ȷ", Ord),
        "emptyset" | "varnothing" => ("∅", Ord),
        "forall" => ("∀", Ord),
        "exists" => ("∃", Ord),
        "nexists" => ("∄", Ord),
        "complement" => ("∁", Ord),
        "neg" | "lnot" => ("¬", Ord),
        "top" => ("⊤", Ord),
        "bot" => ("⊥", Ord),
        "angle" => ("∠", Ord),
        "measuredangle" => ("∡", Ord),
        "triangle" => ("△", Ord),
        "square" | "Box" => ("□", Ord),
        "blacksquare" | "qed" => ("■", Ord),
        "Diamond" => ("◇", Ord),
        "lozenge" => ("◊", Ord),
        "checkmark" => ("✓", Ord),
        "star" => ("⋆", Bin),
        "bigstar" => ("★", Ord),
        "clubsuit" => ("♣", Ord),
        "diamondsuit" => ("♢", Ord),
        "heartsuit" => ("♡", Ord),
        "spadesuit" => ("♠", Ord),
        "flat" => ("♭", Ord),
        "natural" => ("♮", Ord),
        "sharp" => ("♯", Ord),
        "prime" => ("′", Ord),
        "degree" => ("°", Ord),
        "surd" => ("√", Ord),
        "dagger" | "dag" => ("†", Bin),
        "ddagger" | "ddag" => ("‡", Bin),
        "therefore" => ("∴", Rel),
        "because" => ("∵", Rel),
        "S" => ("§", Ord),
        "P" => ("¶", Ord),
        "copyright" => ("©", Ord),
        "pounds" => ("£", Ord),
        "euro" => ("€", Ord),

        // Binary operators.
        "pm" => ("±", Bin),
        "mp" => ("∓", Bin),
        "times" => ("×", Bin),
        "div" => ("÷", Bin),
        "cdot" | "centerdot" => ("·", Bin),
        "ast" => ("∗", Bin),
        "circ" => ("∘", Bin),
        "bullet" => ("∙", Bin),
        "oplus" => ("⊕", Bin),
        "ominus" => ("⊖", Bin),
        "otimes" => ("⊗", Bin),
        "oslash" => ("⊘", Bin),
        "odot" => ("⊙", Bin),
        "cap" => ("∩", Bin),
        "cup" => ("∪", Bin),
        "sqcap" => ("⊓", Bin),
        "sqcup" => ("⊔", Bin),
        "uplus" => ("⊎", Bin),
        "wedge" | "land" => ("∧", Bin),
        "vee" | "lor" => ("∨", Bin),
        "setminus" | "smallsetminus" => ("∖", Bin),
        "wr" => ("≀", Bin),
        "amalg" => ("⨿", Bin),
        "diamond" => ("⋄", Bin),
        "bigcirc" => ("◯", Bin),
        "triangleleft" => ("◁", Bin),
        "triangleright" => ("▷", Bin),
        "lhd" => ("⊲", Bin),
        "rhd" => ("⊳", Bin),
        "unlhd" => ("⊴", Bin),
        "unrhd" => ("⊵", Bin),
        "ltimes" => ("⋉", Bin),
        "rtimes" => ("⋊", Bin),
        "boxplus" => ("⊞", Bin),
        "boxminus" => ("⊟", Bin),
        "boxtimes" => ("⊠", Bin),
        "boxdot" => ("⊡", Bin),
        "dotplus" => ("∔", Bin),
        "intercal" => ("⊺", Bin),
        "barwedge" => ("⊼", Bin),
        "veebar" => ("⊻", Bin),

        // Relations.
        "lt" => ("<", Rel),
        "gt" => (">", Rel),
        "le" | "leq" => ("≤", Rel),
        "ge" | "geq" => ("≥", Rel),
        "leqq" => ("≦", Rel),
        "geqq" => ("≧", Rel),
        "leqslant" => ("⩽", Rel),
        "geqslant" => ("⩾", Rel),
        "ne" | "neq" => ("≠", Rel),
        "ll" => ("≪", Rel),
        "gg" => ("≫", Rel),
        "lll" => ("⋘", Rel),
        "ggg" => ("⋙", Rel),
        "lesssim" => ("≲", Rel),
        "gtrsim" => ("≳", Rel),
        "lessgtr" => ("≶", Rel),
        "gtrless" => ("≷", Rel),
        "nless" => ("≮", Rel),
        "ngtr" => ("≯", Rel),
        "nleq" => ("≰", Rel),
        "ngeq" => ("≱", Rel),
        "approx" => ("≈", Rel),
        "approxeq" => ("≊", Rel),
        "sim" => ("∼", Rel),
        "nsim" => ("≁", Rel),
        "simeq" => ("≃", Rel),
        "cong" => ("≅", Rel),
        "ncong" => ("≇", Rel),
        "equiv" => ("≡", Rel),
        "propto" | "varpropto" => ("∝", Rel),
        "doteq" => ("≐", Rel),
        "triangleq" => ("≜", Rel),
        "coloneqq" | "coloneq" | "Coloneqq" => ("≔", Rel),
        "eqqcolon" | "eqcolon" => ("≕", Rel),
        "asymp" => ("≍", Rel),
        "bowtie" => ("⋈", Rel),
        "smile" => ("⌣", Rel),
        "frown" => ("⌢", Rel),
        "prec" => ("≺", Rel),
        "succ" => ("≻", Rel),
        "preceq" => ("⪯", Rel),
        "succeq" => ("⪰", Rel),
        "preccurlyeq" => ("≼", Rel),
        "succcurlyeq" => ("≽", Rel),
        "in" => ("∈", Rel),
        "notin" => ("∉", Rel),
        "ni" | "owns" => ("∋", Rel),
        "subset" => ("⊂", Rel),
        "supset" => ("⊃", Rel),
        "subseteq" => ("⊆", Rel),
        "supseteq" => ("⊇", Rel),
        "subsetneq" => ("⊊", Rel),
        "supsetneq" => ("⊋", Rel),
        "nsubseteq" => ("⊈", Rel),
        "nsupseteq" => ("⊉", Rel),
        "sqsubset" => ("⊏", Rel),
        "sqsupset" => ("⊐", Rel),
        "sqsubseteq" => ("⊑", Rel),
        "sqsupseteq" => ("⊒", Rel),
        "vdash" => ("⊢", Rel),
        "dashv" => ("⊣", Rel),
        "models" | "vDash" => ("⊨", Rel),
        "Vdash" => ("⊩", Rel),
        "nvdash" => ("⊬", Rel),
        "nvDash" => ("⊭", Rel),
        "mid" => ("∣", Rel),
        "nmid" => ("∤", Rel),
        "parallel" => ("∥", Rel),
        "nparallel" => ("∦", Rel),
        "perp" => ("⊥", Rel),

        // Arrows, all relations as far as spacing goes.
        "to" | "rightarrow" => ("→", Rel),
        "gets" | "leftarrow" => ("←", Rel),
        "leftrightarrow" => ("↔", Rel),
        "Rightarrow" => ("⇒", Rel),
        "Leftarrow" => ("⇐", Rel),
        "Leftrightarrow" => ("⇔", Rel),
        "implies" | "Longrightarrow" => ("⟹", Rel),
        "impliedby" | "Longleftarrow" => ("⟸", Rel),
        "iff" | "Longleftrightarrow" => ("⟺", Rel),
        "longrightarrow" => ("⟶", Rel),
        "longleftarrow" => ("⟵", Rel),
        "longleftrightarrow" => ("⟷", Rel),
        "mapsto" => ("↦", Rel),
        "longmapsto" => ("⟼", Rel),
        "uparrow" => ("↑", Rel),
        "downarrow" => ("↓", Rel),
        "updownarrow" => ("↕", Rel),
        "Uparrow" => ("⇑", Rel),
        "Downarrow" => ("⇓", Rel),
        "Updownarrow" => ("⇕", Rel),
        "nearrow" => ("↗", Rel),
        "searrow" => ("↘", Rel),
        "swarrow" => ("↙", Rel),
        "nwarrow" => ("↖", Rel),
        "hookrightarrow" => ("↪", Rel),
        "hookleftarrow" => ("↩", Rel),
        "leftharpoonup" => ("↼", Rel),
        "leftharpoondown" => ("↽", Rel),
        "rightharpoonup" => ("⇀", Rel),
        "rightharpoondown" => ("⇁", Rel),
        "rightleftharpoons" => ("⇌", Rel),
        "leftrightharpoons" => ("⇋", Rel),
        "leadsto" | "rightsquigarrow" => ("⇝", Rel),
        "twoheadrightarrow" => ("↠", Rel),
        "twoheadleftarrow" => ("↞", Rel),
        "rightarrowtail" => ("↣", Rel),
        "leftarrowtail" => ("↢", Rel),
        "curvearrowright" => ("↷", Rel),
        "curvearrowleft" => ("↶", Rel),
        "circlearrowright" => ("↻", Rel),
        "circlearrowleft" => ("↺", Rel),
        "upharpoonright" => ("↾", Rel),
        "downharpoonright" => ("⇂", Rel),
        "nrightarrow" => ("↛", Rel),
        "nleftarrow" => ("↚", Rel),
        "nleftrightarrow" => ("↮", Rel),
        "nRightarrow" => ("⇏", Rel),
        "nLeftarrow" => ("⇍", Rel),
        "nLeftrightarrow" => ("⇎", Rel),

        // Delimiters.
        "{" | "lbrace" => ("{", Open),
        "}" | "rbrace" => ("}", Close),
        "lbrack" => ("[", Open),
        "rbrack" => ("]", Close),
        "langle" => ("⟨", Open),
        "rangle" => ("⟩", Close),
        "lceil" => ("⌈", Open),
        "rceil" => ("⌉", Close),
        "lfloor" => ("⌊", Open),
        "rfloor" => ("⌋", Close),
        "lgroup" => ("⟮", Open),
        "rgroup" => ("⟯", Close),
        "llbracket" => ("⟦", Open),
        "rrbracket" => ("⟧", Close),
        "ulcorner" => ("⌜", Open),
        "urcorner" => ("⌝", Close),
        "llcorner" => ("⌞", Open),
        "lrcorner" => ("⌟", Close),
        "lvert" => ("|", Open),
        "rvert" => ("|", Close),
        "lVert" => ("‖", Open),
        "rVert" => ("‖", Close),
        "vert" => ("|", Ord),
        "|" | "Vert" => ("‖", Ord),
        "backslash" => ("\\", Ord),

        // Dots and punctuation.
        "ldots" | "dots" | "dotso" | "dotsc" | "mathellipsis" => ("…", Inner),
        "cdots" | "dotsb" | "dotsm" | "dotsi" => ("⋯", Inner),
        "vdots" => ("⋮", Ord),
        "ddots" => ("⋱", Inner),
        "colon" => (":", Punct),
        "ldotp" => (".", Punct),
        "cdotp" => ("·", Punct),

        // Characters TeX needs escaping.
        "#" => ("#", Ord),
        "$" => ("$", Ord),
        "%" => ("%", Ord),
        "&" => ("&", Ord),
        "_" => ("_", Ord),
        _ => return None,
    })
}

/// A large operator, and whether display maths sets its limits above and
/// below. Integrals keep theirs beside, as TeX does.
fn big_operator(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "sum" => ("∑", true),
        "prod" => ("∏", true),
        "coprod" => ("∐", true),
        "bigcup" => ("⋃", true),
        "bigcap" => ("⋂", true),
        "bigvee" => ("⋁", true),
        "bigwedge" => ("⋀", true),
        // The larger forms of these are missing from many terminal fonts,
        // and are drawn a single cell wide either way.
        "bigoplus" => ("⊕", true),
        "bigotimes" => ("⊗", true),
        "bigodot" => ("⊙", true),
        "biguplus" => ("⊎", true),
        "bigsqcup" => ("⊔", true),
        "int" | "smallint" => ("∫", false),
        "intop" => ("∫", true),
        "iint" => ("∬", false),
        "iiint" => ("∭", false),
        "iiiint" => ("⨌", false),
        "oint" => ("∮", false),
        "oiint" => ("∯", false),
        "oiiint" => ("∰", false),
        _ => return None,
    })
}

/// An operator set as an upright word, and whether it takes limits.
fn named_operator(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "lim" => ("lim", true),
        "limsup" => ("lim sup", true),
        "liminf" => ("lim inf", true),
        "max" => ("max", true),
        "min" => ("min", true),
        "sup" => ("sup", true),
        "inf" => ("inf", true),
        "det" => ("det", true),
        "gcd" => ("gcd", true),
        "Pr" => ("Pr", true),
        "injlim" => ("inj lim", true),
        "projlim" => ("proj lim", true),
        "sin" => ("sin", false),
        "cos" => ("cos", false),
        "tan" => ("tan", false),
        "cot" => ("cot", false),
        "sec" => ("sec", false),
        "csc" => ("csc", false),
        "arcsin" => ("arcsin", false),
        "arccos" => ("arccos", false),
        "arctan" => ("arctan", false),
        "sinh" => ("sinh", false),
        "cosh" => ("cosh", false),
        "tanh" => ("tanh", false),
        "coth" => ("coth", false),
        "log" => ("log", false),
        "ln" => ("ln", false),
        "lg" => ("lg", false),
        "exp" => ("exp", false),
        "arg" => ("arg", false),
        "deg" => ("deg", false),
        "dim" => ("dim", false),
        "hom" => ("hom", false),
        "ker" => ("ker", false),
        _ => return None,
    })
}

/// Columns for an explicit space. A thin space is a whole column here: a
/// terminal has nothing narrower, and `f(x)\,dx` run together reads worse.
fn space(name: &str) -> Option<usize> {
    Some(match name {
        // `\` before a line break is a control space, as it is before a space.
        "," | ":" | ">" | ";" | " " | "\n" | "\r" | "\t" | "thinspace" | "medspace"
        | "thickspace" | "enspace" | "space" | "nobreakspace" => 1,
        "!" | "negthinspace" | "negmedspace" | "negthickspace" => 0,
        "quad" => 2,
        "qquad" => 4,
        _ => return None,
    })
}

/// The combining mark for an accent, and whether a longer body takes it on
/// its last letter only.
fn accent(name: &str) -> Option<(char, bool)> {
    Some(match name {
        "hat" | "widehat" => ('\u{302}', false),
        "check" | "widecheck" => ('\u{30c}', false),
        "tilde" | "widetilde" => ('\u{303}', false),
        "acute" => ('\u{301}', false),
        "grave" => ('\u{300}', false),
        "breve" => ('\u{306}', false),
        "bar" => (MACRON, false),
        "dot" => ('\u{307}', false),
        "ddot" => ('\u{308}', false),
        "dddot" => ('\u{20db}', false),
        "ddddot" => ('\u{20dc}', false),
        "mathring" => ('\u{30a}', false),
        "vec" => ('\u{20d7}', true),
        _ => return None,
    })
}

const MACRON: char = '\u{304}';
/// The most characters a combining mark is put on, one per character.
const MAX_MARKED: usize = 64;
const OVERLINE: char = '\u{305}';

fn stretchy_arrow(name: &str) -> Option<Arrow> {
    let (glyph, tail, fill, head) = match name {
        "xrightarrow" => ("→", "", "─", "→"),
        "xleftarrow" => ("←", "←", "─", ""),
        "xleftrightarrow" => ("↔", "←", "─", "→"),
        "xRightarrow" => ("⇒", "", "═", "⇒"),
        "xLeftarrow" => ("⇐", "⇐", "═", ""),
        "xLeftrightarrow" => ("⇔", "⇐", "═", "⇒"),
        "xmapsto" => ("↦", "├", "─", "→"),
        "xlongequal" => ("=", "", "═", ""),
        _ => return None,
    };
    Some(Arrow {
        glyph,
        tail,
        fill,
        head,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Font {
    Plain,
    Bold,
    DoubleStruck,
    Script,
    Fraktur,
    SansSerif,
}

fn font(name: &str) -> Option<Font> {
    Some(match name {
        "mathbb" | "Bbb" | "mathbbm" => Font::DoubleStruck,
        "mathcal" | "mathscr" | "cal" | "scr" => Font::Script,
        "mathfrak" | "frak" => Font::Fraktur,
        "mathbf" | "boldsymbol" | "bm" | "pmb" | "bf" => Font::Bold,
        "mathsf" | "sf" => Font::SansSerif,
        // A terminal draws one face, upright and monospaced, so these change
        // nothing there.
        "mathrm" | "rm" | "mathit" | "it" | "mathnormal" | "mathtt" | "tt" | "mathup" => {
            Font::Plain
        }
        _ => return None,
    })
}

/// A letter or digit in one of Unicode's mathematical alphabets.
///
/// The letters every font has, like `ℝ` and `ℒ`, come from the Letterlike
/// Symbols block. Those are also the gaps in the mathematical alphabets, which
/// were left for them, so they have to be filled in by hand.
fn styled(ch: char, font: Font) -> char {
    let offset = |base: u32, from: char| char::from_u32(base + (ch as u32 - from as u32));
    let mapped = match (font, ch) {
        (Font::DoubleStruck, 'C') => Some('ℂ'),
        (Font::DoubleStruck, 'H') => Some('ℍ'),
        (Font::DoubleStruck, 'N') => Some('ℕ'),
        (Font::DoubleStruck, 'P') => Some('ℙ'),
        (Font::DoubleStruck, 'Q') => Some('ℚ'),
        (Font::DoubleStruck, 'R') => Some('ℝ'),
        (Font::DoubleStruck, 'Z') => Some('ℤ'),
        (Font::DoubleStruck, 'A'..='Z') => offset(0x1D538, 'A'),
        (Font::DoubleStruck, 'a'..='z') => offset(0x1D552, 'a'),
        (Font::DoubleStruck, '0'..='9') => offset(0x1D7D8, '0'),
        (Font::Script, 'B') => Some('ℬ'),
        (Font::Script, 'E') => Some('ℰ'),
        (Font::Script, 'F') => Some('ℱ'),
        (Font::Script, 'H') => Some('ℋ'),
        (Font::Script, 'I') => Some('ℐ'),
        (Font::Script, 'L') => Some('ℒ'),
        (Font::Script, 'M') => Some('ℳ'),
        (Font::Script, 'R') => Some('ℛ'),
        (Font::Script, 'e') => Some('ℯ'),
        (Font::Script, 'g') => Some('ℊ'),
        (Font::Script, 'o') => Some('ℴ'),
        (Font::Script, 'A'..='Z') => offset(0x1D49C, 'A'),
        (Font::Script, 'a'..='z') => offset(0x1D4B6, 'a'),
        (Font::Fraktur, 'C') => Some('ℭ'),
        (Font::Fraktur, 'H') => Some('ℌ'),
        (Font::Fraktur, 'I') => Some('ℑ'),
        (Font::Fraktur, 'R') => Some('ℜ'),
        (Font::Fraktur, 'Z') => Some('ℨ'),
        (Font::Fraktur, 'A'..='Z') => offset(0x1D504, 'A'),
        (Font::Fraktur, 'a'..='z') => offset(0x1D51E, 'a'),
        (Font::Bold, 'A'..='Z') => offset(0x1D400, 'A'),
        (Font::Bold, 'a'..='z') => offset(0x1D41A, 'a'),
        (Font::Bold, '0'..='9') => offset(0x1D7CE, '0'),
        (Font::SansSerif, 'A'..='Z') => offset(0x1D5A0, 'A'),
        (Font::SansSerif, 'a'..='z') => offset(0x1D5BA, 'a'),
        (Font::SansSerif, '0'..='9') => offset(0x1D7E2, '0'),
        _ => None,
    };
    mapped.unwrap_or(ch)
}

fn restyle(nodes: Vec<Node>, font: Font) -> Vec<Node> {
    nodes
        .into_iter()
        .map(|node| restyle_node(node, font))
        .collect()
}

fn restyle_node(node: Node, font: Font) -> Node {
    let letters = |text: String| -> String { text.chars().map(|ch| styled(ch, font)).collect() };
    match node {
        Node::Sym(text, class) => Node::Sym(letters(text), class),
        Node::Text(text) => Node::Text(letters(text)),
        Node::Group(nodes) => Node::Group(restyle(nodes, font)),
        Node::Scripts { base, sub, sup } => Node::Scripts {
            base: Box::new(restyle_node(*base, font)),
            sub: sub.map(|nodes| restyle(nodes, font)),
            sup: sup.map(|nodes| restyle(nodes, font)),
        },
        Node::Frac { num, den, rule } => Node::Frac {
            num: restyle(num, font),
            den: restyle(den, font),
            rule,
        },
        Node::Accent { mark, last, body } => Node::Accent {
            mark,
            last,
            body: restyle(body, font),
        },
        // Operators keep their upright names, and structure is left alone.
        node => node,
    }
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// A block of typeset rows, lined up with its neighbours on its baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Boxed {
    /// Every row exactly `width` columns wide.
    rows: Vec<String>,
    width: usize,
    baseline: usize,
}

impl Boxed {
    fn text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            width: text.width(),
            rows: vec![text],
            baseline: 0,
        }
    }

    fn blank(width: usize, height: usize, baseline: usize) -> Self {
        Self {
            rows: vec![" ".repeat(width); height.max(1)],
            width,
            baseline,
        }
    }

    fn height(&self) -> usize {
        self.rows.len()
    }

    /// Rows below the baseline.
    fn below(&self) -> usize {
        self.height() - 1 - self.baseline
    }
}

#[derive(Debug, Clone, Copy)]
struct Ctx {
    /// Room to stack: fractions over a rule, limits above and below. Only
    /// display maths has it.
    tall: bool,
    /// Inside a script, where TeX leaves out most of its spacing.
    script: bool,
    /// The top of an inline formula, where a line may break.
    breakable: bool,
}

impl Ctx {
    const INLINE: Self = Self {
        tall: false,
        script: false,
        breakable: true,
    };
    const DISPLAY: Self = Self {
        tall: true,
        script: false,
        breakable: false,
    };
    const SCRIPT: Self = Self {
        tall: false,
        script: true,
        breakable: false,
    };

    fn nested(self) -> Self {
        Self {
            breakable: false,
            ..self
        }
    }

    /// The space between two atoms. In a line of prose it is one a line
    /// can't break at.
    fn space(self) -> char {
        if self.tall { ' ' } else { NBSP }
    }
}

enum Item {
    /// An atom as set, its class, and whether it is a fraction with a rule.
    Atom(Boxed, Class, bool),
    Space(usize),
    Middle(String),
}

/// Sets a list of atoms side by side, spaced as TeX spaces them.
///
/// `lead` is the class of an atom that would sit before the list: the left
/// half of an `aligned` line, before the half that begins at its `&`.
fn layout(nodes: &[Node], ctx: Ctx, lead: Option<Class>) -> Boxed {
    let inner = ctx.nested();
    let mut items: Vec<Item> = nodes
        .iter()
        .map(|node| match node {
            Node::Space(width) => Item::Space(*width),
            // Only display maths breaks lines, and it splits on them first.
            Node::Break => Item::Space(2),
            Node::Middle(delimiter) => Item::Middle(delimiter.clone()),
            node => {
                let boxed = layout_node(node, inner);
                // On one line only a fraction written with a slash counts;
                // `½` stands on its own.
                let fraction = matches!(node, Node::Frac { rule: true, .. })
                    && (ctx.tall || boxed.rows[0].contains('/'));
                Item::Atom(boxed, class_of(node), fraction)
            }
        })
        .collect();

    // `\middle|` grows to the height of whatever it sits between, and is
    // spaced as a relation, which is also what the minus in `x \middle| -1`
    // has to see before it to be read as a sign.
    let above = items
        .iter()
        .filter_map(|item| match item {
            Item::Atom(boxed, ..) => Some(boxed.baseline),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let below = items
        .iter()
        .filter_map(|item| match item {
            Item::Atom(boxed, ..) => Some(boxed.below()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    for item in &mut items {
        if let Item::Middle(delimiter) = item {
            let boxed = if ctx.tall {
                tall_delimiter(delimiter, above, below)
            } else {
                Boxed::text(delimiter.as_str())
            };
            *item = Item::Atom(boxed, Class::Rel, false);
        }
    }
    demote(&mut items, lead);

    let mut parts: Vec<Boxed> = Vec::new();
    let mut previous = lead;
    let mut explicit: Option<usize> = None;
    let mut after_fraction = false;
    let mut after_bracketed = false;
    for item in items {
        match item {
            Item::Space(width) => *explicit.get_or_insert(0) += width,
            Item::Atom(mut boxed, class, fraction) => {
                let natural = previous.map_or(0, |previous| gap(previous, class, ctx.script));
                let mut width = explicit.take().map_or(natural, |width| width.max(natural));
                let mut bracketed = false;
                if after_fraction && width == 0 {
                    if ctx.tall {
                        // Two fractions side by side would share one long
                        // rule and read as a single fraction.
                        if fraction {
                            width = 1;
                        }
                    } else {
                        // On one line `a/bc` reads as a over bc, so a
                        // fraction run into what follows it is bracketed, and
                        // so is a second fraction after it: `(a/b)c`,
                        // `(dz/dy)(dy/dx)`.
                        if !after_bracketed && let Some(last) = parts.last_mut() {
                            *last = Boxed::text(format!("({})", one_line(last)));
                        }
                        if fraction {
                            boxed = Boxed::text(format!("({})", one_line(&boxed)));
                            bracketed = true;
                        }
                    }
                }
                if width > 0 {
                    // TeX breaks inline maths after a relation or a binary
                    // operator, and nowhere else.
                    let breaks = ctx.breakable && matches!(previous, Some(Class::Rel | Class::Bin));
                    let space = if breaks { ' ' } else { ctx.space() };
                    parts.push(Boxed::text(space.to_string().repeat(width)));
                }
                parts.push(boxed);
                previous = Some(class);
                after_fraction = fraction;
                after_bracketed = bracketed;
            }
            Item::Middle(_) => {}
        }
    }
    if let Some(width) = explicit.filter(|width| *width > 0) {
        parts.push(Boxed::text(ctx.space().to_string().repeat(width)));
    }
    beside(parts)
}

/// TeX's rule for a binary operator with nothing to combine on one side: it
/// is read as an ordinary symbol, which is what makes the minus in `-x`,
/// `(-1)` and `a = -b` a sign rather than a subtraction.
fn demote(items: &mut [Item], lead: Option<Class>) {
    let mut previous: Option<(Option<usize>, Class)> = lead.map(|class| (None, class));
    for at in 0..items.len() {
        let Item::Atom(_, class, _) = &mut items[at] else {
            continue;
        };
        let before = previous.map(|(_, class)| class);
        if *class == Class::Bin
            && matches!(
                before,
                None | Some(Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct)
            )
        {
            *class = Class::Ord;
        }
        let current = *class;
        if matches!(current, Class::Rel | Class::Close | Class::Punct)
            && let Some((Some(index), Class::Bin)) = previous
            && let Item::Atom(_, class, _) = &mut items[index]
        {
            *class = Class::Ord;
        }
        previous = Some((Some(at), current));
    }
    if let Some((Some(index), Class::Bin)) = previous
        && let Item::Atom(_, class, _) = &mut items[index]
    {
        *class = Class::Ord;
    }
}

/// Columns between two neighbouring atoms. TeX's medium and thick spaces
/// become one, as do its thin spaces beside an operator (`sin x`) and after
/// punctuation. Its other thin spaces are narrower than anything a terminal
/// can draw, so they become none.
fn gap(left: Class, right: Class, script: bool) -> usize {
    use Class::{Bin, Close, Inner, Op, Open, Ord, Punct, Rel};
    let loose = usize::from(!script);
    match (left, right) {
        (Ord | Op | Close | Inner, Op) | (Op, Ord) => 1,
        // Two bracketed groups drawn rows tall would otherwise run together
        // into what looks like one.
        (Op | Inner, Inner) | (Punct, _) => loose,
        (Bin, Ord | Op | Open | Inner) | (Ord | Close | Inner, Bin) => loose,
        (Rel, Ord | Op | Open | Inner) | (Ord | Op | Close | Inner, Rel) => loose,
        _ => 0,
    }
}

fn class_of(node: &Node) -> Class {
    match node {
        Node::Sym(_, class) | Node::Classed(class, _) => *class,
        Node::Op { .. } | Node::Brace { .. } => Class::Op,
        Node::Scripts { base, .. } => class_of(base),
        Node::Stack { arrow: Some(_), .. } => Class::Rel,
        Node::Stack { base, .. } => match base.as_slice() {
            [only] => class_of(only),
            _ => Class::Ord,
        },
        Node::Fenced { .. } | Node::Binom(..) => Class::Inner,
        Node::Array(grid) if !grid.open.is_empty() || !grid.close.is_empty() => Class::Inner,
        _ => Class::Ord,
    }
}

fn layout_node(node: &Node, ctx: Ctx) -> Boxed {
    match node {
        Node::Sym(text, _) => Boxed::text(text.as_str()),
        Node::Text(text) if ctx.tall => Boxed::text(text.as_str()),
        Node::Text(text) => Boxed::text(text.replace(' ', "\u{a0}")),
        Node::Group(nodes) | Node::Classed(_, nodes) => layout(nodes, ctx, None),
        Node::Op { name, .. } => Boxed::text(name.as_str()),
        Node::Scripts { base, sub, sup } => scripts(base, sub.as_deref(), sup.as_deref(), ctx),
        Node::Frac { num, den, rule } => fraction(num, den, *rule, ctx),
        Node::Binom(n, k) => binomial(n, k, ctx),
        Node::Root { index, body } => root(index.as_deref(), body, ctx),
        Node::Accent { mark, last, body } => accented(*mark, *last, body, ctx),
        Node::Wide(kind, body) => wide(*kind, body, ctx),
        Node::Brace { over, body } => brace(*over, body, ctx),
        Node::Stack {
            base,
            over,
            under,
            arrow,
        } => stack(base, over.as_deref(), under.as_deref(), *arrow, ctx),
        Node::Fenced { open, body, close } => fenced(open, body, close, ctx),
        Node::Array(grid) => array(grid, ctx),
        Node::Boxed(body) => framed(body, ctx),
        Node::Phantom(body) => {
            let body = layout(body, ctx, None);
            Boxed::blank(body.width, body.height(), body.baseline)
        }
        Node::Strike(body) => {
            let mut body = layout(body, ctx, None);
            for row in &mut body.rows {
                *row = combine(row, '\u{338}', false);
            }
            body
        }
        Node::Space(width) => Boxed::text(ctx.space().to_string().repeat(*width)),
        Node::Middle(delimiter) => Boxed::text(delimiter.as_str()),
        Node::Break => Boxed::text(""),
    }
}

/// Sub- and superscripts.
///
/// On a single row they use Unicode's small forms when every character has
/// one: `x²`, `aᵢⱼ`, `Aᵀ`. Display maths raises or lowers the rest onto rows
/// of their own. A line of text can't, so there they are marked the way
/// people write maths in plain text: `e^(iπ)`.
fn scripts(base: &Node, sub: Option<&[Node]>, sup: Option<&[Node]>, ctx: Ctx) -> Boxed {
    let body = layout_node(base, ctx);

    if ctx.tall && takes_limits(base) {
        // Limits have room to stack, which is what `\substack` is for.
        let limit = Ctx {
            script: true,
            ..ctx.nested()
        };
        let over = sup.map(|nodes| layout(nodes, limit, None));
        let under = sub.map(|nodes| layout(nodes, limit, None));
        return limits(&body, over, under);
    }

    let sub = sub.map(|nodes| one_line(&layout(nodes, Ctx::SCRIPT, None)));
    let sup = sup.map(|nodes| one_line(&layout(nodes, Ctx::SCRIPT, None)));
    let small_sub = sub.as_deref().and_then(subscript);
    let small_sup = sup.as_deref().and_then(superscript);

    if body.height() == 1 {
        let small = sub.is_none() == small_sub.is_none() && sup.is_none() == small_sup.is_none();
        if small || !ctx.tall {
            // On one line, a fraction, a root or a brace carrying a script is
            // bracketed so the script is read as applying to all of it:
            // `(a/b)²`, not `a/b²`.
            let mut row = match base {
                Node::Frac { .. } | Node::Root { .. } | Node::Brace { .. } if !ctx.tall => {
                    format!("({})", body.rows[0])
                }
                Node::Group(_) if !ctx.tall => grouped(body.rows[0].clone()),
                _ => body.rows[0].clone(),
            };
            if let Some(text) = &sub {
                row.push_str(&small_sub.unwrap_or_else(|| marked('_', text)));
            }
            if let Some(text) = &sup {
                row.push_str(&small_sup.unwrap_or_else(|| marked('^', text)));
            }
            return Boxed::text(row);
        }

        let width = body.width
            + sub
                .as_deref()
                .map_or(0, str::width)
                .max(sup.as_deref().map_or(0, str::width));
        let beside = |text: &str| pad(format!("{}{text}", " ".repeat(body.width)), width);
        let mut rows = Vec::new();
        if let Some(text) = &sup {
            rows.push(beside(text));
        }
        let baseline = rows.len();
        rows.push(pad(body.rows[0].clone(), width));
        if let Some(text) = &sub {
            rows.push(beside(text));
        }
        return Boxed {
            rows,
            width,
            baseline,
        };
    }

    // Beside something tall, level with its top row and its bottom one.
    let sup = sup.map(|text| small_sup.unwrap_or(text));
    let sub = sub.map(|text| small_sub.unwrap_or(text));
    let column = sup
        .as_deref()
        .map_or(0, str::width)
        .max(sub.as_deref().map_or(0, str::width));
    let last = body.height() - 1;
    let rows = body
        .rows
        .iter()
        .enumerate()
        .map(|(at, row)| {
            let script = match (at, &sup, &sub) {
                (0, Some(sup), _) => sup.as_str(),
                (at, _, Some(sub)) if at == last => sub.as_str(),
                _ => "",
            };
            pad(format!("{row}{script}"), body.width + column)
        })
        .collect();
    Boxed {
        rows,
        width: body.width + column,
        baseline: body.baseline,
    }
}

fn takes_limits(base: &Node) -> bool {
    matches!(base, Node::Op { limits: true, .. } | Node::Brace { .. })
}

/// A script in plain-text notation, for when Unicode has no small form of it.
fn marked(mark: char, text: &str) -> String {
    if text.width() <= 1 {
        format!("{mark}{text}")
    } else {
        format!("{mark}({text})")
    }
}

/// Limits above and below their operator, all centred on one another.
fn limits(body: &Boxed, over: Option<Boxed>, under: Option<Boxed>) -> Boxed {
    let over = over.filter(|part| part.width > 0);
    let under = under.filter(|part| part.width > 0);
    let width = [Some(body), over.as_ref(), under.as_ref()]
        .into_iter()
        .flatten()
        .map(|part| part.width)
        .max()
        .unwrap_or(0);

    let mut rows = Vec::new();
    if let Some(over) = &over {
        rows.extend(centred(over, width));
    }
    let baseline = rows.len() + body.baseline;
    rows.extend(centred(body, width));
    if let Some(under) = &under {
        rows.extend(centred(under, width));
    }
    Boxed {
        rows,
        width,
        baseline,
    }
}

fn fraction(num_nodes: &[Node], den_nodes: &[Node], rule: bool, ctx: Ctx) -> Boxed {
    let num = layout(num_nodes, ctx, None);
    let den = layout(den_nodes, ctx, None);

    if ctx.tall {
        let width = num.width.max(den.width).max(1);
        let mut rows = centred(&num, width);
        if rule {
            let baseline = rows.len();
            rows.push("─".repeat(width));
            rows.extend(centred(&den, width));
            return Boxed {
                rows,
                width,
                baseline,
            };
        }
        rows.extend(centred(&den, width));
        let baseline = (rows.len() - 1) / 2;
        return Boxed {
            rows,
            width,
            baseline,
        };
    }

    let (num, den) = (one_line(&num), one_line(&den));
    if !rule {
        return Boxed::text(format!("{num},\u{a0}{den}"));
    }
    if let Some(glyph) = vulgar(&num, &den) {
        return Boxed::text(glyph);
    }
    // `1/2a` reads as half of `a`, so a product below the line is bracketed.
    // A derivative is the exception everyone already reads correctly: `dy/dx`.
    let derivative = differential(num_nodes) && differential(den_nodes);
    let den = if juxtaposed(den_nodes) && !derivative && !bracketed(&den) {
        format!("({den})")
    } else {
        grouped(den)
    };
    Boxed::text(format!("{}/{den}", grouped(num)))
}

/// Whether atoms sit side by side as a product, as in `2a` or `bc`, rather
/// than forming one term like `n`, `10`, `k!` or `P(B)`.
fn juxtaposed(nodes: &[Node]) -> bool {
    let mut depth = 0usize;
    let mut terms = 0;
    let mut in_number = false;
    for node in nodes {
        let digit = matches!(node, Node::Sym(text, Class::Ord) if text.chars().all(|ch| ch.is_ascii_digit() || ch == '.'));
        match node {
            Node::Sym(_, Class::Open) => depth += 1,
            Node::Sym(_, Class::Close) => depth = depth.saturating_sub(1),
            Node::Space(_) | Node::Break => {}
            _ if depth > 0 => {}
            _ if digit && in_number => {}
            Node::Sym(_, Class::Punct) => {}
            _ => terms += 1,
        }
        in_number = digit;
    }
    terms > 1
}

/// Whether a side of a fraction starts with a `d` or a `∂`, as both sides of a
/// derivative do.
fn differential(nodes: &[Node]) -> bool {
    match nodes.first() {
        Some(Node::Sym(text, _)) => matches!(text.as_str(), "d" | "∂" | "δ" | "Δ" | "D"),
        Some(Node::Scripts { base, .. }) => differential(std::slice::from_ref(base)),
        Some(Node::Group(nodes)) => differential(nodes),
        _ => false,
    }
}

/// A fraction Unicode has a single character for: `½`, `¾`.
fn vulgar(num: &str, den: &str) -> Option<&'static str> {
    Some(match (num, den) {
        ("1", "2") => "½",
        ("1", "3") => "⅓",
        ("2", "3") => "⅔",
        ("1", "4") => "¼",
        ("3", "4") => "¾",
        ("1", "5") => "⅕",
        ("2", "5") => "⅖",
        ("3", "5") => "⅗",
        ("4", "5") => "⅘",
        ("1", "6") => "⅙",
        ("5", "6") => "⅚",
        ("1", "8") => "⅛",
        ("3", "8") => "⅜",
        ("5", "8") => "⅝",
        ("7", "8") => "⅞",
        _ => return None,
    })
}

/// Brackets around one side of a fraction set on one line, when it needs
/// them to be read as one thing: `(a + b)/2`, but `n(n + 1)/2`.
fn grouped(text: String) -> String {
    let mut depth = 0usize;
    let loose = text.chars().any(|ch| match ch {
        '(' | '[' | '{' | '⟨' => {
            depth += 1;
            false
        }
        ')' | ']' | '}' | '⟩' => {
            depth = depth.saturating_sub(1);
            false
        }
        ' ' | NBSP | '/' => depth == 0,
        _ => false,
    });
    if loose { format!("({text})") } else { text }
}

/// Whether a whole piece of text is one bracketed group, like `(a + b)`.
fn bracketed(text: &str) -> bool {
    let Some(open) = text.chars().next() else {
        return false;
    };
    let close = match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return false,
    };
    let mut depth = 0usize;
    let count = text.chars().count();
    for (at, ch) in text.chars().enumerate() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return at == count - 1;
            }
        }
    }
    false
}

fn binomial(n: &[Node], k: &[Node], ctx: Ctx) -> Boxed {
    if !ctx.tall {
        let n = one_line(&layout(n, ctx, None));
        let k = one_line(&layout(k, ctx, None));
        return Boxed::text(format!("C({n},\u{a0}{k})"));
    }
    let stacked = fraction(n, k, false, ctx);
    let (above, below) = (stacked.baseline, stacked.below());
    beside(vec![
        tall_delimiter("(", above, below),
        stacked,
        tall_delimiter(")", above, below),
    ])
}

fn root(index: Option<&[Node]>, body: &[Node], ctx: Ctx) -> Boxed {
    let index = index
        .map(|nodes| one_line(&layout(nodes, Ctx::SCRIPT, None)))
        .filter(|text| !text.is_empty());
    let small_index = index
        .as_deref()
        .map(|index| superscript(index).unwrap_or_else(|| format!("({index})")));
    let sign = match index.as_deref() {
        None => "√".to_string(),
        Some("3") => "∛".to_string(),
        Some("4") => "∜".to_string(),
        Some(_) => format!("{}√", small_index.as_deref().unwrap_or_default()),
    };
    let body = layout(body, ctx, None);

    if !ctx.tall {
        let text = one_line(&body);
        let simple = text.width() <= 1
            || text.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
            || bracketed(&text);
        return Boxed::text(if simple {
            format!("{sign}{text}")
        } else {
            format!("{sign}({text})")
        });
    }

    let sign_width = sign.width();
    if body.height() == 1 {
        // The bar sits at the foot of the row above, which is where the top of
        // the radical reaches up to.
        let width = sign_width + body.width;
        return Boxed {
            rows: vec![
                format!("{}{}", " ".repeat(sign_width), "_".repeat(body.width)),
                format!("{sign}{}", body.rows[0]),
            ],
            width,
            baseline: 1,
        };
    }

    // Taller than a row: a radical drawn as a long stroke up the body's side,
    // with any index tucked into the crook of it.
    let height = body.height();
    let stroke = height + 1;
    let prefix = small_index.as_deref().map_or(0, str::width);
    let mut rows = vec![format!(
        "{}{}",
        " ".repeat(prefix + stroke),
        "_".repeat(body.width)
    )];
    for (at, row) in body.rows.iter().enumerate() {
        let level = at + 1;
        let lead = match small_index.as_deref() {
            Some(index) if level == height - 1 => index.to_string(),
            _ => " ".repeat(prefix),
        };
        let radical = if level == height {
            format!("╲╱{}", " ".repeat(stroke - 2))
        } else {
            let before = height + 1 - level;
            format!("{}╱{}", " ".repeat(before), " ".repeat(stroke - before - 1))
        };
        rows.push(format!("{lead}{radical}{row}"));
    }
    Boxed {
        rows,
        width: prefix + stroke + body.width,
        baseline: body.baseline + 1,
    }
}

fn accented(mark: char, last: bool, body: &[Node], ctx: Ctx) -> Boxed {
    let body = layout(body, ctx, None);
    if body.height() > 1 {
        return body;
    }
    let row = &body.rows[0];
    let letters = row.chars().filter(|&ch| is_letter(ch)).count();
    // A bar over several letters is an overline, which joins up where a row
    // of macrons would not.
    let mark = if mark == MACRON && letters > 1 {
        OVERLINE
    } else {
        mark
    };
    Boxed::text(combine(row, mark, last))
}

/// Puts a combining mark on every visible character, or only the last.
///
/// Nothing is marked past a few dozen characters. No accent spans that much,
/// and nesting accents past the depth limit would otherwise mark the whole of
/// the rest of the input once per level.
fn combine(text: &str, mark: char, last: bool) -> String {
    let count = text.chars().filter(|&ch| is_letter(ch)).count();
    if count > MAX_MARKED {
        return text.to_string();
    }
    let mut seen = 0;
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if is_letter(ch) {
            seen += 1;
            if !last || seen == count {
                out.push(mark);
            }
        }
    }
    out
}

fn is_letter(ch: char) -> bool {
    !ch.is_whitespace() && ch.width().unwrap_or(0) > 0
}

fn wide(kind: Wide, body: &[Node], ctx: Ctx) -> Boxed {
    let body = layout(body, ctx, None);
    if !ctx.tall || (body.height() == 1 && body.width <= 1) {
        let row = one_line(&body);
        return Boxed::text(match kind {
            Wide::Overline => combine(&row, OVERLINE, false),
            Wide::Underline => combine(&row, '\u{332}', false),
            Wide::RightArrow => combine(&row, '\u{20d7}', true),
            Wide::LeftArrow => combine(&row, '\u{20d6}', true),
            Wide::BothArrow => combine(&row, '\u{20e1}', true),
        });
    }

    // An arrow needs its head, and both heads for `↔`, however narrow the
    // body under it.
    let width = match kind {
        Wide::Overline | Wide::Underline => body.width,
        Wide::RightArrow | Wide::LeftArrow => body.width.max(1),
        Wide::BothArrow => body.width.max(2),
    };
    let line = match kind {
        Wide::Overline => "_".repeat(width),
        Wide::Underline => "‾".repeat(width),
        Wide::RightArrow => format!("{}→", "─".repeat(width - 1)),
        Wide::LeftArrow => format!("←{}", "─".repeat(width - 1)),
        Wide::BothArrow => format!("←{}→", "─".repeat(width - 2)),
    };
    let mut rows = centred(&body, width);
    if kind == Wide::Underline {
        rows.push(line);
        return Boxed {
            rows,
            width,
            baseline: body.baseline,
        };
    }
    rows.insert(0, line);
    Boxed {
        rows,
        width,
        baseline: body.baseline + 1,
    }
}

/// `\overbrace` and `\underbrace`, drawn with box-drawing corners and a tee
/// pointing at the label.
fn brace(over: bool, body: &[Node], ctx: Ctx) -> Boxed {
    let body = layout(body, ctx, None);
    if !ctx.tall {
        return body;
    }
    let width = body.width;
    let (left, tee, right) = if over {
        ('┌', '┴', '┐')
    } else {
        ('└', '┬', '┘')
    };
    let line: String = match width {
        0 => String::new(),
        1 => tee.to_string(),
        width => (0..width)
            .map(|at| match at {
                0 => left,
                at if at == width - 1 => right,
                at if at == width / 2 => tee,
                _ => '─',
            })
            .collect(),
    };
    let mut rows = body.rows;
    if over {
        rows.insert(0, line);
        Boxed {
            rows,
            width,
            baseline: body.baseline + 1,
        }
    } else {
        rows.push(line);
        Boxed {
            rows,
            width,
            baseline: body.baseline,
        }
    }
}

fn stack(
    base: &[Node],
    over: Option<&[Node]>,
    under: Option<&[Node]>,
    arrow: Option<Arrow>,
    ctx: Ctx,
) -> Boxed {
    let over = over
        .map(|nodes| one_line(&layout(nodes, Ctx::SCRIPT, None)))
        .filter(|text| !text.is_empty());
    let under = under
        .map(|nodes| one_line(&layout(nodes, Ctx::SCRIPT, None)))
        .filter(|text| !text.is_empty());

    let body = match arrow {
        Some(arrow) if ctx.tall => {
            let label = over
                .as_deref()
                .map_or(0, str::width)
                .max(under.as_deref().map_or(0, str::width));
            Boxed::text(arrow.stretched(label + 2))
        }
        Some(arrow) => Boxed::text(arrow.glyph),
        None => layout(base, ctx, None),
    };
    if ctx.tall {
        return limits(&body, over.map(Boxed::text), under.map(Boxed::text));
    }
    // In a line, what would sit above and below becomes a superscript and a
    // subscript.
    let mut row = one_line(&body);
    if let Some(text) = &under {
        row.push_str(&subscript(text).unwrap_or_else(|| marked('_', text)));
    }
    if let Some(text) = &over {
        row.push_str(&superscript(text).unwrap_or_else(|| marked('^', text)));
    }
    Boxed::text(row)
}

fn fenced(open: &str, body: &[Node], close: &str, ctx: Ctx) -> Boxed {
    let body = layout(body, ctx, None);
    if body.height() == 1 {
        return Boxed::text(format!("{open}{}{close}", body.rows[0]));
    }
    let (above, below) = (body.baseline, body.below());
    beside(vec![
        tall_delimiter(open, above, below),
        body,
        tall_delimiter(close, above, below),
    ])
}

/// A delimiter drawn as tall as `above + 1 + below` rows, from the pieces
/// Unicode provides for exactly this.
fn tall_delimiter(delimiter: &str, above: usize, below: usize) -> Boxed {
    let height = above + 1 + below;
    if delimiter.is_empty() {
        return Boxed::blank(0, height, above);
    }
    if height == 1 {
        return Boxed::text(delimiter);
    }

    let last = height - 1;
    let middle = last / 2;
    let pieces = |top: &'static str, between: &'static str, bottom: &'static str| {
        (0..height)
            .map(|at| match at {
                0 => top,
                at if at == last => bottom,
                _ => between,
            })
            .collect::<Vec<_>>()
    };
    let curly = |pair: [&'static str; 2], ends: [&'static str; 2], centre: &'static str| {
        if height == 2 {
            return pair.to_vec();
        }
        (0..height)
            .map(|at| match at {
                0 => ends[0],
                at if at == last => ends[1],
                at if at == middle => centre,
                _ => "⎪",
            })
            .collect()
    };
    let angle = |upper: &'static str, point: &'static str, lower: &'static str| {
        (0..height)
            .map(|at| {
                if !height.is_multiple_of(2) && at == middle {
                    point
                } else if at * 2 < height {
                    upper
                } else {
                    lower
                }
            })
            .collect::<Vec<_>>()
    };

    let rows: Vec<&str> = match delimiter {
        "(" => pieces("⎛", "⎜", "⎝"),
        ")" => pieces("⎞", "⎟", "⎠"),
        "[" => pieces("⎡", "⎢", "⎣"),
        "]" => pieces("⎤", "⎥", "⎦"),
        "⌈" => pieces("⎡", "⎢", "⎢"),
        "⌉" => pieces("⎤", "⎥", "⎥"),
        "⌊" => pieces("⎢", "⎢", "⎣"),
        "⌋" => pieces("⎥", "⎥", "⎦"),
        "{" => curly(["⎰", "⎱"], ["⎧", "⎩"], "⎨"),
        "}" => curly(["⎱", "⎰"], ["⎫", "⎭"], "⎬"),
        "|" | "∣" => vec!["│"; height],
        "‖" => vec!["‖"; height],
        "⟨" => angle("╱", "⟨", "╲"),
        "⟩" => angle("╲", "⟩", "╱"),
        // Anything else is not built to stretch, so it sits on the baseline.
        other => {
            let width = other.width();
            let mut rows = vec![" ".repeat(width); height];
            rows[above] = other.to_string();
            return Boxed {
                rows,
                width,
                baseline: above,
            };
        }
    };
    Boxed {
        rows: rows.into_iter().map(str::to_string).collect(),
        width: 1,
        baseline: above,
    }
}

fn array(grid: &Grid, ctx: Ctx) -> Boxed {
    let pairs = matches!(grid.columns, Columns::Pairs);
    let align = |column: usize| match &grid.columns {
        Columns::All(align) => *align,
        Columns::Pairs if column.is_multiple_of(2) => Align::Right,
        Columns::Pairs => Align::Left,
        Columns::Spec(spec) => spec
            .get(column)
            .or(spec.last())
            .copied()
            .unwrap_or(Align::Center),
    };
    // The second half of an `aligned` pair carries on the line the first half
    // began, so its `=` is spaced as though the left side were still there.
    let lead = |column: usize| (pairs && !column.is_multiple_of(2)).then_some(Class::Ord);
    let cells: Vec<Vec<Boxed>> = grid
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(column, cell)| layout(cell, ctx.nested(), lead(column)))
                .collect()
        })
        .collect();

    if !ctx.tall {
        let rows: Vec<String> = cells
            .iter()
            .map(|row| {
                let cells: Vec<String> = row.iter().map(one_line).collect();
                let row = if pairs {
                    cells
                        .chunks(2)
                        .map(|pair| pair.concat())
                        .collect::<Vec<_>>()
                        .join("\u{a0}\u{a0}")
                } else {
                    cells.join(",\u{a0}")
                };
                // A line that continues the one above starts at its `=`.
                row.trim_start_matches([' ', NBSP]).to_string()
            })
            .collect();
        return Boxed::text(format!(
            "{}{}{}",
            grid.open,
            rows.join(";\u{a0}"),
            grid.close
        ));
    }

    let columns = cells.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            cells
                .iter()
                .filter_map(|row| row.get(column))
                .map(|cell| cell.width)
                .max()
                .unwrap_or(0)
        })
        .collect();
    let gap = |column: usize| {
        if column == 0 || (pairs && !column.is_multiple_of(2)) {
            0
        } else {
            2
        }
    };
    let width: usize = widths.iter().sum::<usize>() + (0..columns).map(gap).sum::<usize>();

    let mut rows: Vec<String> = Vec::new();
    let mut previous_tall = false;
    for (at, row) in cells.iter().enumerate() {
        let tall = row.iter().any(|cell| cell.height() > 1);
        // Rows of fractions would otherwise run into one another.
        if at > 0 && (tall || previous_tall) {
            rows.push(" ".repeat(width));
        }
        previous_tall = tall;

        let mut parts = Vec::new();
        for (column, &column_width) in widths.iter().enumerate() {
            if gap(column) > 0 {
                parts.push(Boxed::text(" ".repeat(gap(column))));
            }
            let cell = row.get(column).cloned().unwrap_or_else(|| Boxed::text(""));
            parts.push(aligned(cell, column_width, align(column)));
        }
        rows.extend(beside(parts).rows);
    }
    if rows.is_empty() {
        return Boxed::text("");
    }

    let baseline = (rows.len() - 1) / 2;
    let body = Boxed {
        rows,
        width,
        baseline,
    };
    let (above, below) = (body.baseline, body.below());
    let mut parts = vec![tall_delimiter(grid.open, above, below)];
    // A brace is drawn hard against what follows it; the others look right
    // without the room.
    if grid.open == "{" {
        parts.push(Boxed::blank(1, body.height(), above));
    }
    let brace_close = grid.close == "}";
    parts.push(body);
    if brace_close {
        parts.push(Boxed::blank(1, above + below + 1, above));
    }
    parts.push(tall_delimiter(grid.close, above, below));
    beside(parts)
}

fn aligned(cell: Boxed, width: usize, align: Align) -> Boxed {
    let spare = width.saturating_sub(cell.width);
    let left = match align {
        Align::Left => 0,
        Align::Center => spare / 2,
        Align::Right => spare,
    };
    Boxed {
        rows: cell
            .rows
            .iter()
            .map(|row| format!("{}{row}{}", " ".repeat(left), " ".repeat(spare - left)))
            .collect(),
        width,
        baseline: cell.baseline,
    }
}

fn framed(body: &[Node], ctx: Ctx) -> Boxed {
    let body = layout(body, ctx, None);
    if !ctx.tall {
        return body;
    }
    let inside = body.width + 2;
    let mut rows = vec![format!("┌{}┐", "─".repeat(inside))];
    rows.extend(body.rows.iter().map(|row| format!("│ {row} │")));
    rows.push(format!("└{}┘", "─".repeat(inside)));
    Boxed {
        rows,
        width: inside + 2,
        baseline: body.baseline + 1,
    }
}

/// Lines boxes up side by side on their baselines.
fn beside(parts: Vec<Boxed>) -> Boxed {
    let above = parts.iter().map(|part| part.baseline).max().unwrap_or(0);
    let below = parts.iter().map(Boxed::below).max().unwrap_or(0);
    let mut rows = vec![String::new(); above + 1 + below];
    for part in &parts {
        let top = above - part.baseline;
        for (at, row) in rows.iter_mut().enumerate() {
            match at.checked_sub(top).and_then(|at| part.rows.get(at)) {
                Some(line) => row.push_str(line),
                None => row.push_str(&" ".repeat(part.width)),
            }
        }
    }
    Boxed {
        rows,
        width: parts.iter().map(|part| part.width).sum(),
        baseline: above,
    }
}

/// A box's rows, centred in `width` columns.
fn centred(boxed: &Boxed, width: usize) -> Vec<String> {
    let spare = width.saturating_sub(boxed.width);
    let left = " ".repeat(spare / 2);
    let right = " ".repeat(spare - spare / 2);
    boxed
        .rows
        .iter()
        .map(|row| format!("{left}{row}{right}"))
        .collect()
}

fn pad(text: String, width: usize) -> String {
    let short = width.saturating_sub(text.width());
    if short == 0 {
        text
    } else {
        text + &" ".repeat(short)
    }
}

/// The single row of a box set without room to stack.
fn one_line(boxed: &Boxed) -> String {
    match boxed.rows.as_slice() {
        [only] => only.clone(),
        rows => rows
            .iter()
            .map(|row| row.trim())
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    }
}

// ---------------------------------------------------------------------------
// Small forms
// ---------------------------------------------------------------------------

/// Text in Unicode's superscript forms, if every character has one.
fn superscript(text: &str) -> Option<String> {
    text.chars()
        .map(|ch| {
            Some(match ch {
                '0' => '⁰',
                '1' => '¹',
                '2' => '²',
                '3' => '³',
                '4'..='9' => char::from_u32(0x2074 + (ch as u32 - '4' as u32))?,
                '+' => '⁺',
                '−' | '-' => '⁻',
                '=' => '⁼',
                '(' => '⁽',
                ')' => '⁾',
                'a' => 'ᵃ',
                'b' => 'ᵇ',
                'c' => 'ᶜ',
                'd' => 'ᵈ',
                'e' => 'ᵉ',
                'f' => 'ᶠ',
                'g' => 'ᵍ',
                'h' => 'ʰ',
                'i' => 'ⁱ',
                'j' => 'ʲ',
                'k' => 'ᵏ',
                'l' => 'ˡ',
                'm' => 'ᵐ',
                'n' => 'ⁿ',
                'o' => 'ᵒ',
                'p' => 'ᵖ',
                'r' => 'ʳ',
                's' => 'ˢ',
                't' => 'ᵗ',
                'u' => 'ᵘ',
                'v' => 'ᵛ',
                'w' => 'ʷ',
                'x' => 'ˣ',
                'y' => 'ʸ',
                'z' => 'ᶻ',
                'A' => 'ᴬ',
                'B' => 'ᴮ',
                'D' => 'ᴰ',
                'E' => 'ᴱ',
                'G' => 'ᴳ',
                'H' => 'ᴴ',
                'I' => 'ᴵ',
                'J' => 'ᴶ',
                'K' => 'ᴷ',
                'L' => 'ᴸ',
                'M' => 'ᴹ',
                'N' => 'ᴺ',
                'O' => 'ᴼ',
                'P' => 'ᴾ',
                'R' => 'ᴿ',
                'T' => 'ᵀ',
                'U' => 'ᵁ',
                'V' => 'ⱽ',
                'W' => 'ᵂ',
                'α' => 'ᵅ',
                'β' => 'ᵝ',
                'γ' => 'ᵞ',
                'δ' => 'ᵟ',
                'ε' | 'ϵ' => 'ᵋ',
                'θ' => 'ᶿ',
                'ι' => 'ᶥ',
                'φ' | 'ϕ' => 'ᵠ',
                'χ' => 'ᵡ',
                // Already up where a superscript sits.
                '′' | '″' | '‴' | '⁗' | '†' | '‡' | '*' => ch,
                '∗' => '*',
                '∘' => '°',
                ' ' | NBSP => NBSP,
                _ => return None,
            })
        })
        .collect()
}

/// Text in Unicode's subscript forms, if every character has one.
fn subscript(text: &str) -> Option<String> {
    text.chars()
        .map(|ch| {
            Some(match ch {
                '0'..='9' => char::from_u32(0x2080 + (ch as u32 - '0' as u32))?,
                '+' => '₊',
                '−' | '-' => '₋',
                '=' => '₌',
                '(' => '₍',
                ')' => '₎',
                'a' => 'ₐ',
                'e' => 'ₑ',
                'h' => 'ₕ',
                'i' => 'ᵢ',
                'j' => 'ⱼ',
                'k' => 'ₖ',
                'l' => 'ₗ',
                'm' => 'ₘ',
                'n' => 'ₙ',
                'o' => 'ₒ',
                'p' => 'ₚ',
                'r' => 'ᵣ',
                's' => 'ₛ',
                't' => 'ₜ',
                'u' => 'ᵤ',
                'v' => 'ᵥ',
                'x' => 'ₓ',
                'β' => 'ᵦ',
                'γ' => 'ᵧ',
                'ρ' => 'ᵨ',
                'φ' | 'ϕ' => 'ᵩ',
                'χ' => 'ᵪ',
                // Already down where a subscript sits.
                ',' | '.' => ch,
                ' ' | NBSP => NBSP,
                _ => return None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inline maths with its non-breaking spaces shown as ordinary ones, which
    /// is how it looks on screen.
    fn line(tex: &str) -> String {
        inline(tex).replace(NBSP, " ")
    }

    fn rows(tex: &str) -> Vec<String> {
        display(tex).rows
    }

    #[test]
    fn spacing_follows_tex_rather_than_the_source() {
        assert_eq!(line("a=b+c"), "a = b + c");
        assert_eq!(line("a   =   b"), "a = b");
        assert_eq!(line(r"f(x,y)"), "f(x, y)");
        assert_eq!(line(r"\sin x + \cos(x)"), "sin x + cos(x)");
    }

    #[test]
    fn a_minus_sign_with_nothing_before_it_is_a_sign() {
        assert_eq!(line("-x + (-1) = -b"), "−x + (−1) = −b");
        assert_eq!(line("x^{-1}"), "x⁻¹");
    }

    #[test]
    fn a_line_may_break_only_after_a_relation_or_an_operator() {
        // As TeX allows. Anywhere else the formula holds together.
        assert_eq!(inline("a=b+c"), "a\u{a0}= b\u{a0}+ c");
        assert_eq!(inline(r"\sin x"), "sin\u{a0}x");
    }

    #[test]
    fn scripts_use_unicode_small_forms_where_they_exist() {
        assert_eq!(line("x^2 + a_{ij} + A^T + x_{n+1}"), "x² + aᵢⱼ + Aᵀ + xₙ₊₁");
        assert_eq!(line(r"x_{\text{max}}"), "xₘₐₓ");
        assert_eq!(line(r"f'(x) + f''(x)"), "f′(x) + f″(x)");
        assert_eq!(line(r"90^\circ"), "90°");
    }

    #[test]
    fn scripts_without_small_forms_are_marked_on_one_line() {
        assert_eq!(line(r"e^{i\pi}"), "e^(iπ)");
        assert_eq!(line(r"x_\alpha"), "x_α");
        assert_eq!(line("e^{-x^2}"), "e^(−x²)", "a script inside a script");
    }

    #[test]
    fn fractions_on_one_line_are_bracketed_only_where_needed() {
        assert_eq!(line(r"\frac{a+b}{2}"), "(a + b)/2");
        assert_eq!(line(r"\frac{n(n+1)}{2}"), "n(n + 1)/2");
        assert_eq!(line(r"\frac{(a+b)}{2}"), "(a + b)/2", "not doubled");
        assert_eq!(line(r"\frac{1}{2a}"), "1/(2a)", "1/2a reads as half of a");
        assert_eq!(line(r"\frac{dy}{dx}"), "dy/dx");
        assert_eq!(line(r"\frac{\partial^2 u}{\partial t^2}"), "∂²u/∂t²");
        assert_eq!(line(r"\frac{x^k}{k!}"), "xᵏ/k!");
        assert_eq!(line(r"\frac{a}{b}^2"), "(a/b)²");
    }

    #[test]
    fn common_fractions_are_single_characters() {
        assert_eq!(line(r"\frac{1}{2}mv^2"), "½mv²");
        assert_eq!(line(r"\frac34"), "¾");
    }

    #[test]
    fn roots_on_one_line() {
        assert_eq!(line(r"\sqrt{2} + \sqrt{10}"), "√2 + √10");
        assert_eq!(line(r"\sqrt{x+1}"), "√(x + 1)");
        assert_eq!(line(r"\sqrt[3]{x}"), "∛x");
        assert_eq!(line(r"\sqrt[n]{x}"), "ⁿ√x");
    }

    #[test]
    fn the_quadratic_formula_reads_correctly_on_one_line() {
        assert_eq!(
            line(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"),
            "x = (−b ± √(b² − 4ac))/(2a)"
        );
    }

    #[test]
    fn symbols_and_alphabets() {
        assert_eq!(
            line(r"\forall \epsilon > 0, \exists \delta \in \mathbb{R}"),
            "∀ϵ > 0, ∃δ ∈ ℝ"
        );
        assert_eq!(
            line(r"\mathbb{E}[X], \mathcal{L}, \mathbf{v}"),
            "𝔼[X], ℒ, 𝐯"
        );
        assert_eq!(line(r"x \not\in A, a \not= b"), "x ∉ A, a ≠ b");
        assert_eq!(line(r"\hat{x}, \vec{v}"), "x\u{302}, v\u{20d7}");
        assert_eq!(line(r"\{x \mid x > 0\}"), "{x ∣ x > 0}");
        assert_eq!(line(r"a \equiv b \pmod{n}"), "a ≡ b (mod n)");
    }

    #[test]
    fn text_keeps_its_spaces() {
        assert_eq!(line(r"\text{if } x > 0 \text{ then } y"), "if x > 0 then y");
        assert_eq!(line(r"\text{for $x > 0$}"), "for x > 0");
    }

    #[test]
    fn unknown_commands_are_shown_as_typed() {
        // A macro from someone's MathJax preamble can't be expanded, but it
        // can be left where it was rather than vanishing.
        assert_eq!(line(r"\norm{x} + \R"), r"\norm{x} + \R");
        assert_eq!(line(r"\foo x"), r"\foo x", "not run into the next letter");
    }

    #[test]
    fn lt_and_gt_are_the_relations_they_stand_for() {
        // Obsidian users write them to keep `<` from being read as HTML.
        assert_eq!(line(r"a \lt b \gt c"), "a < b > c");
    }

    #[test]
    fn fractions_side_by_side_stay_apart() {
        // The chain rule, which would otherwise read as one fraction.
        assert_eq!(line(r"\frac{dz}{dy}\frac{dy}{dx}"), "(dz/dy)(dy/dx)");
        assert_eq!(
            rows(r"\frac{dz}{dy}\frac{dy}{dx}"),
            ["dz dy", "── ──", "dy dx"]
        );
        // `a/bc` would read as a over bc.
        assert_eq!(line(r"\frac{a}{b}c"), "(a/b)c");
        assert_eq!(line(r"2\frac{a}{b}"), "2a/b");
        assert_eq!(line(r"\frac{1}{2}x"), "½x");
    }

    #[test]
    fn a_middle_delimiter_is_spaced_as_a_relation() {
        assert_eq!(
            line(r"\left\{ x \middle| -1 < x < 1 \right\}"),
            "{x | −1 < x < 1}"
        );
    }

    #[test]
    fn a_brace_label_on_one_line_applies_to_all_of_it() {
        assert_eq!(line(r"\underbrace{a+b}_{n}"), "(a + b)ₙ");
    }

    #[test]
    fn a_backslash_before_a_line_break_is_a_space() {
        assert_eq!(rows("a\\\nb"), ["a b"]);
    }

    #[test]
    fn nesting_past_the_limit_is_not_marked_level_by_level() {
        // Every accent past the limit used to mark the whole remainder again,
        // which on this input built a string of about a million characters.
        let tex = format!("{}x", r"\hat".repeat(5_000));
        assert!(inline(&tex).chars().count() < 30_000);
        let tex = format!("{}x", r"\cancel".repeat(5_000));
        assert!(inline(&tex).chars().count() < 40_000);
    }

    #[test]
    fn half_typed_maths_still_renders() {
        for tex in [
            r"\frac{1}{",
            "x^",
            r"\left( x",
            r"\begin{pmatrix} a & b",
            r"\sqrt[",
            "}}}",
            r"\",
            "",
        ] {
            let _ = inline(tex);
            let _ = display(tex);
        }
    }

    #[test]
    fn absurd_nesting_is_cut_short_rather_than_overflowing() {
        let deep = format!("{}x{}", "{".repeat(20_000), "}".repeat(20_000));
        assert!(inline(&deep).contains('x'));
        let chained = format!("{}x", r"\sqrt".repeat(20_000));
        let _ = display(&chained);
    }

    #[test]
    fn a_display_fraction_stacks_over_a_rule() {
        let set = display(r"\frac{a+b}{2}");
        assert_eq!(set.rows, ["a + b", "─────", "  2  "]);
        assert_eq!(set.baseline, 1, "the rule lines up with what is beside it");
        assert_eq!(rows(r"x = \frac{1}{2}"), ["    1", "x = ─", "    2"]);
    }

    #[test]
    fn display_limits_sit_above_and_below() {
        assert_eq!(rows(r"\sum_{i=1}^{n} i"), [" n   ", " ∑  i", "i=1  "]);
        assert_eq!(rows(r"\lim_{x \to 0} f"), ["lim f", "x→0  "]);
        // Integrals keep theirs beside, as TeX does.
        assert_eq!(rows(r"\int_0^1 x"), ["∫₀¹ x"]);
    }

    #[test]
    fn display_raises_scripts_that_have_no_small_form() {
        let set = display(r"e^{i\pi}");
        assert_eq!(set.rows, [" iπ", "e  "]);
        assert_eq!(set.baseline, 1);
    }

    #[test]
    fn display_roots_carry_a_bar() {
        assert_eq!(rows(r"\sqrt{x}"), [" _", "√x"]);
        assert_eq!(
            rows(r"\sqrt{\frac{a}{b}}"),
            ["    _", "   ╱a", "  ╱ ─", "╲╱  b"]
        );
    }

    #[test]
    fn brackets_grow_with_what_they_hold() {
        assert_eq!(
            rows(r"\left(\frac{a}{b}\right)^2"),
            ["⎛a⎞²", "⎜─⎟ ", "⎝b⎠ "]
        );
        assert_eq!(rows(r"\left(x\right)"), ["(x)"], "one row stays one row");
    }

    #[test]
    fn matrices_cases_and_aligned_lines() {
        assert_eq!(
            rows(r"A = \begin{pmatrix} 1 & 2 \\ 3 & 4 \end{pmatrix}"),
            ["A = ⎛1  2⎞", "    ⎝3  4⎠"]
        );
        assert_eq!(
            rows(r"\begin{cases} x & x \ge 0 \\ -x & x < 0 \end{cases}"),
            ["⎰ x   x ≥ 0", "⎱ −x  x < 0"]
        );
        // Lined up on the `=`, which keeps its spacing after the `&`.
        assert_eq!(
            rows(r"\begin{aligned} a &= b + c \\ &= d \end{aligned}"),
            ["a = b + c", "  = d    "]
        );
    }

    #[test]
    fn a_line_break_in_display_maths_starts_a_centred_line() {
        assert_eq!(rows(r"x + y = 1 \\ x = 0"), ["x + y = 1", "  x = 0  "]);
    }

    #[test]
    fn a_tag_is_kept_apart_for_the_margin() {
        let set = display(r"E = mc^2 \tag{1}");
        assert_eq!(set.rows, ["E = mc²"]);
        assert_eq!(set.tag.as_deref(), Some("(1)"));
        assert_eq!(display(r"x \tag*{A}").tag.as_deref(), Some("A"));
    }

    #[test]
    fn every_display_row_is_as_wide_as_the_box() {
        for tex in [
            r"\frac{a}{b} + \sqrt{\frac{1}{x}}",
            r"\begin{bmatrix} \frac12 & 0 \\ 0 & 1 \end{bmatrix}",
            r"\underbrace{a + b}_{n} = \overbrace{c}^{m}",
            r"\lim_{n\to\infty}\left(1+\frac1n\right)^n",
            r"\hat{x} + \overline{AB}",
            r"\xrightarrow[below]{above}",
            r"\boxed{x^2}",
            r"\overleftrightarrow{\frac{a}{b}}",
            r"\overrightarrow{\phantom{\frac{a}{b}}}",
            r"\frac{dz}{dy}\frac{dy}{dx}",
            r"\left\{ x \middle| \frac{1}{x} > 0 \right\}",
        ] {
            let set = display(tex);
            for row in &set.rows {
                assert_eq!(row.width(), set.width, "{tex}: {:?}", set.rows);
            }
            assert!(set.baseline < set.rows.len(), "{tex}");
        }
    }
}
