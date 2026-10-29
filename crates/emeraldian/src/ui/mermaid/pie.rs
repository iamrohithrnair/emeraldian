//! Pie charts: `pie`.
//!
//! A pie of up to five slices is drawn round, out of quadrant blocks, each
//! cell in the colour of the slice that covers most of it, with a legend
//! beside it giving each slice's colour, its label, its value when
//! `showData` asks for it, and its share. Slices go round clockwise from the
//! top in the order they are written, as Mermaid draws them. Five is as many
//! as every theme has colours for, so a pie of more slices, or one in a pane
//! too narrow for a round one, is drawn as a bar for each slice instead,
//! labelled, which reads better than slivers of colours that repeat.
//!
//! Drawing a pie from quadrant blocks, giving each cell to the slice that
//! covers most of it, and falling back to bars are ideas taken from
//! prismantis (MIT), which draws Mermaid for Claude Code. The code is our
//! own.

use unicode_width::UnicodeWidthStr;

use super::canvas::Canvas;
use super::label::{self, truncate, widest};
use super::{Diagram, EIGHTHS, Ink, Role, key, pairs, syntax};

/// Past this a pie is a list, and no longer one anyone reads as a pie.
const MAX_SLICES: usize = 60;
/// The most slices drawn round.
const ROUND: usize = 5;
/// The radius of a round pie in rows, at its largest and smallest. Each row
/// is two of the pie's dots high and each column one wide, which, with a
/// terminal's cells twice as tall as they are wide, draws a circle.
const RADII: [usize; 2] = [6, 4];
/// Columns of a slice's label before it is cut short.
const LABEL_WIDTH: usize = 24;
/// Columns between a round pie and its legend.
const GAP: usize = 3;
/// The quadrant block for each set of quarters filled, by bits: upper left
/// 1, upper right 2, lower left 4, lower right 8.
const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub label: String,
    pub value: f64,
    /// The value as it was written, which is how it is shown.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pie {
    pub title: Option<String>,
    /// Whether each slice's value is shown beside its share.
    pub show_data: bool,
    pub slices: Vec<Slice>,
}

