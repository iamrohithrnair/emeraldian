//! A drawn diagram as a picture: each cell painted twice a terminal's size,
//! in its run's colour on the page's, and written out as a PNG.
//!
//! Lines, corners and arrowheads are drawn as shapes, not lettered from a
//! font, so they meet the cell beside them exactly, the way a terminal's own
//! box drawing does: a line runs from the middle of one edge of its cell to
//! the middle of another, and a corner, a junction or an arrowhead joins the
//! lines that come into it. Everything else is lettered from the glyph sheet,
//! and a character the sheet hasn't got, Chinese or an emoji, is an empty box
//! as wide as the character, so what follows it stays in its column. A letter
//! written with marks after it, rather than as one character, is lettered
//! without them, since nothing here shapes text.
//!
//! The picture is made a row of cells at a time, and each row is handed to
//! the PNG encoder as soon as it is painted, so drawing even a huge diagram
//! holds one strip of 36 rows of pixels rather than the whole picture.
//!
//! Drawing box-drawing characters to fill their cells so they join, rather
//! than as a font draws them, is an idea taken from prismantis (MIT).

use std::io::Write;

use super::canvas::{clusters, columns};
use super::glyphs::{CELL_H, CELL_W, Glyphs};
use super::{Diagram, Ink};

/// An sRGB colour.
pub type Rgb = [u8; 3];

/// Blank page around the drawing, in pixels.
const MARGIN: usize = 36;

/// The most pixels a picture may have, about 80 million: what the largest
/// diagram the canvas draws takes at 400 columns by 300 rows, with its
/// margin. Its memory is bounded anyway, a strip at a time, but not the time
/// it takes, about a second for one this size.
const MAX_PIXELS: usize = 81_000_000;

/// Pixels to the metre at 144 dots to the inch, twice a screen's 72, so an
/// app that reads it shows the picture at the size of the terminal it came
/// from, as a screenshot on a high-density screen is shown.
const DENSITY: u32 = 5669;

/// A diagram drawn and encoded.
#[derive(Debug)]
pub struct Picture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Draws `diagram` on `page`, each run in the colour `paint` gives its ink.
pub fn draw(diagram: &Diagram, paint: &dyn Fn(Ink) -> Rgb, page: Rgb) -> Result<Picture, String> {
    let cells = cells(diagram, paint);
    let columns = cells.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let (width, height) = (
        columns * CELL_W + 2 * MARGIN,
        cells.len().max(1) * CELL_H + 2 * MARGIN,
    );
    let too_big = || format!("it is too big, {columns} columns by {} rows", cells.len());
    if width.saturating_mul(height) > MAX_PIXELS {
        return Err(too_big());
    }
    let (Ok(png_width), Ok(png_height)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(too_big());
    };
    let glyphs = Glyphs::load()?;

    let failed = |err: ::png::EncodingError| format!("the picture couldn't be encoded: {err}");
    let mut png = Vec::new();
    let mut encoder = ::png::Encoder::new(&mut png, png_width, png_height);
    encoder.set_color(::png::ColorType::Rgb);
    encoder.set_depth(::png::BitDepth::Eight);
    // Balanced rather than fast: a third the size for a few milliseconds
    // more, and the encoded picture is most of what a copy holds in memory.
    encoder.set_compression(::png::Compression::Balanced);
    encoder.set_pixel_dims(Some(::png::PixelDimensions {
        xppu: DENSITY,
        yppu: DENSITY,
        unit: ::png::Unit::Meter,
    }));
    let mut writer = encoder.write_header().map_err(failed)?;
    let mut stream = writer.stream_writer().map_err(failed)?;

    let mut strip = Strip {
        pixels: Vec::with_capacity(width * CELL_H * 3),
        blank: page.repeat(width),
        width,
    };
    let write_err = |err: std::io::Error| format!("the picture couldn't be encoded: {err}");
    for _ in 0..MARGIN {
        stream.write_all(&strip.blank).map_err(write_err)?;
    }
    for (row, line) in cells.iter().enumerate() {
        strip.clear();
        for (column, cell) in line.iter().enumerate() {
            if cell.glyph == ' ' {
                continue;
            }
            let reach = reach(&cells, row, column);
            strip.cell(MARGIN + column * CELL_W, cell, reach, &glyphs);
        }
        for pixels in strip.pixels.chunks_exact(width * 3) {
            stream.write_all(pixels).map_err(write_err)?;
        }
    }
    for _ in 0..MARGIN {
        stream.write_all(&strip.blank).map_err(write_err)?;
    }
    stream.finish().map_err(failed)?;
    writer.finish().map_err(failed)?;
    Ok(Picture {
        png,
        width: png_width,
        height: png_height,
    })
}

/// One character of the diagram, in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    glyph: char,
    ink: Rgb,
    /// Columns it takes: more than one for a wide character or cluster,
    /// whose other columns are the blank cells after it.
    span: usize,
}

const BLANK: Cell = Cell {
    glyph: ' ',
    ink: [0; 3],
    span: 1,
};

