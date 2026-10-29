//! Draws the glyph sheet a diagram copied as a picture is lettered from.
//!
//! Each character is drawn once, here, at the size the picture uses: white on
//! black in a cell of 18 by 36 pixels, the cell of a terminal at 15 pixels
//! drawn twice as large, with its baseline where a terminal would put it.
//! The cells are laid out 32 to a row, in the order of `glyphs.txt`, and kept
//! four bits deep, which is all the edge of a letter needs.
//!
//! The app then copies these cells rather than reading a font, so it carries
//! no font parser and no rasteriser, only this picture. Box-drawing
//! characters and block elements are left out: the app draws those as lines
//! and shapes of its own, so they join up from one cell to the next.

use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};

/// One cell, in pixels. Keep in step with `CELL_W` and `CELL_H` in
/// `crates/emeraldian/src/ui/mermaid/glyphs.rs`.
const CELL_W: usize = 18;
const CELL_H: usize = 36;
/// The font's size: JetBrains Mono's letters are 0.6 em wide, so 30 pixels
/// to the em makes them exactly one cell wide.
const EM: f32 = 30.0;
/// Cells to a row of the sheet.
const COLUMNS: usize = 32;

/// The characters drawn, where the font has them: Latin, Greek and Cyrillic,
/// with the punctuation, arrows, maths and symbols that turn up in a label.
const RANGES: &[(u32, u32)] = &[
    (0x0021, 0x007e),
    (0x00a1, 0x024f),
    (0x0370, 0x04ff),
    (0x1e00, 0x1eff),
    (0x2010, 0x205e),
    (0x20a0, 0x20c0),
    (0x2100, 0x23ff),
    (0x2460, 0x24ff),
    // Not 0x2500 to 0x259f: the app draws box drawing and blocks itself.
    (0x25a0, 0x27bf),
    (0x2b00, 0x2bff),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, font, out] = args.as_slice() else {
        eprintln!("usage: glyph-atlas <JetBrainsMono-Regular.ttf> <crates/emeraldian/assets>");
        std::process::exit(2);
    };
    let out = Path::new(out);
    let data = std::fs::read(font).expect("the font can be read");
    let font = FontRef::try_from_slice(&data).expect("the font parses");

    let per_em = font.units_per_em().expect("the font has units per em");
    let scale = PxScale::from(EM * font.height_unscaled() / per_em);
    let scaled = font.as_scaled(scale);
    assert!(
        (scaled.h_advance(font.glyph_id('M')) - CELL_W as f32).abs() < 0.01,
        "a letter is one cell wide"
    );
    // The font's line box centred in the cell, as a terminal centres it.
    let baseline = ((CELL_H as f32 - (scaled.ascent() - scaled.descent())) / 2.0
        + scaled.ascent())
    .round();

    let glyph = |c: char| {
        let id = font.glyph_id(c);
        (id.0 != 0)
            .then(|| font.outline_glyph(id.with_scale_and_position(scale, point(0.0, baseline))))
            .flatten()
    };
    let chars: Vec<char> = RANGES
        .iter()
        .flat_map(|&(first, last)| first..=last)
        .filter_map(char::from_u32)
        .filter(|&c| glyph(c).is_some())
        .collect();

    let width = COLUMNS * CELL_W;
    let height = chars.len().div_ceil(COLUMNS) * CELL_H;
    let mut sheet = vec![0u8; width * height];
    for (at, &c) in chars.iter().enumerate() {
        let (left, top) = ((at % COLUMNS) * CELL_W, (at / COLUMNS) * CELL_H);
        let outline = glyph(c).expect("kept because it has an outline");
        let bounds = outline.px_bounds();
        outline.draw(|x, y, coverage| {
            let x = bounds.min.x as i32 + x as i32;
            let y = bounds.min.y as i32 + y as i32;
            // What falls outside its cell is cut off, as a terminal cuts it.
            if (0..CELL_W as i32).contains(&x) && (0..CELL_H as i32).contains(&y) {
                let level = (coverage.clamp(0.0, 1.0) * 15.0).round() as u8;
                let pixel = &mut sheet[(top + y as usize) * width + left + x as usize];
                *pixel = (*pixel).max(level);
            }
        });
    }

    // Two pixels to a byte, the left one in the high bits.
    let packed: Vec<u8> = sheet
        .chunks(2)
        .map(|pair| pair[0] << 4 | pair.get(1).copied().unwrap_or(0))
        .collect();
    let licence = std::fs::read_to_string(out.join("JetBrainsMono-OFL.txt"))
        .expect("the font's licence is next to the sheet");
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Four);
        encoder.set_compression(png::Compression::High);
        encoder
            .add_text_chunk(
                "Title".into(),
                format!("JetBrains Mono Regular at {EM} px, in cells of {CELL_W} by {CELL_H}"),
            )
            .expect("text chunk");
        encoder
            .add_text_chunk(
                "Copyright".into(),
                "Copyright 2020 The JetBrains Mono Project Authors \
                 (https://github.com/JetBrains/JetBrainsMono)"
                    .into(),
            )
            .expect("text chunk");
        encoder
            .add_ztxt_chunk("License".into(), licence)
            .expect("text chunk");
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&packed).expect("pixels");
    }
    std::fs::write(out.join("glyphs.png"), &png).expect("the sheet is written");
    std::fs::write(out.join("glyphs.txt"), chars.iter().collect::<String>())
        .expect("the list is written");
    println!(
        "{} glyphs, {width}x{height} pixels, {} bytes",
        chars.len(),
        png.len()
    );
}
