//! A drawn diagram taken out of the app: as text, or as a picture in the
//! colours it has on screen.
//!
//! Both work from a [`Diagram`] alone, rows of runs each with its ink, so
//! every kind of diagram copies the same way, whatever drew it. Nothing here
//! runs until a diagram is copied: the reading pane only notes where each
//! diagram is and puts a mark at its top right.
//!
//! The picture is drawn on the theme's page, each run in the colour it is on
//! screen. Colours the terminal picks, the `terminal` theme's, have no value
//! the app can read, so they are given common ones (see [`rgb`]).

use std::rc::Rc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

use emeraldian_theme::Palette;

use super::picture::{self, Picture, Rgb};
use super::{Diagram, Ink};
use crate::app::Regions;
use crate::ui::note::DiagramInks;

/// The mark at the top right of each diagram that copies it.
pub const MARK: &str = "⧉";

/// The diagram as plain text: its rows, with nothing to say what colour
/// anything is.
#[must_use]
pub fn text(diagram: &Diagram) -> String {
    let rows: Vec<String> = diagram
        .rows
        .iter()
        .map(|row| {
            let line: String = row.iter().map(|(text, _)| text.as_str()).collect();
            line.trim_end().to_string()
        })
        .collect();
    rows.join("\n")
}

/// The diagram drawn as a PNG on the theme's page, in the colours the
/// reading pane gives it.
pub fn picture(diagram: &Diagram, palette: &Palette) -> Result<Picture, String> {
    let palette = in_rgb(palette);
    let inks = DiagramInks::new(&palette);
    let picked = diagram.colours(inks.hues.strokes.len());
    let paint = |ink: Ink| rgb(inks.colour(&palette, &picked, ink), palette.dark, false);
    picture::draw(diagram, &paint, rgb(palette.bg_primary, palette.dark, true))
}

/// `palette` with the colours a diagram is drawn in made RGB, so the theme's
/// own rules for picking readable colours can measure them.
fn in_rgb(palette: &Palette) -> Palette {
    let dark = palette.dark;
    let fix = |color: Color| {
        let [r, g, b] = rgb(color, dark, false);
        Color::Rgb(r, g, b)
    };
    let mut fixed = palette.clone();
    let [r, g, b] = rgb(palette.bg_primary, dark, true);
    fixed.bg_primary = Color::Rgb(r, g, b);
    for slot in [
        &mut fixed.text_normal,
        &mut fixed.text_muted,
        &mut fixed.callout_note,
        &mut fixed.callout_warning,
        &mut fixed.callout_success,
        &mut fixed.callout_tip,
        &mut fixed.callout_question,
        &mut fixed.callout_danger,
        &mut fixed.code_fg,
        &mut fixed.syn_keyword,
    ] {
        *slot = fix(*slot);
    }
    fixed
}

/// The RGB of a theme colour. A colour the terminal decides, a name like
/// `blue` or its own default, gets the value a common dark terminal gives it,
/// VS Code's, or the light one's for the default on a light theme.
#[must_use]
pub fn rgb(color: Color, dark: bool, page: bool) -> Rgb {
    const ANSI: [Rgb; 16] = [
        [0x00, 0x00, 0x00],
        [0xcd, 0x31, 0x31],
        [0x0d, 0xbc, 0x79],
        [0xe5, 0xe5, 0x10],
        [0x24, 0x72, 0xc8],
        [0xbc, 0x3f, 0xbc],
        [0x11, 0xa8, 0xcd],
        [0xe5, 0xe5, 0xe5],
        [0x66, 0x66, 0x66],
        [0xf1, 0x4c, 0x4c],
        [0x23, 0xd1, 0x8b],
        [0xf5, 0xf5, 0x43],
        [0x3b, 0x8e, 0xea],
        [0xd6, 0x70, 0xd6],
        [0x29, 0xb8, 0xdb],
        [0xff, 0xff, 0xff],
    ];
    let named = |at: usize| ANSI[at];
    match color {
        Color::Rgb(r, g, b) => [r, g, b],
        Color::Reset => match (dark, page) {
            (true, true) => [0x1e, 0x1e, 0x1e],
            (true, false) => [0xd4, 0xd4, 0xd4],
            (false, true) => [0xff, 0xff, 0xff],
            (false, false) => [0x1f, 0x1f, 0x1f],
        },
        Color::Black => named(0),
        Color::Red => named(1),
        Color::Green => named(2),
        Color::Yellow => named(3),
        Color::Blue => named(4),
        Color::Magenta => named(5),
        Color::Cyan => named(6),
        Color::Gray => named(7),
        Color::DarkGray => named(8),
        Color::LightRed => named(9),
        Color::LightGreen => named(10),
        Color::LightYellow => named(11),
        Color::LightBlue => named(12),
        Color::LightMagenta => named(13),
        Color::LightCyan => named(14),
        Color::White => named(15),
        Color::Indexed(at) => match at {
            0..16 => named(usize::from(at)),
            16..232 => {
                // The 6×6×6 cube.
                let step = |level: u8| if level == 0 { 0 } else { 55 + level * 40 };
                let at = at - 16;
                [step(at / 36), step(at / 6 % 6), step(at % 6)]
            }
            _ => {
                let grey = 8 + (at - 232) * 10;
                [grey; 3]
            }
        },
    }
}

