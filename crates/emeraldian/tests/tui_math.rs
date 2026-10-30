//! End-to-end proof that the reading pane sets maths with utftex.
//!
//! These tests drive the real binary in an emulated terminal through
//! `tui-test`, and assert on what actually reaches the screen. The layout
//! tests in `ui::note` prove the renderer's own rows; this file proves the
//! path a reader takes — argv, config, the event loop, the reading cache —
//! ends in the same glyphs, including for the formula that was reported as
//! not rendering.
//!
//! Everything here is deterministic: fixed terminal sizes, unique temporary
//! vaults and config directories, and bounded waits on screen content rather
//! than sleeps.
//!
//! The suite runs on Linux and macOS. On Windows it would not be isolated: the
//! app resolves its config, state and cache directories through `dirs`, whose
//! Windows backend asks the Known Folder API and ignores `%APPDATA%`,
//! `%LOCALAPPDATA%` and `%USERPROFILE%`, and there is no config-path override
//! to fall back on. The fixture could not keep a run out of the developer's
//! real profile — it would read their settings and write their state — so the
//! suite is gated rather than left to depend on the machine it runs on. Nothing
//! in the app is Unix-specific; the rest of the workspace tests cover Windows.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tui_test::{
    AutomaticRecording, AutomaticRecordingMode, KeyAction, OpenOptions, Operation, OperationResult,
    RunOptions, Session, Timeouts,
};

/// Generous enough for a debug build on a loaded machine, small enough that a
/// hang fails the suite rather than the job.
const TIMEOUT_MS: u64 = 10_000;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A throwaway vault plus the config and state directories a run writes to.
///
/// Everything the binary would touch in `$HOME` is redirected here, so a test
/// can never read or rewrite the developer's real config, and parallel tests
/// cannot see each other's UI state.
struct Fixture {
    root: PathBuf,
    vault: PathBuf,
    config_home: PathBuf,
    home: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let serial = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "emeraldian-tui-{label}-{}-{serial}",
            std::process::id()
        ));
        // A crashed earlier run may have left the same name behind.
        let _ = std::fs::remove_dir_all(&root);
        let vault = root.join("vault");
        let config_home = root.join("config");
        let home = root.join("home");
        for dir in [&vault, &config_home, &home] {
            std::fs::create_dir_all(dir).expect("create fixture directory");
        }
        let fixture = Self {
            root,
            vault,
            config_home,
            home,
        };
        // Pictures off: a probe would ask the terminal what it can draw, and
        // this one has no graphics protocol to offer. Sidebars and the ribbon
        // off so the reading pane gets the width an equation needs.
        let config_dir = fixture.config_dir();
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            "[images]\nenabled = false\n\n\
             [ui]\nshow_left_sidebar = false\nshow_right_sidebar = false\n\
             show_ribbon = false\nshow_hints = false\n",
        )
        .expect("write config");
        fixture
    }

    /// The directory the app reads `config.toml` from, under this fixture.
    ///
    /// The app asks `dirs::config_dir()`, which follows each platform's own
    /// convention: `$XDG_CONFIG_HOME` (or `~/.config`) on Linux, but
    /// `~/Library/Application Support` on macOS, which ignores the XDG
    /// variables. The file has to land where the app actually looks, or the
    /// run would silently use the defaults, sidebars up and images probed.
    fn config_dir(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            self.home.join("Library/Application Support/emeraldian")
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.config_home.join("emeraldian")
        }
    }

    fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.vault.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create note directory");
        }
        std::fs::write(&path, body).expect("write note");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A live session that closes itself, so a failing assertion still tears the
/// child down instead of leaking it into the next test.
struct Tui {
    session: Session,
}