/// The diagram as a grid of cells, each with its colour.
///
/// A row is taken a grapheme cluster at a time, each in the columns the
/// canvas gave it, so the picture's columns are the screen's. A cluster of
/// more than one character, a letter with marks on it or emoji joined into
/// one, is lettered as its first character: the sheet has no marks to put on
/// a letter and no emoji, so `é` written as `e` and an accent is drawn as
/// `e`, and `👨‍💻` is an empty box two columns wide, as `👨` alone would be.
fn cells(diagram: &Diagram, paint: &dyn Fn(Ink) -> Rgb) -> Vec<Vec<Cell>> {
    diagram
        .rows
        .iter()
        .map(|row| {
            let mut cells = Vec::with_capacity(diagram.width);
            for (text, ink) in row {
                let ink = paint(*ink);
                for cluster in clusters(text) {
                    // The canvas leaves out what takes no columns, and so does
                    // the terminal it is drawn in.
                    let span = columns(cluster);
                    if span == 0 {
                        continue;
                    }
                    let glyph = cluster.chars().next().unwrap_or(' ');
                    cells.push(Cell { glyph, ink, span });
                    cells.extend(std::iter::repeat_n(BLANK, span - 1));
                }
            }
            cells
        })
        .collect()
}

/// The lines the cells around `(row, column)` send into it: from above, the
/// right, below and the left, as [`arms`] weighs them.
fn reach(cells: &[Vec<Cell>], row: usize, column: usize) -> [u8; 4] {
    let at = |row: Option<usize>, column: Option<usize>| -> [u8; 4] {
        row.zip(column)
            .and_then(|(row, column)| cells.get(row)?.get(column))
            .map_or([0; 4], |cell| arms(cell.glyph))
    };
    [
        at(row.checked_sub(1), Some(column))[DOWN],
        at(Some(row), column.checked_add(1))[LEFT],
        at(row.checked_add(1), Some(column))[UP],
        at(Some(row), column.checked_sub(1))[RIGHT],
    ]
}

/// Indexes into an arm's weights.
const UP: usize = 0;
const RIGHT: usize = 1;
const DOWN: usize = 2;
const LEFT: usize = 3;

/// Weights of a line: none, light, heavy and double.
const NONE: u8 = 0;
const LIGHT: u8 = 1;
const HEAVY: u8 = 2;
const DOUBLE: u8 = 3;

/// The lines each box-drawing character from U+2500 sends out, up, right,
/// down and left, as `0` none, `1` light, `2` heavy or `3` double.
const BOX: &str = "\
    0101 0202 1010 2020 0101 0202 1010 2020 0101 0202 1010 2020 0110 0210 0120 0220 \
    0011 0012 0021 0022 1100 1200 2100 2200 1001 1002 2001 2002 1110 1210 2110 1120 \
    2120 2210 1220 2220 1011 1012 2011 1021 2021 2012 1022 2022 0111 0112 0211 0212 \
    0121 0122 0221 0222 1101 1102 1201 1202 2101 2102 2201 2202 1111 1112 1211 1212 \
    2111 1121 2121 2112 2211 1122 1221 2212 1222 2122 2221 2222 0101 0202 1010 2020 \
    0303 3030 0310 0130 0330 0013 0031 0033 1300 3100 3300 1003 3001 3003 1310 3130 \
    3330 1013 3031 3033 0313 0131 0333 1303 3101 3303 1313 3131 3333 0110 0011 1001 \
    1100 0000 0000 0000 0001 1000 0100 0010 0002 2000 0200 0020 0201 1020 0102 2010";

/// The lines a character sends out of its cell, up, right, down and left, by
/// weight. Nothing for anything that isn't a box-drawing character.
fn arms(glyph: char) -> [u8; 4] {
    let Some(at) = (glyph as usize).checked_sub(0x2500).filter(|&at| at < 0x80) else {
        return [NONE; 4];
    };
    let digits = &BOX.as_bytes()[at * 5..at * 5 + 4];
    [
        digits[0] - b'0',
        digits[1] - b'0',
        digits[2] - b'0',
        digits[3] - b'0',
    ]
}

/// How many dashes a dashed box-drawing character has, or `None` for one
/// that is drawn solid.
fn dashes(glyph: char) -> Option<usize> {
    match glyph {
        '┄'..='┇' => Some(3),
        '┈'..='┋' => Some(4),
        '╌'..='╏' => Some(2),
        _ => None,
    }
}

/// The middle of a cell, where its lines meet.
const MID_X: usize = CELL_W / 2;
const MID_Y: usize = CELL_H / 2;

/// Half the thickness of a light and of a heavy line; a double line is two
/// light ones, `RAIL` either side of the middle.
const HALF_LIGHT: usize = 1;
const HALF_HEAVY: usize = 2;
const RAIL: usize = 2;

/// Half a line of `weight`, or 0 for none or a double one.
fn half(weight: u8) -> usize {
    match weight {
        LIGHT => HALF_LIGHT,
        HEAVY => HALF_HEAVY,
        _ => 0,
    }
}

