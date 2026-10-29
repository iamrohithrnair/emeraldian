//! Copying a diagram out of the reading pane.
//!
//! `y`, or a click on the `⧉` at a diagram's top right, puts it on the
//! clipboard as a PNG in the colours it has on screen, for a document or a
//! chat. `Y`, or a right click on the mark, copies its box-drawing text, for
//! anywhere that takes only text. The picture is drawn when it is asked for
//! and dropped as soon as it is on the clipboard.
//!
//! Where the clipboard can't take a picture, over SSH or without a tool for
//! it, the picture is saved to a file instead, and the status bar says where.
//! The text goes the way all copied text does, OSC 52 included, so it gets
//! through where the picture can't.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::app::App;
use crate::clipboard::ImageError;
use crate::ui::mermaid::{Diagram, export};

/// What a diagram is copied as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Image,
    Text,
}

/// Copies a diagram in the reading pane: the note's diagram at `at`, or with
/// `None`, the one nearest the top of the pane.
pub fn diagram(app: &mut App, at: Option<usize>, format: Format) {
    let found = at
        .or_else(|| app.regions.diagrams.first().copied())
        .and_then(|at| Some((at, Rc::clone(&app.reading.diagrams().get(at)?.diagram))));
    let Some((at, diagram)) = found else {
        app.error("no diagram on screen to copy");
        return;
    };
    match format {
        Format::Text => {
            app.clipboard.text(&export::text(&diagram));
            app.info("copied the diagram as text");
        }
        Format::Image => image(app, at, &diagram),
    }
}

fn image(app: &mut App, at: usize, diagram: &Diagram) {
    let picture = match export::picture(diagram, &app.theme.palette) {
        Ok(picture) => picture,
        Err(why) => {
            app.error(format!(
                "couldn't copy the diagram as a picture: {why}. Y copies it as text"
            ));
            return;
        }
    };
    let (why, expected) = match app.clipboard.image(&picture.png) {
        Ok(()) => {
            app.info(format!(
                "copied the diagram as a picture, {}×{} PNG",
                picture.width, picture.height
            ));
            return;
        }
        Err(ImageError::Unreachable(why)) => (why.to_string(), true),
        Err(ImageError::Failed(said)) => (format!("the clipboard refused it: {said}"), false),
    };
    let saved = match app.clipboard.folder() {
        Some(folder) => save(&folder, &file_name(app, at), &picture.png),
        None => Err("there is nowhere to save it".to_string()),
    };
    match saved {
        Ok(path) if expected => app.info(format!("saved the diagram as {} ({why})", shown(&path))),
        Ok(path) => app.error(format!("saved the diagram as {} ({why})", shown(&path))),
        Err(err) => app.error(format!(
            "couldn't copy the diagram ({why}) or save it: {err}"
        )),
    }
}

/// What a saved picture is called: after its note, and which diagram of the
/// note it is, so copying it again replaces it rather than piling up files.
fn file_name(app: &App, at: usize) -> String {
    let title = app
        .active_note()
        .map(|id| app.note_title(id))
        .unwrap_or_default();
    let mut stem = String::new();
    for c in title.chars().take(60) {
        let keep = c.is_alphanumeric() || c == '_';
        if keep {
            stem.push(c);
        } else if !stem.is_empty() && !stem.ends_with('-') {
            stem.push('-');
        }
    }
    let stem = stem.trim_end_matches('-');
    let number = at + 1;
    if stem.is_empty() {
        format!("diagram-{number}.png")
    } else {
        format!("{stem}-diagram-{number}.png")
    }
}

fn save(folder: &Path, name: &str, png: &[u8]) -> Result<PathBuf, String> {
    let path = folder.join(name);
    std::fs::create_dir_all(folder)
        .and_then(|()| std::fs::write(&path, png))
        .map_err(|err| err.to_string())?;
    Ok(path)
}

