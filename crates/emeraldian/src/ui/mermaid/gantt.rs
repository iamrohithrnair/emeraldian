//! Gantt charts: `gantt`.
//!
//! Each task is a row: its name on the left, under the heading of its
//! section, and a bar on a time axis that runs along the top, from the first
//! start to the last end. A bar is drawn in the colour of its section, in
//! whole blocks with an eighth block where it ends part way through a
//! column, so tasks close in length still look different. A done task is
//! shaded lighter, a milestone is a diamond, and a `vert` marker is a dotted
//! line down the chart, named under it. The axis is marked at
//! steps that suit the span, from a second to a century, and labelled with
//! the time, the day or the date.
//!
//! Dates are read the way `dateFormat` says, with `YYYY`, `YY`, `MM`, `M`,
//! `DD`, `D`, `HH`, `H`, `mm`, `m`, `ss`, `s`, `SSS`, `X` and `x`
//! understood, and so are lengths, `after` and `until`, whichever order the
//! tasks they name are written in. A month is taken as thirty days and a
//! year as 365, `excludes` is not applied, so a task over a weekend is drawn
//! as long as the calendar makes it, and `crit` looks like any other task.
//! A chart of lengths alone, with no date to start from, is drawn from day
//! nought, where Mermaid would start it today.
//!
//! Reading dates by their format, `after` and `until`, the steps the axis is
//! marked at, and ending bars in eighth blocks are ideas taken from
//! prismantis (MIT), which draws Mermaid for Claude Code. The code is our
//! own.

use std::collections::HashMap;

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::label::{self, truncate};
use super::{Diagram, EIGHTHS, Ink, Role, key, pairs, syntax};

const MAX_TASKS: usize = 300;
const MAX_SECTIONS: usize = 60;
/// Columns of a task's name before it is cut short.
const NAME_WIDTH: usize = 24;
/// Columns the time axis may take, at least and at most.
const AXIS: (usize, usize) = (20, 100);

const SECOND: i64 = 1000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;
const WEEK: i64 = 7 * DAY;
const MONTH: i64 = 30 * DAY;
const YEAR: i64 = 365 * DAY;
/// The longest time anything may span, about ten thousand years, which keeps
/// every sum well inside an `i64`.
const LONGEST: i64 = 10_000 * YEAR;

/// The steps the axis may be marked at, shortest first.
const STEPS: [i64; 27] = [
    SECOND,
    5 * SECOND,
    15 * SECOND,
    30 * SECOND,
    MINUTE,
    5 * MINUTE,
    15 * MINUTE,
    30 * MINUTE,
    HOUR,
    2 * HOUR,
    6 * HOUR,
    12 * HOUR,
    DAY,
    2 * DAY,
    WEEK,
    2 * WEEK,
    MONTH,
    3 * MONTH,
    6 * MONTH,
    YEAR,
    2 * YEAR,
    5 * YEAR,
    10 * YEAR,
    25 * YEAR,
    50 * YEAR,
    100 * YEAR,
    1000 * YEAR,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    /// The section it is in, by its place in [`Gantt::sections`].
    pub section: usize,
    /// When it starts and ends, in milliseconds.
    pub start: i64,
    pub end: i64,
    pub done: bool,
    pub milestone: bool,
    /// A line down the chart at its start, which takes no row.
    pub vert: bool,
}

/// What the marks on the time axis say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Dates, and times of day where the marks are closer than a day.
    Calendar,
    /// Times of day, for a `dateFormat` with no date in it.
    Clock,
    /// Time since the start, for a chart of lengths alone.
    Elapsed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gantt {
    pub title: Option<String>,
    /// Each section's name, empty for the tasks written before the first.
    pub sections: Vec<String>,
    pub tasks: Vec<Task>,
    pub axis: Axis,
}

// ---------------------------------------------------------------- parsing

/// Settings that only change how a chart looks in a browser, or what happens
/// on a click.
const IGNORED: &[&str] = &[
    "excludes",
    "includes",
    "axisFormat",
    "tickInterval",
    "todayMarker",
    "weekday",
    "weekend",
    "topAxis",
    "displayMode",
    "click",
];

enum Start {
    /// When the task before it ends.
    Previous,
    At(i64),
    After(Vec<String>),
}

enum End {
    At(i64),
    Lasting(i64),
    Until(Vec<String>),
}

