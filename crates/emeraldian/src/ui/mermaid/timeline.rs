//! Timelines: `timeline`.
//!
//! Drawn down the page rather than across it, which suits a terminal however
//! many periods there are: each period's name on the left, a dot for it on a
//! rule running down beside the names, and its events to the right, one
//! under another, each branching off the rule. Sections are headings over
//! their periods, each in a colour of its own that their rule, dots and names
//! are drawn in too; without sections, each period has a colour of its own.
//! Names and events wrap to fit the pane, and `<br>` breaks a line where it
//! is written.

use unicode_width::UnicodeWidthStr;

use super::canvas::{Canvas, Stroke};
use super::label::{self, wrap};
use super::{Diagram, Ink, Role, key, syntax};

/// Past these a timeline is too long to read in a terminal.
const MAX_PERIODS: usize = 300;
const MAX_EVENTS: usize = 600;
/// The narrowest pane laid out for: a narrower one pans across it.
const LEAST_WIDTH: usize = 32;
/// Columns events wrap at, at most, so a wide pane doesn't stretch them too
/// long to read.
const EVENT_WIDTH: usize = 60;
/// Columns a period's name takes, at most, before it wraps.
const NAME_WIDTH: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Period {
    /// Its name, in the lines `<br>` breaks it into.
    pub name: Vec<String>,
    /// Each event, in the lines `<br>` breaks it into.
    pub events: Vec<Vec<String>>,
    /// The section it is in, by its place in [`Timeline::sections`].
    pub section: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub title: Option<String>,
    pub sections: Vec<Vec<String>>,
    pub periods: Vec<Period>,
}

/// Reads the lines after `timeline`.
pub fn parse(lines: &[&str]) -> Option<Timeline> {
    let mut timeline = Timeline {
        title: None,
        sections: Vec::new(),
        periods: Vec::new(),
    };
    let mut events = 0;
    // Inside an `accDescr { … }` block, which describes the timeline for a
    // screen reader in a browser.
    let mut skipping = false;
    for line in lines {
        let line = line.trim();
        if skipping {
            skipping = !line.contains('}');
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = syntax::keyword(line, "title") {
            timeline.title = Some(label::unwrapped(rest).join(" "));
            continue;
        }
        if let Some(rest) = syntax::keyword(line, "section") {
            timeline.sections.push(label::unwrapped(rest));
            continue;
        }
        if line.starts_with("accTitle") || line.starts_with("accDescr") {
            skipping = line.contains('{') && !line.contains('}');
            continue;
        }
        // `period : event : event`, or `: event` for more of the one before.
        let (name, rest) = match line.strip_prefix(':') {
            Some(rest) => (None, rest),
            None => match line.split_once(':') {
                Some((name, rest)) => (Some(name), rest),
                None => (Some(line), ""),
            },
        };
        let found: Vec<Vec<String>> = rest
            .split(':')
            .map(label::unwrapped)
            .filter(|event| !event.is_empty())
            .collect();
        events += found.len();
        if events > MAX_EVENTS {
            return None;
        }
        match name {
            Some(name) => {
                if timeline.periods.len() >= MAX_PERIODS {
                    return None;
                }
                timeline.periods.push(Period {
                    name: label::unwrapped(name),
                    events: found,
                    section: timeline.sections.len().checked_sub(1),
                });
            }
            None => timeline.periods.last_mut()?.events.extend(found),
        }
    }
    if timeline.periods.is_empty() {
        return None;
    }
    Some(timeline)
}

/// Lines wrapped to fit `width` columns.
fn fit(lines: &[String], width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        wrap(line, width.max(1), &mut out);
    }
    out
}

/// Where a period's dot, name and events go.
struct Placed {
    /// Its row, which its dot and the first line of its name are on.
    row: usize,
    /// The row its stretch of the rule runs down to: the next period's dot,
    /// or its own last event at the end of a group.
    end: usize,
    name: Vec<String>,
    /// Each line of its events, with its row and whether an event starts
    /// there, branching off the rule.
    events: Vec<(usize, String, bool)>,
    owner: usize,
}

