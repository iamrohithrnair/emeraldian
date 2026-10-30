//! A grid of terminal cells that diagrams are drawn on.
//!
//! Lines are kept as the directions they leave each cell in, not as
//! characters, so two edges that meet or cross join up into the right
//! box-drawing piece whatever order they were drawn in. Text and box outlines
//! are plain characters, and a line never draws over one.
//!
//! Everything drawn carries an [`Ink`]: what it is, and whose colour it
//! takes. Where two lines meet, the cell keeps the colour of the one drawn
//! last, as the top line would on paper, unless that one is no one's.
//!
//! Text is written a grapheme cluster at a time: what a terminal draws as
//! one, like a letter with the marks above and below it (the Thai `ที่` or
//! the Hindi `हि`) or emoji joined into one (`👨‍💻`). Each cluster takes the
//! columns ratatui gives it when it draws the row, which for nearly all text
//! are the columns the layout measured it at, so a row is exactly as wide on
//! screen as the canvas says. A cell holds a character, and only a cluster of
//! more than one is kept to one side, so plain text costs what it always did.

use ratatui::buffer::CellWidth;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use super::{Diagram, Ink, Role};

/// The directions a line leaves a cell in.
pub const UP: u8 = 1;
pub const DOWN: u8 = 2;
pub const LEFT: u8 = 4;
pub const RIGHT: u8 = 8;

/// How a line is drawn: Mermaid's `-->`, `-.->` and `==>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Solid,
    Dotted,
    Thick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Empty,
    /// A character on its own.
    Glyph(char, Ink),
    /// A grapheme cluster of more than one character, by its place in
    /// [`Canvas::clusters`].
    Cluster(u32, Ink),
    /// A column after the first of a character or cluster wider than one.
    Wide,
    /// The directions lines leave the cell in, how they are drawn, and whose
    /// colour they take.
    Line(u8, Stroke, Option<u16>),
}

/// The most cells a single diagram may take, so one enormous diagram can't
/// take a large share of memory: about 400 columns by 300 rows.
pub const MAX_CELLS: usize = 120_000;

pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    /// The text of every [`Cell::Cluster`], end to end, and where each one's
    /// is in it. Plain text has no clusters, so for most diagrams these stay
    /// empty and are never allocated.
    spill: String,
    clusters: Vec<std::ops::Range<usize>>,
}

/// Columns one grapheme cluster takes on screen: what ratatui gives it when
/// it draws the row, and none for a control character, which it leaves out.
///
/// That is what `unicode-width` measures the cluster at, as the layout
/// measures words, but for halfwidth katakana's sound marks, `ﾞ` and `ﾟ`,
/// which terminals give a column of their own.
#[must_use]
pub fn columns(cluster: &str) -> usize {
    let mut chars = cluster.chars();
    match (chars.next(), chars.next()) {
        (Some(glyph), None) => char_columns(glyph),
        _ if cluster.contains(char::is_control) => 0,
        _ => usize::from(cluster.cell_width()),
    }
}

/// [`columns`] for a cluster of one character, as most are, without
/// measuring it as a string.
fn char_columns(glyph: char) -> usize {
    match glyph {
        _ if glyph.is_control() => 0,
        // What ratatui gives them, where `unicode-width` gives them none.
        '\u{ff9e}' | '\u{ff9f}' => 1,
        _ => glyph.width().unwrap_or(0),
    }
}

/// Columns text takes on screen, a cluster at a time, as [`columns`] counts.
#[must_use]
pub fn text_columns(text: &str) -> usize {
    clusters(text).map(columns).sum()
}

/// The grapheme clusters of `text`, the pieces a terminal draws as one.
///
/// ASCII is a cluster to a character, but for a carriage return and line
/// feed, so it is split without the work of finding where clusters end,
/// which is most of the cost of drawing words.
pub fn clusters(text: &str) -> Clusters<'_> {
    if text.is_ascii() {
        Clusters::Ascii(text)
    } else {
        Clusters::Mixed(text.graphemes(true))
    }
}

