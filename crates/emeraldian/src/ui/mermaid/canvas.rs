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
    Glyph(char, Ink),
    /// The right half of a character two columns wide.
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
        if let Some(cell) = self.at(x, y) {
            *cell = Cell::Glyph(glyph, ink);
        }
    }

    /// Writes text from `x` rightwards, clipped at the edge. Returns the
    /// columns it took.
    pub fn text(&mut self, x: usize, y: usize, text: &str, ink: Ink) -> usize {
        let mut column = x;
        for glyph in text.chars() {
            let Some(wide) = glyph.width() else { continue };
            if wide == 0 {
                continue;
            }
            if column + wide > self.width {
                break;
            }
            self.put(column, y, glyph, ink);
            if wide == 2
                && let Some(cell) = self.at(column + 1, y)
            {
                *cell = Cell::Wide;
            }
            column += wide;
        }
        column - x
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
        for row in self.cells.chunks(self.width) {
            let mut runs: Vec<(String, Ink)> = Vec::new();
            let mut columns = 0;
            let mut used = 0;
            for cell in row {
                let (glyph, ink) = match *cell {
                    Cell::Empty => (' ', Ink::plain(Role::Text)),
                    Cell::Wide => {
                        columns += 1;
                        used = columns;
                        continue;
                    }
                    Cell::Glyph(glyph, ink) => (glyph, ink),
                    Cell::Line(bits, stroke, owner) => (
                        line_glyph(bits, stroke),
                        Ink {
                            role: Role::Line,
                            owner,
                        },
                    ),
                };
                columns += 1;
                if glyph != ' ' {
                    used = columns;
                }
                // Spaces join whatever run is open, so a row breaks into as
                // few spans as it can.
                match runs.last_mut() {
                    Some((text, last)) if *last == ink || glyph == ' ' => text.push(glyph),
                    _ => runs.push((glyph.to_string(), ink)),
                }
            }
            trim_end(&mut runs);
            widest = widest.max(used);
            rows.push(runs);
        }
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
        let mut canvas = Canvas::new(5, 1).unwrap();
        canvas.text(1, 0, "ab", Ink::plain(Role::Text));
        canvas.line((0, 0), (4, 0), Stroke::Solid, None);
        assert_eq!(plain(&canvas.finish()), ["─ab──"]);
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
}