/// A row of cells being painted: `CELL_H` rows of pixels, the full width of
/// the picture.
struct Strip {
    pixels: Vec<u8>,
    /// One row of pixels of bare page.
    blank: Vec<u8>,
    width: usize,
}

impl Strip {
    fn clear(&mut self) {
        self.pixels.clear();
        for _ in 0..CELL_H {
            self.pixels.extend_from_slice(&self.blank);
        }
    }

    /// Mixes `ink` into the pixel at `x`, `y` by `alpha` of 255.
    fn blend(&mut self, x: usize, y: usize, ink: Rgb, alpha: u8) {
        if alpha == 0 || x >= self.width || y >= CELL_H {
            return;
        }
        let at = (y * self.width + x) * 3;
        let (ink_part, page_part) = (u16::from(alpha), 255 - u16::from(alpha));
        for (pixel, ink) in self.pixels[at..at + 3].iter_mut().zip(ink) {
            *pixel =
                ((u16::from(*pixel) * page_part + u16::from(ink) * ink_part + 127) / 255) as u8;
        }
    }

    /// Fills pixels `x0..x1` by `y0..y1` of the cell at `left`, clipped to
    /// the cell.
    fn fill(&mut self, left: usize, (x0, x1): (usize, usize), (y0, y1): (usize, usize), ink: Rgb) {
        for y in y0..y1.min(CELL_H) {
            for x in x0..x1.min(CELL_W) {
                self.blend(left + x, y, ink, 255);
            }
        }
    }

    /// Paints what `inside` says is inside, in the cell at `left` and the
    /// `span` cells from it, smoothing its edges by sampling each pixel
    /// sixteen times.
    fn shape(&mut self, left: usize, span: usize, ink: Rgb, inside: impl Fn(f32, f32) -> bool) {
        const AT: [f32; 4] = [0.125, 0.375, 0.625, 0.875];
        for y in 0..CELL_H {
            for x in 0..CELL_W * span {
                let mut hits = 0u16;
                for dy in AT {
                    for dx in AT {
                        hits += u16::from(inside(x as f32 + dx, y as f32 + dy));
                    }
                }
                self.blend(left + x, y, ink, (hits * 255 / 16) as u8);
            }
        }
    }

    /// Paints one cell, whose neighbours send it the lines in `reach`.
    fn cell(&mut self, left: usize, cell: &Cell, reach: [u8; 4], glyphs: &Glyphs) {
        let ink = cell.ink;
        let glyph = cell.glyph;
        match glyph {
            '╭' | '╮' | '╯' | '╰' => self.arc(left, arms(glyph), ink),
            '╱' | '╲' | '╳' => self.diagonals(left, glyph, reach, ink),
            '\u{2500}'..='\u{257f}' => match dashes(glyph) {
                Some(count) => self.dashed(left, arms(glyph), count, ink),
                None => self.lines(left, arms(glyph), ink),
            },
            '\u{2580}'..='\u{259f}' => self.block(left, glyph, ink),
            '\u{2800}'..='\u{28ff}' => self.braille(left, glyph, ink),
            _ => {
                if let Some(mark) = Mark::of(glyph) {
                    self.mark(left, mark, reach, ink);
                } else if let Some(letter) = glyphs.get(glyph) {
                    // A letter one column wide that the terminal gives two,
                    // as it does some symbols, sits in the middle of them.
                    let shift = (cell.span - 1) * CELL_W / 2;
                    for y in 0..CELL_H {
                        for x in 0..CELL_W {
                            self.blend(left + shift + x, y, ink, letter.coverage(x, y));
                        }
                    }
                } else {
                    self.missing(left, cell.span, ink);
                }
            }
        }
    }

    /// Straight lines from the middle of the cell to the middle of each side
    /// it has an arm on, joined in the middle the way each weight joins.
    fn lines(&mut self, left: usize, arms: [u8; 4], ink: Rgb) {
        for side in [UP, RIGHT, DOWN, LEFT] {
            let weight = arms[side];
            if weight == NONE {
                continue;
            }
            let across = side == RIGHT || side == LEFT;
            // The arms at right angles to this one, toward the start of the
            // cross axis and toward its end, and the arm opposite.
            let (before, after) = if across {
                (arms[UP], arms[DOWN])
            } else {
                (arms[LEFT], arms[RIGHT])
            };
            let opposite = arms[(side + 2) % 4];
            let (length, middle, cross_middle) = if across {
                (CELL_W, MID_X, MID_Y)
            } else {
                (CELL_H, MID_Y, MID_X)
            };
            // Whether the arm runs toward the start of its axis, up or left.
            let backward = side == UP || side == LEFT;
            // How far past the middle a rail or a line runs: negative stops
            // short of it.
            let span = |past: isize| -> (usize, usize) {
                let middle = middle as isize;
                if backward {
                    (0, (middle + past).clamp(0, length as isize) as usize)
                } else {
                    ((middle - past).clamp(0, length as isize) as usize, length)
                }
            };
            let mut paint = |along: (usize, usize), cross: (usize, usize)| {
                if across {
                    self.fill(left, along, cross, ink);
                } else {
                    self.fill(left, cross, along, ink);
                }
            };
            if weight == DOUBLE {
                // Two rails, each ending where it meets what is beside it:
                // the nearer rail of a double line, the far one round an
                // outside corner, or a single line.
                for (offset, beside, other) in [(-1, before, after), (1, after, before)] {
                    let past = match (beside, other) {
                        (DOUBLE, _) => -1,
                        (NONE, DOUBLE) => 3,
                        (NONE, NONE) => 1,
                        (NONE, single) | (single, _) => half(single) as isize,
                    };
                    let rail = cross_middle.wrapping_add_signed(offset * RAIL as isize);
                    paint(span(past), (rail - HALF_LIGHT, rail + HALF_LIGHT));
                }
            } else {
                let thickness = half(weight);
                let doubles = usize::from(before == DOUBLE) + usize::from(after == DOUBLE);
                let past = if doubles == 2 && opposite == NONE {
                    // Up to a double line that runs straight past, and no
                    // further.
                    -1
                } else if doubles > 0 {
                    // Across both rails of a double line it meets or crosses.
                    3
                } else {
                    // Far enough to cover the lines it meets.
                    thickness.max(half(before)).max(half(after)) as isize
                };
                paint(
                    span(past),
                    (cross_middle - thickness, cross_middle + thickness),
                );
            }
        }
    }

