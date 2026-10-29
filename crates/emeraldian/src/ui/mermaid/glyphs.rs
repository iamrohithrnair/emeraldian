//! The letters a diagram copied as a picture is written in.
//!
//! JetBrains Mono, drawn ahead of time into a sheet of cells at the size the
//! picture uses, one cell a character, so the app needs no font parser or
//! rasteriser of its own: a picture's letters are copied out of the sheet.
//! The sheet is 78 KB of the binary, and is unpacked only while a picture is
//! being drawn. `scripts/glyph-atlas` made it, from the font's own release.
//!
//! The font is © 2020 The JetBrains Mono Project Authors, under the SIL Open
//! Font License 1.1, which is in `assets/JetBrainsMono-OFL.txt` and in the
//! sheet itself.

use std::io::Cursor;

/// One cell of the picture, in pixels: a terminal's at 15 pixels, twice over,
/// so a copy is sharp on a high-density screen and in a document.
pub const CELL_W: usize = 18;
pub const CELL_H: usize = 36;

/// Cells to a row of the sheet.
const COLUMNS: usize = 32;

/// The sheet: four bits of coverage a pixel, white on black.
const SHEET: &[u8] = include_bytes!("../../../assets/glyphs.png");

/// The characters in the sheet, in its order, which is theirs.
const CHARS: &str = include_str!("../../../assets/glyphs.txt");

/// The sheet, unpacked.
pub struct Glyphs {
    chars: Vec<char>,
    /// Two pixels a byte, the left one in the high bits.
    packed: Vec<u8>,
    /// Bytes to a row of `packed`.
    stride: usize,
}

impl Glyphs {
    /// Unpacks the sheet.
    pub fn load() -> Result<Self, String> {
        let broken = |err: png::DecodingError| format!("the glyph sheet is damaged: {err}");
        let mut decoder = png::Decoder::new(Cursor::new(SHEET));
        decoder.set_ignore_text_chunk(true);
        let mut reader = decoder.read_info().map_err(broken)?;
        let info = reader.info();
        if info.color_type != png::ColorType::Grayscale || info.bit_depth != png::BitDepth::Four {
            return Err("the glyph sheet is not four-bit grey".to_string());
        }
        let (width, height) = (info.width as usize, info.height as usize);
        let size = reader
            .output_buffer_size()
            .ok_or("the glyph sheet is too large")?;
        let mut packed = vec![0; size];
        let frame = reader.next_frame(&mut packed).map_err(broken)?;
        packed.truncate(frame.buffer_size());
        let chars: Vec<char> = CHARS.chars().collect();
        let fits = width == COLUMNS * CELL_W
            && height == chars.len().div_ceil(COLUMNS) * CELL_H
            && packed.len() == frame.line_size * height;
        if !fits {
            return Err("the glyph sheet doesn't match its list of characters".to_string());
        }
        Ok(Self {
            chars,
            packed,
            stride: frame.line_size,
        })
    }

    /// The character's cell, or `None` when the font hasn't got it.
    pub fn get(&self, c: char) -> Option<Glyph<'_>> {
        let at = self.chars.binary_search(&c).ok()?;
        Some(Glyph {
            glyphs: self,
            left: (at % COLUMNS) * CELL_W,
            top: (at / COLUMNS) * CELL_H,
        })
    }
}

/// One character's cell of the sheet.
#[derive(Clone, Copy)]
pub struct Glyph<'a> {
    glyphs: &'a Glyphs,
    left: usize,
    top: usize,
}

impl Glyph<'_> {
    /// How much of the pixel at `x`, `y` in the cell the letter covers, from
    /// 0 to 255.
    #[must_use]
    pub fn coverage(&self, x: usize, y: usize) -> u8 {
        if x >= CELL_W || y >= CELL_H {
            return 0;
        }
        let column = self.left + x;
        let byte = self.glyphs.packed[(self.top + y) * self.glyphs.stride + column / 2];
        let level = if column.is_multiple_of(2) {
            byte >> 4
        } else {
            byte & 0x0f
        };
        level * 17
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_unpacks_and_matches_its_list() {
        let glyphs = Glyphs::load().expect("loads");
        assert!(
            glyphs.chars.len() > 900,
            "Latin, Greek, Cyrillic and symbols"
        );
        assert!(
            glyphs.chars.windows(2).all(|pair| pair[0] < pair[1]),
            "in order, so a character is found by halving"
        );
        for c in ['A', 'g', 'é', 'Ж', 'λ', '→', '…', '≤', '€', '×'] {
            let glyph = glyphs.get(c).unwrap_or_else(|| panic!("{c} is there"));
            let inked = (0..CELL_H)
                .flat_map(|y| (0..CELL_W).map(move |x| (x, y)))
                .filter(|&(x, y)| glyph.coverage(x, y) > 0)
                .count();
            assert!(inked > 10, "{c} has ink");
        }
        assert!(glyphs.get(' ').is_none(), "a space has nothing to draw");
        assert!(glyphs.get('─').is_none(), "lines are drawn, not lettered");
        assert!(
            glyphs.get('漢').is_none(),
            "no Chinese in a font this small"
        );
    }

    #[test]
    fn a_letter_sits_on_its_baseline_inside_its_cell() {
        let glyphs = Glyphs::load().expect("loads");
        let rows = |c: char| {
            let glyph = glyphs.get(c).expect("there");
            let inked: Vec<usize> = (0..CELL_H)
                .filter(|&y| (0..CELL_W).any(|x| glyph.coverage(x, y) > 128))
                .collect();
            (inked[0], inked[inked.len() - 1])
        };
        let (top, bottom) = rows('H');
        let (_, under) = rows('g');
        assert!(top > 2 && bottom < CELL_H - 4, "{top}..{bottom}");
        assert!(under > bottom, "a descender goes below the line");
        // Full coverage is full: the four bits come back as 255.
        let solid = glyphs.get('H').expect("there");
        let most = (0..CELL_H)
            .flat_map(|y| (0..CELL_W).map(move |x| (x, y)))
            .map(|(x, y)| solid.coverage(x, y))
            .max();
        assert_eq!(most, Some(255));
        assert_eq!(solid.coverage(CELL_W, 0), 0, "nothing outside the cell");
    }
}
