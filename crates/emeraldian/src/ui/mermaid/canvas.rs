//! A grid of terminal cells that diagrams are drawn on.
//!
//! Lines are kept as the directions they leave each cell in, not as
//! characters, so two edges that meet or cross join up into the right
//! box-drawing piece whatever order they were drawn in. Text and box outlines
//! are plain characters, and a line never draws over one.

use unicode_width::UnicodeWidthChar;

use super::{Diagram, Role};

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
    Glyph(char, Role),
    /// The right half of a character two columns wide.
    Wide,
    Line(u8, Stroke),
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
    pub fn put(&mut self, x: usize, y: usize, glyph: char, role: Role) {
        if let Some(cell) = self.at(x, y) {
            *cell = Cell::Glyph(glyph, role);
        }
    }

    /// Writes text from `x` rightwards, clipped at the edge. Returns the
    /// columns it took.
    pub fn text(&mut self, x: usize, y: usize, text: &str, role: Role) -> usize {
        let mut column = x;
        for glyph in text.chars() {
            let Some(wide) = glyph.width() else { continue };
            if wide == 0 {
                continue;
            }
            if column + wide > self.width {
                break;
            }
            self.put(column, y, glyph, role);
            if wide == 2
                && let Some(cell) = self.at(column + 1, y)
            {
                *cell = Cell::Wide;
            }
            column += wide;
        }
        column - x
    }

    /// Adds line directions to a cell, unless it holds text or an outline.
    fn mark(&mut self, x: usize, y: usize, bits: u8, stroke: Stroke) {
        let Some(cell) = self.at(x, y) else { return };
        *cell = match *cell {
            Cell::Empty => Cell::Line(bits, stroke),
            Cell::Line(old, was) => {
                Cell::Line(old | bits, if was == stroke { was } else { Stroke::Solid })
            }
            other => other,
        };
    }

    /// A straight line between two cells in the same row or column, joined
    /// at both ends to whatever it meets there.
    pub fn line(&mut self, from: (usize, usize), to: (usize, usize), stroke: Stroke) {
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
                self.mark(x, y0, bits, stroke);
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
                self.mark(x0, y, bits, stroke);
            }
        }
    }

    /// Lines through each point in turn.
    pub fn path(&mut self, points: &[(usize, usize)], stroke: Stroke) {
        for pair in points.windows(2) {
            self.line(pair[0], pair[1], stroke);
        }
    }

    /// A box outline, its corners chosen by `shape`.
    pub fn outline(
        &mut self,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
        corners: [char; 4],
        double: bool,
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
            self.put(column, y, across, Role::Border);
            self.put(column, bottom, across, Role::Border);
        }
        for row in y + 1..bottom {
            self.put(x, row, down, Role::Border);
            self.put(right, row, down, Role::Border);
        }
        self.put(x, y, corners[0], Role::Border);
        self.put(right, y, corners[1], Role::Border);
        self.put(x, bottom, corners[2], Role::Border);
        self.put(right, bottom, corners[3], Role::Border);
    }

    /// The finished picture, as rows of runs that share a role, with the
    /// blank space at the ends of rows dropped.
    pub fn finish(self) -> Diagram {
        let mut rows = Vec::with_capacity(self.height);
        let mut widest = 0;
        for row in self.cells.chunks(self.width) {
            let mut runs: Vec<(String, Role)> = Vec::new();
            let mut columns = 0;
            let mut used = 0;
            for cell in row {
                let (glyph, role) = match *cell {
                    Cell::Empty => (' ', Role::Text),
                    Cell::Wide => {
                        columns += 1;
                        used = columns;
                        continue;
                    }
                    Cell::Glyph(glyph, role) => (glyph, role),
                    Cell::Line(bits, stroke) => (line_glyph(bits, stroke), Role::Line),
                };
                columns += 1;
                if glyph != ' ' {
                    used = columns;
                }
                // Spaces join whatever run is open, so a row breaks into as
                // few spans as it can.
                match runs.last_mut() {
                    Some((text, last)) if *last == role || glyph == ' ' => text.push(glyph),
                    _ => runs.push((glyph.to_string(), role)),
                }
            }
            trim_end(&mut runs);
            widest = widest.max(used);
            rows.push(runs);
        }
        Diagram {
            rows,
            width: widest,
        }
    }
}

fn trim_end(runs: &mut Vec<(String, Role)>) {
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
        canvas.line((0, 1), (4, 1), Stroke::Solid);
        canvas.line((2, 0), (2, 2), Stroke::Solid);
        canvas.path(&[(0, 0), (1, 0), (1, 1)], Stroke::Solid);
        assert_eq!(plain(&canvas.finish()), ["─╮│", "─┴┼──", "  │"]);
    }

    #[test]
    fn lines_never_draw_over_text() {
        let mut canvas = Canvas::new(5, 1).unwrap();
        canvas.text(1, 0, "ab", Role::Text);
        canvas.line((0, 0), (4, 0), Stroke::Solid);
        assert_eq!(plain(&canvas.finish()), ["─ab──"]);
    }

    #[test]
    fn wide_characters_take_two_columns_and_clip_whole() {
        let mut canvas = Canvas::new(5, 1).unwrap();
        let used = canvas.text(0, 0, "日本語", Role::Text);
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
