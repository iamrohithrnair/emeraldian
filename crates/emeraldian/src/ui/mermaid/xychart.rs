//! XY charts: `xychart-beta`, and `xychart` as newer Mermaid calls it.
//!
//! Bars stand up from the x-axis, a column or more each, topped with an
//! eighth block so close values still look different, and lines step from
//! one value to the next in box-drawing characters, over the bars where
//! they cross them. Each series has a colour of its own. The y-axis is
//! marked at round numbers, the x-axis's categories or numbers are written
//! under their columns, every other one or fewer where they don't all fit,
//! and the series that have names get a legend. A `horizontal` chart lays
//! the same out on its side, its bars running along the rows.
//!
//! A value axis given as `4000 --> 11000` is widened to the round numbers
//! around it, so each mark falls on a row, and a bar stands from nought
//! when nought is on the axis, from the bottom when it isn't. Marking the
//! axis at whole rows and standing bars from nought follow prismantis (MIT),
//! which draws charts for Claude Code with beautiful-mermaid (MIT). The code
//! is our own.

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::label::{self, truncate, widest};
use super::{Diagram, Ink, Role, key};

const MAX_SERIES: usize = 20;
const MAX_POINTS: usize = 500;
/// Rows the plot of an upright chart aims for, and columns at most for the
/// value axis of one on its side.
const PLOT_ROWS: usize = 12;
const PLOT_COLUMNS: (usize, usize) = (20, 80);
/// Columns of a category's label, at most.
const LABEL_WIDTH: usize = 12;
/// A bar's top cell, by the eighths of it filled, and a sideways bar's end.
const RISING: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const GROWING: [char; 9] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: Option<String>,
    /// A line rather than bars.
    pub line: bool,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub title: Option<String>,
    /// Laid on its side, with the bars running along the rows.
    pub horizontal: bool,
    pub x_title: Option<String>,
    /// The x-axis's categories, or nothing when it is numbers or not given.
    pub categories: Vec<String>,
    /// The x-axis's numbers, as `min --> max`.
    pub x_range: Option<(f64, f64)>,
    pub y_title: Option<String>,
    pub y_range: Option<(f64, f64)>,
    pub series: Vec<Series>,
}

// ---------------------------------------------------------------- parsing

/// Reads a chart from what follows `xychart-beta` on its first line, which
/// may say `horizontal`, and the lines after it.
pub fn parse(header: &str, lines: &[&str]) -> Option<Chart> {
    let horizontal = match header.trim() {
        "" | "vertical" => false,
        "horizontal" => true,
        _ => return None,
    };
    let mut chart = Chart {
        title: None,
        horizontal,
        x_title: None,
        categories: Vec::new(),
        x_range: None,
        y_title: None,
        y_range: None,
        series: Vec::new(),
    };
    let mut described = false;
    for line in lines {
        let line = line.trim();
        if described {
            described = !line.contains('}');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match word {
            "title" => chart.title = Some(text(rest)).filter(|title| !title.is_empty()),
            "x-axis" => {
                let (title, values) = axis(rest)?;
                chart.x_title = title;
                match values {
                    Values::None => {}
                    Values::Range(low, high) => chart.x_range = Some((low, high)),
                    Values::Categories(categories) => chart.categories = categories,
                }
            }
            "y-axis" => {
                let (title, values) = axis(rest)?;
                chart.y_title = title;
                match values {
                    Values::None => {}
                    Values::Range(low, high) => chart.y_range = Some((low, high)),
                    Values::Categories(_) => return None,
                }
            }
            "bar" | "line" => {
                if chart.series.len() >= MAX_SERIES {
                    return None;
                }
                chart.series.push(series(rest, word == "line")?);
            }
            w if w.starts_with("accTitle") || w.starts_with("accDescr") => {
                described = w.starts_with("accDescr") && line.contains('{') && !line.contains('}');
            }
            _ => return None,
        }
    }
    if chart.series.is_empty() || chart.categories.len() > MAX_POINTS {
        return None;
    }
    Some(chart)
}

/// Words, without the quotes they may be in, and without markup.
fn text(raw: &str) -> String {
    let raw = raw.trim();
    let inner = raw
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(raw);
    label::joined(inner)
}

/// A quoted title, or a single word, at the start of `rest`, and what
/// follows it.
fn title(rest: &str) -> Option<(Option<String>, &str)> {
    let rest = rest.trim();
    if let Some(quoted) = rest.strip_prefix('"') {
        let close = quoted.find('"')?;
        return Some((Some(text(&quoted[..close])), &quoted[close + 1..]));
    }
    let starts_number =
        rest.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'));
    if rest.is_empty() || rest.starts_with('[') || starts_number {
        return Some((None, rest));
    }
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '[')
        .unwrap_or(rest.len());
    Some((Some(text(&rest[..end])), &rest[end..]))
}

