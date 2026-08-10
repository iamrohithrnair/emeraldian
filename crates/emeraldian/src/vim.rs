//! Modal editing, behind `editor.vim`.
//!
//! The state machine lives here and nowhere else: [`Editor`] stays the plain
//! line buffer everything else parses and indexes, and gains only primitive
//! operations that vim happens to need. Everything about *modes* — what a key
//! means, what is pending, what the last change was — is in this module.
//!
//! Two names are easy to confuse and worth pinning down. The app has two
//! top-level states, **emeraldian mode** (the editor as it behaves with the
//! setting off) and **vim mode**; `F4` toggles between them. Inside vim mode
//! are vim's own modes, and [`VimMode::Normal`] always means vim's Normal.
//!
//! Keys arrive here already filtered: [`crate::keys::handle_editing`] routes to
//! [`handle`] only when the setting is on, the note pane has focus, and the tab
//! is being edited rather than read.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::App;

/// Which vim mode the editor is in.
///
/// Insert is deliberately the same code path the editor uses with vim off, so
/// there is one implementation of "typing into a note" rather than two that can
/// drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    /// `V`: the selection covers whole lines however far along them it started.
    VisualLine,
}

impl VimMode {
    /// What the status bar shows.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Visual => "VISUAL",
            Self::VisualLine => "V-LINE",
        }
    }

    /// Whether the cursor sits *on* a character rather than between two.
    #[must_use]
    pub fn is_normal_like(self) -> bool {
        !matches!(self, Self::Insert)
    }
}

/// The unnamed register.
///
/// One register rather than vim's twenty-six: named registers are a power
/// feature that costs a parser and a map, and nothing in a notes app reaches
/// for them. `linewise` is what makes `yy` then `p` put a line below rather
/// than splicing it into the middle of the current one.
#[derive(Debug, Clone, Default)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

/// A key that is waiting for the one after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Pending {
    #[default]
    None,
    /// `g` typed, waiting for `g` in `gg`.
    G,
    /// `r` typed, waiting for the replacement character.
    Replace,
    /// `d` typed, waiting for the second `d`.
    Delete,
    /// `y` typed, waiting for the second `y`.
    Yank,
}

/// Everything vim mode remembers.
#[derive(Debug, Clone, Default)]
pub struct Vim {
    pub mode: VimMode,
    /// The count being typed, as in the `3` of `3dd`.
    count: Option<usize>,
    pending: Pending,
    pub register: Register,
    /// The keys typed so far in an unfinished command, shown in the status bar
    /// the way vim's `showcmd` does — so a half-typed `2d` is visible rather
    /// than looking like the editor has stopped responding.
    pub showcmd: String,
}

impl Vim {
    /// Drops every scrap of pending state.
    ///
    /// Called when leaving vim mode, switching tabs and on `Esc`, so a
    /// half-typed command can never outlive the thing it was aimed at.
    pub fn reset(&mut self) {
        self.mode = VimMode::Normal;
        self.clear_pending();
    }

    fn clear_pending(&mut self) {
        self.count = None;
        self.pending = Pending::None;
        self.showcmd.clear();
    }

    /// The count typed, or 1 — `dd` and `1dd` mean the same thing.
    fn count(&self) -> usize {
        self.count.unwrap_or(1).max(1)
    }

    fn push_count(&mut self, digit: u32) {
        let value = self.count.unwrap_or(0) * 10 + digit as usize;
        // A count long enough to overflow is a stuck key, not an intention.
        self.count = Some(value.min(100_000));
    }
}

/// Handles one key. Returns whether it was consumed.
///
/// Takes the whole `App` rather than just the editor because the wrap-following
/// motions need the viewport the last frame recorded — the same reason
/// [`crate::keys::move_in_editor`] does.
pub fn handle(app: &mut App, key: KeyEvent) -> bool {
    match app.vim.mode {
        VimMode::Insert => insert(app, key),
        VimMode::Normal => normal(app, key),
        VimMode::Visual | VimMode::VisualLine => visual(app, key),
    }
}

// ---------------------------------------------------------------------------
// Insert
// ---------------------------------------------------------------------------

/// Insert mode is emeraldian mode with an exit.
///
/// Returning `false` hands the key to the ordinary editing path, so there is
/// exactly one implementation of typing, list continuation, `Ctrl+B` and the
/// rest — and anything added there works in vim mode for free.
fn insert(app: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // `Ctrl+[` is what the terminal sends for Escape, and vim users type it
    // interchangeably.
    let escape = key.code == KeyCode::Esc || (ctrl && key.code == KeyCode::Char('['));
    if escape {
        leave_insert(app);
        return true;
    }
    false
}

/// Leaves Insert for Normal, stepping the cursor left as vim does.
///
/// The step-left is not a quirk to smooth over: it is what puts the cursor on
/// the character just typed, so `Esc` then `x` deletes it.
fn leave_insert(app: &mut App) {
    app.vim.mode = VimMode::Normal;
    app.vim.clear_pending();
    if let Some(editor) = app.editor_mut() {
        editor.commit();
        let cursor = editor.cursor();
        if cursor.col > 0 {
            editor.goto(cursor.line, cursor.col - 1);
        }
        editor.clamp_normal();
    }
}

