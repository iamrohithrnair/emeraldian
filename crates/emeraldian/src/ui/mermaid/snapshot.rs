//! A snapshot of every diagram in a corpus, drawn at several widths, with
//! each run's ink, its keys and its links, so a refactor can be checked to
//! draw exactly what it drew before. Run by hand:
//!
//! ```text
//! MERMAID_CORPUS=corpus.txt MERMAID_SNAPSHOT=out.txt \
//!     cargo test --release -p emeraldian -- --ignored snapshot::snapshot
//! ```
//!
//! The corpus holds records written as `<bytes>\n<source>\n`.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use super::{Diagram, Role, draw};

const WIDTHS: [usize; 6] = [1, 24, 48, 80, 120, 200];

fn corpus() -> Vec<String> {
    let path = std::env::var("MERMAID_CORPUS").expect("MERMAID_CORPUS");
    let data = std::fs::read(path).expect("corpus");
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let line = data[at..].iter().position(|&b| b == b'\n').expect("length") + at;
        let length: usize = std::str::from_utf8(&data[at..line])
            .unwrap()
            .parse()
            .unwrap();
        let start = line + 1;
        out.push(String::from_utf8(data[start..start + length].to_vec()).unwrap());
        at = start + length + 1;
    }
    out
}

fn role(role: Role) -> char {
    match role {
        Role::Text => 'T',
        Role::Border => 'B',
        Role::Line => 'L',
        Role::Arrow => 'A',
        Role::Label => 'W',
        Role::Note => 'N',
    }
}

fn write(out: &mut String, diagram: Option<&Diagram>) {
    let Some(diagram) = diagram else {
        out.push_str("None\n");
        return;
    };
    let _ = writeln!(
        out,
        "width {} keys {:?} links {:?}",
        diagram.width, diagram.keys, diagram.links
    );
    for row in &diagram.rows {
        let text: String = row.iter().map(|(text, _)| text.as_str()).collect();
        out.push_str(&text);
        out.push('\t');
        for (text, ink) in row {
            let owner = ink.owner.map_or("-".to_string(), |o| o.to_string());
            let _ = write!(out, "{}{}*{} ", role(ink.role), owner, text.chars().count());
        }
        out.push('\n');
    }
}

#[test]
#[ignore = "writes a snapshot for the refactor, run by hand"]
fn snapshot() {
    let sources = corpus();
    let path = std::env::var("MERMAID_SNAPSHOT").expect("MERMAID_SNAPSHOT");
    let mut out = String::new();
    let mut times: Vec<(Duration, usize)> = Vec::new();
    for (number, source) in sources.iter().enumerate() {
        let lines: Vec<String> = source.lines().map(str::to_string).collect();
        let mut spent = Duration::ZERO;
        for width in WIDTHS {
            let start = Instant::now();
            let drawn = draw(&lines, width);
            spent += start.elapsed();
            let _ = writeln!(out, "#{number} w{width}");
            write(&mut out, drawn.as_ref());
        }
        times.push((spent, number));
    }
    std::fs::write(&path, out).expect("snapshot");
    times.sort_unstable_by(|a, b| b.cmp(a));
    let total: Duration = times.iter().map(|(t, _)| *t).sum();
    let mut report = format!("{} sources, all widths in {total:?}\n", sources.len());
    for (time, number) in times.iter().take(20) {
        let _ = writeln!(report, "#{number}: {time:?}");
    }
    std::fs::write(format!("{path}.times"), report).expect("times");
}

/// Lays out a note of fifty diagrams of every kind, cold and warm, and
/// prints the medians.
#[test]
#[ignore = "a benchmark, run by hand"]
fn bench_note() {
    use emeraldian_core::markdown;
    use emeraldian_core::test_support::TempVault;
    use emeraldian_theme::{Palette, presets};

    use crate::ui::note::{Ctx, render_document};

    let path = std::env::var("MERMAID_NOTE").expect("MERMAID_NOTE");
    let note = std::fs::read_to_string(path).expect("note");
    let runs: usize = std::env::var("MERMAID_RUNS").map_or(40, |r| r.parse().unwrap());
    let median = |mut times: Vec<Duration>| {
        times.sort_unstable();
        times[times.len() / 2]
    };
    let lay_out = move |note: &str| {
        let vault = TempVault::new("bench-mermaid");
        let index = vault.index();
        let palette = Palette::from(&presets::default_theme());
        let document = markdown::parse(note);
        let start = Instant::now();
        let lines = render_document(&document, &mut Ctx::text(&palette, &index), 100);
        let first = start.elapsed();
        let mut warm = Vec::new();
        for _ in 0..runs {
            let start = Instant::now();
            let again = render_document(&document, &mut Ctx::text(&palette, &index), 100);
            warm.push(start.elapsed());
            assert_eq!(again.len(), lines.len());
        }
        (first, warm, lines.len())
    };
    let mut cold = Vec::new();
    let mut warm = Vec::new();
    let mut rows = 0;
    for _ in 0..runs {
        // A thread of its own each time, so the cache starts empty.
        let note = note.clone();
        let (first, again, height) = std::thread::spawn(move || lay_out(&note))
            .join()
            .expect("lays out");
        cold.push(first);
        warm.push(median(again));
        rows = height;
    }
    println!(
        "{rows} rows; first layout median {:?} (min {:?}); warm re-layout median {:?}",
        median(cold.clone()),
        cold.iter().min().unwrap(),
        median(warm)
    );
}