/// Draws the copy mark at the top right of each diagram whose first row is
/// on screen, and notes for the mouse where each mark is, and for `y` which
/// diagrams are showing, top first.
///
/// `placed` are the note's diagrams, `scroll` its first row on screen,
/// `hscroll` how far wide blocks are panned, and `width` the columns they
/// were laid out in. A mark sits a column clear of its diagram's widest row,
/// or at the pane's edge when the diagram reaches it.
#[allow(clippy::too_many_arguments)]
pub fn marks(
    buffer: &mut Buffer,
    area: Rect,
    placed: &[Placed],
    scroll: usize,
    hscroll: usize,
    width: usize,
    style: Style,
    regions: &mut Regions,
) {
    let height = usize::from(area.height);
    let width = width.min(usize::from(area.width));
    for (at, diagram) in placed.iter().enumerate() {
        let (top, bottom) = (diagram.line, diagram.line + diagram.rows);
        if bottom <= scroll || top >= scroll + height || width < 2 {
            continue;
        }
        regions.diagrams.push(at);
        if top < scroll {
            continue;
        }
        let reach = diagram.keep + diagram.diagram.width;
        let panned = if reach > width {
            hscroll.min(reach - width)
        } else {
            0
        };
        let column = (reach + 1).saturating_sub(panned).clamp(1, width - 1);
        let (Ok(x), Ok(y)) = (u16::try_from(column), u16::try_from(top - scroll)) else {
            continue;
        };
        buffer.set_string(area.x + x, area.y + y, MARK, style);
        // The mark and the column before it: two are an easier target.
        regions.diagram_marks.push((
            Rect {
                x: area.x + x - 1,
                y: area.y + y,
                width: 2,
                height: 1,
            },
            at,
        ));
    }
}

/// A diagram laid out in the reading pane, kept so it can be copied.
#[derive(Debug, Clone)]
pub struct Placed {
    /// Index of its first row in the laid-out lines.
    pub line: usize,
    pub rows: usize,
    /// Columns on its left that don't pan: the indent, and the bars of any
    /// quote or callout it sits in.
    pub keep: usize,
    pub diagram: Rc<Diagram>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::mermaid::{Role, render};

    fn drawn(source: &str) -> Rc<Diagram> {
        let lines: Vec<String> = source.lines().map(str::to_string).collect();
        render(&lines, 120).expect("draws")
    }

    #[test]
    fn text_is_the_rows_as_drawn_without_colour() {
        let diagram = drawn("graph LR\n  A[Start] --> B[End]");
        let text = text(&diagram);
        assert_eq!(text.lines().count(), diagram.rows.len());
        assert!(text.contains("Start") && text.contains("End") && text.contains('▶'));
        assert!(!text.contains('\x1b'), "no escape codes");
        assert!(text.lines().all(|line| line == line.trim_end()));
        assert_eq!(text, crate::ui::mermaid::plain(&diagram));
    }

    #[test]
    fn the_picture_is_on_the_themes_page_in_the_screens_colours() {
        let diagram = drawn("graph LR\n  A[Start] --> B[End]");
        let theme = emeraldian_theme::presets::builtin_by_name("obsidian-light").expect("a theme");
        let palette = Palette::from(&theme);
        let picture = picture(&diagram, &palette).expect("draws");
        assert!(picture.png.starts_with(b"\x89PNG\r\n\x1a\n"));
        // The page in the corner, from the theme.
        let mut decoder = ::png::Decoder::new(std::io::Cursor::new(&picture.png));
        decoder.set_ignore_text_chunk(true);
        let mut reader = decoder.read_info().expect("decodes");
        let mut pixels = vec![0; reader.output_buffer_size().expect("fits")];
        reader.next_frame(&mut pixels).expect("decodes");
        assert_eq!(pixels[..3], rgb(palette.bg_primary, false, true));
        // The two boxes' colours, as the reading pane picks them.
        let inks = DiagramInks::new(&palette);
        let picked = diagram.colours(inks.hues.strokes.len());
        for owner in 0..2 {
            let colour = rgb(
                inks.colour(&palette, &picked, Ink::of(Role::Border, owner)),
                false,
                false,
            );
            assert!(
                pixels.as_chunks::<3>().0.contains(&colour),
                "box {owner} is outlined in {colour:?}"
            );
        }
    }

    #[test]
    fn colours_the_terminal_picks_are_given_values() {
        assert_eq!(rgb(Color::Rgb(1, 2, 3), true, false), [1, 2, 3]);
        assert_eq!(rgb(Color::Indexed(16), true, false), [0, 0, 0]);
        assert_eq!(rgb(Color::Indexed(231), true, false), [255, 255, 255]);
        assert_eq!(rgb(Color::Indexed(196), true, false), [255, 0, 0]);
        assert_eq!(rgb(Color::Indexed(244), true, false), [128, 128, 128]);
        assert_eq!(
            rgb(Color::Indexed(1), true, false),
            rgb(Color::Red, true, false)
        );
        assert_ne!(
            rgb(Color::Reset, true, true),
            rgb(Color::Reset, true, false)
        );
        assert_ne!(
            rgb(Color::Reset, true, true),
            rgb(Color::Reset, false, true)
        );

        // The terminal theme, all names and defaults, still draws in colour.
        let theme = emeraldian_theme::presets::builtin_by_name("terminal").expect("a theme");
        let diagram = drawn("graph TD\n  A --> B --> C");
        let picture = picture(&diagram, &Palette::from(&theme)).expect("draws");
        assert!(picture.width > 0 && picture.height > 0);
    }
}