/// Reads a pie from what follows `pie` on its first line, which may say
/// `showData` and give a title, and the lines after it.
pub fn parse(header: &str, lines: &[&str]) -> Option<Pie> {
    let mut pie = Pie {
        title: None,
        show_data: false,
        slices: Vec::new(),
    };
    let mut rest = header.trim();
    if let Some(after) = syntax::keyword(rest, "showData") {
        pie.show_data = true;
        rest = after;
    }
    if let Some(title) = syntax::keyword(rest, "title") {
        pie.title = Some(title)
            .filter(|title| !title.is_empty())
            .map(label::joined);
    } else if !rest.is_empty() {
        return None;
    }
    let mut described = false;
    for line in lines {
        let line = line.trim();
        // A long description in braces runs over several lines.
        if described {
            described = !line.contains('}');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(title) = syntax::keyword(line, "title") {
            pie.title = Some(title)
                .filter(|title| !title.is_empty())
                .map(label::joined);
        } else if syntax::keyword(line, "showData").is_some_and(str::is_empty) {
            pie.show_data = true;
        } else if line.starts_with("accTitle") || line.starts_with("accDescr") {
            described = line.starts_with("accDescr") && line.contains('{') && !line.contains('}');
        } else {
            if pie.slices.len() >= MAX_SLICES {
                return None;
            }
            pie.slices.push(slice(line)?);
        }
    }
    let total: f64 = pie.slices.iter().map(|slice| slice.value).sum();
    if !(total.is_finite() && total > 0.0) {
        return None;
    }
    Some(pie)
}

/// `"Label" : 42.5`. A slice may be nothing, but never less.
fn slice(line: &str) -> Option<Slice> {
    let quoted = line.strip_prefix('"')?;
    let close = quoted.find('"')?;
    let label = &quoted[..close];
    let text = quoted[close + 1..].trim_start().strip_prefix(':')?.trim();
    let value: f64 = text.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some(Slice {
        label: truncate(&label::joined(label), LABEL_WIDTH),
        value,
        text: text.to_string(),
    })
}

/// Each slice's line of the legend: its label, padded so the shares line
/// up, then its value and its share.
fn legend(pie: &Pie) -> Vec<(String, String)> {
    let total: f64 = pie.slices.iter().map(|slice| slice.value).sum();
    let shares: Vec<String> = pie
        .slices
        .iter()
        .map(|slice| {
            let share = (slice.value / total * 100.0).round();
            if share == 0.0 && slice.value > 0.0 {
                "<1%".to_string()
            } else {
                format!("{share}%")
            }
        })
        .collect();
    let labels: Vec<String> = pie.slices.iter().map(|s| s.label.clone()).collect();
    let label_width = widest(&labels);
    let value_width = pie.slices.iter().map(|s| s.text.width()).max().unwrap_or(0);
    let share_width = shares.iter().map(|share| share.width()).max().unwrap_or(0);
    pie.slices
        .iter()
        .zip(shares)
        .map(|(slice, share)| {
            let pad = " ".repeat(label_width - slice.label.width());
            let label = format!("{}{pad}", slice.label);
            let mut tail = String::new();
            if pie.show_data {
                let gap = " ".repeat(value_width - slice.text.width());
                tail.push_str(&format!("{gap}{}  ", slice.text));
            }
            tail.push_str(&" ".repeat(share_width - share.width()));
            tail.push_str(&share);
            (label, tail)
        })
        .collect()
}

/// Draws a pie, round when it can be and as bars when not. `width` is the
/// room the pane has.
pub fn draw(pie: &Pie, width: usize) -> Option<Diagram> {
    let legend = legend(pie);
    // A title wraps only where it is wider than the pane.
    let title = label::title(pie.title.as_deref(), width.max(20));
    let line_width = legend
        .first()
        .map_or(0, |(label, tail)| 3 + label.width() + 1 + tail.width());
    let radius = (pie.slices.len() <= ROUND)
        .then(|| {
            RADII
                .into_iter()
                .find(|r| 4 * r + GAP + line_width <= width)
        })
        .flatten();
    let mut canvas = match radius {
        Some(radius) => round(pie, &legend, radius, &title)?,
        None => bars(pie, &legend, width, &title)?,
    };
    label::draw_title(&mut canvas, &title);
    let mut diagram = canvas.finish();
    diagram.keys = pie.slices.iter().map(|slice| key(&slice.label)).collect();
    // Neighbouring slices differ, and so do the first and last, which meet
    // at the top of a round pie.
    let count = pie.slices.len();
    let ring = (0..count).map(|at| (at, (at + 1) % count));
    diagram.links = pairs(
        ring.filter(|(a, b)| a != b)
            .map(|(a, b)| (a.min(b), a.max(b))),
    );
    diagram.links.sort_unstable();
    diagram.links.dedup();
    Some(diagram)
}

/// A round pie of `radius` rows, with the legend beside it.
fn round(
    pie: &Pie,
    legend: &[(String, String)],
    radius: usize,
    title: &[String],
) -> Option<Canvas> {
    let (columns, rows) = (4 * radius, 2 * radius);
    let above = label::above(title);
    let total: f64 = pie.slices.iter().map(|slice| slice.value).sum();
    // Where each slice ends, as a share of a turn.
    let mut seen = 0.0;
    let ends: Vec<f64> = pie
        .slices
        .iter()
        .map(|slice| {
            seen += slice.value / total;
            seen
        })
        .collect();
    let r = 2.0 * radius as f64;
    let slice_at = |x: f64, y: f64| -> Option<usize> {
        let (dx, dy) = (x - r, y - r);
        if dx * dx + dy * dy > r * r {
            return None;
        }
        // Clockwise from the top.
        let turn = (dx.atan2(-dy) / std::f64::consts::TAU).rem_euclid(1.0);
        ends.iter()
            .position(|&end| turn < end)
            .or(Some(pie.slices.len() - 1))
    };
    let legend_width = legend
        .iter()
        .map(|(label, tail)| 3 + label.width() + 1 + tail.width())
        .max()
        .unwrap_or(0);
    let width = (columns + GAP + legend_width).max(widest(title)).max(1);
    let mut canvas = Canvas::new(width, above + rows)?;
    for cy in 0..rows {
        for cx in 0..columns {
            let mut mask = 0;
            let mut votes = [0u8; ROUND];
            for (bit, sx, sy) in [(1, 0, 0), (2, 1, 0), (4, 0, 1), (8, 1, 1)] {
                let x = cx as f64 + (f64::from(sx) + 0.5) / 2.0;
                let y = 2.0 * cy as f64 + f64::from(sy) + 0.5;
                if let Some(slice) = slice_at(x, y) {
                    mask |= bit;
                    if let Some(vote) = votes.get_mut(slice) {
                        *vote += 1;
                    }
                }
            }
            if mask == 0 {
                continue;
            }
            // The slice that covers most of the cell, the first on a tie.
            let most = (0..votes.len())
                .max_by_key(|&s| (votes[s], std::cmp::Reverse(s)))
                .unwrap_or(0);
            canvas.put(cx, above + cy, QUADRANTS[mask], Ink::of(Role::Border, most));
        }
    }
    let top = above + rows.saturating_sub(legend.len()) / 2;
    for (at, (label, tail)) in legend.iter().enumerate() {
        let x = columns + GAP;
        let y = top + at;
        canvas.text(x, y, "██", Ink::of(Role::Border, at));
        let used = canvas.text(x + 3, y, label, Ink::plain(Role::Text));
        canvas.text(x + 3 + used + 1, y, tail, Ink::plain(Role::Label));
    }
    Some(canvas)
}

/// A bar for each slice, as long against the longest as the slice is
/// against the largest, each beside its label and its share.
fn bars(pie: &Pie, legend: &[(String, String)], width: usize, title: &[String]) -> Option<Canvas> {
    let above = label::above(title);
    let label_width = legend.first().map_or(0, |(label, _)| label.width());
    let tail_width = legend.first().map_or(0, |(_, tail)| tail.width());
    let bar = width
        .saturating_sub(label_width + tail_width + 4)
        .clamp(8, 40);
    let largest = pie
        .slices
        .iter()
        .map(|slice| slice.value)
        .fold(0.0f64, f64::max);
    let total = (label_width + 2 + bar + 2 + tail_width)
        .max(widest(title))
        .max(1);
    let mut canvas = Canvas::new(total, above + pie.slices.len())?;
    for (at, (slice, (label, tail))) in pie.slices.iter().zip(legend).enumerate() {
        let y = above + at;
        canvas.text(0, y, label, Ink::plain(Role::Text));
        let eighths = if largest > 0.0 {
            (slice.value / largest * (bar * 8) as f64).round() as usize
        } else {
            0
        };
        // Anything at all shows as at least a sliver.
        let eighths = if slice.value > 0.0 { eighths.max(1) } else { 0 };
        let x = label_width + 2;
        for column in 0..eighths.div_ceil(8) {
            let block = EIGHTHS[(eighths - 8 * column).min(8)];
            canvas.put(x + column, y, block, Ink::of(Role::Border, at));
        }
        canvas.text(x + bar + 2, y, tail, Ink::plain(Role::Label));
    }
    Some(canvas)
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn pie(source: &str) -> Option<Pie> {
        let mut lines = source.lines();
        let header = lines.next()?.trim();
        let header = header.strip_prefix("pie")?;
        let rest: Vec<&str> = lines.collect();
        parse(header, &rest)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&pie(source).expect("parses"), width).expect("draws"))
    }

    const PETS: &str =
        "pie title Pets adopted by volunteers\n  \"Dogs\" : 386\n  \"Cats\" : 85\n  \"Rats\" : 15";

    #[test]
    fn titles_data_and_slices_are_read() {
        let p = pie(PETS).unwrap();
        assert_eq!(p.title.as_deref(), Some("Pets adopted by volunteers"));
        assert!(!p.show_data);
        let labels: Vec<&str> = p.slices.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["Dogs", "Cats", "Rats"]);
        assert_eq!(p.slices[0].value, 386.0);

        let p = pie("pie showData\n  title Key elements\n  accTitle: Elements\n  accDescr {\n    long\n  }\n  \"Calcium\" : 42.96\n  \"Iron\" : .5").unwrap();
        assert!(p.show_data);
        assert_eq!(p.title.as_deref(), Some("Key elements"));
        assert_eq!(p.slices[1].text, ".5");
        assert!(pie("pie\n  showData\n  \"A\" : 1").unwrap().show_data);
    }

    #[test]
    fn broken_pies_are_not_drawn() {
        for source in [
            "pie",
            "pie title Nothing in it",
            "pie\n  \"A\" : 0",
            "pie\n  \"A\" : -3",
            "pie\n  \"A\" : NaN",
            "pie\n  \"A\" : inf",
            "pie\n  \"A\" 3",
            "pie\n  A : 3",
            "pie\n  \"unclosed : 3",
            "pie\n  \"A\" : lots",
            "pie sideways\n  \"A\" : 3",
            "pie\n  \"A\" : 1\n  not a slice",
        ] {
            assert!(pie(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_few_slices_draw_as_a_round_pie_with_a_legend() {
        let out = drawn(PETS, 80);
        assert_eq!(out.lines().next(), Some("Pets adopted by volunteers"));
        assert!(out.contains("Dogs") && out.contains("79%"), "{out}");
        assert!(out.contains("<1%") || out.contains("3%"), "{out}");
        assert!(out.contains('█') && out.contains('▄'), "a disc:\n{out}");
        let disc = out.lines().nth(2).unwrap();
        assert!(
            disc.trim_start().starts_with('▄') || disc.contains('▄'),
            "{out}"
        );
    }

    #[test]
    fn values_show_when_asked() {
        let out = drawn("pie showData\n  \"Dogs\" : 386\n  \"Cats\" : 85.5", 80);
        assert!(out.contains("386") && out.contains("85.5"), "{out}");
    }

    #[test]
    fn many_slices_or_a_narrow_pane_draw_as_bars() {
        let mut source = String::from("pie\n");
        for at in 0..8 {
            source.push_str(&format!("  \"Slice {at}\" : {}\n", at + 1));
        }
        let out = drawn(&source, 100);
        assert_eq!(out.lines().count(), 8, "{out}");
        assert!(
            out.lines()
                .all(|line| line.contains('█') || line.contains('▏')),
            "{out}"
        );
        let narrow = drawn(PETS, 24);
        assert!(
            narrow.lines().any(|line| line.starts_with("Dogs")),
            "{narrow}"
        );
    }

    #[test]
    fn each_slice_is_its_own_colour_and_the_words_are_readable() {
        let diagram = draw(&pie(PETS).unwrap(), 80).unwrap();
        assert_eq!(diagram.keys, [key("Dogs"), key("Cats"), key("Rats")]);
        assert_eq!(diagram.links, [(0, 1), (0, 2), (1, 2)]);
        let owners: std::collections::BTreeSet<Option<u16>> = diagram
            .rows
            .iter()
            .flatten()
            .filter(|(_, ink)| ink.role == Role::Border)
            .map(|(_, ink)| ink.owner)
            .collect();
        assert_eq!(owners.len(), 3, "every slice is drawn: {owners:?}");
        for (text, ink) in diagram.rows.iter().flatten() {
            if text.chars().any(char::is_alphanumeric) {
                assert!(
                    matches!(ink.role, Role::Text | Role::Label),
                    "{text:?} {ink:?}"
                );
            }
        }
    }

    #[test]
    fn a_zero_slice_unicode_and_a_tiny_share_are_handled() {
        let out = drawn(
            "pie\n  \"日本語のラベル\" : 1000\n  \"none\" : 0\n  \"tiny\" : 1",
            120,
        );
        assert!(
            out.contains("日本語のラベル") && out.contains("<1%") && out.contains("0%"),
            "{out}"
        );
    }

    #[test]
    fn huge_and_odd_pies_stay_bounded() {
        let mut many = String::from("pie\n");
        for at in 0..MAX_SLICES + 1 {
            many.push_str(&format!("  \"s{at}\" : 1\n"));
        }
        assert!(pie(&many).is_none(), "too many slices");
        let mut most = String::from("pie showData title Lots\n");
        for at in 0..MAX_SLICES {
            most.push_str(&format!("  \"{}\" : 1e300\n", "x".repeat(at)));
        }
        let parsed = pie(&most);
        if let Some(parsed) = parsed {
            for width in [0, 1, 40, 500] {
                let _ = draw(&parsed, width);
            }
        }
        let big = pie("pie\n  \"A\" : 1e308\n  \"B\" : 1e308");
        assert!(big.is_none(), "a total past what a float holds");
    }
}