    /// A straight dashed line across or down the cell.
    fn dashed(&mut self, left: usize, arms: [u8; 4], count: usize, ink: Rgb) {
        let across = arms[LEFT] != NONE;
        let half = half(arms[LEFT].max(arms[UP]));
        let length = if across { CELL_W } else { CELL_H };
        for dash in 0..count {
            // Each dash takes three fifths of its share, centred in it.
            let start = (dash * 5 + 1) * length / (5 * count);
            let end = (dash * 5 + 4) * length / (5 * count);
            if across {
                self.fill(left, (start, end), (MID_Y - half, MID_Y + half), ink);
            } else {
                self.fill(left, (MID_X - half, MID_X + half), (start, end), ink);
            }
        }
    }

    /// A rounded corner: a quarter circle from the middle of one side, and
    /// a straight run on to the middle of the other.
    fn arc(&mut self, left: usize, arms: [u8; 4], ink: Rgb) {
        let radius = MID_X as f32;
        let (x, y) = (MID_X as f32, MID_Y as f32);
        let centre_x = if arms[RIGHT] != NONE {
            x + radius
        } else {
            x - radius
        };
        let centre_y = if arms[DOWN] != NONE {
            y + radius
        } else {
            y - radius
        };
        let half = HALF_LIGHT as f32;
        self.shape(left, 1, ink, |px, py| {
            let inward =
                (px - centre_x) * (x - centre_x) >= 0.0 && (py - centre_y) * (y - centre_y) >= 0.0;
            let distance = (px - centre_x).hypot(py - centre_y);
            inward && (distance - radius).abs() <= half
        });
        // The rest of the way down or up, past where the circle meets the
        // middle of the cell.
        let rows = if arms[DOWN] != NONE {
            (MID_Y + MID_X, CELL_H)
        } else {
            (0, MID_Y - MID_X)
        };
        self.fill(left, (MID_X - HALF_LIGHT, MID_X + HALF_LIGHT), rows, ink);
    }

    /// `╱`, `╲` and `╳`. Where lines come in from two sides, as at the
    /// corners of a diamond, a diagonal cuts across between them instead of
    /// running corner to corner, so the outline is closed.
    fn diagonals(&mut self, left: usize, glyph: char, reach: [u8; 4], ink: Rgb) {
        let (w, h) = (CELL_W as f32, CELL_H as f32);
        let (mx, my) = (MID_X as f32, MID_Y as f32);
        let comes = |side: usize| reach[side] != NONE;
        let mut lines: Vec<((f32, f32), (f32, f32))> = Vec::new();
        if glyph != '╲' {
            lines.push(
                match (comes(RIGHT) || comes(DOWN), comes(LEFT) || comes(UP)) {
                    (true, false) => ((w, my), (mx, h)),
                    (false, true) => ((0.0, my), (mx, 0.0)),
                    _ => ((0.0, h), (w, 0.0)),
                },
            );
        }
        if glyph != '╱' {
            lines.push(
                match (comes(LEFT) || comes(DOWN), comes(RIGHT) || comes(UP)) {
                    (true, false) => ((0.0, my), (mx, h)),
                    (false, true) => ((mx, 0.0), (w, my)),
                    _ => ((0.0, 0.0), (w, h)),
                },
            );
        }
        self.shape(left, 1, ink, |x, y| {
            lines
                .iter()
                .any(|&(from, to)| near_segment((x, y), from, to, HALF_LIGHT as f32 + 0.15))
        });
    }