impl Tui {
    /// Runs the binary on `note` at a fixed size and waits for its first frame.
    fn open(label: &str, fixture: &Fixture, note: &Path, cols: u16, rows: u16) -> Self {
        let session = Session::new(format!("{label}-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        let defaults = OpenOptions::default();
        let options = RunOptions {
            program: env!("CARGO_BIN_EXE_emeraldian").to_string(),
            args: vec![note.to_string_lossy().into_owned()],
            backend: defaults.backend,
            profile: defaults.profile,
            style: defaults.style,
            cols,
            rows,
            cwd: None,
            env: vec![
                ("HOME".into(), fixture.home.display().to_string()),
                (
                    "XDG_CONFIG_HOME".into(),
                    fixture.config_home.display().to_string(),
                ),
                (
                    "XDG_DATA_HOME".into(),
                    fixture.config_home.display().to_string(),
                ),
                (
                    "XDG_CACHE_HOME".into(),
                    fixture.config_home.display().to_string(),
                ),
                // The app asks the terminal whether it speaks the Kitty
                // keyboard protocol and gives it a second to answer. This
                // terminal does not answer, and the question is skipped inside
                // a multiplexer, so claiming to be in one keeps start-up from
                // costing a second per test.
                ("TMUX".into(), "emeraldian-tui-test".into()),
                ("TERM".into(), "xterm-256color".into()),
            ],
            // No shell integration markers: the app is not a shell.
            wait_ready: Some(false),
            restart: false,
            timeouts: Timeouts {
                text: Some(TIMEOUT_MS),
                idle: Some(TIMEOUT_MS),
                ready: Some(TIMEOUT_MS),
                ..Default::default()
            },
            recording: AutomaticRecording {
                mode: AutomaticRecordingMode::Disabled,
                directory: None,
            },
        };
        session.run(options).expect("start emeraldian");
        let tui = Self { session };
        tui.settle();
        tui
    }

    /// Waits for the app to stop painting, so the screen is not read mid-frame.
    fn settle(&self) {
        self.session
            .execute(Operation::WaitIdle {
                timeout_ms: Some(TIMEOUT_MS),
            })
            .expect("the app goes idle");
    }

    fn wait_for(&self, text: &str) {
        self.session
            .get_by_text(text)
            .wait_with_timeout(Some(TIMEOUT_MS))
            .unwrap_or_else(|err| panic!("{text:?} never appeared: {err}\n{}", self.screen()));
    }

    fn press(&self, keys: &[&str]) {
        self.session
            .execute(Operation::Key {
                keys: keys.iter().map(|key| (*key).to_string()).collect(),
                action: KeyAction::Press,
            })
            .expect("send keys");
        self.settle();
    }

    fn resize(&self, cols: u16, rows: u16) {
        self.session
            .execute(Operation::Resize { cols, rows })
            .expect("resize");
        self.settle();
    }

    /// The visible screen as one string, rows separated by newlines.
    fn screen(&self) -> String {
        match self.session.execute(Operation::Text { full: false }) {
            Ok(OperationResult::Text(text)) => text,
            Ok(other) => panic!("expected text, got {other:?}"),
            Err(err) => panic!("read screen: {err}"),
        }
    }

    /// The screen with its rows trimmed, for finding a stack of rows.
    fn rows(&self) -> Vec<String> {
        self.screen()
            .lines()
            .map(|row| row.trim().to_string())
            .collect()
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = self.session.close();
    }
}

/// The screen with the non-breaking spaces the reading pane puts inside an
/// inline formula shown as the plain spaces they read as. They are there on
/// purpose — `wrap_spans` only breaks on an ASCII space, so a formula stays
/// whole and moves to the next row rather than splitting mid-expression — but
/// they should not make an assertion about visible text awkward.
fn readable(screen: &str) -> String {
    screen.replace('\u{a0}', " ")
}

/// Whether three consecutive rows are `top`, `rule`, `bottom`.
fn has_stack(rows: &[String], top: &str, rule: &str, bottom: &str) -> bool {
    rows.windows(3).any(|window| window == [top, rule, bottom])
}

/// Columns before `needle` on the row that holds it.
fn indent_of(screen: &str, needle: &str) -> usize {
    let row = screen
        .lines()
        .find(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no row holds {needle:?}:\n{screen}"));
    row.find(needle).expect("found above")
}

#[test]
fn the_reported_note_is_typeset_by_utftex() {
    // The note from the report, byte for byte. It used to show as typed.
    let fixture = Fixture::new("reported");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n$$\nf(x) = \\int_{-\\infty}^\\infty \\hat{f}(\\xi) e^{2\\pi i \\xi x} d\\xi\n$$\n",
    );
    let tui = Tui::open("reported", &fixture, &note, 100, 34);
    tui.wait_for("f(x)");
    let screen = tui.screen();

    // The equation, as utftex draws it: the integral sign is three rows of
    // box-drawing glyphs, the hat is a combining accent, and the limits are
    // set above and below.
    assert!(screen.contains("f(x)"), "{screen}");
    for glyph in ['⌠', '⎮', '⌡'] {
        assert!(
            screen.contains(glyph),
            "the integral's {glyph:?} is missing:\n{screen}"
        );
    }
    assert!(screen.contains('∞'), "{screen}");
    assert!(screen.contains('ξ'), "{screen}");
    // `\hat{f}` sets as an f with a combining circumflex.
    assert!(screen.contains("f\u{302}"), "{screen}");
    // The lower limit. utftex 1.31 sets a literal `-` as U+002D (a hyphen,
    // not the U+2212 minus the hand renderer uses); `\minus` would give
    // U+2212, but the note says `-\infty`, so this is the engine's glyph.
    assert!(
        screen.contains("-∞"),
        "the lower limit is missing:\n{screen}"
    );

    // None of the TeX survives into the reading pane.
    for raw in ["$$", "\\int", "\\hat", "\\xi", "d\\xi", "\\infty"] {
        assert!(
            !screen.contains(raw),
            "{raw:?} is shown as typed:\n{screen}"
        );
    }
}

#[test]
fn inline_maths_is_set_in_the_line() {
    let fixture = Fixture::new("inline");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\nEnergy is $x^2 + y_1$ roughly.\n",
    );
    let tui = Tui::open("inline", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let screen = readable(&tui.screen());

    assert!(
        screen.contains("Energy is x² + y₁ roughly."),
        "the formula is not set in the line:\n{screen}"
    );
    assert!(!screen.contains('$'), "a delimiter is shown:\n{screen}");
    assert!(!screen.contains('\\'), "raw TeX is shown:\n{screen}");
}

#[test]
fn a_display_fraction_is_stacked() {
    let fixture = Fixture::new("frac");
    let note = fixture.write("test lol.md", "# test lol\n\n$$\\frac{a}{b}$$\n");
    let tui = Tui::open("frac", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let rows = tui.rows();

    assert!(
        has_stack(&rows, "a", "─", "b"),
        "the fraction is not stacked:\n{}",
        rows.join("\n")
    );
    assert!(
        !rows.iter().any(|row| row.contains("frac")),
        "the TeX is shown as typed:\n{}",
        rows.join("\n")
    );
}

#[test]
fn maths_fences_are_typeset_for_every_alias() {
    // `math`, `latex` and `tex` are the three the reading pane treats as
    // maths. Each gets a construct that only the engine can draw.
    let fixture = Fixture::new("fences");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n\
         ```math\n\\frac{a}{b}\n```\n\n\
         ```latex\n\\sum_{i=1}^n i\n```\n\n\
         ```tex\n\\sqrt{x}\n```\n",
    );
    let tui = Tui::open("fences", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let rows = tui.rows();
    let screen = rows.join("\n");

    assert!(has_stack(&rows, "a", "─", "b"), "{screen}");
    // The summation sign and its limits.
    assert!(screen.contains('⎲') && screen.contains('⎳'), "{screen}");
    // The radical, drawn in box characters.
    assert!(screen.contains('╲') && screen.contains("┌─┐"), "{screen}");

    for raw in ["```", "\\frac", "\\sum", "\\sqrt"] {
        assert!(
            !screen.contains(raw),
            "{raw:?} is shown as typed:\n{screen}"
        );
    }
    for label in ["math", "latex", "tex"] {
        assert!(
            !rows.iter().any(|row| row == label),
            "the fence's {label:?} label is shown:\n{screen}"
        );
    }
}

#[test]
fn an_equation_number_sits_at_the_right_margin() {
    let fixture = Fixture::new("tag");
    let note = fixture.write("test lol.md", "# test lol\n\n$$E = mc^2 \\tag{1}$$\n");
    let tui = Tui::open("tag", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let screen = tui.screen();

    let equation = screen
        .lines()
        .find(|row| row.contains("E = mc²"))
        .unwrap_or_else(|| panic!("the equation is missing:\n{screen}"));
    assert!(
        equation.trim_end().ends_with("(1)"),
        "the number is not on the equation's row: {equation:?}"
    );
    assert!(
        equation.chars().count() >= 95,
        "the number is not at the right margin (row is {} wide): {equation:?}",
        equation.chars().count()
    );
    assert!(!screen.contains("\\tag"), "{screen}");
}

#[test]
fn ctrl_e_shows_the_tex_and_toggles_back() {
    let fixture = Fixture::new("toggle");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n$$\nf(x) = \\int_{-\\infty}^\\infty \\hat{f}(\\xi) e^{2\\pi i \\xi x} d\\xi\n$$\n\n$$\\frac{a}{b}$$\n",
    );
    let tui = Tui::open("toggle", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    assert!(tui.screen().contains('⌡'), "reading mode is typeset");

    // The editor shows the note's own bytes: the delimiters and the TeX.
    tui.press(&["Ctrl+E"]);
    tui.wait_for("\\int");
    let editing = tui.screen();
    for raw in ["$$", "\\int", "\\hat", "\\frac", "d\\xi"] {
        assert!(
            editing.contains(raw),
            "{raw:?} is missing from the editor:\n{editing}"
        );
    }
    assert!(!editing.contains('⌡'), "the editor typeset it:\n{editing}");

    // And back again, to the same setting.
    tui.press(&["Ctrl+E"]);
    tui.wait_for("⌡");
    let reading = tui.screen();
    assert!(!reading.contains("\\int"), "{reading}");
    assert!(!reading.contains("\\frac"), "{reading}");
    assert!(reading.contains("f(x)"), "{reading}");
    assert!(
        reading.contains('─'),
        "the fraction is not stacked:\n{reading}"
    );
}

#[test]
fn a_wide_equation_pans_without_disturbing_the_prose() {
    let fixture = Fixture::new("pan");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n\
         Prose before the equation.\n\n\
         $$abcdefghijklmnopqrstuvwxyz + ABCDEFGHIJKLMNOPQRSTUVWXYZ$$\n\n\
         Prose after the equation.\n",
    );
    let tui = Tui::open("pan", &fixture, &note, 50, 24);
    tui.wait_for("test lol");
    let start = tui.screen();
    assert!(start.contains("abcdefghijklmnopqrstuvwxyz"), "{start}");
    assert!(
        !start.contains("TUVWXYZ"),
        "the whole equation fits:\n{start}"
    );

    // `l` pans four columns at a time. The equation is 55 columns and the
    // prose column 41, so the clamp is 14; four presses reach it, where the
    // last column of the equation comes into view.
    for _ in 0..4 {
        tui.press(&["l"]);
    }
    tui.wait_for("TUVWXYZ");
    let right = tui.screen();
    assert!(
        right.contains("TUVWXYZ"),
        "the right edge is unreachable:\n{right}"
    );
    assert!(
        !right.contains("abcdefghijklmnopqrstuvwxyz"),
        "the equation did not move:\n{right}"
    );
    // Only the wide block pans; the prose around it stays put.
    assert!(right.contains("Prose before the equation."), "{right}");
    assert!(right.contains("Prose after the equation."), "{right}");

    for _ in 0..4 {
        tui.press(&["h"]);
    }
    tui.wait_for("abcdefghijklmnopqrstuvwxyz");
    let left = tui.screen();
    assert!(left.contains("abcdefghijklmnopqrstuvwxyz"), "{left}");
    assert!(left.contains("Prose before the equation."), "{left}");
}

#[test]
fn maths_survives_a_resize() {
    let fixture = Fixture::new("resize");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n$$\nf(x) = \\int_{-\\infty}^\\infty \\hat{f}(\\xi) e^{2\\pi i \\xi x} d\\xi\n$$\n",
    );
    let tui = Tui::open("resize", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let before = tui.screen();
    assert!(before.contains('⌡'), "{before}");

    let centred = indent_of(&before, "f(x)");
    tui.resize(140, 44);
    // A resize arrives out of band, so there is no text to wait for. Poll
    // for the equation's new column instead: a wider pane centres it
    // further in, which is the reading cache laying the note out again
    // rather than reusing the rows it had at the old width.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(TIMEOUT_MS);
    let after = loop {
        let screen = tui.screen();
        if screen
            .lines()
            .find(|row| row.contains("f(x)"))
            .and_then(|row| row.find("f(x)"))
            .is_some_and(|at| at > centred)
        {
            break screen;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the equation was not re-centred after the resize:\n{screen}"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert!(after.contains('⌡'), "the resize lost the setting:\n{after}");
    assert!(after.contains('∞') && after.contains('ξ'), "{after}");
    assert!(!after.contains("\\int"), "{after}");
}

#[test]
fn adversarial_maths_falls_back_without_killing_the_app() {
    let fixture = Fixture::new("adversarial");
    let note = fixture.write(
        "test lol.md",
        "# test lol\n\n\
         One $\\over a b$ two.\n\n\
         Three ${{{{{{{{{{{{{{{{{{{{{{{{{{{{{{x}}}}}}}}}}}}}}}}}}}}}}}}}}}}}}$ four.\n\n\
         Five $a \\over b \\over c \\over d \\over e \\over f \\over g \\over h \
         \\over i \\over j \\over k \\over l \\over m \\over n \\over o \\over p \
         \\over q \\over r \\over s \\over t$ six.\n",
    );
    fixture.write("other.md", "# other\n\nHere is $E=mc^2$.\n");
    let tui = Tui::open("adversarial", &fixture, &note, 100, 34);
    tui.wait_for("test lol");
    let screen = readable(&tui.screen());

    // A leading `\over` segfaults utftex 1.31, so the hand renderer sets it;
    // 30 nested braces and 20 `\over`s are refused for the same reason (stack
    // overflow and a stall). Each still reads, and none crashes the app.
    assert!(screen.contains("One /(ab) two."), "{screen}");
    assert!(screen.contains("Three x four."), "{screen}");
    assert!(
        screen.contains("Five abcdefghijklmnopqrs/t six."),
        "{screen}"
    );

    // The app is still alive and still sets maths in the next note it opens.
    tui.press(&["Ctrl+O"]);
    tui.wait_for("Quick switcher");
    tui.session
        .execute(Operation::Submit {
            data: Some("other".into()),
        })
        .expect("open the other note");
    tui.settle();
    tui.wait_for("E=mc²");
    let next = readable(&tui.screen());
    assert!(next.contains("Here is E=mc²."), "{next}");
}
