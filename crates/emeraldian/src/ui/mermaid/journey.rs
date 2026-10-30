//! User journeys: `journey`.
//!
//! Each section is a heading in a colour of its own, and each task in it a
//! line: its name, a bar of five cells filled as far as its score, the score
//! itself, and who takes part, in quiet words. Mermaid draws each score as a
//! face, higher or lower on the page; a bar reads as quickly, and never
//! depends on how a terminal draws a face. A score past either end of one to
//! five is taken as the end it is past, and a fraction is rounded.

use unicode_width::UnicodeWidthStr;

use super::canvas::Canvas;
use super::label::{self, wrap};
use super::{Diagram, Ink, Role, key, pairs, syntax};

/// Past this a journey is too long to read in a terminal.
const MAX_TASKS: usize = 300;
/// The narrowest pane laid out for: a narrower one pans across it.
const LEAST_WIDTH: usize = 32;
/// Columns a task's name takes, at most, before it wraps, and the same for
/// the people who take part in it.
const NAME_WIDTH: usize = 32;
const ACTOR_WIDTH: usize = 40;
/// The cells of a score's bar, one for each point.
const BAR: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    /// From one to five.
    pub score: u8,
    pub actors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// `None` for the tasks written before any section.
    pub name: Option<String>,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Journey {
    pub title: Option<String>,
    pub sections: Vec<Section>,
}

/// Reads the lines after `journey`.
pub fn parse(lines: &[&str]) -> Option<Journey> {
    let mut journey = Journey::default();
    let mut tasks = 0;
    let mut described = syntax::Descriptions::default();
    for line in lines {
        let line = line.trim();
        if line.is_empty() || described.skip(line) {
            continue;
        }
        if let Some(rest) = syntax::keyword(line, "title") {
            journey.title = Some(words(rest));
            continue;
        }
        if let Some(rest) = syntax::keyword(line, "section") {
            journey.sections.push(Section {
                name: Some(words(rest)),
                tasks: Vec::new(),
            });
            continue;
        }
        // `Task name: score: actor, actor`.
        let mut parts = line.splitn(3, ':');
        let name = words(parts.next()?);
        let score: f64 = parts.next()?.trim().parse().ok()?;
        if name.is_empty() || !score.is_finite() {
            return None;
        }
        let actors = parts
            .next()
            .unwrap_or("")
            .split(',')
            .map(words)
            .filter(|actor| !actor.is_empty())
            .collect();
        tasks += 1;
        if tasks > MAX_TASKS {
            return None;
        }
        if journey.sections.is_empty() {
            journey.sections.push(Section {
                name: None,
                tasks: Vec::new(),
            });
        }
        journey.sections.last_mut()?.tasks.push(Task {
            name,
            score: score.round().clamp(1.0, BAR as f64) as u8,
            actors,
        });
    }
    if tasks == 0 {
        return None;
    }
    Some(journey)
}

/// Text without its markup, on one line.
fn words(raw: &str) -> String {
    super::label::unwrapped(raw).join(" ")
}

/// Where a task goes: the row it starts on, its name and the people in it
/// as the lines they wrap to, its score, and its section's colour.
struct Line {
    row: usize,
    name: Vec<String>,
    people: Vec<String>,
    score: u8,
    owner: usize,
}

/// Draws a journey to fit a pane `width` columns wide, or `None` when it is
/// too big.
pub fn draw(journey: &Journey, width: usize) -> Option<Diagram> {
    let room = width.max(LEAST_WIDTH);
    let named = journey.sections.iter().any(|s| s.name.is_some());
    let indent = if named { 2 } else { 0 };
    let tasks = journey.sections.iter().flat_map(|s| &s.tasks);
    let widest_name = tasks.map(|task| task.name.width()).max().unwrap_or(0);
    // The name gives way first, then the people.
    let name_width = widest_name
        .min(NAME_WIDTH)
        .min(room.saturating_sub(indent + 22).max(8));
    let bar = indent + name_width + 2;
    let score = bar + BAR + 1;
    let actors = score + 3;
    let actor_width = room.saturating_sub(actors).clamp(10, ACTOR_WIDTH);

    let title = label::title(journey.title.as_deref(), room);
    let mut row = label::above(&title);
    let mut width = title.iter().map(|line| line.width()).max().unwrap_or(0);
    // What goes on each row: a section's heading, or a task's name and the
    // people in it, line by line, with its score on the first.
    let mut headings: Vec<(usize, String, usize)> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for (owner, section) in journey.sections.iter().enumerate() {
        if owner > 0 {
            row += 1;
        }
        if let Some(name) = &section.name {
            let mut heading = Vec::new();
            wrap(name, room, &mut heading);
            for line in heading {
                width = width.max(line.width());
                headings.push((row, line, owner));
                row += 1;
            }
        }
        for task in &section.tasks {
            let mut name = Vec::new();
            wrap(&task.name, name_width, &mut name);
            let mut people = Vec::new();
            if !task.actors.is_empty() {
                wrap(&task.actors.join(", "), actor_width, &mut people);
            }
            width = width.max(actors);
            for line in &people {
                width = width.max(actors + line.width());
            }
            let height = name.len().max(people.len()).max(1);
            lines.push(Line {
                row,
                name,
                people,
                score: task.score,
                owner,
            });
            row += height;
        }
    }
    let mut canvas = Canvas::new(width, row)?;

    label::draw_title(&mut canvas, &title);
    for (row, line, owner) in &headings {
        canvas.text(0, *row, line, Ink::of(Role::Label, *owner));
    }
    let empty = Ink::plain(Role::Line);
    for Line {
        row,
        name,
        people,
        score: points,
        owner,
    } in &lines
    {
        for (at, line) in name.iter().enumerate() {
            canvas.text(indent, row + at, line, Ink::plain(Role::Text));
        }
        let filled = Ink::of(Role::Border, *owner);
        for cell in 0..BAR {
            let (glyph, ink) = if cell < usize::from(*points) {
                ('█', filled)
            } else {
                ('░', empty)
            };
            canvas.put(bar + cell, *row, glyph, ink);
        }
        canvas.text(
            score,
            *row,
            &points.to_string(),
            Ink::of(Role::Label, *owner),
        );
        for (at, line) in people.iter().enumerate() {
            canvas.text(actors, row + at, line, Ink::plain(Role::Label));
        }
    }

    let mut diagram = canvas.finish();
    diagram.keys = journey
        .sections
        .iter()
        .map(|section| key(section.name.as_deref().unwrap_or("")))
        .collect();
    diagram.links = pairs((1..journey.sections.len()).map(|k| (k - 1, k)));
    Some(diagram)
}