/// Draws a timeline to fit a pane `width` columns wide, or `None` when it is
/// too big.
pub fn draw(timeline: &Timeline, width: usize) -> Option<Diagram> {
    let room = width.max(LEAST_WIDTH);
    let sectioned = !timeline.sections.is_empty();
    let indent = if sectioned { 2 } else { 0 };
    let widest_name = timeline
        .periods
        .iter()
        .flat_map(|period| &period.name)
        .map(|line| line.width())
        .max()
        .unwrap_or(0);
    let name_width = widest_name.min(NAME_WIDTH).min((room / 3).max(8));
    // The rule's column, a space clear of the names, and where events start,
    // past the branch to them.
    let rule = indent + name_width + 1;
    let text = rule + 4;
    let event_width = room.saturating_sub(text).clamp(12, EVENT_WIDTH);

    // Each run of periods in one section is a group, with the colour it is
    // drawn in: the section's, or each period's own when there are none.
    let mut owners: Vec<String> = Vec::new();
    let mut groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
    for (at, period) in timeline.periods.iter().enumerate() {
        match groups.last_mut() {
            Some((section, members)) if *section == period.section => members.push(at),
            _ => groups.push((period.section, vec![at])),
        }
    }

    let mut row = 0;
    let mut width = 0;
    let title = label::title(timeline.title.as_deref(), room);
    row += label::above(&title);
    let mut headings: Vec<(usize, Vec<String>, usize)> = Vec::new();
    let mut placed: Vec<Placed> = Vec::new();
    for (group, (section, members)) in groups.iter().enumerate() {
        if group > 0 {
            row += 1;
        }
        let heading = section
            .and_then(|section| timeline.sections.get(section))
            .filter(|_| sectioned);
        if sectioned {
            let name = heading.map(|lines| lines.join(" ")).unwrap_or_default();
            owners.push(name);
        }
        if let Some(lines) = heading {
            let lines = fit(lines, room);
            headings.push((row, lines.clone(), owners.len() - 1));
            row += lines.len();
        }
        let first = placed.len();
        for (k, &at) in members.iter().enumerate() {
            let period = &timeline.periods[at];
            if !sectioned {
                owners.push(period.name.join(" "));
            }
            let name = fit(&period.name, name_width);
            let mut events = Vec::new();
            let mut line = 0;
            for event in &period.events {
                for (n, text) in fit(event, event_width).into_iter().enumerate() {
                    events.push((row + line, text, n == 0));
                    line += 1;
                }
            }
            let end = events
                .iter()
                .filter(|(_, _, starts)| *starts)
                .map(|(row, _, _)| *row)
                .max()
                .unwrap_or(row);
            let height = name.len().max(line).max(1);
            placed.push(Placed {
                row,
                end,
                name,
                events,
                owner: owners.len() - 1,
            });
            row += height;
            if k + 1 < members.len() {
                row += 1;
            }
        }
        for at in first + 1..placed.len() {
            placed[at - 1].end = placed[at].row;
        }
    }
    for line in title.iter().chain(headings.iter().flat_map(|h| &h.1)) {
        width = width.max(line.width());
    }
    for period in &placed {
        width = width.max(rule + 3);
        for (_, line, _) in &period.events {
            width = width.max(text + line.width());
        }
    }
    let mut canvas = Canvas::new(width, row)?;

    label::draw_title(&mut canvas, &title);
    for (row, lines, owner) in &headings {
        for (at, line) in lines.iter().enumerate() {
            canvas.text(0, row + at, line, Ink::of(Role::Label, *owner));
        }
    }
    for period in &placed {
        let owner = Ink::of(Role::Line, period.owner).owner;
        if period.end > period.row {
            canvas.line((rule, period.row), (rule, period.end), Stroke::Solid, owner);
        }
        for (row, line, starts) in &period.events {
            if *starts {
                canvas.line((rule, *row), (rule + 2, *row), Stroke::Solid, owner);
            }
            canvas.text(text, *row, line, Ink::plain(Role::Text));
        }
        canvas.put(rule, period.row, '●', Ink::of(Role::Border, period.owner));
        for (k, line) in period.name.iter().enumerate() {
            let x = (rule - 1).saturating_sub(line.width());
            canvas.text(x, period.row + k, line, Ink::of(Role::Label, period.owner));
        }
    }

    let mut diagram = canvas.finish();
    diagram.keys = owners.iter().map(|name| key(name)).collect();
    diagram.links = (1..owners.len())
        .filter_map(|k| Some((u16::try_from(k - 1).ok()?, u16::try_from(k).ok()?)))
        .collect();
    Some(diagram)
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn timeline(source: &str) -> Option<Timeline> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&timeline(source).expect("parses"), width).expect("draws"))
    }

    const SOCIAL: &str = "    title History of Social Media Platform
    2002 : LinkedIn
    2004 : Facebook : Google
    2005 : YouTube
    2006 : Twitter";

    const INDUSTRY: &str = "    title Timeline of Industrial Revolution
    section 17th-20th century
        Industry 1.0 : Machinery, Water power, Steam <br>power
        Industry 2.0 : Electricity, Internal combustion engine, Mass production
        Industry 3.0 : Electronics, Computers, Automation
    section 21st century
        Industry 4.0 : Internet, Robotics, Internet of Things
        Industry 5.0 : Artificial intelligence, Big data, 3D printing
                     : Quantum computing";

    #[test]
    fn periods_events_and_sections_are_read() {
        let t = timeline(INDUSTRY).unwrap();
        assert_eq!(
            t.title.as_deref(),
            Some("Timeline of Industrial Revolution")
        );
        assert_eq!(t.sections, [["17th-20th century"], ["21st century"]]);
        assert_eq!(t.periods.len(), 5);
        assert_eq!(t.periods[0].name, ["Industry 1.0"]);
        assert_eq!(
            t.periods[0].events,
            [vec![
                "Machinery, Water power, Steam".to_string(),
                "power".to_string()
            ]]
        );
        assert_eq!(t.periods[3].section, Some(1));
        assert_eq!(
            t.periods[4].events.len(),
            2,
            "a line of its own adds an event"
        );

        let t = timeline(SOCIAL).unwrap();
        assert!(t.sections.is_empty());
        assert_eq!(t.periods[1].events, [["Facebook"], ["Google"]]);
        assert_eq!(t.periods[0].section, None);
    }

    #[test]
    fn broken_timelines_are_not_drawn() {
        for source in [
            "",
            "title Only a title",
            ": an event before any period",
            "section Empty",
        ] {
            assert!(timeline(source).is_none(), "{source:?} should not parse");
        }
        assert!(timeline("accDescr {\n  2002 : not a period\n}\n2003 : real").is_some());
        assert_eq!(
            timeline("accDescr {\n2002 : hidden\n}\n2003 : real")
                .unwrap()
                .periods
                .len(),
            1
        );
    }

    #[test]
    fn a_timeline_runs_down_the_page() {
        let out = drawn(SOCIAL, 80);
        let expected = "\
History of Social Media Platform

2002 ●── LinkedIn
     │
2004 ●── Facebook
     ├── Google
     │
2005 ●── YouTube
     │
2006 ●── Twitter";
        assert_eq!(out, expected);
    }

    #[test]
    fn sections_head_their_periods_and_long_events_wrap() {
        let out = drawn(INDUSTRY, 60);
        let rows: Vec<&str> = out.lines().collect();
        assert_eq!(rows[2], "17th-20th century");
        assert_eq!(rows[3], "  Industry 1.0 ●── Machinery, Water power, Steam");
        assert_eq!(rows[4], "               │   power");
        assert!(rows.iter().all(|row| row.width() <= 60), "{out}");
        assert!(out.contains("21st century"), "{out}");
        assert!(out.contains("╰── Quantum computing"), "{out}");
    }

    #[test]
    fn sections_and_periods_have_their_colours() {
        let diagram = draw(&timeline(INDUSTRY).unwrap(), 100).unwrap();
        assert_eq!(
            diagram.keys,
            [key("17th-20th century"), key("21st century")]
        );
        assert_eq!(diagram.links, [(0, 1)]);
        let ink_of = |needle: &str| {
            diagram
                .rows
                .iter()
                .flatten()
                .find(|(text, _)| text.contains(needle))
                .map(|(_, ink)| *ink)
        };
        assert_eq!(ink_of("21st century"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("Industry 4.0"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("Internet"), Some(Ink::plain(Role::Text)));
        assert_eq!(ink_of("●"), Some(Ink::of(Role::Border, 0)));

        let diagram = draw(&timeline(SOCIAL).unwrap(), 100).unwrap();
        assert_eq!(diagram.keys.len(), 4, "a colour for each period");
    }

    #[test]
    fn narrow_panes_and_unicode_keep_their_columns() {
        let source = "2002年 : 日本語の長い出来事がここにあります。さらに続きます : 🙂 smiles\nA very long period name that wraps : café";
        for width in [1, 40, 100, 160] {
            let out = drawn(source, width);
            let widest = out.lines().map(UnicodeWidthStr::width).max().unwrap();
            assert!(widest <= width.max(LEAST_WIDTH), "{width}:\n{out}");
            assert!(out.contains("🙂") && out.contains("café"), "{out}");
        }
    }

    #[test]
    fn huge_timelines_stay_bounded() {
        let mut many = String::new();
        for at in 0..MAX_PERIODS + 1 {
            many.push_str(&format!("{at} : event\n"));
        }
        assert!(timeline(&many).is_none(), "too many periods");

        let mut long = String::new();
        for at in 0..200 {
            long.push_str(&format!("{at} : {} : more\n", "word ".repeat(30)));
        }
        let start = std::time::Instant::now();
        if let Some(diagram) = draw(&timeline(&long).unwrap(), 40) {
            assert!(diagram.rows.len() * diagram.width <= super::super::canvas::MAX_CELLS);
        }
        assert!(start.elapsed().as_secs() < 2);
    }

    #[test]
    fn mangled_timelines_never_panic() {
        let sample = format!("timeline\n{INDUSTRY}\n        accTitle: Ages");
        let lines = |text: &str| -> Vec<String> { text.lines().map(str::to_string).collect() };
        for (at, c) in sample.char_indices() {
            let cut = &sample[..at];
            let dropped = format!("{}{}", &sample[..at], &sample[at + c.len_utf8()..]);
            for width in [1, 40, 200] {
                let _ = super::super::draw(&lines(cut), width);
                let _ = super::super::draw(&lines(&dropped), width);
            }
        }
    }
}