/// See [`clusters`].
pub enum Clusters<'a> {
    Ascii(&'a str),
    Mixed(unicode_segmentation::Graphemes<'a>),
}

impl<'a> Iterator for Clusters<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        match self {
            Self::Ascii(rest) => {
                if rest.is_empty() {
                    return None;
                }
                let length = if rest.starts_with("\r\n") { 2 } else { 1 };
                let (cluster, after) = rest.split_at(length);
                *rest = after;
                Some(cluster)
            }
            Self::Mixed(graphemes) => graphemes.next(),
        }
    }
}

impl Canvas {
    /// A blank canvas, or `None` when it would be over [`MAX_CELLS`].
    pub fn new(width: usize, height: usize) -> Option<Self> {
        let size = width.checked_mul(height)?;
        if size > MAX_CELLS || width == 0 || height == 0 {
            return None;
        }
        Some(Self {
            width,
            height,
            cells: vec![Cell::Empty; size],
            spill: String::new(),
            clusters: Vec::new(),
        })
    }

    fn at(&mut self, x: usize, y: usize) -> Option<&mut Cell> {
        if x < self.width && y < self.height {
            self.cells.get_mut(y * self.width + x)
        } else {
            None
        }
    }

    /// Puts one character, whatever was there before.
    pub fn put(&mut self, x: usize, y: usize, glyph: char, ink: Ink) {
        self.place(x, y, Cell::Glyph(glyph, ink), char_columns(glyph));
    }

    /// Writes text from `x` rightwards, clipped at the edge. Returns the
    /// columns it took.
    ///
    /// Each grapheme cluster is kept whole, in the columns [`columns`] gives
    /// it, and one that would straddle the edge is left off with the rest.
    /// What takes no columns, a control character or marks with no letter
    /// before them to sit on, the terminal would draw nothing for, so it is
    /// passed over.
    pub fn text(&mut self, x: usize, y: usize, text: &str, ink: Ink) -> usize {
        let mut column = x;
        for cluster in clusters(text) {
            let wide = columns(cluster);
            if wide == 0 {
                continue;
            }
            if column.saturating_add(wide) > self.width {
                break;
            }
            if y < self.height {
                let mut chars = cluster.chars();
                let cell = match (chars.next(), chars.next()) {
                    (Some(glyph), None) => Some(Cell::Glyph(glyph, ink)),
                    _ => self.keep(cluster, ink),
                };
                if let Some(cell) = cell {
                    self.place(column, y, cell, wide);
                }
            }
            column += wide;
        }
        column - x
    }

    /// A cell for a cluster of more than one character, which is kept to
    /// one side, unless there are already more than a cell can count.
    fn keep(&mut self, cluster: &str, ink: Ink) -> Option<Cell> {
        let at = u32::try_from(self.clusters.len()).ok()?;
        let start = self.spill.len();
        self.spill.push_str(cluster);
        self.clusters.push(start..self.spill.len());
        Some(Cell::Cluster(at, ink))
    }

    /// Puts `cell` in the `wide` columns from `x`, whatever was there
    /// before, unless it would run off the edge.
    fn place(&mut self, x: usize, y: usize, cell: Cell, wide: usize) {
        if wide == 0 || y >= self.height || x.saturating_add(wide) > self.width {
            return;
        }
        self.unseat(x, y, wide);
        let row = y * self.width;
        self.cells[row + x] = cell;
        self.cells[row + x + 1..row + x + wide].fill(Cell::Wide);
    }

    /// Blanks what is left of any character or cluster that writing over
    /// the `wide` columns from `x` would cut in two, as a terminal does, so
    /// half of one never stays behind to push the rest of the row along.
    fn unseat(&mut self, x: usize, y: usize, wide: usize) {
        let row = &mut self.cells[y * self.width..(y + 1) * self.width];
        // One that starts to the left and runs into the first column.
        if row[x] == Cell::Wide {
            let start = row[..x]
                .iter()
                .rposition(|cell| *cell != Cell::Wide)
                .unwrap_or(0);
            row[start..x].fill(Cell::Empty);
        }
        // One that runs on past the last.
        for cell in row[x + wide..].iter_mut() {
            if *cell != Cell::Wide {
                break;
            }
            *cell = Cell::Empty;
        }
    }