enum Values {
    None,
    Range(f64, f64),
    Categories(Vec<String>),
}

/// `"title" min --> max`, `title [a, "b c"]`, or just a title.
fn axis(rest: &str) -> Option<(Option<String>, Values)> {
    let (title, rest) = title(rest)?;
    let rest = rest.trim();
    let values = if rest.is_empty() {
        Values::None
    } else if let Some(inside) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        Values::Categories(items(inside)?.iter().map(|item| text(item)).collect())
    } else {
        let (low, high) = rest.split_once("-->")?;
        let (low, high) = (number(low)?, number(high)?);
        if high <= low {
            return None;
        }
        Values::Range(low, high)
    };
    Some((title, values))
}

/// `bar [1, 2]`, `bar "name" [1, 2]` or `bar name [1, 2]`.
fn series(rest: &str, line: bool) -> Option<Series> {
    let (name, rest) = title(rest)?;
    let inside = rest.trim().strip_prefix('[')?.strip_suffix(']')?;
    let values: Vec<f64> = items(inside)?
        .iter()
        .map(|item| number(item))
        .collect::<Option<_>>()?;
    if values.is_empty() || values.len() > MAX_POINTS {
        return None;
    }
    Some(Series {
        name: name.filter(|name| !name.is_empty()),
        line,
        values,
    })
}