struct Spec {
    name: String,
    section: usize,
    id: Option<String>,
    start: Start,
    end: End,
    done: bool,
    milestone: bool,
    vert: bool,
}

/// Reads the lines after `gantt`.
pub fn parse(lines: &[&str]) -> Option<Gantt> {
    let lines: Vec<&str> = lines.iter().map(|line| line.trim()).collect();
    let format = lines
        .iter()
        .find_map(|line| syntax::keyword(line, "dateFormat"))
        .map_or_else(|| Format::new("YYYY-MM-DD"), Format::new)?;
    let inclusive = lines
        .iter()
        .any(|line| syntax::keyword(line, "inclusiveEndDates").is_some());
    let mut title = None;
    let mut sections: Vec<String> = Vec::new();
    let mut specs: Vec<Spec> = Vec::new();
    let mut described = syntax::Descriptions::default();
    for &line in &lines {
        if described.skip(line) {
            continue;
        }
        if line.is_empty()
            || syntax::keyword(line, "dateFormat").is_some()
            || syntax::keyword(line, "inclusiveEndDates").is_some()
        {
            continue;
        }
        if let Some(text) = syntax::keyword(line, "title") {
            title = Some(label::joined(text)).filter(|text| !text.is_empty());
            continue;
        }
        if let Some(name) = syntax::keyword(line, "section") {
            if sections.len() >= MAX_SECTIONS {
                return None;
            }
            sections.push(truncate(&label::joined(name), 2 * NAME_WIDTH));
            continue;
        }
        if IGNORED.contains(&syntax::first_word(line)) {
            continue;
        }
        if specs.len() >= MAX_TASKS {
            return None;
        }
        // Tasks before the first section are in one with no name.
        if sections.is_empty() {
            sections.push(String::new());
        }
        specs.push(task(line, sections.len() - 1, &format, inclusive)?);
    }
    if specs.is_empty() {
        return None;
    }
    let (tasks, dated) = resolve(&specs)?;
    let axis = match (dated, format.has_date()) {
        (false, _) => Axis::Elapsed,
        (true, true) => Axis::Calendar,
        (true, false) => Axis::Clock,
    };
    Some(Gantt {
        title,
        sections,
        tasks,
        axis,
    })
}

/// `Name : done, id, 2024-01-01, 3d`.
fn task(line: &str, section: usize, format: &Format, inclusive: bool) -> Option<Spec> {
    let (name, meta) = line.split_once(':')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut items: Vec<&str> = meta.split(',').map(str::trim).collect();
    let (mut done, mut milestone, mut vert) = (false, false, false);
    while let Some(&first) = items.first() {
        match first {
            "done" => done = true,
            "milestone" => milestone = true,
            "vert" => vert = true,
            "active" | "crit" => {}
            _ => break,
        }
        items.remove(0);
    }
    let (id, start, end) = match items[..] {
        [end] => (None, None, end),
        [start, end] => (None, Some(start), end),
        [id, start, end] => (Some(id), Some(start), end),
        _ => return None,
    };
    let start = match start {
        None => Start::Previous,
        Some(text) => match text.strip_prefix("after ") {
            Some(ids) => Start::After(names(ids)?),
            None => Start::At(format.read(text)?),
        },
    };
    let end = if let Some(ids) = end.strip_prefix("until ") {
        End::Until(names(ids)?)
    } else if let Some(length) = length(end) {
        End::Lasting(length)
    } else {
        let at = format.read(end)?;
        End::At(if inclusive { at.checked_add(DAY)? } else { at })
    };
    Some(Spec {
        name: truncate(&label::joined(name), NAME_WIDTH),
        section,
        id: id.filter(|id| !id.is_empty()).map(str::to_string),
        start,
        end,
        done,
        milestone,
        vert,
    })
}

/// The ids in `after a b c`.
fn names(text: &str) -> Option<Vec<String>> {
    let ids: Vec<String> = text.split_whitespace().map(str::to_string).collect();
    (!ids.is_empty()).then_some(ids)
}

/// A length such as `3d`, `1.5h` or `500ms`, in milliseconds.
fn length(text: &str) -> Option<i64> {
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
    let (number, unit) = text.split_at(split);
    let unit = match unit {
        "ms" => 1,
        "s" => SECOND,
        "m" => MINUTE,
        "h" => HOUR,
        "d" => DAY,
        "w" => WEEK,
        "M" => MONTH,
        "y" => YEAR,
        _ => return None,
    };
    let number: f64 = number.parse().ok()?;
    let length = number * unit as f64;
    (length.is_finite() && (0.0..=LONGEST as f64).contains(&length)).then(|| length.round() as i64)
}

