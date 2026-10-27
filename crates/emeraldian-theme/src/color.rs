//! Color string parsing.
//!
//! Theme slots are stored as strings so a user can drop a TOML theme into their
//! config without emeraldian needing to know about it at compile time. A slot
//! is one of:
//!
//! - a hex color, `#rrggbb` or the short `#rgb` form
//! - one of the terminal's own ANSI color names (`red`, `brightblue`, `gray`, …)
//! - `reset`, meaning "inherit whatever the terminal itself uses"
//!
//! `reset` is what makes the `terminal` theme work: rather than imposing a
//! palette, it defers to the colors the user already configured in their
//! terminal emulator, so emeraldian never clashes with the rest of their setup.
//!
//! It also measures colours: how much they contrast, the way WCAG does, and
//! how different they look, in OKLab, which is what keeps colours picked from
//! a theme readable on its page.

use ratatui::style::Color;

/// Parses a theme color string. Anything unrecognized resolves to
/// [`Color::Reset`] rather than panicking, so a typo in a user's theme file
/// degrades to "terminal default" instead of taking the app down.
pub fn parse(s: &str) -> Color {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex).unwrap_or(Color::Reset);
    }

    match s.to_ascii_lowercase().as_str() {
        "reset" | "none" | "" => Color::Reset,
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" | "brightred" => Color::LightRed,
        "lightgreen" | "brightgreen" => Color::LightGreen,
        "lightyellow" | "brightyellow" => Color::LightYellow,
        "lightblue" | "brightblue" => Color::LightBlue,
        "lightmagenta" | "brightmagenta" => Color::LightMagenta,
        "lightcyan" | "brightcyan" => Color::LightCyan,
        other => other.parse::<u8>().map_or(Color::Reset, Color::Indexed),
    }
}

fn parse_hex(hex: &str) -> Option<Color> {
    let bytes = hex.as_bytes();
    match bytes.len() {
        // #rgb — each nibble is doubled, so `#f0a` means `#ff00aa`.
        3 => {
            let r = nibble(bytes[0])?;
            let g = nibble(bytes[1])?;
            let b = nibble(bytes[2])?;
            Some(Color::Rgb(r * 17, g * 17, b * 17))
        }
        6 => {
            let r = byte(bytes[0], bytes[1])?;
            let g = byte(bytes[2], bytes[3])?;
            let b = byte(bytes[4], bytes[5])?;
            Some(Color::Rgb(r, g, b))
        }
        _ => None,
    }
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn byte(hi: u8, lo: u8) -> Option<u8> {
    Some(nibble(hi)? * 16 + nibble(lo)?)
}

/// Blends `a` toward `b` by `t` (0.0 → `a`, 1.0 → `b`).
///
/// Only meaningful for two true-color values; if either side is an ANSI or
/// `reset` color there is nothing to interpolate between, so `a` is returned
/// unchanged. Used to derive hover/selection tints from a theme's base colors.
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// Perceived luminance in 0.0..=1.0, or `None` for non-RGB colors.
///
/// Used to decide whether text drawn on top of a color should be light or dark.
pub fn luminance(color: Color) -> Option<f32> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    Some((0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b)) / 255.0)
}

/// The contrast between two colours as WCAG 2 measures it, from 1.0 for a
/// colour against itself to 21.0 for black on white.
///
/// `None` unless both are RGB: what `red` or `reset` look like is up to the
/// terminal, so there is nothing to measure.
#[must_use]
pub fn contrast(a: Color, b: Color) -> Option<f32> {
    let (a, b) = (relative_luminance(a)?, relative_luminance(b)?);
    let (light, dark) = if a > b { (a, b) } else { (b, a) };
    Some(((light + 0.05) / (dark + 0.05)) as f32)
}

/// `color`, made lighter or darker until it stands at least `ratio`:1 against
/// `background`, keeping its hue.
///
/// Only its lightness moves, in OKLab, and only as far as it has to: away
/// from the background, so a colour gets lighter on a dark page and darker on
/// a light one. A colour that already stands out comes back unchanged, and so
/// does anything that isn't RGB. When the background allows no colour that
/// much contrast, as a mid-grey page doesn't, it goes as far as it can.
#[must_use]
pub fn readable(color: Color, background: Color, ratio: f32) -> Color {
    let (Some(now), Some(page), Some([lightness, a, b])) = (
        contrast(color, background),
        relative_luminance(background),
        oklab(color),
    ) else {
        return color;
    };
    if now >= ratio {
        return color;
    }
    // Black stands furthest from a page lighter than this, white from one
    // darker: it is where the two are equally far.
    let furthest = if page > 0.179 { 0.0 } else { 1.0 };
    let (mut near, mut far) = (lightness, furthest);
    for _ in 0..16 {
        let middle = (near + far) / 2.0;
        if contrast(from_oklab([middle, a, b]), background).is_some_and(|c| c >= ratio) {
            far = middle;
        } else {
            near = middle;
        }
    }
    from_oklab([far, a, b])
}