    /// The block elements: parts of the cell filled, and the three shades.
    fn block(&mut self, left: usize, glyph: char, ink: Rgb) {
        let (w, h) = (CELL_W, CELL_H);
        let code = glyph as usize - 0x2580;
        match code {
            0x00 => self.fill(left, (0, w), (0, h / 2), ink),
            // Lower eighths, up to the full block.
            0x01..=0x08 => self.fill(left, (0, w), (h - code * h / 8, h), ink),
            // Left eighths, from seven down to one.
            0x09..=0x0f => self.fill(left, (0, (16 - code) * w / 8), (0, h), ink),
            0x10 => self.fill(left, (w / 2, w), (0, h), ink),
            // Light, medium and dark shade.
            0x11..=0x13 => {
                let alpha = (code - 0x10) as u8 * 64;
                for y in 0..h {
                    for x in 0..w {
                        self.blend(left + x, y, ink, alpha);
                    }
                }
            }
            0x14 => self.fill(left, (0, w), (0, h / 8), ink),
            0x15 => self.fill(left, (w - w / 8, w), (0, h), ink),
            _ => {
                // Quadrants, as bits: upper left, upper right, lower left,
                // lower right.
                const QUADRANTS: [u8; 10] = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14];
                let mask = QUADRANTS[code - 0x16];
                for (bit, x, y) in [
                    (1, (0, w / 2), (0, h / 2)),
                    (2, (w / 2, w), (0, h / 2)),
                    (4, (0, w / 2), (h / 2, h)),
                    (8, (w / 2, w), (h / 2, h)),
                ] {
                    if mask & bit != 0 {
                        self.fill(left, x, y, ink);
                    }
                }
            }
        }
    }

    /// A braille cell: up to eight dots, two across and four down.
    fn braille(&mut self, left: usize, glyph: char, ink: Rgb) {
        let dots = glyph as u32 - 0x2800;
        // Dots 1 to 8, as Unicode numbers their bits.
        const PLACES: [(usize, usize); 8] = [
            (0, 0),
            (0, 1),
            (0, 2),
            (1, 0),
            (1, 1),
            (1, 2),
            (0, 3),
            (1, 3),
        ];
        let centres: Vec<(f32, f32)> = PLACES
            .iter()
            .enumerate()
            .filter(|&(bit, _)| dots & (1 << bit) != 0)
            .map(|(_, &(column, row))| {
                (
                    (column * 2 + 1) as f32 * CELL_W as f32 / 4.0,
                    (row * 2 + 1) as f32 * CELL_H as f32 / 8.0,
                )
            })
            .collect();
        self.shape(left, 1, ink, |x, y| {
            centres
                .iter()
                .any(|&(cx, cy)| (x - cx).hypot(y - cy) <= 2.5)
        });
    }

    /// An arrowhead or a marker on a line, with the line drawn on up to it
    /// from whichever side one comes in.
    fn mark(&mut self, left: usize, mark: Mark, reach: [u8; 4], ink: Rgb) {
        let (mx, my) = (MID_X as f32, MID_Y as f32);
        // How far the mark reaches from the middle, up and down, and across.
        let (reach_y, reach_x) = match mark.shape {
            MarkShape::Triangle(UP | DOWN) => (mark.size * 0.43, mark.size * 0.5),
            MarkShape::Triangle(_) => (mark.size * 0.5, mark.size * 0.43),
            MarkShape::Square => (mark.size * 0.4, mark.size * 0.4),
            MarkShape::Diamond => (mark.size * 0.6, mark.size * 0.45),
            MarkShape::Circle { .. } => (mark.size * 0.4, mark.size * 0.4),
        };
        for side in [UP, RIGHT, DOWN, LEFT] {
            let weight = reach[side];
            if weight == NONE || weight == DOUBLE {
                continue;
            }
            let thickness = half(weight);
            let (column, row) = (
                (MID_X - thickness, MID_X + thickness),
                (MID_Y - thickness, MID_Y + thickness),
            );
            // Each runs a pixel into the mark, so they meet.
            let pixel = |at: f32| at.round() as usize;
            match side {
                UP => self.fill(left, column, (0, pixel(my - reach_y) + 1), ink),
                DOWN => self.fill(left, column, (pixel(my + reach_y) - 1, CELL_H), ink),
                LEFT => self.fill(left, (0, pixel(mx - reach_x) + 1), row, ink),
                _ => self.fill(left, (pixel(mx + reach_x) - 1, CELL_W), row, ink),
            }
        }
        let stroke = HALF_LIGHT as f32 + 0.1;
        match mark.shape {
            MarkShape::Triangle(pointing) => {
                let (tip, base_a, base_b) = match pointing {
                    UP => (
                        (mx, my - reach_y),
                        (mx - reach_x, my + reach_y),
                        (mx + reach_x, my + reach_y),
                    ),
                    DOWN => (
                        (mx, my + reach_y),
                        (mx - reach_x, my - reach_y),
                        (mx + reach_x, my - reach_y),
                    ),
                    LEFT => (
                        (mx - reach_x, my),
                        (mx + reach_x, my - reach_y),
                        (mx + reach_x, my + reach_y),
                    ),
                    _ => (
                        (mx + reach_x, my),
                        (mx - reach_x, my - reach_y),
                        (mx - reach_x, my + reach_y),
                    ),
                };
                let corners = [tip, base_a, base_b];
                self.shape(left, 1, ink, |x, y| {
                    if mark.filled {
                        in_triangle((x, y), corners)
                    } else {
                        (0..3).any(|at| {
                            near_segment((x, y), corners[at], corners[(at + 1) % 3], stroke)
                        })
                    }
                });
            }
            MarkShape::Square => self.shape(left, 1, ink, |x, y| {
                let (dx, dy) = ((x - mx).abs(), (y - my).abs());
                let inside = dx <= reach_x && dy <= reach_y;
                inside
                    && (mark.filled || dx >= reach_x - 2.0 * stroke || dy >= reach_y - 2.0 * stroke)
            }),
            MarkShape::Diamond => self.shape(left, 1, ink, |x, y| {
                let distance = (x - mx).abs() / reach_x + (y - my).abs() / reach_y;
                distance <= 1.0 && (mark.filled || distance >= 1.0 - 2.0 * stroke / reach_x)
            }),
            MarkShape::Circle { rings } => self.shape(left, 1, ink, |x, y| {
                let distance = (x - mx).hypot(y - my);
                let radius = reach_x;
                if mark.filled {
                    distance <= radius
                } else {
                    (distance - radius + stroke).abs() <= stroke
                        || (rings == 2 && (distance - radius * 0.5).abs() <= stroke)
                        || (rings == 0 && distance <= radius * 0.45)
                }
            }),
        }
    }

    /// An empty box where the sheet has no letter: as wide as the character,
    /// so the rest of the row stays where it was.
    fn missing(&mut self, left: usize, span: usize, ink: Rgb) {
        let (x0, x1) = (3, CELL_W * span - 3);
        let (y0, y1) = (MID_Y - 11, MID_Y + 11);
        let line = 2;
        for y in y0..y1 {
            for x in x0..x1 {
                let edge = x < x0 + line || x >= x1 - line || y < y0 + line || y >= y1 - line;
                if edge {
                    self.blend(left + x, y, ink, 160);
                }
            }
        }
    }
}