    /// The character a cell holds, if it holds one rather than a line.
    pub fn glyph(&self, x: usize, y: usize) -> Option<char> {
        if x >= self.width || y >= self.height {
            return None;
        }
        match self.cells[y * self.width + x] {
            Cell::Glyph(glyph, _) => Some(glyph),
            _ => None,
        }
    }

    /// Whether a cell holds only a line running across it, which words can be
    /// written over without cutting a line that runs up or down through it.
    pub fn is_rule(&self, x: usize, y: usize) -> bool {
        x < self.width
            && y < self.height
            && matches!(self.cells[y * self.width + x], Cell::Line(bits, _, _) if bits & (UP | DOWN) == 0)
    }

    /// Adds line directions to a cell, unless it holds text or an outline.
    fn mark(&mut self, x: usize, y: usize, bits: u8, stroke: Stroke, owner: Option<u16>) {
        let Some(cell) = self.at(x, y) else { return };
        *cell = match *cell {
            Cell::Empty => Cell::Line(bits, stroke, owner),
            Cell::Line(old, was, under) => Cell::Line(
                old | bits,
                if was == stroke { was } else { Stroke::Solid },
                owner.or(under),
            ),
            other => other,
        };
    }

    /// A straight line between two cells in the same row or column, joined
    /// at both ends to whatever it meets there, in `owner`'s colour.
    pub fn line(
        &mut self,
        from: (usize, usize),
        to: (usize, usize),
        stroke: Stroke,
        owner: Option<u16>,
    ) {
        let ((x0, y0), (x1, y1)) = (from, to);
        if y0 == y1 {
            let (left, right) = (x0.min(x1), x0.max(x1));
            for x in left..=right {
                let mut bits = 0;
                if x > left {
                    bits |= LEFT;
                }
                if x < right {
                    bits |= RIGHT;
                }
                self.mark(x, y0, bits, stroke, owner);
            }
        } else if x0 == x1 {
            let (top, bottom) = (y0.min(y1), y0.max(y1));
            for y in top..=bottom {
                let mut bits = 0;
                if y > top {
                    bits |= UP;
                }
                if y < bottom {
                    bits |= DOWN;
                }
                self.mark(x0, y, bits, stroke, owner);
            }
        }
    }

    /// Lines through each point in turn.
    pub fn path(&mut self, points: &[(usize, usize)], stroke: Stroke, owner: Option<u16>) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], stroke, owner);
        }
    }

    /// A box outline, its corners chosen by `shape`.
    #[allow(clippy::too_many_arguments)]
    pub fn outline(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        corners: [char; 4],
        double: bool,
        ink: Ink,
    ) {
        if width < 2 || height < 2 {
            return;
        }
        let (across, down) = if double {
            ('═', '║')
        } else {
            ('─', '│')
        };
        let (right, bottom) = (x + width - 1, y + height - 1);
        for column in x + 1..right {
            self.put(column, y, across, ink);
            self.put(column, bottom, across, ink);
        }
        for row in y + 1..bottom {
            self.put(x, row, down, ink);
            self.put(right, row, down, ink);
        }
        self.put(x, y, corners[0], ink);
        self.put(right, y, corners[1], ink);
        self.put(x, bottom, corners[2], ink);
        self.put(right, bottom, corners[3], ink);
    }

    /// The finished picture, as rows of runs that share an ink, with the
    /// blank space at the ends of rows dropped. Its keys and links are left
    /// for the caller, which knows who the owners are.
    pub fn finish(self) -> Diagram {
        let mut rows = Vec::with_capacity(self.height);
        let mut widest = 0;
        let mut bytes = [0; 4];
        for row in self.cells.chunks(self.width) {
            let mut runs: Vec<(String, Ink)> = Vec::new();
            let mut columns = 0;
            let mut used = 0;
            for cell in row {
                let (text, ink): (&str, Ink) = match *cell {
                    Cell::Empty => (" ", Ink::plain(Role::Text)),
                    Cell::Wide => {
                        columns += 1;
                        used = columns;
                        continue;
                    }
                    Cell::Glyph(glyph, ink) => (glyph.encode_utf8(&mut bytes), ink),
                    Cell::Cluster(at, ink) => {
                        (&self.spill[self.clusters[at as usize].clone()], ink)
                    }
                    Cell::Line(bits, stroke, owner) => (
                        line_glyph(bits, stroke).encode_utf8(&mut bytes),
                        Ink {
                            role: Role::Line,
                            owner,
                        },
                    ),
                };
                columns += 1;
                let blank = text == " ";
                if !blank {
                    used = columns;
                }
                // Spaces join whatever run is open, so a row breaks into as
                // few spans as it can.
                match runs.last_mut() {
                    Some((run, last)) if *last == ink || blank => run.push_str(text),
                    _ => runs.push((text.to_string(), ink)),
                }
            }
            trim_end(&mut runs);
            widest = widest.max(used);
            rows.push(runs);
        }
        // A row is never wider than the diagram says, measured either the
        // way ratatui draws it or the way a line of text is measured, so the
        // copy mark beside it and the panning both line up.
        debug_assert!(
            rows.iter().all(|row| {
                let drawn: usize = row.iter().map(|(text, _)| text_columns(text)).sum();
                let measured: usize = row
                    .iter()
                    .map(|(text, _)| unicode_width::UnicodeWidthStr::width(text.as_str()))
                    .sum();
                drawn <= widest && measured <= widest
            }),
            "a row is wider than its diagram"
        );
        Diagram {
            rows,
            width: widest,
            keys: Vec::new(),
            links: Vec::new(),
        }
    }
}