/// Works out when each task starts and ends, going round again for those
/// that name a task written after them, and whether any date was given.
fn resolve(specs: &[Spec]) -> Option<(Vec<Task>, bool)> {
    let ids: HashMap<&str, usize> = specs
        .iter()
        .enumerate()
        .filter_map(|(at, spec)| Some((spec.id.as_deref()?, at)))
        .collect();
    for spec in specs {
        let named = match (&spec.start, &spec.end) {
            (Start::After(a), End::Until(b)) => a.iter().chain(b).collect::<Vec<_>>(),
            (Start::After(a), _) => a.iter().collect(),
            (_, End::Until(b)) => b.iter().collect(),
            _ => Vec::new(),
        };
        if named.iter().any(|id| !ids.contains_key(id.as_str())) {
            return None;
        }
    }
    // A chart starts at its earliest date, or at nought if it gives none.
    let given = specs.iter().flat_map(|spec| {
        let start = match spec.start {
            Start::At(at) => Some(at),
            _ => None,
        };
        let end = match spec.end {
            End::At(at) => Some(at),
            _ => None,
        };
        start.into_iter().chain(end)
    });
    let earliest = given.min();
    let dated = earliest.is_some();
    let anchor = earliest.unwrap_or(0);

    let mut starts: Vec<Option<i64>> = vec![None; specs.len()];
    let mut ends: Vec<Option<i64>> = vec![None; specs.len()];
    for _ in 0..=specs.len() {
        let mut moved = false;
        for (at, spec) in specs.iter().enumerate() {
            if starts[at].is_none() {
                starts[at] = match &spec.start {
                    Start::Previous if at == 0 => Some(anchor),
                    Start::Previous => ends[at - 1],
                    Start::At(time) => Some(*time),
                    Start::After(names) => names
                        .iter()
                        .map(|id| ends[ids[id.as_str()]])
                        .collect::<Option<Vec<i64>>>()
                        .and_then(|ends| ends.into_iter().max()),
                };
                moved |= starts[at].is_some();
            }
            if let (Some(start), None) = (starts[at], ends[at]) {
                ends[at] = match &spec.end {
                    End::At(time) => Some(*time),
                    End::Lasting(length) => start.checked_add(*length),
                    End::Until(names) => names
                        .iter()
                        .map(|id| starts[ids[id.as_str()]])
                        .collect::<Option<Vec<i64>>>()
                        .and_then(|starts| starts.into_iter().min()),
                };
                moved |= ends[at].is_some();
            }
        }
        if !moved {
            break;
        }
    }
    let mut tasks = Vec::with_capacity(specs.len());
    for (at, spec) in specs.iter().enumerate() {
        let (start, end) = (starts[at]?, ends[at]?);
        let far = |time: i64| time.checked_sub(anchor).is_none_or(|d| d.abs() > LONGEST);
        if end < start || far(start) || far(end) {
            return None;
        }
        tasks.push(Task {
            name: spec.name.clone(),
            section: spec.section,
            start,
            end,
            done: spec.done,
            milestone: spec.milestone,
            vert: spec.vert,
        });
    }
    Some((tasks, dated))
}

/// A part of a `dateFormat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Piece {
    Year,
    ShortYear,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    Milli,
    /// Seconds since 1970, `X`.
    Unix,
    /// Milliseconds since 1970, `x`.
    UnixMilli,
    Literal(char),
}

/// How dates are written, as `dateFormat` gives it.
struct Format(Vec<Piece>);