/// The shape of an arrowhead or a marker.
#[derive(Debug, Clone, Copy, PartialEq)]
enum MarkShape {
    /// Pointing up, right, down or left.
    Triangle(usize),
    Square,
    Diamond,
    /// An outline only, with 1 ring, 2 rings, or 0: a ring round a dot.
    Circle {
        rings: u8,
    },
}

/// An arrowhead or marker, and how big and how solid.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mark {
    shape: MarkShape,
    filled: bool,
    /// Across, in pixels.
    size: f32,
}

impl Mark {
    fn of(glyph: char) -> Option<Self> {
        use MarkShape::{Circle, Diamond, Square, Triangle};
        let (big, small) = (CELL_W as f32 - 2.0, CELL_W as f32 - 8.0);
        let (shape, filled, size) = match glyph {
            '▲' => (Triangle(UP), true, big),
            '▶' | '►' => (Triangle(RIGHT), true, big),
            '▼' => (Triangle(DOWN), true, big),
            '◀' | '◄' => (Triangle(LEFT), true, big),
            '△' => (Triangle(UP), false, big),
            '▷' | '▻' => (Triangle(RIGHT), false, big),
            '▽' => (Triangle(DOWN), false, big),
            '◁' | '◅' => (Triangle(LEFT), false, big),
            '▴' => (Triangle(UP), true, small),
            '▸' => (Triangle(RIGHT), true, small),
            '▾' => (Triangle(DOWN), true, small),
            '◂' => (Triangle(LEFT), true, small),
            '▵' => (Triangle(UP), false, small),
            '▹' => (Triangle(RIGHT), false, small),
            '▿' => (Triangle(DOWN), false, small),
            '◃' => (Triangle(LEFT), false, small),
            '■' => (Square, true, big),
            '□' => (Square, false, big),
            '▪' => (Square, true, small),
            '▫' => (Square, false, small),
            '◆' => (Diamond, true, big),
            '◇' => (Diamond, false, big),
            '●' => (Circle { rings: 1 }, true, big),
            '○' => (Circle { rings: 1 }, false, big),
            '◎' => (Circle { rings: 2 }, false, big),
            '◉' => (Circle { rings: 0 }, false, big),
            '•' => (Circle { rings: 1 }, true, small),
            _ => return None,
        };
        Some(Self {
            shape,
            filled,
            size,
        })
    }
}