fn trim_end(runs: &mut Vec<(String, Ink)>) {
    while let Some((text, _)) = runs.last_mut() {
        let kept = text.trim_end().len();
        if kept == 0 {
            runs.pop();
        } else {
            text.truncate(kept);
            break;
        }
    }
}

/// The box-drawing character for a cell lines leave in `bits` directions.
///
/// Edges turn on rounded corners, so they never read as the corner of a box.
#[must_use]
pub fn line_glyph(bits: u8, stroke: Stroke) -> char {
    let heavy = stroke == Stroke::Thick;
    let dotted = stroke == Stroke::Dotted;
    match bits {
        0 => ' ',
        b if b & (LEFT | RIGHT) == 0 => match stroke {
            Stroke::Solid => '│',
            Stroke::Dotted => '┆',
            Stroke::Thick => '┃',
        },
        b if b & (UP | DOWN) == 0 => match stroke {
            Stroke::Solid => '─',
            Stroke::Dotted => '┄',
            Stroke::Thick => '━',
        },
        _ if dotted || !heavy => match bits {
            b if b == DOWN | RIGHT => '╭',
            b if b == DOWN | LEFT => '╮',
            b if b == UP | RIGHT => '╰',
            b if b == UP | LEFT => '╯',
            b if b == UP | DOWN | RIGHT => '├',
            b if b == UP | DOWN | LEFT => '┤',
            b if b == LEFT | RIGHT | DOWN => '┬',
            b if b == LEFT | RIGHT | UP => '┴',
            _ => '┼',
        },
        _ => match bits {
            b if b == DOWN | RIGHT => '┏',
            b if b == DOWN | LEFT => '┓',
            b if b == UP | RIGHT => '┗',
            b if b == UP | LEFT => '┛',
            b if b == UP | DOWN | RIGHT => '┣',
            b if b == UP | DOWN | LEFT => '┫',
            b if b == LEFT | RIGHT | DOWN => '┳',
            b if b == LEFT | RIGHT | UP => '┻',
            _ => '╋',
        },
    }
}

#[cfg(test)]
mod tests {
    use unicode_width::UnicodeWidthStr;

    use super::*;

    fn plain(diagram: &Diagram) -> Vec<String> {
        diagram
            .rows
            .iter()
            .map(|row| row.iter().map(|(text, _)| text.as_str()).collect())
            .collect()
    }