/// How different two colours look: how far apart they are in OKLab, where
/// about 0.02 is the least difference most people notice and black is 1.0
/// from white. `None` unless both are RGB.
#[must_use]
pub fn difference(a: Color, b: Color) -> Option<f32> {
    let (a, b) = (oklab(a)?, oklab(b)?);
    let squares: f64 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
    Some(squares.sqrt() as f32)
}

/// How colourful a colour is, as its OKLab chroma: 0.0 for a grey, around
/// 0.1 to 0.2 for the hues of a theme. `None` unless it is RGB.
#[must_use]
pub fn chroma(color: Color) -> Option<f32> {
    let [_, a, b] = oklab(color)?;
    Some(a.hypot(b) as f32)
}

/// The nearest of xterm's 256 colours, picked the way tmux picks it when the
/// terminal outside can't show 24-bit colour: from the 6×6×6 cube, or the
/// grey ramp when a grey is nearer. `None` unless it is RGB.
#[must_use]
pub fn xterm_256(color: Color) -> Option<u8> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    let level = |v: u8| match v {
        0..48 => 0,
        48..114 => 1,
        _ => (v - 35) / 40,
    };
    const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    let (qr, qg, qb) = (level(r), level(g), level(b));
    let away = |x: u8, y: u8, z: u8| {
        let square = |p: u8, q: u8| (i32::from(p) - i32::from(q)).pow(2);
        square(x, r) + square(y, g) + square(z, b)
    };
    let cube = away(
        STEPS[usize::from(qr)],
        STEPS[usize::from(qg)],
        STEPS[usize::from(qb)],
    );
    let average = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let step = if average > 238 {
        23
    } else {
        (average.saturating_sub(3) / 10) as u8
    };
    let grey = 8 + 10 * step;
    Some(if away(grey, grey, grey) < cube {
        232 + step
    } else {
        16 + 36 * qr + 6 * qg + qb
    })
}

/// A channel of an sRGB colour as linear light, 0.0 to 1.0.
fn linear(channel: u8) -> f64 {
    let c = f64::from(channel) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light back to an sRGB channel, clipped to what a screen can show.
fn encode(light: f64) -> u8 {
    let c = light.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

/// WCAG's relative luminance: how much light a colour gives, 0.0 to 1.0.
fn relative_luminance(color: Color) -> Option<f64> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    Some(0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b))
}