impl Format {
    /// `None` for a format with a part this can't read, such as `MMM`.
    fn new(spec: &str) -> Option<Self> {
        let chars: Vec<char> = spec.trim().chars().collect();
        let mut pieces = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            let c = chars[at];
            // Anything but a letter stands for itself. Only a letter's run is
            // counted, once for the whole run, not again at each character.
            if !c.is_alphabetic() {
                pieces.push(Piece::Literal(c));
                at += 1;
                continue;
            }
            let run = chars[at..].iter().take_while(|&&d| d == c).count();
            let piece = match (c, run) {
                ('Y', 4) => Piece::Year,
                ('Y', 2) => Piece::ShortYear,
                ('M', 1 | 2) => Piece::Month,
                ('D', 1 | 2) => Piece::Day,
                ('H', 1 | 2) => Piece::Hour,
                ('m', 1 | 2) => Piece::Minute,
                ('s', 1 | 2) => Piece::Second,
                ('S', 1..=3) => Piece::Milli,
                ('X', 1) => Piece::Unix,
                ('x', 1) => Piece::UnixMilli,
                _ => return None,
            };
            pieces.push(piece);
            at += run;
        }
        (!pieces.is_empty()).then_some(Self(pieces))
    }

    /// Whether dates written this way say which day, rather than only the
    /// time of day.
    fn has_date(&self) -> bool {
        self.0.iter().any(|piece| {
            matches!(
                piece,
                Piece::Year
                    | Piece::ShortYear
                    | Piece::Month
                    | Piece::Day
                    | Piece::Unix
                    | Piece::UnixMilli
            )
        })
    }

    /// The time `text` names, in milliseconds since 1970, or `None` if it
    /// isn't written this way or isn't a real date.
    fn read(&self, text: &str) -> Option<i64> {
        let text = text.trim().as_bytes();
        let mut at = 0;
        let (mut year, mut month, mut day) = (1970i64, 1i64, 1i64);
        let (mut hour, mut minute, mut second, mut milli) = (0i64, 0i64, 0i64, 0i64);
        let mut since: Option<i64> = None;
        for &piece in &self.0 {
            if let Piece::Literal(c) = piece {
                let mut buffer = [0u8; 4];
                let literal = c.encode_utf8(&mut buffer).as_bytes();
                if !text[at..].starts_with(literal) {
                    return None;
                }
                at += literal.len();
                continue;
            }
            let (least, most) = match piece {
                Piece::Year => (4, 4),
                Piece::ShortYear => (2, 2),
                Piece::Milli => (1, 3),
                Piece::Unix | Piece::UnixMilli => (1, 15),
                _ => (1, 2),
            };
            let digits = text[at..]
                .iter()
                .take(most)
                .take_while(|b| b.is_ascii_digit())
                .count();
            if digits < least {
                return None;
            }
            let value: i64 = std::str::from_utf8(&text[at..at + digits])
                .ok()?
                .parse()
                .ok()?;
            at += digits;
            match piece {
                Piece::Year => year = value,
                Piece::ShortYear => {
                    year = if value > 68 {
                        1900 + value
                    } else {
                        2000 + value
                    }
                }
                Piece::Month => month = value,
                Piece::Day => day = value,
                Piece::Hour => hour = value,
                Piece::Minute => minute = value,
                Piece::Second => second = value,
                Piece::Milli => milli = value,
                Piece::Unix => since = value.checked_mul(SECOND),
                Piece::UnixMilli => since = Some(value),
                Piece::Literal(_) => {}
            }
        }
        if at != text.len() {
            return None;
        }
        if let Some(since) = since {
            return (since.abs() <= LONGEST).then_some(since);
        }
        let valid = (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
            && (0..24).contains(&hour)
            && (0..60).contains(&minute)
            && (0..60).contains(&second);
        valid.then(|| {
            days_from_civil(year, month, day) * DAY
                + hour * HOUR
                + minute * MINUTE
                + second * SECOND
                + milli
        })
    }
}