    #[test]
    fn lines_that_meet_join_up() {
        let mut canvas = Canvas::new(5, 3).unwrap();
        canvas.line((0, 1), (4, 1), Stroke::Solid, None);
        canvas.line((2, 0), (2, 2), Stroke::Solid, None);
        canvas.path(&[(0, 0), (1, 0), (1, 1)], Stroke::Solid, None);
        assert_eq!(plain(&canvas.finish()), ["─╮│", "─┴┼──", "  │"]);
    }

    #[test]
    fn lines_never_draw_over_text() {
        let mut canvas = Canvas::new(5, 2).unwrap();
        canvas.text(1, 0, "ab", Ink::plain(Role::Text));
        canvas.line((0, 0), (4, 0), Stroke::Solid, None);
        // Nor over either half of a wide character.
        canvas.text(1, 1, "日", Ink::plain(Role::Text));
        canvas.line((0, 1), (4, 1), Stroke::Solid, None);
        assert_eq!(plain(&canvas.finish()), ["─ab──", "─日──"]);
    }

    #[test]
    fn where_lines_cross_the_last_one_drawn_keeps_its_colour() {
        let mut canvas = Canvas::new(3, 3).unwrap();
        canvas.line((1, 0), (1, 2), Stroke::Solid, Some(0));
        canvas.line((0, 1), (2, 1), Stroke::Solid, Some(1));
        // A line of no one's, like a `loop` block's rule, goes underneath.
        canvas.line((0, 1), (2, 1), Stroke::Dotted, None);
        let diagram = canvas.finish();
        let owners: Vec<Option<u16>> = diagram.rows[1].iter().map(|(_, ink)| ink.owner).collect();
        assert_eq!(owners, [Some(1)], "one run, all the crossing line's");
        let above = diagram.rows[0].last().map(|(_, ink)| ink.owner);
        assert_eq!(above, Some(Some(0)), "away from the crossing it is its own");
    }

    #[test]
    fn wide_characters_take_two_columns_and_clip_whole() {
        let mut canvas = Canvas::new(5, 1).unwrap();
        let used = canvas.text(0, 0, "日本語", Ink::plain(Role::Text));
        assert_eq!(
            used, 4,
            "the third would straddle the edge, so it is left off"
        );
        let diagram = canvas.finish();
        assert_eq!(plain(&diagram), ["日本"]);
        assert_eq!(diagram.width, 4);
    }

    #[test]
    fn oversized_canvases_are_refused() {
        assert!(Canvas::new(MAX_CELLS, 2).is_none());
        assert!(Canvas::new(usize::MAX, usize::MAX).is_none());
        assert!(Canvas::new(0, 4).is_none());
    }

    #[test]
    fn a_letter_keeps_the_marks_on_it() {
        // Thai's vowel and tone marks, Hindi's vowel signs and virama, and
        // Vietnamese accents written as marks after their letter rather than
        // as one character with it.
        for (word, wide) in [("ที่นี่", 2), ("हिन्दी", 5), ("Tie\u{302}\u{301}ng", 5)]
        {
            let mut canvas = Canvas::new(8, 1).unwrap();
            let used = canvas.text(0, 0, word, Ink::plain(Role::Text));
            assert_eq!(used, wide, "{word}");
            assert_eq!(used, word.width(), "{word} as the layout measures it");
            assert_eq!(plain(&canvas.finish()), [word]);
        }
    }

    #[test]
    fn joined_emoji_take_the_two_columns_they_are_measured_at() {
        let coder = "👨\u{200d}💻";
        let label = format!("{coder}{coder} Coding");
        let mut canvas = Canvas::new(20, 1).unwrap();
        let used = canvas.text(0, 0, &label, Ink::plain(Role::Text));
        assert_eq!(used, 11);
        assert_eq!(used, label.width(), "as the layout measures it");
        let diagram = canvas.finish();
        assert_eq!(plain(&diagram), [label]);
        assert_eq!(diagram.width, 11);

        // A flag is one cluster too, and clips whole.
        let mut canvas = Canvas::new(3, 1).unwrap();
        assert_eq!(canvas.text(0, 0, "🇯🇵🇺🇸", Ink::plain(Role::Text)), 2);
        assert_eq!(plain(&canvas.finish()), ["🇯🇵"]);
    }