/// Whether `point` is within `reach` of the segment from `a` to `b`.
fn near_segment(point: (f32, f32), a: (f32, f32), b: (f32, f32), reach: f32) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 {
        0.0
    } else {
        (((point.0 - a.0) * dx + (point.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (nx, ny) = (a.0 + t * dx, a.1 + t * dy);
    (point.0 - nx).hypot(point.1 - ny) <= reach
}

/// Whether `point` is inside the triangle with these corners.
fn in_triangle(point: (f32, f32), [a, b, c]: [(f32, f32); 3]) -> bool {
    let side = |p: (f32, f32), q: (f32, f32)| {
        (q.0 - p.0) * (point.1 - p.1) - (q.1 - p.1) * (point.0 - p.0)
    };
    let (ab, bc, ca) = (side(a, b), side(b, c), side(c, a));
    (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::super::canvas::{
        DOWN as CANVAS_DOWN, LEFT as CANVAS_LEFT, RIGHT as CANVAS_RIGHT, Stroke, UP as CANVAS_UP,
        line_glyph,
    };
    use super::super::{Ink, Role};
    use super::*;

    const PAGE: Rgb = [30, 30, 30];
    const RED: Rgb = [220, 50, 47];
    const BLUE: Rgb = [38, 139, 210];
    const WHITE: Rgb = [230, 230, 230];

    /// A picture decoded back to its pixels, to look at.
    struct Decoded {
        width: usize,
        height: usize,
        pixels: Vec<u8>,
    }

    impl Decoded {
        fn of(picture: &Picture) -> Self {
            let mut decoder = ::png::Decoder::new(std::io::Cursor::new(&picture.png));
            decoder.set_ignore_text_chunk(true);
            let mut reader = decoder.read_info().expect("a PNG");
            let info = reader.info();
            assert_eq!(info.color_type, ::png::ColorType::Rgb);
            assert_eq!(info.bit_depth, ::png::BitDepth::Eight);
            assert_eq!(
                info.pixel_dims.map(|dims| (dims.xppu, dims.yppu)),
                Some((DENSITY, DENSITY)),
                "marked as twice a screen's density"
            );
            let mut pixels = vec![0; reader.output_buffer_size().expect("fits")];
            let frame = reader.next_frame(&mut pixels).expect("decodes");
            Self {
                width: frame.width as usize,
                height: frame.height as usize,
                pixels,
            }
        }

        fn at(&self, x: usize, y: usize) -> Rgb {
            let at = (y * self.width + x) * 3;
            [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
        }

        /// Every pixel of the cell at `column`, `row`.
        fn cell(&self, column: usize, row: usize) -> Vec<Rgb> {
            let (left, top) = (MARGIN + column * CELL_W, MARGIN + row * CELL_H);
            (top..top + CELL_H)
                .flat_map(|y| (left..left + CELL_W).map(move |x| (x, y)))
                .map(|(x, y)| self.at(x, y))
                .collect()
        }

        /// The colours in a cell, other than the page.
        fn inks(&self, column: usize, row: usize) -> Vec<Rgb> {
            let mut inks: Vec<Rgb> = self
                .cell(column, row)
                .into_iter()
                .filter(|&p| p != PAGE)
                .collect();
            inks.sort_unstable();
            inks.dedup();
            inks
        }
    }

    fn diagram(rows: &[&[(&str, Option<u16>, Role)]]) -> Diagram {
        let rows: Vec<Vec<(String, Ink)>> = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&(text, owner, role)| (text.to_string(), Ink { role, owner }))
                    .collect()
            })
            .collect();
        let width = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(text, _)| unicode_width::UnicodeWidthStr::width(text.as_str()))
                    .sum()
            })
            .max()
            .unwrap_or(0);
        Diagram {
            rows,
            width,
            keys: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Owner 0 is red, owner 1 blue, and what is no one's white.
    fn paint(ink: Ink) -> Rgb {
        match ink.owner {
            Some(0) => RED,
            Some(_) => BLUE,
            None => WHITE,
        }
    }

    #[test]
    fn each_cell_is_painted_in_its_own_colour_and_nothing_else() {
        let drawn = diagram(&[
            &[
                ("╭─╮", Some(0), Role::Border),
                ("  ", None, Role::Text),
                ("──▶", Some(1), Role::Line),
            ],
            &[
                ("│", Some(0), Role::Border),
                ("A", None, Role::Text),
                ("│", Some(0), Role::Border),
            ],
            &[("╰─╯", Some(0), Role::Border)],
        ]);
        let picture = draw(&drawn, &paint, PAGE).expect("draws");
        let decoded = Decoded::of(&picture);
        assert_eq!(
            (decoded.width, decoded.height),
            (8 * CELL_W + 2 * MARGIN, 3 * CELL_H + 2 * MARGIN)
        );
        assert_eq!(
            (picture.width as usize, picture.height as usize),
            (decoded.width, decoded.height)
        );

        // The margin and the gap between the box and the arrow are bare page.
        assert_eq!(decoded.at(0, 0), PAGE);
        assert_eq!(decoded.at(decoded.width - 1, decoded.height - 1), PAGE);
        assert!(decoded.inks(3, 0).is_empty() && decoded.inks(4, 0).is_empty());
        // The box is red, through and through.
        assert_eq!(
            decoded.inks(1, 0),
            [RED],
            "a straight run is exactly its colour"
        );
        for (column, row) in [(0, 0), (2, 0), (0, 1), (2, 1), (0, 2), (2, 2)] {
            let inks = decoded.inks(column, row);
            assert!(inks.contains(&RED), "({column}, {row}) is outlined");
            assert!(!inks.contains(&BLUE) && !inks.contains(&WHITE));
        }
        // The arrow is blue, and so is its head.
        assert_eq!(decoded.inks(5, 0), [BLUE]);
        assert!(decoded.inks(7, 0).contains(&BLUE));
        // The letter is the plain colour, anti-aliased against the page.
        let letter = decoded.inks(1, 1);
        assert!(letter.contains(&WHITE), "{letter:?}");
        assert!(letter.len() > 2, "with softened edges");
        assert!(!letter.contains(&RED));
    }

    #[test]
    fn lines_meet_at_the_middle_of_each_edge_so_neighbours_join() {
        let drawn = diagram(&[
            &[("─┬─", Some(0), Role::Line)],
            &[(" │ ", Some(0), Role::Line)],
        ]);
        let decoded = Decoded::of(&draw(&drawn, &paint, PAGE).expect("draws"));
        let middle = MARGIN + MID_Y;
        // Along the middle row, the line runs unbroken from the first cell's
        // left edge to the last one's right edge.
        for x in MARGIN..MARGIN + 3 * CELL_W {
            assert_eq!(decoded.at(x, middle), RED, "x = {x}");
        }
        // And down the middle column, from the junction to the bottom.
        let column = MARGIN + CELL_W + MID_X;
        for y in middle..MARGIN + 2 * CELL_H {
            assert_eq!(decoded.at(column, y), RED, "y = {y}");
        }
        // Nothing above the junction's arm.
        assert_eq!(decoded.at(column, MARGIN + 2), PAGE);
    }

    #[test]
    fn every_line_the_canvas_draws_comes_apart_into_the_arms_it_was_drawn_with() {
        for bits in 1..16u8 {
            for stroke in [Stroke::Solid, Stroke::Dotted, Stroke::Thick] {
                let glyph = line_glyph(bits, stroke);
                let has = arms(glyph);
                for (side, bit) in [
                    (UP, CANVAS_UP),
                    (RIGHT, CANVAS_RIGHT),
                    (DOWN, CANVAS_DOWN),
                    (LEFT, CANVAS_LEFT),
                ] {
                    // A lone arm is drawn as a straight line through.
                    let lone = bits.count_ones() == 1;
                    if !lone {
                        assert_eq!(has[side] != NONE, bits & bit != 0, "{glyph} for {bits:04b}");
                    }
                }
                assert!(has.iter().any(|&weight| weight != NONE), "{glyph}");
            }
        }
        assert_eq!(arms('═'), [0, 3, 0, 3]);
        assert_eq!(arms('╔'), [0, 3, 3, 0]);
        assert_eq!(arms('┿'), [1, 2, 1, 2]);
        assert_eq!(arms('╿'), [2, 0, 1, 0]);
        assert_eq!(arms('A'), [0; 4]);
    }

    #[test]
    fn a_wide_character_without_a_letter_is_a_box_two_columns_wide() {
        let drawn = diagram(&[&[("漢x", None, Role::Text)]]);
        let decoded = Decoded::of(&draw(&drawn, &paint, PAGE).expect("draws"));
        assert!(
            !decoded.inks(0, 0).is_empty() && !decoded.inks(1, 0).is_empty(),
            "both halves"
        );
        // The `x` after it is in the third column, where the terminal puts it.
        assert!(!decoded.inks(2, 0).is_empty());
        assert_eq!(decoded.width, 3 * CELL_W + 2 * MARGIN);
    }

    #[test]
    fn a_cluster_is_lettered_as_its_first_character_in_the_columns_it_takes() {
        let picture = |text: &str| {
            let drawn = diagram(&[&[(text, None, Role::Text)]]);
            Decoded::of(&draw(&drawn, &paint, PAGE).expect("draws"))
        };
        // An accent written after its letter is left off: the sheet has no
        // marks to put on one.
        let bare = picture("ex");
        let marked = picture("e\u{301}x");
        assert_eq!(marked.width, bare.width);
        assert_eq!(marked.cell(0, 0), bare.cell(0, 0));
        assert_eq!(marked.cell(1, 0), bare.cell(1, 0));
        // Emoji joined into one are one box as wide as the screen draws
        // them, so the `x` after them is in the third column, as on screen.
        let joined = picture("👨\u{200d}💻x");
        assert_eq!(joined.width, 3 * CELL_W + 2 * MARGIN);
        assert!(!joined.inks(0, 0).is_empty() && !joined.inks(1, 0).is_empty());
        assert_eq!(joined.cell(2, 0), bare.cell(1, 0));
    }

    #[test]
    fn a_diagram_too_big_for_a_picture_is_refused() {
        let row = "─".repeat(5000);
        let rows: Vec<&[(&str, Option<u16>, Role)]> = Vec::new();
        let mut drawn = diagram(&rows);
        drawn.rows = vec![vec![(row, Ink::plain(Role::Line))]; 1000];
        drawn.width = 5000;
        let refused = draw(&drawn, &paint, PAGE).expect_err("too big");
        assert!(refused.contains("too big"), "{refused}");
    }
}