fn leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a date in the Gregorian calendar, by Howard
/// Hinnant's method.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let shifted = (month + 9) % 12;
    let of_year = (153 * shifted + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

/// The year, month and day of a day counted from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let of_cycle = days - era * 146_097;
    let of_era = (of_cycle - of_cycle / 1460 + of_cycle / 36_524 - of_cycle / 146_096) / 365;
    let of_year = of_cycle - (365 * of_era + of_era / 4 - of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

// ---------------------------------------------------------------- drawing

/// Where the axis is marked: at steps of `step` from a round time, a
/// midnight, a Monday or the first of a month, from `from` to `to`.
fn ticks(from: i64, to: i64, step: i64, axis: Axis) -> Vec<i64> {
    let mut out = Vec::new();
    if axis == Axis::Elapsed || (axis == Axis::Clock && step >= DAY) {
        let mut at = from;
        while at <= to && out.len() < 1000 {
            out.push(at);
            at += step;
        }
        return out;
    }
    if step >= MONTH {
        // Months and years go by the calendar, from the first of a month,
        // counted from January so quarters and years fall where they should.
        let months = if step >= YEAR {
            12 * (step / YEAR)
        } else {
            step / MONTH
        };
        let (year, month, day) = civil_from_days(from.div_euclid(DAY));
        let mut index = year * 12 + (month - 1);
        if day != 1 || from.rem_euclid(DAY) != 0 {
            index += 1;
        }
        index += (months - index.rem_euclid(months)) % months;
        while out.len() < 1000 {
            let at = days_from_civil(index.div_euclid(12), index.rem_euclid(12) + 1, 1) * DAY;
            if at > to {
                break;
            }
            out.push(at);
            index += months;
        }
        return out;
    }
    // Weeks start on a Monday, which 1970-01-05 was.
    let origin = if step >= WEEK { 4 * DAY } else { 0 };
    let mut at = origin + (from - origin).div_euclid(step) * step;
    if at < from {
        at += step;
    }
    while at <= to && out.len() < 1000 {
        out.push(at);
        at += step;
    }
    out
}

/// A mark on the axis, written as short as the step allows.
fn stamp(at: i64, from: i64, step: i64, axis: Axis, one_year: bool) -> String {
    if axis == Axis::Elapsed || (axis == Axis::Clock && step >= DAY) {
        let since = at - from;
        return match step {
            s if s < MINUTE => format!("{}s", since / SECOND),
            s if s < HOUR => format!("{}m", since / MINUTE),
            s if s < DAY => format!("{}h", since / HOUR),
            s if s < YEAR => format!("{}d", since / DAY),
            _ => format!("{}y", since / YEAR),
        };
    }
    let (year, month, day) = civil_from_days(at.div_euclid(DAY));
    let time = at.rem_euclid(DAY);
    match step {
        s if s < MINUTE => format!(
            "{:02}:{:02}:{:02}",
            time / HOUR,
            time % HOUR / MINUTE,
            time % MINUTE / SECOND
        ),
        // A mark at midnight says which day it is, on a calendar.
        s if s < DAY && (time != 0 || axis == Axis::Clock) => {
            format!("{:02}:{:02}", time / HOUR, time % HOUR / MINUTE)
        }
        s if s < MONTH && one_year => format!("{month:02}-{day:02}"),
        s if s < MONTH => format!("{year}-{month:02}-{day:02}"),
        s if s < YEAR => format!("{year}-{month:02}"),
        _ => format!("{year}"),
    }
}

/// Draws a chart, its time axis fitted to `width`, the room the pane has.
pub fn draw(gantt: &Gantt, width: usize) -> Option<Diagram> {
    let from = gantt.tasks.iter().map(|task| task.start).min()?;
    let to = gantt.tasks.iter().map(|task| task.end).max()?;
    // A chart of moments alone still needs an axis of some length.
    let span = if to > from {
        to - from
    } else if gantt.axis == Axis::Calendar {
        DAY
    } else {
        HOUR
    };
    let named = gantt.sections.iter().any(|name| !name.is_empty());
    let indent = if named { 2 } else { 0 };
    let names = gantt
        .tasks
        .iter()
        .map(|task| indent + task.name.width())
        .max()
        .unwrap_or(0);
    let axis = width.saturating_sub(names + 3).clamp(AXIS.0, AXIS.1);
    let left = names + 2;

    // The step that marks the axis as often as its labels leave room for.
    let (start_year, ..) = civil_from_days(from.div_euclid(DAY));
    let (end_year, ..) = civil_from_days((from + span).div_euclid(DAY));
    let one_year = start_year == end_year;
    let sample = stamp(from, from, DAY, gantt.axis, one_year).width().max(5);
    let room = (axis / (sample + 3)).max(1) as i64;
    let step = STEPS
        .into_iter()
        .find(|&step| span / step <= room)
        .unwrap_or(STEPS[STEPS.len() - 1]);
    let marks = ticks(from, from + span, step, gantt.axis);
    let column = |at: i64| (at - from) as f64 / span as f64 * axis as f64;

    let title = label::title(gantt.title.as_deref(), left + axis + 1);
    let above = label::above(&title);
    let headings = if named { gantt.sections.len() } else { 0 };
    let rows = gantt.tasks.iter().filter(|task| !task.vert).count();
    let verts = gantt.tasks.iter().any(|task| task.vert);
    let height = above + 2 + headings + rows + usize::from(verts);
    let mut canvas = Canvas::new(left + axis + 1, height)?;
    label::draw_title(&mut canvas, &title);

    // The axis, marked at each step and labelled where a label fits.
    let (labels, rule) = (above, above + 1);
    canvas.line((left, rule), (left + axis, rule), Stroke::Solid, None);
    let mut free = 0;
    for &mark in &marks {
        let x = column(mark).round() as usize;
        canvas.put(left + x, rule, '┬', Ink::plain(Role::Line));
        let text = stamp(mark, from, step, gantt.axis, one_year);
        let start = x.min((axis + 1).saturating_sub(text.width()));
        if start >= free {
            canvas.text(left + start, labels, &text, Ink::plain(Role::Label));
            free = start + text.width() + 1;
        }
    }

    // Markers first, so the bars and headings they cross are drawn over
    // them.
    let first = rule + 1;
    let footer = first + headings + rows;
    let mut free = 0;
    for task in gantt.tasks.iter().filter(|task| task.vert) {
        let x = (column(task.start).round() as usize).min(axis);
        if footer > first {
            canvas.line(
                (left + x, first),
                (left + x, footer - 1),
                Stroke::Dotted,
                None,
            );
        }
        let start = x.min((axis + 1).saturating_sub(task.name.width()));
        if start >= free {
            canvas.text(left + start, footer, &task.name, Ink::plain(Role::Label));
            free = start + task.name.width() + 1;
        }
    }
    let mut row = first;
    for (section, name) in gantt.sections.iter().enumerate() {
        if named {
            canvas.text(0, row, name, Ink::of(Role::Label, section));
            row += 1;
        }
        for task in gantt
            .tasks
            .iter()
            .filter(|task| task.section == section && !task.vert)
        {
            canvas.text(indent, row, &task.name, Ink::plain(Role::Text));
            let ink = Ink::of(Role::Border, section);
            let (start, end) = (column(task.start), column(task.end));
            if task.milestone {
                let x = (start.round() as usize).min(axis - 1);
                canvas.put(left + x, row, '◆', ink);
            } else {
                let cells = Cells {
                    left,
                    row,
                    axis,
                    ink,
                };
                cells.bar(&mut canvas, start, end, task.done);
            }
            row += 1;
        }
    }

    let mut diagram = canvas.finish();
    diagram.keys = gantt.sections.iter().map(|name| key(name)).collect();
    diagram.links = pairs((1..gantt.sections.len()).map(|k| (k - 1, k)));
    Some(diagram)
}

/// Where a task's bar goes: the column the axis starts at, the row, the
/// columns of the axis, and the ink.
struct Cells {
    left: usize,
    row: usize,
    axis: usize,
    ink: Ink,
}

impl Cells {
    /// A bar from column `start` to column `end`, both counted in fractions
    /// of a column: whole blocks, a half block where it starts part way into
    /// one, and an eighth block where it ends part way through one. A done
    /// task is shaded, a cell at a time.
    fn bar(&self, canvas: &mut Canvas, start: f64, end: f64, done: bool) {
        let limit = self.axis * 8;
        let to = ((end * 8.0).round().max(0.0) as usize).clamp(1, limit);
        let from = ((start * 8.0).round().max(0.0) as usize).min(to - 1);
        if done {
            let first = from / 8;
            let last = to.div_ceil(8).max(first + 1).min(self.axis);
            for x in first..last {
                canvas.put(self.left + x, self.row, '▒', self.ink);
            }
            return;
        }
        for x in from / 8..to.div_ceil(8) {
            let (cell_start, cell_end) = (x * 8, x * 8 + 8);
            let (lit_from, lit_to) = (from.max(cell_start), to.min(cell_end));
            let glyph = match (lit_from == cell_start, lit_to == cell_end) {
                (true, true) => '█',
                // Filled from the left, as the end of a bar.
                (true, false) => EIGHTHS[lit_to - cell_start],
                // Filled to the right, as the start of one.
                (false, true) if lit_to - lit_from >= 6 => '█',
                (false, true) if lit_to - lit_from >= 3 => '▐',
                (false, true) => '▕',
                // Inside one cell.
                (false, false) => '▌',
            };
            canvas.put(self.left + x, self.row, glyph, self.ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn gantt(source: &str) -> Option<Gantt> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&gantt(source).expect("parses"), width).expect("draws"))
    }

    fn day(year: i64, month: i64, day: i64) -> i64 {
        days_from_civil(year, month, day) * DAY
    }

    const DOCS: &str = "    dateFormat  YYYY-MM-DD
    title       Adding GANTT diagram functionality to mermaid
    excludes    weekends
    section A section
    Completed task            :done,    des1, 2014-01-06,2014-01-08
    Active task               :active,  des2, 2014-01-09, 3d
    Future task               :         des3, after des2, 5d
    Future task2              :         des4, after des3, 5d
    section Critical tasks
    Completed task in the critical line :crit, done, 2014-01-06,24h
    Implement parser and jison          :crit, done, after des1, 2d
    Create tests for parser             :crit, active, 3d
    Future task in critical line        :crit, 5d
    Create tests for renderer           :2d
    Add to mermaid                      :until isadded
    Functionality added                 :milestone, isadded, 2014-01-25, 0d";

    #[test]
    fn dates_lengths_after_and_until_are_worked_out() {
        let g = gantt(DOCS).unwrap();
        assert_eq!(
            g.title.as_deref(),
            Some("Adding GANTT diagram functionality to mermaid")
        );
        assert_eq!(g.sections, ["A section", "Critical tasks"]);
        let t = &g.tasks;
        assert_eq!((t[0].start, t[0].end), (day(2014, 1, 6), day(2014, 1, 8)));
        assert!(t[0].done);
        assert_eq!((t[1].start, t[1].end), (day(2014, 1, 9), day(2014, 1, 12)));
        assert_eq!(t[2].start, day(2014, 1, 12), "after des2");
        assert_eq!(t[3].end, day(2014, 1, 22));
        assert_eq!(t[4].end, day(2014, 1, 7), "24h");
        assert_eq!(t[5].start, day(2014, 1, 8), "after des1");
        assert_eq!(t[6].start, t[5].end, "follows the task before");
        // `until` names a milestone written after it.
        assert_eq!(t[9].end, day(2014, 1, 25));
        assert!(t[10].milestone);
        assert_eq!(t[10].section, 1);
        assert_eq!(g.axis, Axis::Calendar);
    }

    #[test]
    fn date_formats_are_read_as_written() {
        let read = |format: &str, text: &str| Format::new(format).and_then(|f| f.read(text));
        assert_eq!(read("YYYY-MM-DD", "2024-02-29"), Some(day(2024, 2, 29)));
        assert_eq!(read("YYYY-MM-DD", "2023-02-29"), None, "not a leap year");
        assert_eq!(read("DD/MM/YY", "05/11/99"), Some(day(1999, 11, 5)));
        assert_eq!(
            read("YYYY-MM-DD HH:mm", "2024-01-01 13:30"),
            Some(day(2024, 1, 1) + 13 * HOUR + 30 * MINUTE)
        );
        assert_eq!(read("HH:mm:ss", "10:00:05"), Some(10 * HOUR + 5 * SECOND));
        assert_eq!(read("X", "86400"), Some(DAY));
        assert_eq!(read("x", "1500"), Some(1500));
        assert_eq!(read("YYYYMMDD", "20240115"), Some(day(2024, 1, 15)));
        assert_eq!(read("YYYY-MM-DD", "2024-13-01"), None);
        assert_eq!(read("YYYY-MM-DD", "2024-01-01x"), None);
        assert!(
            Format::new("MMM D, YYYY").is_none(),
            "month names aren't read"
        );
        assert_eq!(
            civil_from_days(days_from_civil(1999, 12, 31)),
            (1999, 12, 31)
        );
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn lengths_alone_start_from_day_nought() {
        let g = gantt("  Design : 5d\n  Build : 10d\n  Ship : milestone, 0d").unwrap();
        assert_eq!(g.axis, Axis::Elapsed);
        assert_eq!((g.tasks[1].start, g.tasks[1].end), (5 * DAY, 15 * DAY));
        let out = drawn("  Design : 5d\n  Build : 10d", 80);
        assert!(out.contains("0d") && out.contains("Design"), "{out}");
    }

    #[test]
    fn broken_charts_are_not_drawn() {
        for source in [
            "",
            "title Only a title",
            "section Only a section",
            "Task without metadata",
            "Task : after nobody, 3d",
            "Task : 2024-01-01, until nobody",
            "Task : 2024-01-01, 3 days",
            "Task : 2024-02-30, 3d",
            "Task : 2024-01-05, 2024-01-01",
            "Task : a, b, 2024-01-01, 3d",
            "Task : 99999999999999999999d",
            "dateFormat MMMM\nTask : January, 3d",
            "A : a1, after b1, 1d\nB : b1, after a1, 1d",
        ] {
            assert!(gantt(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_chart_draws_with_an_axis_headings_bars_and_milestones() {
        let out = drawn(DOCS, 100);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[0], "Adding GANTT diagram functionality to mermaid");
        assert!(rows[2].contains("01-"), "{out}");
        assert!(rows[3].contains('┬') && rows[3].contains('─'), "{out}");
        assert!(rows[4].starts_with("A section"), "{out}");
        assert!(
            rows[5].starts_with("  Completed task") && rows[5].contains('▒'),
            "{out}"
        );
        assert!(rows[6].contains('█'), "{out}");
        assert!(out.contains('◆'), "{out}");
        for row in &rows {
            assert!(row.width() <= 100, "{row}");
        }
    }

    #[test]
    fn each_section_has_its_colour_and_the_words_are_readable() {
        let diagram = draw(&gantt(DOCS).unwrap(), 100).unwrap();
        assert_eq!(diagram.keys, [key("A section"), key("Critical tasks")]);
        assert_eq!(diagram.links, [(0, 1)]);
        let bars: std::collections::BTreeSet<Option<u16>> = diagram
            .rows
            .iter()
            .flatten()
            .filter(|(text, _)| text.contains('█'))
            .map(|(_, ink)| ink.owner)
            .collect();
        assert_eq!(bars.len(), 2, "{bars:?}");
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
    fn vertical_markers_take_no_row_and_are_named_under_the_chart() {
        let out = drawn(
            "dateFormat HH:mm\nsection Day\nWork : 09:00, 8h\nLunch : vert, v1, 12:30, 0m\nDone : milestone, 17:00, 0m",
            80,
        );
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows.len(), 2 + 1 + 2 + 1, "{out}");
        assert!(rows.last().unwrap().contains("Lunch"), "{out}");
        assert!(out.contains('┆'), "{out}");
    }

    #[test]
    fn short_and_long_spans_are_marked_sensibly() {
        let hours = drawn(
            "dateFormat HH:mm\nStand-up : 09:00, 15m\nWork : 09:15, 3h",
            80,
        );
        assert!(hours.contains("09:") || hours.contains("10:00"), "{hours}");
        let years = drawn("Plan : 2020-01-01, 2024-06-30", 80);
        assert!(years.contains("2021") && years.contains("2023"), "{years}");
        let close = drawn("A : 2024-01-01, 10d\nB : 2024-01-01, 11d", 60);
        let rows: Vec<&str> = close.lines().collect();
        assert_ne!(rows[2], rows[3], "a day apart, drawn apart:\n{close}");
    }

    #[test]
    fn huge_unicode_and_odd_charts_stay_bounded() {
        let mut many = String::from("dateFormat YYYY-MM-DD\n");
        for at in 0..=MAX_TASKS {
            many.push_str(&format!("Task {at} : 1d\n"));
        }
        assert!(gantt(&many).is_none(), "too many tasks");
        let mut chain = String::from("dateFormat YYYY-MM-DD\nFirst : t0, 2024-01-01, 1d\n");
        for at in 1..MAX_TASKS {
            chain.push_str(&format!("Task {at} : t{at}, after t{}, 1d\n", at - 1));
        }
        let g = gantt(&chain).unwrap();
        let drawn_chain = draw(&g, 120).unwrap();
        assert!(drawn_chain.rows.len() * drawn_chain.width <= super::super::canvas::MAX_CELLS);

        let out = drawn(
            "section 段落\n日本語のタスク : 2024-01-01, 3d\n🙂 emoji : 2d",
            80,
        );
        assert!(out.contains("日本語のタスク"), "{out}");
        if let Some(wide) = gantt("A : 0001-01-01, 9999-12-31") {
            let _ = draw(&wide, 80);
        }
        for width in [0, 1, 1000] {
            let _ = draw(&gantt(DOCS).unwrap(), width);
        }
    }
}