/// A colour in OKLab (Björn Ottosson, 2020): its lightness, then how green or
/// red and how blue or yellow it is, on scales where equal steps look about
/// equally far apart.
fn oklab(color: Color) -> Option<[f64; 3]> {
    let Color::Rgb(r, g, b) = color else {
        return None;
    };
    let (r, g, b) = (linear(r), linear(g), linear(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    Some([
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ])
}

/// An OKLab colour back in sRGB, clipped to what a screen can show.
fn from_oklab([lightness, a, b]: [f64; 3]) -> Color {
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    Color::Rgb(
        encode(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        encode(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        encode(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_forms() {
        assert_eq!(parse("#1e1e1e"), Color::Rgb(0x1e, 0x1e, 0x1e));
        assert_eq!(parse("#f0a"), Color::Rgb(0xff, 0x00, 0xaa));
        assert_eq!(parse("  #8b6cef  "), Color::Rgb(0x8b, 0x6c, 0xef));
    }

    #[test]
    fn parses_names_case_insensitively() {
        assert_eq!(parse("Red"), Color::Red);
        assert_eq!(parse("darkgrey"), Color::DarkGray);
        assert_eq!(parse("brightblue"), Color::LightBlue);
        assert_eq!(parse("42"), Color::Indexed(42));
    }

    #[test]
    fn unknown_and_empty_fall_back_to_reset() {
        assert_eq!(parse(""), Color::Reset);
        assert_eq!(parse("reset"), Color::Reset);
        assert_eq!(parse("chartreuse"), Color::Reset);
        assert_eq!(parse("#12345"), Color::Reset);
    }

    #[test]
    fn blend_interpolates_rgb_only() {
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        assert_eq!(blend(black, white, 0.5), Color::Rgb(128, 128, 128));
        assert_eq!(blend(black, white, 0.0), black);
        // Nothing to interpolate against a terminal-default color.
        assert_eq!(blend(Color::Reset, white, 0.5), Color::Reset);
    }

    #[test]
    fn contrast_follows_wcag() {
        let black = Color::Rgb(0, 0, 0);
        let white = Color::Rgb(255, 255, 255);
        let ratio = |a, b| contrast(a, b).expect("both are RGB");
        assert!((ratio(black, white) - 21.0).abs() < 0.01);
        assert!(
            (ratio(white, black) - 21.0).abs() < 0.01,
            "either way round"
        );
        assert!((ratio(white, white) - 1.0).abs() < 0.001);
        // WCAG's own example: #767676 is the lightest grey that passes AA on
        // white, at 4.54:1.
        assert!((ratio(parse("#767676"), white) - 4.54).abs() < 0.01);
        assert_eq!(contrast(Color::Red, white), None);
    }

    #[test]
    fn readable_moves_lightness_away_from_the_page_and_keeps_the_hue() {
        let white = Color::Rgb(255, 255, 255);
        let dark = parse("#1e1e1e");
        // Obsidian light's cyan is 2.3:1 on white, too faint to read.
        let cyan = parse("#00bfbc");
        let darker = readable(cyan, white, 4.5);
        let ratio = contrast(darker, white).expect("RGB");
        assert!(
            (4.5..4.8).contains(&ratio),
            "only as far as it has to: {ratio}"
        );
        let (Color::Rgb(r, g, b), Color::Rgb(_, g0, b0)) = (darker, cyan) else {
            panic!("RGB in, RGB out");
        };
        assert!(g < g0 && b < b0, "darker on a light page: {darker:?}");
        assert!(g > r + 40 && b > r + 40, "still cyan: {darker:?}");
        assert!(difference(darker, cyan).expect("RGB") < 0.25);

        // On a dark page the same colour goes the other way.
        let navy = parse("#283593");
        let lighter = readable(navy, dark, 4.5);
        assert!(luminance(lighter) > luminance(navy));
        assert!(contrast(lighter, dark).expect("RGB") >= 4.5);

        // A colour that already reads is left alone, and so is anything the
        // terminal decides.
        assert_eq!(readable(dark, white, 4.5), dark);
        assert_eq!(readable(Color::Cyan, white, 4.5), Color::Cyan);
        assert_eq!(readable(cyan, Color::Reset, 4.5), cyan);
    }

    #[test]
    fn readable_does_what_it_can_on_a_page_that_allows_nothing() {
        // Nothing reaches 7:1 on mid-grey, so the best on offer comes back.
        let grey = Color::Rgb(119, 119, 119);
        let best = readable(parse("#8b6cef"), grey, 7.0);
        assert!(contrast(best, grey).expect("RGB") > 3.5, "{best:?}");
    }

    #[test]
    fn xterm_256_rounds_as_tmux_does() {
        assert_eq!(xterm_256(parse("#000000")), Some(16));
        assert_eq!(xterm_256(parse("#ffffff")), Some(231));
        assert_eq!(xterm_256(parse("#5f87af")), Some(67), "on the cube exactly");
        assert_eq!(xterm_256(parse("#808080")), Some(244), "a grey to the ramp");
        assert_eq!(xterm_256(parse("#1e1e1e")), Some(234));
        assert_eq!(xterm_256(parse("#4d9fff")), Some(75));
        assert_eq!(xterm_256(Color::Blue), None);
    }

    #[test]
    fn difference_and_chroma_measure_how_colours_look() {
        let blue = parse("#4d9fff");
        assert_eq!(difference(blue, blue), Some(0.0));
        let near = difference(parse("#ff75a0"), parse("#f7768e")).expect("RGB");
        let far = difference(blue, parse("#e9973f")).expect("RGB");
        assert!(near < 0.05 && far > 0.2, "{near} {far}");
        assert!(chroma(parse("#808080")).expect("RGB") < 0.001, "grey");
        assert!(chroma(blue).expect("RGB") > 0.1);
        assert_eq!(chroma(Color::Blue), None);
    }
}