#[cfg(test)]
mod tests {
    use super::super::plain;
    use super::*;

    fn journey(source: &str) -> Option<Journey> {
        let lines: Vec<&str> = source.lines().collect();
        parse(&lines)
    }

    fn drawn(source: &str, width: usize) -> String {
        plain(&draw(&journey(source).expect("parses"), width).expect("draws"))
    }

    const DAY: &str = "    title My working day
    section Go to work
      Make tea: 5: Me
      Go upstairs: 3: Me
      Do work: 1: Me, Cat
    section Go home
      Go downstairs: 5: Me
      Sit down: 5: Me";

    #[test]
    fn sections_tasks_scores_and_people_are_read() {
        let j = journey(DAY).unwrap();
        assert_eq!(j.title.as_deref(), Some("My working day"));
        assert_eq!(j.sections.len(), 2);
        assert_eq!(j.sections[0].name.as_deref(), Some("Go to work"));
        let work = &j.sections[0].tasks[2];
        assert_eq!(work.name, "Do work");
        assert_eq!(work.score, 1);
        assert_eq!(work.actors, ["Me", "Cat"]);
    }

    #[test]
    fn scores_past_either_end_are_held_to_it() {
        let j = journey("Way up: 9: Me\nWay down: -2\nHalf: 2.6: Me").unwrap();
        let scores: Vec<u8> = j.sections[0].tasks.iter().map(|t| t.score).collect();
        assert_eq!(scores, [5, 1, 3]);
        assert_eq!(j.sections[0].name, None, "tasks before any section");
        assert!(j.sections[0].tasks[1].actors.is_empty());
    }

    #[test]
    fn broken_journeys_are_not_drawn() {
        for source in [
            "",
            "title Only a title",
            "section Empty",
            "A task with no score",
            "Task: lots: Me",
            ": 3: Me",
            "Task: NaN: Me",
        ] {
            assert!(journey(source).is_none(), "{source:?} should not parse");
        }
    }

    #[test]
    fn a_journey_draws_as_bars_under_its_sections() {
        let out = drawn(DAY, 80);
        let expected = "\
My working day

Go to work
  Make tea       █████ 5  Me
  Go upstairs    ███░░ 3  Me
  Do work        █░░░░ 1  Me, Cat

Go home
  Go downstairs  █████ 5  Me
  Sit down       █████ 5  Me";
        assert_eq!(out, expected);
    }

    #[test]
    fn each_section_has_its_colour_and_people_are_quiet() {
        let diagram = draw(&journey(DAY).unwrap(), 80).unwrap();
        assert_eq!(diagram.keys, [key("Go to work"), key("Go home")]);
        assert_eq!(diagram.links, [(0, 1)]);
        let ink_of = |needle: &str| super::super::ink_of(&diagram, needle);
        assert_eq!(ink_of("Go home"), Some(Ink::of(Role::Label, 1)));
        assert_eq!(ink_of("Cat"), Some(Ink::plain(Role::Label)));
        assert_eq!(ink_of("█"), Some(Ink::of(Role::Border, 0)));
        assert_eq!(ink_of("░"), Some(Ink::plain(Role::Line)));
        assert_eq!(ink_of("Make tea"), Some(Ink::plain(Role::Text)));
    }

    #[test]
    fn narrow_panes_and_unicode_keep_their_columns() {
        let source = "section 出勤\n  お茶を淹れるという長い作業の名前: 4: 私, 猫, and a very long list of other people\n  🙂 Smile: 5: Me";
        for width in [1, 40, 100, 160] {
            let out = drawn(source, width);
            let widest = out.lines().map(UnicodeWidthStr::width).max().unwrap();
            assert!(widest <= width.max(LEAST_WIDTH), "{width}:\n{out}");
            assert!(out.contains("出勤") && out.contains("🙂"), "{out}");
        }
    }

    #[test]
    fn huge_journeys_stay_bounded() {
        let mut many = String::new();
        for at in 0..MAX_TASKS + 1 {
            many.push_str(&format!("Task {at}: 3: Me\n"));
        }
        assert!(journey(&many).is_none(), "too many tasks");

        let mut long = String::new();
        for at in 0..MAX_TASKS {
            if at % 10 == 0 {
                long.push_str(&format!("section {}\n", "part ".repeat(40)));
            }
            long.push_str(&format!(
                "{}: {}: {}\n",
                "step ".repeat(30),
                at % 7,
                "someone, ".repeat(20)
            ));
        }
        let start = std::time::Instant::now();
        if let Some(diagram) = draw(&journey(&long).unwrap(), 40) {
            assert!(diagram.rows.len() * diagram.width <= super::super::canvas::MAX_CELLS);
        }
        assert!(start.elapsed().as_secs() < 2);
    }
}