/// Enters Insert, ending the current undo group so the insertion is its own.
fn enter_insert(app: &mut App) {
    app.vim.mode = VimMode::Insert;
    app.vim.clear_pending();
    if let Some(editor) = app.editor_mut() {
        editor.commit();
    }
}

// ---------------------------------------------------------------------------
// Normal
// ---------------------------------------------------------------------------

fn normal(app: &mut App, key: KeyEvent) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // A pending key owns whatever comes next, before anything else is read.
    match app.vim.pending {
        Pending::Replace => {
            if let KeyCode::Char(ch) = key.code {
                let count = app.vim.count();
                if let Some(editor) = app.editor_mut() {
                    editor.replace_char(ch, count);
                    editor.commit();
                }
            }
            app.vim.clear_pending();
            return true;
        }
        // An operator waits for the key that says what to act on. Phase one
        // understands only the doubled form — `dd`, `yy` — so anything else
        // abandons the command rather than guessing at it.
        Pending::Delete => {
            let count = app.vim.count();
            app.vim.pending = Pending::None;
            if key.code == KeyCode::Char('d') {
                let text = with_editor_out(app, |editor| {
                    let line = editor.cursor().line;
                    let text = editor.take_lines(line, count);
                    editor.move_first_nonblank(false);
                    editor.commit();
                    text
                });
                if let Some(text) = text {
                    app.vim.register = Register {
                        text,
                        linewise: true,
                    };
                }
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        Pending::Yank => {
            let count = app.vim.count();
            app.vim.pending = Pending::None;
            if key.code == KeyCode::Char('y') {
                let text = with_editor_out(app, |editor| {
                    let line = editor.cursor().line;
                    editor.copy_lines(line, count)
                });
                if let Some(text) = text {
                    let lines = text.lines().count();
                    app.vim.register = Register {
                        text,
                        linewise: true,
                    };
                    app.info_yank(lines);
                }
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        Pending::G => {
            app.vim.pending = Pending::None;
            match key.code {
                KeyCode::Char('g') => {
                    let line = app.vim.count.map_or(0, |n| n.saturating_sub(1));
                    if let Some(editor) = app.editor_mut() {
                        editor.goto(line, 0);
                        editor.move_first_nonblank(false);
                    }
                }
                // `gj`/`gk` are the by-screen-row counterparts of `j`/`k`.
                KeyCode::Char('j') => move_row(app, 1),
                KeyCode::Char('k') => move_row(app, -1),
                _ => {}
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        Pending::None => {}
    }

    if ctrl {
        return normal_ctrl(app, key);
    }

    match key.code {
        // A leading zero is the motion; any later digit is part of a count.
        KeyCode::Char('0') if app.vim.count.is_none() => {
            with_editor(app, |editor| editor.move_line_zero(false));
        }
        KeyCode::Char(ch @ '0'..='9') => {
            app.vim.push_count(ch.to_digit(10).unwrap_or(0));
            app.vim.showcmd.push(ch);
            return true;
        }

        // ---- motions ----------------------------------------------------
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => repeat(app, |app| {
            with_editor(app, |editor| editor.move_left(false));
        }),
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => repeat(app, |app| {
            with_editor(app, |editor| editor.move_right(false));
        }),
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.vim.count() as isize;
            with_editor(app, |editor| editor.move_line(count, false));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let count = app.vim.count() as isize;
            with_editor(app, |editor| editor.move_line(-count, false));
        }
        KeyCode::Char('^') | KeyCode::Home => {
            with_editor(app, |editor| editor.move_first_nonblank(false));
        }
        KeyCode::Char('$') | KeyCode::End => {
            with_editor(app, |editor| editor.move_line_end_sticky(false));
        }
        KeyCode::Char('G') => {
            let line = app.vim.count.map(|n| n.saturating_sub(1));
            with_editor(app, |editor| match line {
                Some(line) => {
                    editor.goto(line, 0);
                    editor.move_first_nonblank(false);
                }
                None => editor.move_document_end(false),
            });
        }
        KeyCode::Char('g') => {
            app.vim.pending = Pending::G;
            app.vim.showcmd.push('g');
            return true;
        }

        // ---- entering insert ---------------------------------------------
        KeyCode::Char('i') => {
            enter_insert(app);
            return true;
        }
        KeyCode::Char('I') => {
            with_editor(app, |editor| editor.move_first_nonblank(false));
            enter_insert(app);
            return true;
        }
        KeyCode::Char('a') => {
            with_editor(app, |editor| {
                let cursor = editor.cursor();
                if cursor.col < editor.line_len_at(cursor.line) {
                    editor.goto(cursor.line, cursor.col + 1);
                }
            });
            enter_insert(app);
            return true;
        }
        KeyCode::Char('A') => {
            with_editor(app, |editor| editor.move_line_end(false));
            enter_insert(app);
            return true;
        }
        KeyCode::Char('o') => {
            with_editor(app, |editor| editor.open_line(false));
            enter_insert(app);
            return true;
        }
        KeyCode::Char('O') => {
            with_editor(app, |editor| editor.open_line(true));
            enter_insert(app);
            return true;
        }

        // ---- edits --------------------------------------------------------
        KeyCode::Char('x') | KeyCode::Delete => {
            let count = app.vim.count();
            let text = with_editor_out(app, |editor| {
                let text = editor.take_chars(count);
                editor.commit();
                text
            });
            if let Some(text) = text.filter(|t| !t.is_empty()) {
                app.vim.register = Register {
                    text,
                    linewise: false,
                };
            }
        }
        // An operator does nothing on its own — it waits to be told what to act
        // on, which is the second `d` in `dd`.
        KeyCode::Char('d') => {
            app.vim.pending = Pending::Delete;
            app.vim.showcmd.push('d');
            return true;
        }
        KeyCode::Char('y') => {
            app.vim.pending = Pending::Yank;
            app.vim.showcmd.push('y');
            return true;
        }
        KeyCode::Char('p' | 'P') => {
            let after = key.code == KeyCode::Char('p');
            let register = app.vim.register.clone();
            let count = app.vim.count();
            with_editor(app, |editor| {
                for _ in 0..count {
                    editor.put(&register.text, register.linewise, after);
                }
                editor.commit();
            });
        }
        KeyCode::Char('r') => {
            app.vim.pending = Pending::Replace;
            app.vim.showcmd.push('r');
            return true;
        }
        KeyCode::Char('u') => {
            with_editor(app, |editor| {
                editor.undo();
            });
        }

        // ---- visual --------------------------------------------------------
        KeyCode::Char('v') => {
            app.vim.mode = VimMode::Visual;
            with_editor(app, |editor| editor.begin_selection());
            app.vim.clear_pending();
            return true;
        }
        KeyCode::Char('V') => {
            app.vim.mode = VimMode::VisualLine;
            with_editor(app, |editor| editor.begin_selection());
            select_lines(app);
            app.vim.clear_pending();
            return true;
        }

        // Escape clears a half-typed command; with nothing pending there is
        // nothing left for it to do here, so it leaves for the reading view —
        // which is what Escape has always meant in this editor.
        KeyCode::Esc => {
            if app.vim.count.is_some() || !app.vim.showcmd.is_empty() {
                app.vim.clear_pending();
                return true;
            }
            return false;
        }

        // `q` would quit from every other pane. In the editor it must not, and
        // vim's own meaning for it is macro recording, which this does not do —
        // so it is deliberately inert rather than surprising.
        KeyCode::Char('q') => {}

        _ => return false,
    }

    app.vim.clear_pending();
    clamp(app);
    true
}

/// The Ctrl keys vim defines, which the global map lets through in Normal mode.
fn normal_ctrl(app: &mut App, key: KeyEvent) -> bool {
    let (_, height) = crate::ui::note::edit_viewport(app);
    let half = (height / 2).max(1) as isize;
    let page = height.saturating_sub(1).max(1) as isize;

    match key.code {
        KeyCode::Char('r') => {
            with_editor(app, |editor| {
                editor.redo();
            });
        }
        KeyCode::Char('d') => move_row(app, half),
        KeyCode::Char('u') => move_row(app, -half),
        KeyCode::Char('f') => move_row(app, page),
        KeyCode::Char('b') => move_row(app, -page),
        _ => return false,
    }

    app.vim.clear_pending();
    clamp(app);
    true
}

// ---------------------------------------------------------------------------
// Visual
// ---------------------------------------------------------------------------

fn visual(app: &mut App, key: KeyEvent) -> bool {
    let linewise = app.vim.mode == VimMode::VisualLine;

    match key.code {
        KeyCode::Esc => {
            app.vim.mode = VimMode::Normal;
            app.vim.clear_pending();
            with_editor(app, |editor| {
                editor.goto(editor.cursor().line, editor.cursor().col)
            });
            clamp(app);
            return true;
        }

        KeyCode::Char(ch @ '1'..='9') => {
            app.vim.push_count(ch.to_digit(10).unwrap_or(0));
            app.vim.showcmd.push(ch);
            return true;
        }

        KeyCode::Char('h') | KeyCode::Left => repeat(app, |app| {
            with_editor(app, |editor| editor.move_left(true));
        }),
        KeyCode::Char('l') | KeyCode::Right => repeat(app, |app| {
            with_editor(app, |editor| editor.move_right(true));
        }),
        KeyCode::Char('j') | KeyCode::Down => {
            let count = app.vim.count() as isize;
            with_editor(app, |editor| editor.move_line(count, true));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let count = app.vim.count() as isize;
            with_editor(app, |editor| editor.move_line(-count, true));
        }
        KeyCode::Char('0') => with_editor(app, |editor| editor.move_line_zero(true)),
        KeyCode::Char('^') => with_editor(app, |editor| editor.move_first_nonblank(true)),
        KeyCode::Char('$') => with_editor(app, |editor| editor.move_line_end(true)),
        KeyCode::Char('G') => with_editor(app, |editor| editor.move_document_end(true)),

        // Switching between the two visual flavours, as vim does.
        KeyCode::Char('v') => {
            app.vim.mode = if linewise {
                VimMode::Visual
            } else {
                VimMode::Normal
            };
            if app.vim.mode == VimMode::Normal {
                with_editor(app, |editor| {
                    editor.goto(editor.cursor().line, editor.cursor().col)
                });
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        KeyCode::Char('V') => {
            app.vim.mode = VimMode::VisualLine;
        }

        KeyCode::Char('d' | 'x') => {
            let text = cut_selection(app, linewise);
            app.vim.register = Register { text, linewise };
            app.vim.mode = VimMode::Normal;
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        KeyCode::Char('y') => {
            let text = copy_selection(app, linewise);
            let lines = text.lines().count();
            // Vim leaves the cursor at the start of what was yanked.
            with_editor(app, |editor| {
                if let Some((start, _)) = editor.selection() {
                    editor.goto(start.line, start.col);
                }
            });
            app.vim.register = Register { text, linewise };
            app.vim.mode = VimMode::Normal;
            app.vim.clear_pending();
            app.info_yank(lines);
            clamp(app);
            return true;
        }
        KeyCode::Char('c' | 's') => {
            let text = cut_selection(app, linewise);
            // A linewise change leaves an empty line to type on rather than
            // splicing the next line onto the previous one.
            if linewise {
                with_editor(app, |editor| editor.open_line(true));
            }
            app.vim.register = Register { text, linewise };
            enter_insert(app);
            return true;
        }
        KeyCode::Char('>') => {
            with_editor(app, |editor| {
                editor.indent(true);
                editor.commit();
            });
            app.vim.mode = VimMode::Normal;
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        KeyCode::Char('<') => {
            with_editor(app, |editor| {
                editor.indent(false);
                editor.commit();
            });
            app.vim.mode = VimMode::Normal;
            app.vim.clear_pending();
            clamp(app);
            return true;
        }

        _ => return false,
    }

    if app.vim.mode == VimMode::VisualLine {
        select_lines(app);
    }
    app.vim.clear_pending();
    true
}

/// Stretches the selection to cover whole lines, for `V`.
///
/// The buffer's selection is charwise only, so line mode is expressed by
/// pinning the ends to the start and end of the outermost lines rather than by
/// teaching the buffer a second kind of selection it would otherwise never use.
fn select_lines(app: &mut App) {
    let Some(editor) = app.editor_mut() else {
        return;
    };
    let cursor = editor.cursor();
    let Some((start, end)) = editor.selection().or(Some((cursor, cursor))) else {
        return;
    };
    // Which end the cursor is on decides which way the selection is anchored,
    // so extending upward from the start of a line still covers that line.
    let downward = cursor.line >= start.line;
    let (top, bottom) = (start.line.min(end.line), start.line.max(end.line));
    let bottom_len = editor.line_len_at(bottom);

    if downward {
        editor.goto(top, 0);
        editor.begin_selection();
        editor.goto_extend(bottom, bottom_len);
    } else {
        editor.goto(bottom, bottom_len);
        editor.begin_selection();
        editor.goto_extend(top, 0);
    }
}

/// The lines a visual selection touches, as `(first, count)`.
fn selected_lines(app: &mut App) -> Option<(usize, usize)> {
    let editor = app.editor_mut()?;
    let cursor = editor.cursor();
    let (start, end) = editor.selection().unwrap_or((cursor, cursor));
    Some((start.line, end.line - start.line + 1))
}

/// Removes the selection and returns it.
///
/// The two flavours are genuinely different operations rather than one with a
/// flag. Linewise goes through [`Editor::take_lines`], which removes whole
/// lines *and their line breaks* — deleting the characters instead would leave
/// an empty line behind where the text used to be.
///
/// Charwise has to reconcile a difference in what "selected" means: vim's
/// visual selection includes the character under the cursor, and the buffer's
/// selection stops short of it. Without stretching it by one, `v` `l` `l` `d`
/// would delete two characters where vim deletes three.
fn cut_selection(app: &mut App, linewise: bool) -> String {
    if linewise {
        let Some((first, count)) = selected_lines(app) else {
            return String::new();
        };
        return with_editor_out(app, |editor| {
            let text = editor.take_lines(first, count);
            editor.commit();
            text
        })
        .unwrap_or_default();
    }

    extend_inclusive(app);
    with_editor_out(app, |editor| {
        let text = editor.selected_text().unwrap_or_default();
        editor.delete_selection();
        editor.commit();
        text
    })
    .unwrap_or_default()
}

/// The selection's text, left where it is.
fn copy_selection(app: &mut App, linewise: bool) -> String {
    if linewise {
        let Some((first, count)) = selected_lines(app) else {
            return String::new();
        };
        return with_editor_out(app, |editor| editor.copy_lines(first, count)).unwrap_or_default();
    }
    extend_inclusive(app);
    with_editor_out(app, |editor| editor.selected_text().unwrap_or_default()).unwrap_or_default()
}

/// Stretches a charwise selection to include the character under the cursor.
fn extend_inclusive(app: &mut App) {
    let Some(editor) = app.editor_mut() else {
        return;
    };
    let Some((start, end)) = editor.selection() else {
        // A selection of nothing still covers the character sat on, which is
        // what makes `v` then `d` behave like `x`.
        let cursor = editor.cursor();
        editor.goto(cursor.line, cursor.col);
        editor.begin_selection();
        let len = editor.line_len_at(cursor.line);
        editor.goto_extend(cursor.line, (cursor.col + 1).min(len));
        return;
    };
    let len = editor.line_len_at(end.line);
    editor.goto(start.line, start.col);
    editor.begin_selection();
    editor.goto_extend(end.line, (end.col + 1).min(len));
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn with_editor<T>(app: &mut App, f: impl FnOnce(&mut crate::editor::Editor) -> T) {
    if let Some(editor) = app.editor_mut() {
        f(editor);
    }
}

fn with_editor_out<T>(app: &mut App, f: impl FnOnce(&mut crate::editor::Editor) -> T) -> Option<T> {
    app.editor_mut().map(f)
}

/// Runs an action the count number of times.
fn repeat(app: &mut App, f: impl Fn(&mut App)) {
    for _ in 0..app.vim.count() {
        f(app);
    }
}

/// Moves by screen rows, which needs the geometry the last frame recorded.
fn move_row(app: &mut App, delta: isize) {
    let (width, _) = crate::ui::note::edit_viewport(app);
    let wrap = app.config.editor.wrap;
    let Some(editor) = app.editor_mut() else {
        return;
    };
    let layout = editor.layout(width, wrap);
    editor.move_row(&layout, delta, false);
}

/// Pulls the cursor back onto a character, in every mode that needs it.
fn clamp(app: &mut App) {
    if !app.vim.mode.is_normal_like() {
        return;
    }
    with_editor(app, crate::editor::Editor::clamp_normal);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Mode;
    use crate::config::Config;
    use crossterm::event::KeyCode;
    use emeraldian_core::test_support::TempVault;

    /// An app with vim mode already on and a note open for editing.
    ///
    /// The flag is set directly rather than through the toggle: turning it on
    /// for real writes the config file, which is a different thing to test and
    /// not something every test should be doing.
    fn app(text: &str) -> (TempVault, App) {
        let vault = TempVault::new("vim");
        vault.write("N.md", text);
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        let id = app.index.id_of_rel("N.md").expect("indexed");
        app.open_note(id);
        app.active_mut().expect("tab").mode = Mode::Editing;
        // Build the editor so a test can position the cursor before typing.
        let _ = app.editor_mut();
        (vault, app)
    }

    fn press(app: &mut App, ch: char) {
        crate::keys::handle(app, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()));
    }

    fn type_str(app: &mut App, keys: &str) {
        for ch in keys.chars() {
            press(app, ch);
        }
    }

    fn esc(app: &mut App) {
        crate::keys::handle(app, KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
    }

    fn text(app: &mut App) -> String {
        app.editor_mut().expect("editor").text()
    }

    fn cursor(app: &mut App) -> (usize, usize) {
        let c = app.editor_mut().expect("editor").cursor();
        (c.line, c.col)
    }

    #[test]
    fn a_note_opens_in_normal_mode_where_letters_are_commands() {
        let (_v, mut app) = app("hello\nworld\n");
        assert_eq!(app.vim.mode, VimMode::Normal);

        // The whole point of the feature: `j` moves rather than typing a `j`.
        press(&mut app, 'j');
        assert_eq!(cursor(&mut app), (1, 0));
        assert_eq!(text(&mut app), "hello\nworld\n", "nothing was typed");
    }

    #[test]
    fn i_starts_typing_and_esc_stops() {
        let (_v, mut app) = app("bc\n");
        press(&mut app, 'i');
        assert_eq!(app.vim.mode, VimMode::Insert);

        press(&mut app, 'a');
        assert_eq!(text(&mut app), "abc\n");

        esc(&mut app);
        assert_eq!(app.vim.mode, VimMode::Normal);
        assert_eq!(
            cursor(&mut app),
            (0, 0),
            "leaving insert steps back onto the character just typed"
        );
    }

    #[test]
    fn escape_from_normal_mode_leaves_the_editor() {
        // Esc in Insert goes to Normal, and Esc again falls through to the
        // reading view — so the pre-vim muscle memory still arrives, one key
        // later.
        let (_v, mut app) = app("text\n");
        press(&mut app, 'i');
        esc(&mut app);
        assert_eq!(app.active().expect("tab").mode, Mode::Editing);

        esc(&mut app);
        assert_eq!(app.active().expect("tab").mode, Mode::Reading);
    }

    #[test]
    fn escape_clears_a_half_typed_command_before_leaving() {
        let (_v, mut app) = app("text\n");
        press(&mut app, '2');
        assert_eq!(app.vim.showcmd, "2");

        esc(&mut app);
        assert!(app.vim.showcmd.is_empty(), "the count is abandoned");
        assert_eq!(
            app.active().expect("tab").mode,
            Mode::Editing,
            "and the first Escape does not also leave the editor"
        );
    }

    #[test]
    fn a_and_o_insert_in_the_right_places() {
        let (_v, mut app) = app("ab\n");
        press(&mut app, 'a');
        press(&mut app, 'X');
        assert_eq!(text(&mut app), "aXb\n", "a inserts after the cursor");

        esc(&mut app);
        press(&mut app, 'o');
        press(&mut app, 'Y');
        assert_eq!(text(&mut app), "aXb\nY\n", "o opens the line below");

        esc(&mut app);
        press(&mut app, 'O');
        press(&mut app, 'Z');
        assert_eq!(text(&mut app), "aXb\nZ\nY\n", "O opens the line above");
    }

    #[test]
    fn x_deletes_characters_and_fills_the_register() {
        let (_v, mut app) = app("abcdef\n");
        press(&mut app, 'x');
        assert_eq!(text(&mut app), "bcdef\n");
        assert_eq!(app.vim.register.text, "a");
        assert!(!app.vim.register.linewise);

        type_str(&mut app, "3x");
        assert_eq!(text(&mut app), "ef\n", "a count repeats the delete");
        assert_eq!(app.vim.register.text, "bcd");
    }

    #[test]
    fn x_stops_at_the_end_of_the_line() {
        // Running off the end and pulling the next line up would make `5x` on a
        // short line silently join two paragraphs.
        let (_v, mut app) = app("ab\ncd\n");
        type_str(&mut app, "9x");
        assert_eq!(text(&mut app), "\ncd\n");
    }

    #[test]
    fn dd_deletes_lines_and_p_puts_them_back() {
        let (_v, mut app) = app("one\ntwo\nthree\n");
        press(&mut app, 'j');
        type_str(&mut app, "dd");
        assert_eq!(text(&mut app), "one\nthree\n");
        assert!(app.vim.register.linewise);

        press(&mut app, 'p');
        assert_eq!(text(&mut app), "one\nthree\ntwo\n", "p puts below");
    }

    #[test]
    fn a_count_deletes_several_lines_at_once() {
        let (_v, mut app) = app("a\nb\nc\nd\n");
        type_str(&mut app, "2dd");
        assert_eq!(text(&mut app), "c\nd\n");
        assert_eq!(app.vim.register.text, "a\nb\n");
    }

    #[test]
    fn yy_copies_without_changing_anything_and_says_so() {
        let (_v, mut app) = app("keep\nother\n");
        type_str(&mut app, "yy");
        assert_eq!(text(&mut app), "keep\nother\n", "yank changes nothing");
        assert_eq!(app.vim.register.text, "keep\n");
        assert!(
            app.status.text.contains("yanked"),
            "an invisible action needs a word in the status bar, got {:?}",
            app.status.text
        );

        press(&mut app, 'P');
        assert_eq!(text(&mut app), "keep\nkeep\nother\n", "P puts above");
    }

    #[test]
    fn r_replaces_one_character_in_place() {
        let (_v, mut app) = app("cat\n");
        type_str(&mut app, "rb");
        assert_eq!(text(&mut app), "bat\n");
        assert_eq!(cursor(&mut app), (0, 0), "the cursor stays put");
    }

    #[test]
    fn r_refuses_rather_than_replacing_past_the_end_of_the_line() {
        let (_v, mut app) = app("ab\n");
        type_str(&mut app, "9rz");
        assert_eq!(text(&mut app), "ab\n", "a partial replace would be worse");
    }

    #[test]
    fn u_undoes_a_whole_insert_rather_than_one_keystroke() {
        let (_v, mut app) = app("\n");
        press(&mut app, 'i');
        for ch in "hello".chars() {
            press(&mut app, ch);
        }
        esc(&mut app);
        assert_eq!(text(&mut app), "hello\n");

        press(&mut app, 'u');
        assert_eq!(text(&mut app), "\n", "one undo, one thought");
    }

    #[test]
    fn ctrl_r_redoes_what_u_undid() {
        let (_v, mut app) = app("abc\n");
        press(&mut app, 'x');
        assert_eq!(text(&mut app), "bc\n");
        press(&mut app, 'u');
        assert_eq!(text(&mut app), "abc\n");

        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "bc\n");
    }

    #[test]
    fn motions_reach_the_ends_of_the_line_and_the_note() {
        let (_v, mut app) = app("  indented\nsecond\nlast line\n");
        press(&mut app, '$');
        assert_eq!(cursor(&mut app), (0, 9), "on the last character, not past");

        press(&mut app, '0');
        assert_eq!(cursor(&mut app), (0, 0));
        press(&mut app, '^');
        assert_eq!(cursor(&mut app), (0, 2), "the first thing that isn't blank");

        press(&mut app, 'G');
        assert_eq!(cursor(&mut app).0, 2);
        type_str(&mut app, "gg");
        assert_eq!(cursor(&mut app).0, 0);
    }

    #[test]
    fn a_count_before_g_goes_to_that_line() {
        let (_v, mut app) = app("a\nb\nc\nd\n");
        type_str(&mut app, "3G");
        assert_eq!(cursor(&mut app).0, 2, "3G is the third line, 1-based");
    }

    #[test]
    fn j_and_k_move_by_source_line_not_by_screen_row() {
        // The paragraph below wraps to several rows. Vim's `j` crosses the
        // whole thing in one press; `gj` is the by-row counterpart.
        let (_v, mut app) = app(&format!("{}\nsecond\n", "word ".repeat(40)));
        press(&mut app, 'j');
        assert_eq!(
            cursor(&mut app).0,
            1,
            "one press crosses the wrapped paragraph"
        );
    }

    #[test]
    fn the_cursor_never_rests_past_the_last_character() {
        // Vim's Normal cursor is *on* a character. Landing past the end would
        // draw the block in empty space and make `x` a no-op.
        let (_v, mut app) = app("long line\nab\n");
        press(&mut app, '$');
        press(&mut app, 'j');
        let (line, col) = cursor(&mut app);
        assert_eq!(line, 1);
        assert_eq!(col, 1, "clamped onto the last character of the short line");
    }

    #[test]
    fn dollar_sticks_to_the_end_of_each_line_it_passes() {
        let (_v, mut app) = app("short\nmuch longer line\nmid\n");
        press(&mut app, '$');
        press(&mut app, 'j');
        assert_eq!(
            cursor(&mut app),
            (1, 15),
            "$ then j stays at the end, which is vim's curswant"
        );
    }

    #[test]
    fn visual_mode_selects_and_deletes() {
        let (_v, mut app) = app("abcdef\n");
        press(&mut app, 'v');
        assert_eq!(app.vim.mode, VimMode::Visual);
        type_str(&mut app, "ll");
        press(&mut app, 'd');

        assert_eq!(text(&mut app), "def\n");
        assert_eq!(app.vim.mode, VimMode::Normal, "d ends the selection");
        assert_eq!(app.vim.register.text, "abc");
    }

    #[test]
    fn visual_line_mode_takes_whole_lines() {
        let (_v, mut app) = app("one\ntwo\nthree\n");
        press(&mut app, 'j');
        press(&mut app, 'V');
        assert_eq!(app.vim.mode, VimMode::VisualLine);
        press(&mut app, 'd');

        assert_eq!(text(&mut app), "one\nthree\n");
        assert!(app.vim.register.linewise, "so p puts it back as a line");
    }

    #[test]
    fn visual_line_started_mid_line_still_covers_the_whole_line() {
        // The bug this guards: anchoring at the cursor column, so `V` from the
        // middle of a line deletes only half of it.
        let (_v, mut app) = app("hello\nworld\n");
        type_str(&mut app, "ll");
        press(&mut app, 'V');
        press(&mut app, 'd');
        assert_eq!(text(&mut app), "world\n");
    }

    #[test]
    fn visual_mode_indents_the_selection() {
        let (_v, mut app) = app("one\ntwo\n");
        press(&mut app, 'V');
        press(&mut app, 'j');
        press(&mut app, '>');
        assert_eq!(text(&mut app), "    one\n    two\n");
    }

    #[test]
    fn escape_leaves_visual_without_touching_the_text() {
        let (_v, mut app) = app("abcdef\n");
        press(&mut app, 'v');
        type_str(&mut app, "lll");
        esc(&mut app);

        assert_eq!(app.vim.mode, VimMode::Normal);
        assert_eq!(text(&mut app), "abcdef\n");
    }

    #[test]
    fn c_in_visual_mode_deletes_and_starts_typing() {
        let (_v, mut app) = app("abcdef\n");
        press(&mut app, 'v');
        type_str(&mut app, "ll");
        press(&mut app, 'c');
        assert_eq!(app.vim.mode, VimMode::Insert);
        press(&mut app, 'X');
        assert_eq!(text(&mut app), "Xdef\n");
    }

    #[test]
    fn a_pending_count_is_visible_while_it_is_being_typed() {
        // Silence looks like a hung editor; vim's showcmd is the fix.
        let (_v, mut app) = app("text\n");
        type_str(&mut app, "12");
        assert_eq!(app.vim.showcmd, "12");

        press(&mut app, 'j');
        assert!(app.vim.showcmd.is_empty(), "cleared once the command runs");
    }

    #[test]
    fn q_in_normal_mode_neither_quits_nor_types() {
        // `q` quits from every other pane, and this is the one place it must
        // not — while also not silently inserting a letter.
        let (_v, mut app) = app("text\n");
        press(&mut app, 'q');

        assert!(app.modal.is_none(), "no quit prompt");
        assert!(!app.quit);
        assert_eq!(text(&mut app), "text\n", "and nothing was typed");
    }

    #[test]
    fn switching_tabs_lands_in_normal_mode() {
        // Arriving in another note with typing already live is how you edit the
        // wrong file.
        let vault = TempVault::new("vim-tabs");
        vault.write("A.md", "a\n");
        vault.write("B.md", "b\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        for rel in ["A.md", "B.md"] {
            let id = app.index.id_of_rel(rel).expect("indexed");
            app.open_note(id);
            app.active_mut().expect("tab").mode = Mode::Editing;
        }

        press(&mut app, 'i');
        assert_eq!(app.vim.mode, VimMode::Insert);
        app.cycle_tab(1);
        assert_eq!(app.vim.mode, VimMode::Normal);
    }

    /// A temp directory for the settings, and an app pointed at it.
    ///
    /// Nothing in these tests may write to the real config directory: toggling
    /// vim mode saves immediately, so without this every run would edit the
    /// settings of whoever ran it.
    fn toggling(tag: &str, text: &str) -> (TempVault, App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("emeraldian-vim-{tag}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).expect("temp dir");

        let (vault, mut app) = app(text);
        app.config_dir = Some(dir.clone());
        (vault, app, dir)
    }

    fn f4(app: &mut App) {
        crate::keys::handle(app, KeyEvent::new(KeyCode::F(4), KeyModifiers::empty()));
    }

    #[test]
    fn f4_turns_vim_mode_on_and_off_again() {
        // A toggle that can be switched on but not off is a trap, and it is the
        // reason the key had to be one vim does not claim.
        let (_v, mut app, dir) = toggling("round-trip", "text\n");
        app.config.editor.vim = false;

        f4(&mut app);
        assert!(app.config.editor.vim, "emeraldian mode -> vim mode");

        // From inside Normal mode, which is the case that rules out every Ctrl
        // binding vim reclaims.
        f4(&mut app);
        assert!(!app.config.editor.vim, "vim mode -> emeraldian mode");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn f4_reaches_the_toggle_from_insert_mode_too() {
        let (_v, mut app, dir) = toggling("from-insert", "text\n");
        press(&mut app, 'i');
        assert_eq!(app.vim.mode, VimMode::Insert);

        f4(&mut app);
        assert!(!app.config.editor.vim);
        assert_eq!(text(&mut app), "text\n", "F4 is not typed into the note");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn toggling_writes_the_config_straight_away() {
        // The setting changes what every key does, so losing it silently on the
        // next launch is a different order of problem from losing a pane width.
        let (_v, mut app, dir) = toggling("persists", "text\n");
        app.config.editor.vim = false;
        f4(&mut app);

        let path = dir.join("config.toml");
        let written = std::fs::read_to_string(&path).expect("config was written");
        assert!(
            written.contains("vim = true"),
            "expected the flag on disk, got:\n{written}"
        );

        let (reloaded, error) = Config::load_from(&path);
        assert!(error.is_none());
        assert!(reloaded.editor.vim, "and it survives a reload");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_reference_opens_the_first_time_only() {
        let (_v, mut app, dir) = toggling("intro", "text\n");
        app.config.editor.vim = false;

        f4(&mut app);
        assert!(
            matches!(app.modal, Some(crate::modal::Modal::Help(_))),
            "a first-time user should not have to guess how to get out"
        );

        app.modal = None;
        f4(&mut app);
        f4(&mut app);
        assert!(app.modal.is_none(), "but only ever once");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaving_vim_mode_clears_a_half_typed_command() {
        // Otherwise F4 from a pending `2d` leaves a block cursor and swallowed
        // keys behind in an editor that is supposed to be back to normal.
        let (_v, mut app, dir) = toggling("clean-exit", "one\ntwo\n");
        type_str(&mut app, "2d");
        assert_eq!(app.vim.showcmd, "2d");

        f4(&mut app);
        assert!(app.vim.showcmd.is_empty());
        assert_eq!(app.vim.mode, VimMode::Normal);

        type_str(&mut app, "d");
        assert_eq!(text(&mut app), "done\ntwo\n", "and typing works again");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_config_write_still_leaves_vim_mode_usable() {
        // A read-only home is no reason to refuse to work; it is a reason to
        // say so.
        let (_v, mut app) = app("text\n");
        app.config.editor.vim = false;
        app.config_dir = Some(std::path::PathBuf::from("/nonexistent/emeraldian-test"));

        f4(&mut app);

        assert!(app.config.editor.vim, "the toggle still took effect");
        assert!(app.status.is_error, "and the user is told it wasn't saved");
    }

    #[test]
    fn with_vim_off_every_letter_is_still_text() {
        // The promise the whole feature rests on: the setting off means the
        // editor behaves exactly as it did before any of this existed.
        let (_v, mut app) = app("");
        app.config.editor.vim = false;

        type_str(&mut app, "jdd");
        assert_eq!(text(&mut app), "jdd\n");
    }

    #[test]
    fn with_vim_off_escape_still_leaves_the_editor_in_one_press() {
        let (_v, mut app) = app("text\n");
        app.config.editor.vim = false;

        esc(&mut app);
        assert_eq!(
            app.active().expect("tab").mode,
            Mode::Reading,
            "the pre-vim behaviour is untouched"
        );
    }

    #[test]
    fn with_vim_off_the_reclaimed_ctrl_keys_keep_their_own_meanings() {
        // The blast radius of the global remap is the risk this feature runs,
        // so it is worth pinning down rather than trusting.
        let (_v, mut app) = app("text\n");
        app.config.editor.vim = false;

        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert!(
            app.index
                .id_of_rel(&format!("{}.md", app.daily_note_name()))
                .is_some()
                || app.active_note().is_some(),
            "Ctrl+D still opens the daily note"
        );

        let tabs = app.tabs.len();
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert!(app.tabs.len() < tabs, "Ctrl+W still closes the tab");
    }

    #[test]
    fn in_vim_mode_the_reclaimed_keys_go_to_the_editor_not_the_app() {
        // Ctrl+R reloading the vault instead of redoing is not a near-miss: it
        // throws the redo stack away along with everything else.
        let (_v, mut app) = app("abc\n");
        press(&mut app, 'x');
        press(&mut app, 'u');

        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "bc\n", "Ctrl+R redid rather than reloading");
    }

    #[test]
    fn ctrl_shift_f_still_searches_from_inside_vim_mode() {
        // Vim binds none of the reclaimed keys with Shift, and the app does —
        // claiming those too would cost a binding for nothing.
        let (_v, mut app) = app("text\n");
        crate::keys::handle(
            &mut app,
            KeyEvent::new(
                KeyCode::Char('f'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
        );
        assert!(
            matches!(app.modal, Some(crate::modal::Modal::Picker(_))),
            "Ctrl+Shift+F must still open the vault search"
        );
    }

    #[test]
    fn insert_mode_leaves_the_global_bindings_alone() {
        // Insert mode is the ordinary editor with an exit, so everything that
        // worked before still works — including the keys Normal mode claims.
        let (_v, mut app) = app("text\n");
        press(&mut app, 'i');

        let tabs = app.tabs.len();
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert!(app.tabs.len() < tabs, "Ctrl+W still closes the tab");
    }

    #[test]
    fn an_older_config_without_the_key_starts_in_emeraldian_mode() {
        // Upgrading must not silently put someone into a modal editor.
        let config: Config = toml::from_str("theme = \"nord\"\n").expect("parse");
        assert!(!config.editor.vim);
    }
}