/// The items of a list, split at commas outside quotes.
fn items(inside: &str) -> Option<Vec<&str>> {
    if inside.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (at, c) in inside.char_indices() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                out.push(inside[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    if quoted || out.len() >= MAX_POINTS {
        return None;
    }
    out.push(inside[start..].trim());
    Some(out)
}

/// A finite number, such as `+1.3`, `.6` or `-3.4`.
fn number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty()
        || !text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.'))
    {
        return None;
    }
    text.parse::<f64>().ok().filter(|value| value.is_finite())
}

// ---------------------------------------------------------------- drawing

/// The marks along a value axis: from `low` to `high` in `steps` steps of
/// `step`, each written to `decimals` places.
#[derive(Debug, Clone, Copy)]
struct Scale {
    low: f64,
    high: f64,
    step: f64,
    steps: usize,
    decimals: usize,
}

impl Scale {
    /// Round numbers around `low` and `high`, about five steps apart.
    fn new(low: f64, high: f64) -> Option<Self> {
        let (low, high) = if high > low {
            (low, high)
        } else {
            (low - 1.0, high + 1.0)
        };
        let raw = (high - low) / 5.0;
        if !(raw.is_finite() && raw > 0.0) {
            return None;
        }
        let magnitude = 10f64.powf(raw.log10().floor());
        let step = match raw / magnitude {
            f if f <= 1.0 => 1.0,
            f if f <= 2.0 => 2.0,
            f if f <= 2.5 => 2.5,
            f if f <= 5.0 => 5.0,
            _ => 10.0,
        } * magnitude;
        let first = (low / step).floor() * step;
        let last = (high / step).ceil() * step;
        let steps = ((last - first) / step).round();
        if !(first.is_finite() && last.is_finite() && (1.0..=50.0).contains(&steps)) {
            return None;
        }
        let decimals = if step >= 1.0 && step.fract() == 0.0 {
            0
        } else {
            (1.0 - step.log10().floor()).clamp(0.0, 6.0) as usize
        };
        Some(Self {
            low: first,
            high: last,
            step,
            steps: steps as usize,
            decimals,
        })
    }

    /// How far along the axis `value` is, from nought at `low` to one at
    /// `high`.
    fn share(&self, value: f64) -> f64 {
        ((value - self.low) / (self.high - self.low)).clamp(0.0, 1.0)
    }

    /// The mark at step `at`, written short.
    fn mark(&self, at: usize) -> String {
        figure(self.low + at as f64 * self.step, self.decimals)
    }
}

/// A number written as short as it reads well: to `decimals` places, with
/// no trailing noughts, and in millions or billions when it is that big.
fn figure(value: f64, decimals: usize) -> String {
    let (value, unit, decimals) = match value.abs() {
        v if v >= 1e9 => (value / 1e9, "B", 2),
        v if v >= 1e6 => (value / 1e6, "M", 2),
        _ => (value, "", decimals),
    };
    let mut out = format!("{value:.decimals$}");
    if out.contains('.') {
        out = out.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if out == "-0" {
        out = "0".to_string();
    }
    out + unit
}

/// What the x-axis says at each of `count` places.
fn names(chart: &Chart, count: usize) -> Vec<String> {
    if !chart.categories.is_empty() {
        let mut names: Vec<String> = chart
            .categories
            .iter()
            .map(|name| truncate(name, LABEL_WIDTH))
            .collect();
        names.resize(count, String::new());
        return names;
    }
    match chart.x_range {
        Some((low, high)) => {
            let decimals = Scale::new(low, high).map_or(2, |scale| scale.decimals);
            (0..count)
                .map(|at| {
                    let share = if count > 1 {
                        at as f64 / (count - 1) as f64
                    } else {
                        0.0
                    };
                    truncate(&figure(low + share * (high - low), decimals), LABEL_WIDTH)
                })
                .collect()
        }
        None => vec![String::new(); count],
    }
}

/// The value axis: the range given, or one around the data that bars, if
/// there are any, stand on nought in.
fn value_scale(chart: &Chart) -> Option<Scale> {
    let values = chart.series.iter().flat_map(|s| s.values.iter().copied());
    let (least, most) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    let bars = chart.series.iter().any(|s| !s.line);
    let (low, high) = match chart.y_range {
        Some(range) => range,
        None if bars => (least.min(0.0), most.max(0.0)),
        None => (least, most),
    };
    Scale::new(low, high)
}

/// The same chart's parts, worked out once for either way up.
struct Parts<'a> {
    scale: Scale,
    names: Vec<String>,
    /// How many places along the x-axis there are.
    count: usize,
    /// The bar series and the line series, each with its place in
    /// [`Chart::series`].
    bars: Vec<(usize, &'a Series)>,
    lines: Vec<(usize, &'a Series)>,
    title: Vec<String>,
}

/// Draws a chart, upright or on its side, fitted to `width`, the room the
/// pane has.
pub fn draw(chart: &Chart, width: usize) -> Option<Diagram> {
    let count = chart
        .series
        .iter()
        .map(|s| s.values.len())
        .chain([chart.categories.len(), 1])
        .max()?;
    let title = label::title(chart.title.as_deref(), width.max(20));
    let parts = Parts {
        scale: value_scale(chart)?,
        names: names(chart, count),
        count,
        bars: chart
            .series
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.line)
            .collect(),
        lines: chart
            .series
            .iter()
            .enumerate()
            .filter(|(_, s)| s.line)
            .collect(),
        title,
    };
    let mut diagram = if chart.horizontal {
        sideways(chart, &parts, width)?
    } else {
        upright(chart, &parts, width)?
    };
    diagram.keys = chart
        .series
        .iter()
        .enumerate()
        .map(|(at, s)| match &s.name {
            Some(name) => key(name),
            None => key(&format!("{} {at}", if s.line { "line" } else { "bar" })),
        })
        .collect();
    // Every series differs from every other, where there are colours enough.
    for a in 0..chart.series.len() {
        for b in a + 1..chart.series.len() {
            if let (Ok(a), Ok(b)) = (u16::try_from(a), u16::try_from(b)) {
                diagram.links.push((a, b));
            }
        }
    }
    Some(diagram)
}

/// The legend: a swatch and a name for each named series.
fn legend(chart: &Chart) -> Vec<(char, usize, String)> {
    chart
        .series
        .iter()
        .enumerate()
        .filter_map(|(at, s)| Some((if s.line { '─' } else { '█' }, at, s.name.clone()?)))
        .collect()
}

fn legend_width(legend: &[(char, usize, String)]) -> usize {
    legend.iter().map(|(_, _, name)| 3 + name.width() + 3).sum()
}

fn draw_legend(canvas: &mut Canvas, row: usize, legend: &[(char, usize, String)]) {
    let mut x = 0;
    for (swatch, owner, name) in legend {
        let role = if *swatch == '█' {
            Role::Border
        } else {
            Role::Line
        };
        canvas.put(x, row, *swatch, Ink::of(role, *owner));
        canvas.put(x + 1, row, *swatch, Ink::of(role, *owner));
        x += 3;
        x += canvas.text(x, row, name, Ink::plain(Role::Text)) + 3;
    }
}

/// Bars standing up from the x-axis, and lines across.
fn upright(chart: &Chart, parts: &Parts, width: usize) -> Option<Diagram> {
    let Parts {
        scale,
        names,
        count,
        bars,
        lines,
        title,
    } = parts;
    let (scale, count) = (*scale, *count);
    let per = (PLOT_ROWS as f64 / scale.steps as f64).round().max(1.0) as usize;
    let plot_rows = per * scale.steps;
    let marks: Vec<String> = (0..=scale.steps).map(|at| scale.mark(at)).collect();
    let gutter = widest(&marks) + 1;
    // Each place on the x-axis as wide as the pane allows, room for a column
    // of each bar series and a gap at least; labels too long for that are
    // thinned out, rather than the chart made wider.
    let least = (bars.len() + 1).max(2);
    let room = width.saturating_sub(gutter + 2);
    let slot = (room / count).min(16).max(least);
    let plot_width = count * slot;
    let bar_width = if bars.is_empty() {
        0
    } else {
        ((slot - 1) / bars.len()).clamp(1, 8)
    };

    let legend = legend(chart);
    let mut row = label::above(title);
    let y_title = chart.y_title.as_ref().map(|_| row);
    row += usize::from(y_title.is_some());
    let top = row;
    let axis = top + plot_rows;
    row = axis + 1;
    let labels = names.iter().any(|name| !name.is_empty()).then_some(row);
    row += usize::from(labels.is_some());
    let x_title = chart.x_title.as_ref().map(|_| row);
    row += usize::from(x_title.is_some());
    let legend_row = (!legend.is_empty()).then_some(row);
    row += usize::from(legend_row.is_some());

    let left = gutter + 1;
    let spots = x_labels(left, slot, names);
    let canvas_width = (left + plot_width + 1)
        .max(
            spots
                .iter()
                .map(|(x, text)| x + text.width())
                .max()
                .unwrap_or(0),
        )
        .max(widest(title))
        .max(legend_width(&legend))
        .max(chart.x_title.as_ref().map_or(0, |t| t.width()))
        .max(chart.y_title.as_ref().map_or(0, |t| t.width()));
    let mut canvas = Canvas::new(canvas_width, row)?;
    label::draw_title(&mut canvas, title);
    let quiet = Ink::plain(Role::Label);
    if let (Some(row), Some(text)) = (y_title, &chart.y_title) {
        canvas.text(0, row, text, quiet);
    }

    // The y-axis, marked and labelled at each step, and the x-axis.
    let rule = Ink::plain(Role::Line);
    for r in top..axis {
        canvas.put(gutter, r, '│', rule);
    }
    canvas.put(gutter, axis, '└', rule);
    for x in left..left + plot_width {
        canvas.put(x, axis, '─', rule);
    }
    for (at, mark) in marks.iter().enumerate() {
        let r = axis - at * per;
        if r < axis {
            canvas.put(gutter, r, '┤', rule);
        }
        canvas.text(gutter - 1 - mark.width(), r, mark, quiet);
    }

    // Where a value is, in eighths of a row up from the bottom of the plot.
    let height = |value: f64| (scale.share(value) * (plot_rows * 8) as f64).round() as usize;
    let floor = (scale.share(0.0) * plot_rows as f64).round() as usize * 8;
    let base = 0.0f64.clamp(scale.low, scale.high);
    let paths: Vec<(usize, Vec<(usize, usize)>)> = lines
        .iter()
        .map(|&(owner, series)| {
            let mut points = Vec::new();
            for (at, &value) in series.values.iter().enumerate() {
                let up = (height(value) / 8).min(plot_rows - 1);
                let y = axis - 1 - up;
                let start = left + at * slot;
                if at == 0 {
                    points.push((start, y));
                } else {
                    points.push((start, points.last().map_or(y, |p: &(usize, usize)| p.1)));
                    points.push((start, y));
                }
                points.push((start + slot - 1, y));
            }
            (owner, points)
        })
        .collect();
    let crossed = Crossed::new(&paths, canvas_width, row);
    for (index, &(owner, series)) in bars.iter().enumerate() {
        let ink = Ink::of(Role::Border, owner);
        for (at, &value) in series.values.iter().enumerate() {
            let group = bars.len() * bar_width;
            let x = left + at * slot + (slot.saturating_sub(group)) / 2 + index * bar_width;
            let level = sliver(height(value), floor, value, base, plot_rows * 8);
            for column in x..x + bar_width {
                for up in 0..plot_rows {
                    let (cell, cell_top) = (up * 8, up * 8 + 8);
                    let glyph = if level >= floor {
                        let lit = level.min(cell_top).saturating_sub(floor.max(cell));
                        RISING[lit.min(8)]
                    } else {
                        let lit = floor.min(cell_top).saturating_sub(level.max(cell));
                        match lit {
                            0 => ' ',
                            1..=3 => '▔',
                            4..=7 => '▀',
                            _ => '█',
                        }
                    };
                    let y = axis - 1 - up;
                    if glyph != ' ' && !crossed.contains(column, y) {
                        canvas.put(column, y, glyph, ink);
                    }
                }
            }
        }
    }
    for (owner, points) in &paths {
        canvas.path(points, Stroke::Solid, u16::try_from(*owner).ok());
    }

    if let Some(row) = labels {
        for (x, text) in &spots {
            canvas.text(*x, row, text, quiet);
        }
    }
    if let (Some(row), Some(text)) = (x_title, &chart.x_title) {
        let x = left + plot_width.saturating_sub(text.width()) / 2;
        canvas.text(x, row, text, quiet);
    }
    if let Some(row) = legend_row {
        draw_legend(&mut canvas, row, &legend);
    }
    Some(canvas.finish())
}

/// A bar's end, in eighths of a cell from the axis, made at least an eighth
/// long when its value is anything at all, so a small one still shows.
fn sliver(level: usize, floor: usize, value: f64, base: f64, most: usize) -> usize {
    if value > base && level <= floor {
        (floor + 1).min(most)
    } else if value < base && level >= floor {
        floor.saturating_sub(1)
    } else {
        level
    }
}

/// Every cell the lines run through, so bars leave them for the lines, on
/// a canvas `width` columns wide.
struct Crossed {
    width: usize,
    cells: Vec<bool>,
}

impl Crossed {
    fn new(paths: &[(usize, Vec<(usize, usize)>)], width: usize, height: usize) -> Self {
        let mut cells = vec![false; width * height];
        for (_, points) in paths {
            for pair in points.windows(2) {
                let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
                for x in x0.min(x1)..=x0.max(x1) {
                    for y in y0.min(y1)..=y0.max(y1) {
                        if x < width
                            && let Some(cell) = cells.get_mut(y * width + x)
                        {
                            *cell = true;
                        }
                    }
                }
            }
        }
        Self { width, cells }
    }

    fn contains(&self, x: usize, y: usize) -> bool {
        x < self.width && self.cells.get(y * self.width + x).copied().unwrap_or(false)
    }
}

/// Where the x-axis's labels go, centred under their columns, and only
/// every other one, or fewer, where they would run into each other.
fn x_labels(left: usize, slot: usize, names: &[String]) -> Vec<(usize, String)> {
    let longest = widest(names).min(LABEL_WIDTH);
    let every = (longest + 1).div_ceil(slot).max(1);
    names
        .iter()
        .enumerate()
        .step_by(every)
        .map(|(at, name)| {
            let room = (every * slot).saturating_sub(1).max(1);
            let text = truncate(name, room);
            let indent = slot.saturating_sub(text.width()) / 2;
            (left + at * slot + indent, text)
        })
        .collect()
}

/// Bars running along the rows from the y-axis, which is on the left, and
/// lines down.
fn sideways(chart: &Chart, parts: &Parts, width: usize) -> Option<Diagram> {
    let Parts {
        scale,
        names,
        count,
        bars,
        lines,
        title,
    } = parts;
    let (scale, count) = (*scale, *count);
    let gutter = widest(names).min(LABEL_WIDTH) + 1;
    let wanted = width
        .saturating_sub(gutter + 3)
        .clamp(PLOT_COLUMNS.0, PLOT_COLUMNS.1);
    let per = (wanted as f64 / scale.steps as f64).round().max(1.0) as usize;
    let plot_width = per * scale.steps;
    let slot = bars.len().max(1) + usize::from(bars.len() > 1);
    let plot_rows = count * slot;

    let legend = legend(chart);
    let mut row = label::above(title);
    let x_title = chart.x_title.as_ref().map(|_| row);
    row += usize::from(x_title.is_some());
    let top = row;
    let axis = top + plot_rows;
    let marks_row = axis + 1;
    row = marks_row + 1;
    let y_title = chart.y_title.as_ref().map(|_| row);
    row += usize::from(y_title.is_some());
    let legend_row = (!legend.is_empty()).then_some(row);
    row += usize::from(legend_row.is_some());

    let left = gutter + 1;
    let marks: Vec<String> = (0..=scale.steps).map(|at| scale.mark(at)).collect();
    let canvas_width = (left + plot_width + widest(&marks) + 1)
        .max(widest(title))
        .max(legend_width(&legend))
        .max(chart.x_title.as_ref().map_or(0, |t| t.width()))
        .max(chart.y_title.as_ref().map_or(0, |t| t.width()));
    let mut canvas = Canvas::new(canvas_width, row)?;
    label::draw_title(&mut canvas, title);
    let quiet = Ink::plain(Role::Label);
    if let (Some(row), Some(text)) = (x_title, &chart.x_title) {
        canvas.text(0, row, text, quiet);
    }

    // The category axis down the left, the value axis along the bottom.
    let rule = Ink::plain(Role::Line);
    for r in top..axis {
        canvas.put(gutter, r, '│', rule);
    }
    canvas.put(gutter, axis, '└', rule);
    for x in left..=left + plot_width {
        canvas.put(x, axis, '─', rule);
    }
    let mut free = left;
    for (at, mark) in marks.iter().enumerate() {
        let x = left + at * per;
        canvas.put(x, axis, '┬', rule);
        let start = x.saturating_sub(mark.width() / 2).max(free);
        if start + mark.width() <= canvas_width && start <= x {
            canvas.text(start, marks_row, mark, quiet);
            free = start + mark.width() + 1;
        }
    }
    for (at, name) in names.iter().enumerate() {
        let r = top + at * slot + (slot - usize::from(bars.len() > 1)).saturating_sub(1) / 2;
        let x = gutter.saturating_sub(1 + name.width());
        canvas.text(x, r, name, quiet);
    }

    // Where a value is, in eighths of a column along from the y-axis.
    let length = |value: f64| (scale.share(value) * (plot_width * 8) as f64).round() as usize;
    let floor = (scale.share(0.0) * plot_width as f64).round() as usize * 8;
    let base = 0.0f64.clamp(scale.low, scale.high);
    let paths: Vec<(usize, Vec<(usize, usize)>)> = lines
        .iter()
        .map(|&(owner, series)| {
            let mut points = Vec::new();
            for (at, &value) in series.values.iter().enumerate() {
                let x = left + (length(value) / 8).min(plot_width - 1);
                let start = top + at * slot;
                if at == 0 {
                    points.push((x, start));
                } else {
                    points.push((points.last().map_or(x, |p: &(usize, usize)| p.0), start));
                    points.push((x, start));
                }
                points.push((x, start + slot - 1));
            }
            (owner, points)
        })
        .collect();
    let crossed = Crossed::new(&paths, canvas_width, row);
    for (index, &(owner, series)) in bars.iter().enumerate() {
        let ink = Ink::of(Role::Border, owner);
        for (at, &value) in series.values.iter().enumerate() {
            let y = top + at * slot + index;
            let level = sliver(length(value), floor, value, base, plot_width * 8);
            for along in 0..plot_width {
                let (cell, cell_end) = (along * 8, along * 8 + 8);
                let glyph = if level >= floor {
                    let lit = level.min(cell_end).saturating_sub(floor.max(cell));
                    GROWING[lit.min(8)]
                } else {
                    let lit = floor.min(cell_end).saturating_sub(level.max(cell));
                    match lit {
                        0 => ' ',
                        1..=3 => '▕',
                        4..=7 => '▐',
                        _ => '█',
                    }
                };
                let x = left + along;
                if glyph != ' ' && !crossed.contains(x, y) {
                    canvas.put(x, y, glyph, ink);
                }
            }
        }
    }
    for (owner, points) in &paths {
        canvas.path(points, Stroke::Solid, u16::try_from(*owner).ok());
    }
    if let (Some(row), Some(text)) = (y_title, &chart.y_title) {
        let x = left + plot_width.saturating_sub(text.width()) / 2;
        canvas.text(x, row, text, quiet);
    }
    if let Some(row) = legend_row {
        draw_legend(&mut canvas, row, &legend);
    }
    Some(canvas.finish())
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn chart(source: &str) -> Option<Chart> {
        let mut lines = source.lines();
        let header = lines.next()?.trim();
        let header = header
            .strip_prefix("xychart-beta")
            .or_else(|| header.strip_prefix("xychart"))?;
        let rest: Vec<&str> = lines.collect();
        parse(header, &rest)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&chart(source).expect("parses"), width).expect("draws"))
    }

    const SALES: &str = "xychart-beta
    title \"Sales Revenue\"
    x-axis [jan, feb, mar, apr, may, jun, jul, aug, sep, oct, nov, dec]
    y-axis \"Revenue (in $)\" 4000 --> 11000
    bar [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]
    line [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]";

    #[test]
    fn titles_axes_and_series_are_read() {
        let c = chart(SALES).unwrap();
        assert_eq!(c.title.as_deref(), Some("Sales Revenue"));
        assert_eq!(c.categories.len(), 12);
        assert_eq!(c.categories[0], "jan");
        assert_eq!(c.y_title.as_deref(), Some("Revenue (in $)"));
        assert_eq!(c.y_range, Some((4000.0, 11000.0)));
        assert_eq!(c.series.len(), 2);
        assert!(!c.series[0].line && c.series[1].line);

        let c = chart(
            "xychart horizontal\n  x-axis Months 1 --> 12\n  y-axis Seconds\n  line \"p50\" [+1.3, .6, 2.4, -.34]\n  bar avg [1, 2]",
        )
        .unwrap();
        assert!(c.horizontal);
        assert_eq!(c.x_title.as_deref(), Some("Months"));
        assert_eq!(c.x_range, Some((1.0, 12.0)));
        assert_eq!(c.y_title.as_deref(), Some("Seconds"));
        assert_eq!(c.series[0].name.as_deref(), Some("p50"));
        assert_eq!(c.series[0].values, [1.3, 0.6, 2.4, -0.34]);
        assert_eq!(c.series[1].name.as_deref(), Some("avg"));
        let quoted = chart("xychart-beta\n  x-axis [\"one, two\", three]\n  bar [1, 2]").unwrap();
        assert_eq!(quoted.categories, ["one, two", "three"]);
    }

    #[test]
    fn broken_charts_are_not_drawn() {
        for source in [
            "xychart-beta",
            "xychart-beta\n  title Only a title",
            "xychart-beta sideways\n  bar [1]",
            "xychart-beta\n  bar []",
            "xychart-beta\n  bar [1, two]",
            "xychart-beta\n  bar [1, NaN]",
            "xychart-beta\n  bar [1, inf]",
            "xychart-beta\n  bar [1, 2",
            "xychart-beta\n  y-axis 10 --> 1\n  bar [1]",
            "xychart-beta\n  y-axis [a, b]\n  bar [1]",
            "xychart-beta\n  x-axis [\"unclosed, b]\n  bar [1]",
            "xychart-beta\n  pie [1]",
        ] {
            assert!(chart(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn an_upright_chart_draws_bars_a_line_axes_and_labels() {
        let out = drawn(SALES, 100);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "Sales Revenue");
        assert!(rows[2].starts_with("Revenue (in $)"), "{out}");
        for words in ["12000", "4000", "jan", "dec"] {
            assert!(out.contains(words), "{words:?} missing:\n{out}");
        }
        assert!(out.contains('█') && out.contains('└'), "{out}");
        assert!(
            out.contains('╭') || out.contains('╮'),
            "the line turns:\n{out}"
        );
        for row in &rows {
            assert!(row.width() <= 100, "{row}");
        }
    }

    #[test]
    fn close_values_get_different_heights() {
        let c = chart("xychart-beta\n  bar [100, 101, 103]").unwrap();
        let diagram = draw(&c, 60).unwrap();
        let columns: Vec<String> = {
            let out = plain(&diagram);
            let rows: Vec<Vec<char>> = out.lines().map(|r| r.chars().collect()).collect();
            let width = rows.iter().map(Vec::len).max().unwrap_or(0);
            (0..width)
                .map(|x| {
                    rows.iter()
                        .map(|r| r.get(x).copied().unwrap_or(' '))
                        .collect()
                })
                .filter(|column: &String| column.contains('█'))
                .collect()
        };
        assert!(columns.len() >= 3);
        assert_ne!(columns.first(), columns.last(), "{columns:?}");
    }

    #[test]
    fn named_series_get_a_legend_and_each_series_its_colour() {
        let source = "xychart-beta\n  x-axis [a, b, c]\n  bar \"rain\" [3, 5, 2]\n  line \"sun\" [1, 4, 6]\n  line [2, 2, 2]";
        let out = drawn(source, 80);
        let last = out.lines().last().unwrap();
        assert!(last.contains("rain") && last.contains("sun"), "{out}");
        let diagram = draw(&chart(source).unwrap(), 80).unwrap();
        assert_eq!(diagram.keys, [key("rain"), key("sun"), key("line 2")]);
        assert_eq!(diagram.links, [(0, 1), (0, 2), (1, 2)]);
        let owners: std::collections::BTreeSet<Option<u16>> = diagram
            .rows
            .iter()
            .flatten()
            .filter(|(_, ink)| matches!(ink.role, Role::Border | Role::Line))
            .map(|(_, ink)| ink.owner)
            .collect();
        assert!(
            owners.contains(&Some(0)) && owners.contains(&Some(1)) && owners.contains(&Some(2))
        );
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
    fn negative_values_hang_below_nought() {
        let out = drawn("xychart-beta\n  x-axis [up, down]\n  bar [5, -5]", 60);
        let rows: Vec<&str> = out.lines().collect();
        let nought = rows
            .iter()
            .position(|r| r.trim_start().starts_with("0 ") || r.contains(" 0┤"))
            .unwrap();
        assert!(rows[..nought].iter().any(|r| r.contains('█')), "{out}");
        assert!(rows[nought..].iter().any(|r| r.contains('█')), "{out}");
    }

    #[test]
    fn a_horizontal_chart_lays_its_bars_along_the_rows() {
        let out = drawn(
            "xychart-beta horizontal\n  title Speed\n  x-axis [cat, dog, \"snail 🐌\"]\n  bar [30, 45, 0.03]\n  line [20, 40, 1]",
            80,
        );
        let rows: Vec<&str> = out.lines().collect();
        let dog = rows.iter().find(|r| r.contains("dog")).unwrap();
        assert!(dog.contains('█'), "{out}");
        assert!(out.contains("snail 🐌"), "{out}");
        assert!(out.contains('┬'), "the value axis is marked:\n{out}");
    }

    #[test]
    fn huge_unicode_and_odd_charts_stay_bounded() {
        let values: Vec<String> = (0..MAX_POINTS)
            .map(|at| format!("{}", at as f64 * 1.5 - 300.0))
            .collect();
        let mut source = String::from("xychart-beta\n");
        for at in 0..MAX_SERIES {
            source.push_str(&format!(
                "  {} \"s{at}\" [{}]\n",
                if at % 2 == 0 { "bar" } else { "line" },
                values.join(", ")
            ));
        }
        let c = chart(&source).unwrap();
        for width in [0, 80, 400] {
            if let Some(d) = draw(&c, width) {
                assert!(d.rows.len() * d.width <= super::super::canvas::MAX_CELLS);
            }
        }
        let mut bars = String::from("xychart-beta\n  x-axis Range 1 --> 1234567.891\n");
        for at in 0..MAX_SERIES {
            bars.push_str(&format!("  bar \"b{at}\" [1, -2, 3]\n"));
        }
        let c = chart(&bars).unwrap();
        for width in [10, 300, 2000] {
            let _ = draw(&c, width);
            let mut flat = c.clone();
            flat.horizontal = true;
            let _ = draw(&flat, width);
        }
        let too_many: Vec<String> = (0..=MAX_POINTS).map(|at| at.to_string()).collect();
        assert!(chart(&format!("xychart-beta\n  bar [{}]", too_many.join(","))).is_none());
        for extreme in ["1e308, -1e308", "1e-300, 2e-300", "0, 0", "5", "-1, -2"] {
            if let Some(c) = chart(&format!(
                "xychart-beta\n  bar [{extreme}]\n  line [{extreme}]"
            )) {
                for width in [1, 60] {
                    let _ = draw(&c, width);
                    let mut flat = c.clone();
                    flat.horizontal = true;
                    let _ = draw(&flat, width);
                }
            }
        }
        let out = drawn(
            "xychart-beta\n  title 売上\n  x-axis [一月, 二月]\n  bar [1, 2]",
            60,
        );
        assert!(out.contains("売上") && out.contains("一月"), "{out}");
    }

    /// Every prefix of a chart, and each with one character dropped, must
    /// come back drawn or as `None`, never as a panic.
    #[test]
    fn mangled_charts_never_panic() {
        let sample = "xychart-beta horizontal\n  title \"Sales 🙂\"\n  x-axis \"Month\" [jan, \"feb x\", mar]\n  y-axis \"Revenue\" -40 --> 110.5\n  bar \"rain\" [5, -6, 7.5]\n  line [+1, .5, 99]";
        let mut cuts: Vec<String> = sample
            .char_indices()
            .map(|(at, _)| sample[..at].to_string())
            .collect();
        cuts.extend(
            sample
                .char_indices()
                .map(|(at, c)| format!("{}{}", &sample[..at], &sample[at + c.len_utf8()..])),
        );
        for cut in cuts {
            if let Some(mut c) = chart(&cut) {
                for width in [1, 40, 200] {
                    let _ = draw(&c, width);
                }
                c.horizontal = !c.horizontal;
                let _ = draw(&c, 60);
            }
        }
    }
}