    #[test]
    fn ascii_is_split_a_character_at_a_time_as_the_full_rules_would() {
        for text in ["", "plain", "a\r\nb\n\rc\t\u{7}", "\r\n\r\n", "x\r"] {
            let quick: Vec<&str> = clusters(text).collect();
            let full: Vec<&str> = text.graphemes(true).collect();
            assert_eq!(quick, full, "{text:?}");
        }
    }

    #[test]
    fn what_takes_no_room_is_left_out() {
        // A mark with no letter before it, a tab and a zero-width space.
        let mut canvas = Canvas::new(8, 1).unwrap();
        let used = canvas.text(0, 0, "\u{301}a\tb\u{200b}c", Ink::plain(Role::Text));
        assert_eq!(used, 3);
        assert_eq!(plain(&canvas.finish()), ["abc"]);
    }

    #[test]
    fn writing_over_half_a_wide_character_blanks_the_rest_of_it() {
        let ink = Ink::plain(Role::Text);
        // Over its right half, as a bar drawn up to a name would be.
        let mut canvas = Canvas::new(6, 1).unwrap();
        canvas.text(0, 0, "日本", ink);
        canvas.put(1, 0, '█', ink);
        let diagram = canvas.finish();
        assert_eq!(plain(&diagram), [" █本"]);
        assert_eq!(diagram.width, 4);
        // Over the left half of joined emoji.
        let mut canvas = Canvas::new(6, 1).unwrap();
        canvas.text(0, 0, "👨\u{200d}💻x", ink);
        canvas.text(0, 0, "a", ink);
        assert_eq!(plain(&canvas.finish()), ["a x"]);
    }

    #[test]
    fn a_cell_costs_no_more_than_a_character_and_its_ink() {
        assert_eq!(size_of::<Cell>(), size_of::<(char, Ink)>());
        let mut canvas = Canvas::new(40, 2).unwrap();
        canvas.text(0, 0, "Plain words, 日本語 and ✓", Ink::plain(Role::Text));
        assert_eq!(canvas.spill.capacity(), 0, "nothing is set aside");
        assert_eq!(canvas.clusters.capacity(), 0);
        canvas.text(0, 1, "ที่นี่", Ink::plain(Role::Text));
        assert_eq!(canvas.clusters.len(), 2, "but clusters");
    }

    /// `source` drawn `width` columns across.
    fn drawn(source: &str, width: usize) -> Diagram {
        let lines: Vec<String> = source.lines().map(str::to_string).collect();
        super::super::draw(&lines, width).unwrap_or_else(|| panic!("draws:\n{source}"))
    }