/// A path as the status bar shows it, with the home folder as `~`.
fn shown(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::stand_in::StandIn;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    const NOTE: &str = "# Plan\n\nThe flow:\n\n```mermaid\ngraph LR\n  A[Start] --> B[End]\n```\n\n```mermaid\nsequenceDiagram\n  Ann->>Bob: hello\n```\n";

    /// A note with two diagrams, read and drawn once, its clipboard a
    /// stand-in.
    fn reading(
        note: &str,
    ) -> (
        TempVault,
        App,
        Rc<std::cell::RefCell<crate::clipboard::stand_in::Copied>>,
    ) {
        let vault = TempVault::new("copy-diagram");
        vault.write("Project plan.md", note);
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let id = app.index.id_of_rel("Project plan.md").expect("indexed");
        app.open_note(id);
        app.active_mut().expect("tab").mode = crate::app::Mode::Reading;
        let (clipboard, copied) = StandIn::new();
        app.clipboard = clipboard;
        draw(&mut app);
        (vault, app, copied)
    }

    fn draw(app: &mut App) {
        let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, app))
            .expect("draw");
    }

    #[test]
    fn the_diagram_nearest_the_top_is_copied_as_a_picture() {
        let (_vault, mut app, copied) = reading(NOTE);
        assert_eq!(app.regions.diagrams, [0, 1], "both on screen, top first");
        diagram(&mut app, None, Format::Image);
        let copied = copied.borrow();
        assert_eq!(copied.images.len(), 1);
        assert!(copied.images[0].starts_with(b"\x89PNG"));
        assert!(!app.status.is_error, "{}", app.status.text);
        assert!(
            app.status
                .text
                .starts_with("copied the diagram as a picture"),
            "{}",
            app.status.text
        );
        assert!(copied.texts.is_empty(), "the picture alone");
    }

    #[test]
    fn as_text_it_is_the_drawing_without_colour() {
        let (_vault, mut app, copied) = reading(NOTE);
        diagram(&mut app, Some(1), Format::Text);
        let text = copied.borrow().texts[0].clone();
        assert!(text.contains("Ann") && text.contains("hello"), "{text}");
        assert!(!text.contains('\x1b'));
        assert_eq!(app.status.text, "copied the diagram as text");
    }

    #[test]
    fn with_no_diagram_on_screen_it_says_so() {
        let (_vault, mut app, copied) = reading("# Nothing to draw\n\nJust words.\n");
        diagram(&mut app, None, Format::Image);
        assert!(app.status.is_error);
        assert_eq!(app.status.text, "no diagram on screen to copy");
        assert!(copied.borrow().images.is_empty());
    }

    #[test]
    fn where_the_clipboard_cant_take_a_picture_it_is_saved_instead() {
        let (vault, mut app, copied) = reading(NOTE);
        let folder = vault.path().join("saved");
        let (mut clipboard, _) = StandIn::new();
        clipboard.refuse = Some(ImageError::Unreachable(
            "no clipboard for pictures over SSH",
        ));
        clipboard.folder = Some(folder.clone());
        app.clipboard = clipboard;

        diagram(&mut app, Some(1), Format::Image);
        let saved = folder.join("Project-plan-diagram-2.png");
        let png = std::fs::read(&saved).expect("saved");
        assert!(png.starts_with(b"\x89PNG"));
        assert!(!app.status.is_error, "expected over SSH, so not an error");
        assert!(
            app.status.text.contains("Project-plan-diagram-2.png"),
            "{}",
            app.status.text
        );
        assert!(app.status.text.contains("over SSH"), "{}", app.status.text);
        assert!(copied.borrow().images.is_empty());
    }

    #[test]
    fn a_tool_that_fails_is_reported_and_the_picture_kept() {
        let (vault, mut app, _) = reading(NOTE);
        let (mut clipboard, _) = StandIn::new();
        clipboard.refuse = Some(ImageError::Failed("xclip failed (exit status: 1)".into()));
        clipboard.folder = Some(vault.path().join("saved"));
        app.clipboard = clipboard;
        diagram(&mut app, None, Format::Image);
        assert!(app.status.is_error);
        assert!(
            app.status.text.contains("xclip failed"),
            "{}",
            app.status.text
        );
        assert!(
            app.status.text.contains("diagram-1.png"),
            "{}",
            app.status.text
        );

        // And with nowhere to save it either, both reasons are given.
        let (mut clipboard, _) = StandIn::new();
        clipboard.refuse = Some(ImageError::Unreachable("no display to copy a picture to"));
        app.clipboard = clipboard;
        diagram(&mut app, None, Format::Image);
        assert!(app.status.is_error);
        assert!(
            app.status.text.contains("no display"),
            "{}",
            app.status.text
        );
        assert!(
            app.status.text.contains("nowhere to save"),
            "{}",
            app.status.text
        );
    }

    #[test]
    fn saved_pictures_are_named_for_their_note() {
        let (_vault, app, _) = reading(NOTE);
        assert_eq!(file_name(&app, 0), "Project-plan-diagram-1.png");
        assert_eq!(shown(Path::new("/nowhere/x.png")), "/nowhere/x.png");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(
                shown(&home.join("a").join("b.png")),
                Path::new("~").join("a").join("b.png").display().to_string()
            );
        }
    }
}