    /// A drawing with each cluster of words an `x` for every column it
    /// takes, so drawings of words that are the same width can be matched.
    fn shape(diagram: &Diagram) -> Vec<String> {
        plain(diagram)
            .iter()
            .map(|row| {
                clusters(row)
                    .map(|cluster| {
                        let drawing = cluster
                            .chars()
                            .all(|c| c == ' ' || ('\u{2500}'..='\u{25ff}').contains(&c));
                        if drawing {
                            cluster.to_string()
                        } else {
                            "x".repeat(columns(cluster))
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn words_keep_their_marks_in_every_kind() {
        for word in ["ที่นี่", "हिन्दी", "Tie\u{302}\u{301}ng"] {
            for source in [
                format!("graph LR\n  A[{word}] --> B"),
                format!("sequenceDiagram\n  A->>B: {word}"),
                format!("classDiagram\n  class A[\"{word}\"]"),
                format!("stateDiagram-v2\n  state \"{word}\" as A\n  A --> B"),
                format!("erDiagram\n  A ||--o{{ B : \"{word}\""),
                format!("C4Context\n  Person(a, \"{word}\")"),
                format!("pie\n  \"{word}\" : 1"),
                format!("gantt\n  dateFormat YYYY-MM-DD\n  {word} : 2024-01-01, 3d"),
                format!("xychart-beta\n  x-axis [\"{word}\", b]\n  bar [1, 2]"),
                format!("mindmap\n  root\n    {word}"),
                format!("timeline\n  {word} : e"),
                format!("journey\n  {word}: 3: Me"),
            ] {
                let text = plain(&drawn(&source, 80)).join("\n");
                assert!(text.contains(word), "{word} lost its marks in\n{text}");
            }
        }
    }

    /// Joined emoji measure two columns, so they must be drawn in two, or
    /// bars run over names, names over the outlines of their boxes, and a
    /// pie's shares off its edge.
    #[test]
    fn joined_emoji_are_laid_out_as_letters_as_wide_would_be() {
        let coder = "👨\u{200d}💻";
        let family = "👨\u{200d}👩\u{200d}👧";
        for (source, width) in [
            ("pie\n  \"WORD WORD Coding\" : 3\n  \"Other\" : 1", 80),
            ("pie\n  \"WORD WORD Coding\" : 3\n  \"Other\" : 1", 24),
            (
                "gantt\n  dateFormat YYYY-MM-DD\n  WORD WORD Code : 2024-01-01, 3d\n  Test : 2d",
                80,
            ),
            ("journey\n  WORD WORD Code: 3: Me", 80),
            ("classDiagram\n  class A[\"WORD x\"]\n  A --> B", 80),
            ("stateDiagram-v2\n  state \"WORD x\" as A\n  A --> B", 80),
            ("mindmap\n  root((WORD WORD))\n    child", 80),
            ("graph LR\n  A[\"WORD x\"] --> B", 80),
            ("timeline\n  WORD WORD : one\n  b : two", 80),
            ("xychart-beta\n  x-axis [\"WORDWORD\", b]\n  bar [1, 2]", 20),
        ] {
            let letters = drawn(&source.replace("WORD", "ab"), width);
            for emoji in [coder, family] {
                let with = drawn(&source.replace("WORD", emoji), width);
                assert_eq!(shape(&with), shape(&letters), "{source} with {emoji}");
                assert_eq!(with.width, letters.width);
                assert!(plain(&with).join("\n").contains(emoji), "drawn whole");
            }
        }
        let pie = drawn(
            &format!("pie\n  \"{coder}{coder} Coding\" : 3\n  \"Other\" : 1"),
            80,
        );
        let text = plain(&pie).join("\n");
        assert!(text.contains("75%") && text.contains("25%"), "{text}");
    }

    /// Words from many scripts, with every way a cluster is built, drawn in
    /// every kind at widths from cramped to roomy: no row may be wider than
    /// its diagram says, however it is measured, and a copy of it as text or
    /// as a picture comes out.
    #[test]
    fn no_row_is_wider_than_its_diagram_whatever_the_words() {
        const PIECES: &[&str] = &[
            "plain",
            "Ünïcödé",
            "日本語",
            "ｆｕｌｌ",
            "ที่นี่",
            "हिन्दी",
            "स्त्री",
            "Tie\u{302}\u{301}ng",
            "e\u{301}\u{301}\u{301}",
            "\u{301}lead",
            "x\u{338}\u{322}\u{31b}\u{317}",
            "a\u{20dd}",
            "👨\u{200d}💻",
            "👨\u{200d}👩\u{200d}👧\u{200d}👦",
            "❤\u{fe0f}",
            "☺\u{fe0f}",
            "1\u{fe0f}\u{20e3}",
            "👍🏽",
            "🏽",
            "🇯🇵",
            "🇯",
            "🏴\u{e0067}\u{e0062}\u{e0065}\u{e006e}\u{e0067}\u{e007f}",
            "السلام",
            "لا",
            "ｶﾞｷﾟ",
            "가",
            "\u{1100}\u{1161}\u{11a8}",
            "\u{1161}",
            "a\u{200b}b",
            "\u{200d}",
            "\u{fe0f}",
            "\t",
            "\u{7}",
            "\u{202e}rtl",
            "\u{ad}",
            "\u{3000}",
            "\u{600}5",
        ];
        const KINDS: &[&str] = &[
            "graph TD\n  A[\"WORD\"] -->|WORD| B(WORD)\n  B --> C{WORD}\n  subgraph S [WORD]\n    C\n  end",
            "graph LR\n  A[\"WORD\"] --> B[\"WORD\"] -.->|WORD| C",
            "sequenceDiagram\n  participant A as WORD\n  A->>B: WORD\n  loop WORD\n    B-->>A: WORD\n  end\n  Note over A,B: WORD",
            "C4Container\n  title WORD\n  Person(c, \"WORD\", \"WORD\")\n  Container_Boundary(b, \"WORD\") {\n    Container(w, \"WORD\", \"WORD\", \"WORD\")\n  }\n  Rel(c, w, \"WORD\", \"WORD\")",
            "classDiagram\n  class A[\"WORD\"] {\n    +WORD\n    +go(WORD) WORD\n  }\n  A <|-- B : WORD\n  note for A \"WORD\"",
            "stateDiagram-v2\n  [*] --> A\n  state \"WORD\" as A\n  A --> B : WORD\n  state B {\n    [*] --> C : WORD\n  }\n  note right of A : WORD",
            "erDiagram\n  A ||--o{ B : \"WORD\"\n  A {\n    string name PK \"WORD\"\n  }",
            "mindmap\n  root((WORD))\n    WORD\n      WORD\n    B[WORD]",
            "timeline\n  title WORD\n  section WORD\n    WORD : WORD : WORD",
            "journey\n  title WORD\n  section WORD\n    WORD: 3: WORD, Me",
            "pie showData title WORD\n  \"WORD\" : 3\n  \"WORD\" : 1",
            "pie\n  \"WORD\" : 1\n  \"b\" : 2\n  \"c\" : 3\n  \"d\" : 4\n  \"e\" : 5\n  \"f\" : 6",
            "gantt\n  dateFormat YYYY-MM-DD\n  title WORD\n  section WORD\n  WORD : a1, 2024-01-01, 3d\n  WORD : milestone, 2024-01-03, 0d",
            "xychart-beta\n  title \"WORD\"\n  x-axis \"WORD\" [\"WORD\", b]\n  y-axis \"WORD\" 0 --> 10\n  bar \"WORD\" [1, 2]\n  line [3, 4]",
            "xychart-beta horizontal\n  x-axis [\"WORD\", \"WORD\"]\n  bar [1, 2]",
        ];
        let palette = emeraldian_theme::Palette::from(&emeraldian_theme::presets::default_theme());
        let mut seed = 0x5eed_u64;
        let mut next = |below: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as usize % below
        };
        let (mut tried, mut drew) = (0, 0);
        for round in 0..24 {
            for kind in KINDS {
                let mut source = String::new();
                for (at, part) in kind.split("WORD").enumerate() {
                    if at > 0 {
                        for piece in 0..1 + next(4) {
                            if piece > 0 {
                                source.push_str(["", " ", "-"][next(3)]);
                            }
                            source.push_str(PIECES[next(PIECES.len())]);
                        }
                    }
                    source.push_str(part);
                }
                let lines: Vec<String> = source.lines().map(str::to_string).collect();
                for width in [8, 21, 40, 80, 200] {
                    tried += 1;
                    let Some(diagram) = super::super::draw(&lines, width) else {
                        continue;
                    };
                    drew += 1;
                    assert!(diagram.rows.len() * diagram.width <= MAX_CELLS);
                    for row in &diagram.rows {
                        let text: String = row.iter().map(|(text, _)| text.as_str()).collect();
                        let runs: usize = row.iter().map(|(text, _)| text_columns(text)).sum();
                        let wide = text_columns(&text).max(runs).max(text.width());
                        assert!(
                            wide <= diagram.width,
                            "a row {wide} wide in a diagram {} wide, at {width}:\n{text}\n{source}",
                            diagram.width,
                        );
                    }
                    let copied = super::super::export::text(&diagram);
                    assert_eq!(copied.lines().count(), diagram.rows.len());
                    if round % 6 == 0 && width == 40 {
                        super::super::export::picture(&diagram, &palette)
                            .unwrap_or_else(|why| panic!("{why}:\n{source}"));
                    }
                }
            }
        }
        assert!(drew * 10 > tried * 8, "only {drew} of {tried} drew");
    }
}
