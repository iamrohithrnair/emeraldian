//! Keyboard handling.
//!
//! Bindings follow Obsidian where it has one (`Ctrl+O`, `Ctrl+P`, `Ctrl+E`,
//! `Ctrl+N`) and vim where Obsidian has nothing to copy (`hjkl` to move,
//! `g`/`G` for top and bottom). Global bindings are checked first, then the
//! focused pane gets what's left, so `Ctrl+P` opens the palette even mid-word.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::actions::{self, dispatch};
use crate::agent;
use crate::app::{Action, App, Focus, Mode, View};
use crate::modal::{Modal, PickerKind, Prompt, PromptIntent};
use crate::ui::panes::{SidebarTarget, sidebar_targets};

/// Columns moved per keypress when panning a wide table sideways.
const PAN_STEP: isize = 4;

/// Whose keyboard conventions to follow.
///
/// The `Ctrl` shortcuts work everywhere. A Mac adds its own on top: `⌘` for
/// `Ctrl`, and `⌥` and `⌘` with the arrows and Backspace, as in every Mac text
/// field. Windows and Linux share theirs. Read from `ui.keyboard`, which
/// follows the machine the app runs on unless told otherwise, as it has to be
/// when a Mac reaches a Linux machine over SSH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    Mac,
    Pc,
}

impl Keyboard {
    #[must_use]
    pub fn of(app: &App) -> Self {
        match app.config.ui.keyboard.trim().to_ascii_lowercase().as_str() {
            "mac" | "macos" => Self::Mac,
            "windows" | "linux" | "pc" => Self::Pc,
            _ if cfg!(target_os = "macos") => Self::Mac,
            _ => Self::Pc,
        }
    }
}

/// Handles one key event.
pub fn handle(app: &mut App, key: KeyEvent) {
    // Terminals that report key releases would otherwise run every binding
    // twice.
    if key.kind == KeyEventKind::Release {
        return;
    }

    // A message from the last action is stale as soon as the user acts again.
    app.status.text.clear();

    // With the Kitty protocol a terminal tells these apart from the keys they
    // were always sent as, and nothing here wants the difference: `Ctrl+[` is
    // Escape to everyone who types it, and `Ctrl+M` is Enter.
    let key = match key.code {
        KeyCode::Char('[') if key.modifiers == KeyModifiers::CONTROL => {
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        }
        KeyCode::Char('m') if key.modifiers == KeyModifiers::CONTROL => {
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
        }
        _ => key,
    };

    // `⌘` reaches the app only from a terminal that passes it on, which most
    // keep for their own menus. When it does, it is an app shortcut, never a
    // vim command or a letter to type.
    let command = key.modifiers.contains(KeyModifiers::SUPER) && Keyboard::of(app) == Keyboard::Mac;
    // Anywhere else the Super key belongs to the desktop, and one that gets
    // through anyway shouldn't type the letter it came with.
    if key.modifiers.contains(KeyModifiers::SUPER) && !command {
        return;
    }
    if !command {
        route(app, key, false);
        return;
    }

    // A key moved in `[keys]` is matched first, and `cmd` there is `Ctrl`.
    let as_ctrl = KeyEvent {
        modifiers: (key.modifiers - KeyModifiers::SUPER) | KeyModifiers::CONTROL,
        ..key
    };
    if app.modal.is_none()
        && let Some(action) = app.keymap.action_for(&normalize_legacy_ctrl(as_ctrl))
    {
        dispatch(app, action);
        return;
    }
    if app.modal.is_none() && vim_motion(app, key) {
        return;
    }
    if app.modal.is_some() || !mac_command(app, key) {
        route(app, command_as_control(key), true);
    }
    // Moving to the end of a line puts an insertion caret past its last
    // character, which is somewhere Normal mode's cursor never rests.
    let normal = app.config.editor.vim && app.vim.mode.is_normal_like();
    if normal
        && app.modal.is_none()
        && app.focus == Focus::Note
        && app.editing()
        && let Some(editor) = app.editor_mut()
    {
        editor.clamp_normal();
    }
}

/// Sends a key on to whatever it means where it was pressed: an overlay, a
/// shortcut moved in `[keys]`, vim, the app-wide bindings, and then the pane
/// with the focus. `command` marks a `⌘` shortcut, which vim never sees.
fn route(app: &mut App, key: KeyEvent, command: bool) {
    // The vim toggle outranks even an overlay. Turning vim on opens its
    // reference the first time, and if that overlay then swallowed F4 the way
    // out would stop working at exactly the moment a new user goes looking for
    // it. Dispatching closes the overlay on the way past.
    if key.code == KeyCode::F(4) && !key.modifiers.contains(KeyModifiers::CONTROL) {
        dispatch(app, Action::ToggleVimMode);
        return;
    }

    let typed = altgr_as_text(key);
    if app.modal.is_some() {
        handle_modal(app, typed);
        return;
    }

    // Keys moved in `[keys]` come before everything but an overlay. The user
    // chose them, and ahead of vim is the only place a chord like `Alt+E` can
    // stand: Normal mode would otherwise read it as a plain `e`. Matched as
    // pressed, so a chord chosen with both `Ctrl` and `Alt` still works.
    if let Some(action) = app.keymap.action_for(&normalize_legacy_ctrl(key)) {
        dispatch(app, action);
        return;
    }
    let key = typed;

    // Vim comes in two layers, and keeping them apart is what stops each from
    // leaking into the other's territory.
    //
    // The first is navigation — moving between panes and through the notes
    // you have visited. That is about the application, not about text, so it
    // works from every pane. Confining it to the editor made `Ctrl+W` a trap:
    // it would take you to the explorer and then, pressed again to come back,
    // close your tab instead.
    //
    // The second is text editing — the modes, motions and operators. That is
    // only meaningful on a buffer you are editing, so it engages nowhere else.
    // Letting it reach a note being *read* gave a phantom `INSERT` on a note
    // that wasn't open, and quietly took `Ctrl+D` away from the daily note.
    //
    // Both layers get first refusal ahead of the global map, and both return
    // whether they used the key. Anything declined falls through below and
    // keeps the meaning it always had — there is no list of exceptions to keep
    // in step with the handlers.
    // The graph has its own keys and its own way out; neither layer applies
    // there.
    // `⌘C`, `⌘X` and `⌘V` do to a visual selection what vim's own `Ctrl` ones
    // do, rather than what they do to an ordinary one.
    let visual = matches!(
        app.vim.mode,
        crate::vim::VimMode::Visual | crate::vim::VimMode::VisualLine
    );
    let clipboard = matches!(key.code, KeyCode::Char('c' | 'C' | 'x' | 'X' | 'v' | 'V'));
    if app.config.editor.vim && app.view == View::Notes && (!command || (visual && clipboard)) {
        let key = normalize_legacy_ctrl(key);
        if !crate::vim::is_typing(app) && crate::vim::navigation(app, key) {
            return;
        }
        if app.focus == Focus::Note && app.editing() && crate::vim::handle(app, key) {
            return;
        }
    }

    if handle_global(app, key) {
        return;
    }

    match app.focus {
        Focus::Explorer => handle_explorer(app, key),
        Focus::Note => handle_note(app, key),
        Focus::Sidebar => handle_sidebar(app, key),
        Focus::Chat => handle_chat(app, key),
        Focus::Graph => handle_graph(app, key),
    }
}

/// In vim's Normal and Visual modes, `⌘` with an arrow is the motion it
/// stands for, `0`, `$`, `gg` or `G`, so a selection grows the way vim grows
/// one and the cursor stays on a character. Returns whether it was one.
fn vim_motion(app: &mut App, key: KeyEvent) -> bool {
    let normal = app.config.editor.vim && app.vim.mode.is_normal_like();
    if !normal || app.view != View::Notes || app.focus != Focus::Note || !app.editing() {
        return false;
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        return false;
    }
    let motion = match key.code {
        KeyCode::Left => "0",
        KeyCode::Right => "$",
        KeyCode::Up => "gg",
        KeyCode::Down => "G",
        _ => return false,
    };
    for ch in motion.chars() {
        crate::vim::handle(app, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    true
}

/// The `⌘` shortcuts a Mac has that aren't `Ctrl` ones in disguise. Returns
/// whether the key was one.
fn mac_command(app: &mut App, key: KeyEvent) -> bool {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let typing = typing_in_note(app);
    let action = match key.code {
        // `⌘⇧[` and `⌘⇧]` step through tabs, as in Safari and Terminal.
        KeyCode::Char('{') => Action::PreviousTab,
        KeyCode::Char('}') => Action::NextTab,
        KeyCode::Char('[') if shift => Action::PreviousTab,
        KeyCode::Char(']') if shift => Action::NextTab,
        // `⌘[` and `⌘]` indent and outdent the line being edited, as in every
        // Mac editor, and anywhere else go back and forward, as in Finder.
        KeyCode::Char(bracket @ ('[' | ']')) if typing => {
            if let Some(editor) = app.editor_mut() {
                editor.indent(bracket == ']');
                editor.commit();
            }
            return true;
        }
        KeyCode::Char('[') => Action::Back,
        KeyCode::Char(']') => Action::Forward,
        // `⌘⌥←` and `⌘⌥→` are Obsidian's own back and forward on a Mac.
        KeyCode::Left if alt => Action::Back,
        KeyCode::Right if alt => Action::Forward,
        _ => return false,
    };
    dispatch(app, action);
    true
}

/// Reads a `⌘` shortcut as the `Ctrl` one it corresponds to, so `⌘S`, `⌘Z`,
/// `⌘⇧Z`, `⌘F` and the rest do what they do in every other Mac app.
///
/// With an arrow, `⌘` goes to the end of the line or the note, and `⌘⌫`
/// deletes back to the start of the line, as in any Mac text field.
fn command_as_control(key: KeyEvent) -> KeyEvent {
    let mut modifiers = key.modifiers - KeyModifiers::SUPER;
    let code = match key.code {
        KeyCode::Left => KeyCode::Home,
        KeyCode::Right => KeyCode::End,
        KeyCode::Up => {
            modifiers |= KeyModifiers::CONTROL;
            KeyCode::Home
        }
        KeyCode::Down => {
            modifiers |= KeyModifiers::CONTROL;
            KeyCode::End
        }
        KeyCode::Backspace => {
            modifiers |= KeyModifiers::CONTROL | KeyModifiers::SHIFT;
            KeyCode::Backspace
        }
        code => {
            modifiers |= KeyModifiers::CONTROL;
            code
        }
    };
    KeyEvent {
        code,
        modifiers,
        ..key
    }
}

/// Reads a character typed with AltGr as that character.
///
/// Windows reports AltGr as `Ctrl+Alt`, so on a German keyboard `@` arrives as
/// `Ctrl+Alt+@` and `]` as `Ctrl+Alt+]`. Taken as shortcuts, the first typed
/// nothing and the second toggled the sidebar. No binding is a symbol with
/// both modifiers, so one is typing; a letter or digit still makes a chord.
fn altgr_as_text(key: KeyEvent) -> KeyEvent {
    let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
    match key.code {
        KeyCode::Char(ch) if key.modifiers.contains(altgr) && !ch.is_ascii_alphanumeric() => {
            KeyEvent {
                modifiers: key.modifiers - altgr,
                ..key
            }
        }
        _ => key,
    }
}

/// Rewrites the control bytes a legacy terminal sends into the keys they mean.
///
/// Without the Kitty keyboard protocol a terminal has no way to say "Ctrl and
/// this punctuation key"; it sends a single C0 byte instead. `Ctrl+\` is `0x1C`
/// and `Ctrl+]` is `0x1D`, and crossterm decodes that range as `Ctrl` plus the
/// digits `4`–`7` — so a binding written as `Ctrl+]` can never match, which is
/// exactly what left both sidebar toggles dead.
///
/// Nothing binds `Ctrl` with a digit, and modals are dispatched before the
/// global map, so no typed input passes through here.
fn normalize_legacy_ctrl(key: KeyEvent) -> KeyEvent {
    if !key.modifiers.contains(KeyModifiers::CONTROL) {
        return key;
    }
    let code = match key.code {
        KeyCode::Char('4') => KeyCode::Char('\\'),
        KeyCode::Char('5') => KeyCode::Char(']'),
        other => other,
    };
    KeyEvent { code, ..key }
}

/// Bindings that work everywhere. Returns whether the key was consumed.
fn handle_global(app: &mut App, key: KeyEvent) -> bool {
    let key = normalize_legacy_ctrl(key);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    let action = match (ctrl, shift, key.code) {
        (true, _, KeyCode::Char('p' | 'P')) => Some(Action::OpenPalette),
        (true, _, KeyCode::Char('o' | 'O')) => Some(Action::OpenSwitcher),
        (true, true, KeyCode::Char('f' | 'F')) => Some(Action::OpenSearch),
        (true, _, KeyCode::Char('n' | 'N')) => Some(Action::NewNote),
        (true, _, KeyCode::Char('s' | 'S')) => Some(Action::Save),
        (true, _, KeyCode::Char('d' | 'D')) => Some(Action::DailyNote),
        (true, _, KeyCode::Char('e' | 'E')) => Some(Action::ToggleMode),
        (true, _, KeyCode::Char('w' | 'W')) => Some(Action::CloseTab),
        // A new tab opens on whatever is chosen next, which here is the quick
        // switcher, and `Ctrl+Shift+T` brings back the last one closed, as in
        // a browser. Themes are in the palette.
        (true, true, KeyCode::Char('t' | 'T')) => Some(Action::ReopenClosedTab),
        (true, false, KeyCode::Char('t' | 'T')) => Some(Action::OpenSwitcher),
        (true, false, KeyCode::Char('f' | 'F')) => Some(Action::OpenFind),
        (true, _, KeyCode::Char('l' | 'L')) => Some(Action::ToggleChat),
        // While typing, `Ctrl+K` inserts a link and `Ctrl+Shift+K` deletes the
        // line, as they do in Obsidian; the editor handles both.
        (true, _, KeyCode::Char('k' | 'K')) if !typing_in_note(app) => Some(Action::CycleSidePanel),
        (true, _, KeyCode::Char('q' | 'Q')) => Some(Action::Quit),
        (true, true, KeyCode::Char('g' | 'G')) => Some(Action::OpenLocalGraph),
        (true, false, KeyCode::Char('g' | 'G')) => Some(Action::OpenGraph),
        (true, _, KeyCode::Char('\\')) => Some(Action::ToggleLeftSidebar),
        (true, _, KeyCode::Char(']')) => Some(Action::ToggleRightSidebar),
        (true, _, KeyCode::Char('r' | 'R')) if app.focus != Focus::Chat => Some(Action::Refresh),
        (true, true, KeyCode::Tab | KeyCode::BackTab) => Some(Action::PreviousTab),
        (true, false, KeyCode::Tab) => Some(Action::NextTab),
        // Browsers' and editors' other tab keys, and the only ones that reach
        // the app from a terminal that keeps `Ctrl+Tab` for itself.
        (true, _, KeyCode::PageUp) => Some(Action::PreviousTab),
        (true, _, KeyCode::PageDown) => Some(Action::NextTab),
        (_, _, KeyCode::F(5)) => Some(Action::Refresh),
        (_, _, KeyCode::F(1)) => Some(Action::OpenHelp),
        (_, _, KeyCode::F(2)) => Some(Action::RenameNote),
        // Windows closes a document with `Ctrl+F4`, as browsers do everywhere.
        (true, _, KeyCode::F(4)) => Some(Action::CloseTab),
        // Closing a tab needs a key that means the same thing in both modes.
        // `Ctrl+W` cannot: vim spends it on the window prefix, and every plain
        // `Ctrl`+letter is already taken by the app, by vim, or by the terminal
        // itself. `Ctrl+Shift+W` looks like the answer and is not — without the
        // Kitty protocol the Shift is unencodable, so it arrives as `Ctrl+W`,
        // arms the prefix and eats the next keystroke. F-keys are unambiguous
        // everywhere, which is the same reason `F4` carries the vim toggle.
        (_, _, KeyCode::F(3)) => Some(Action::CloseTab),
        // Matched here, ahead of everything vim claims, because it has to work
        // from inside Normal mode too — a toggle that can be switched on but
        // not off is a trap. F-keys are the only ones vim leaves alone.
        (_, _, KeyCode::F(4)) => Some(Action::ToggleVimMode),
        // Focus mode: just the note. `F8` because it is Typora's key for the
        // same idea and reaches the app in every terminal; `F11`, the
        // full-screen key everywhere else, works too wherever the terminal
        // doesn't take it for its own full screen first.
        (_, _, KeyCode::F(8) | KeyCode::F(11)) => Some(Action::ToggleFocusMode),
        _ => None,
    };

    if let Some(action) = action {
        dispatch(app, action);
        return true;
    }

    // Back and forward, as in a browser: `Alt+←` and `Alt+→`, or Obsidian's
    // `Ctrl+Alt+←` and `Ctrl+Alt+→`. On a Mac `⌥` with an arrow moves by word
    // while typing, as it does in every Mac text field, so there it is left
    // to the editor.
    let mac = Keyboard::of(app) == Keyboard::Mac;
    let mac_typing = mac && typing_in_note(app);
    if alt && !mac_typing && matches!(key.code, KeyCode::Left | KeyCode::Right) {
        dispatch(
            app,
            if key.code == KeyCode::Left {
                Action::Back
            } else {
                Action::Forward
            },
        );
        return true;
    }
    // Many Mac terminals send `⌥←` and `⌥→` as readline's `Esc b` and `Esc f`,
    // which away from anything being typed are the same back and forward.
    let typing_anywhere = typing_in_note(app) || app.focus == Focus::Chat;
    if mac && alt && !ctrl && !typing_anywhere && matches!(key.code, KeyCode::Char('b' | 'f')) {
        dispatch(
            app,
            if key.code == KeyCode::Char('b') {
                Action::Back
            } else {
                Action::Forward
            },
        );
        return true;
    }

    // Pane cycling skips panes that aren't on screen. Tab moves on from every
    // pane except where it already means something: typing in a note, where it
    // nests a list item, the graph, where it steps to the next node, and a slash
    // command being typed in the chat, where it completes the name. Reading a
    // note is none of those, and the note pane used to keep the key anyway, so
    // opening a note left no way back to the explorer without the mouse.
    // Ctrl+Tab still switches document tabs from anywhere.
    let owns_tab = match app.focus {
        Focus::Note => typing_in_note(app),
        Focus::Graph => true,
        Focus::Chat => crate::slash::is_command(&app.chat.input),
        Focus::Explorer | Focus::Sidebar => false,
    };
    if key.code == KeyCode::Tab && !ctrl && !owns_tab {
        cycle_focus(app, 1);
        return true;
    }
    if key.code == KeyCode::BackTab && !owns_tab {
        cycle_focus(app, -1);
        return true;
    }

    false
}

/// Whether keys are going into the text of a note: the note pane has the
/// focus, its note is being edited, and vim, if it is on, is in Insert mode.
///
/// Only then does `Tab` belong to the note. Vim's Normal and Visual modes edit
/// without typing, and have `>` and `<` for indenting, so like the reading view
/// they let `Tab` move on to the next pane. A note left in edit mode while
/// another pane has the focus isn't being typed in either.
fn typing_in_note(app: &App) -> bool {
    app.focus == Focus::Note
        && app.editing()
        && (!app.config.editor.vim || app.vim.mode == crate::vim::VimMode::Insert)
}

pub fn cycle_focus(app: &mut App, delta: isize) {
    let mut order = vec![Focus::Explorer];
    order.push(if app.view == View::Graph {
        Focus::Graph
    } else {
        Focus::Note
    });
    if app.config.ui.show_right_sidebar {
        order.push(Focus::Sidebar);
    }
    if app.config.ui.show_chat {
        order.push(Focus::Chat);
    }
    if !app.config.ui.show_left_sidebar {
        order.retain(|f| *f != Focus::Explorer);
    }
    // Focus mode shows nothing but the note, so there is nowhere to move to.
    if app.focus_mode {
        order.retain(|f| matches!(f, Focus::Note | Focus::Graph));
    }
    if order.is_empty() {
        return;
    }

    let current = order.iter().position(|f| *f == app.focus).unwrap_or(0) as isize;
    let next = (current + delta).rem_euclid(order.len() as isize) as usize;
    app.focus = order[next];
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

fn handle_modal(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match app.modal.as_mut() {
        Some(Modal::Picker(picker)) => match key.code {
            KeyCode::Esc => app.modal = None,
            KeyCode::Enter => {
                let action = picker.selected_entry().map(|e| e.action.clone());
                if let Some(action) = action {
                    dispatch(app, action);
                } else {
                    app.modal = None;
                }
            }
            KeyCode::Up => picker.move_selection(-1),
            KeyCode::Down => picker.move_selection(1),
            KeyCode::Char('p') if ctrl => picker.move_selection(-1),
            KeyCode::Char('n') if ctrl => picker.move_selection(1),
            KeyCode::PageUp => picker.move_selection(-10),
            KeyCode::PageDown => picker.move_selection(10),
            KeyCode::Backspace => {
                picker.backspace();
                actions::update_live(app);
            }
            KeyCode::Char('u') if ctrl => {
                picker.clear();
                actions::update_live(app);
            }
            // `Ctrl+F` again widens a find to every note. It is also the way
            // there from a terminal that sends `Ctrl+Shift+F` as `Ctrl+F`.
            KeyCode::Char('f' | 'F') if ctrl && picker.kind == PickerKind::Find => {
                let query = std::mem::take(&mut picker.query);
                dispatch(app, Action::OpenSearch);
                if let Some(Modal::Picker(search)) = app.modal.as_mut() {
                    search.cursor = query.chars().count();
                    search.query = query;
                }
                actions::update_live(app);
            }
            KeyCode::Char('v' | 'V') if ctrl => {
                let text = crate::clipboard::paste();
                paste(app, &text);
            }
            // Any other `Ctrl` key, `⌘` ones included, is a shortcut that
            // doesn't apply here, not a letter to type.
            KeyCode::Char(ch) if !ctrl => {
                picker.insert(ch);
                actions::update_live(app);
            }
            _ => {}
        },

        Some(Modal::Prompt(prompt)) => match key.code {
            KeyCode::Esc => app.modal = None,
            KeyCode::Enter => {
                let prompt = prompt.clone();
                actions::submit_prompt(app, prompt);
            }
            KeyCode::Backspace => prompt.backspace(),
            KeyCode::Left => prompt.cursor = prompt.cursor.saturating_sub(1),
            KeyCode::Right => {
                prompt.cursor = (prompt.cursor + 1).min(prompt.value.chars().count());
            }
            KeyCode::Char('v' | 'V') if ctrl => {
                let text = crate::clipboard::paste();
                paste(app, &text);
            }
            KeyCode::Char(ch) if !ctrl => prompt.insert(ch),
            _ => {}
        },

        Some(Modal::Confirm(confirm)) => match key.code {
            // An explicit yes proceeds; anything else cancels, so a stray
            // keypress can't delete a note. Enter agrees because the dialog is
            // asking a question the user just triggered on purpose.
            KeyCode::Char('y' | 'Y') | KeyCode::Enter => {
                let action = confirm.action.clone();
                actions::confirm(app, action);
            }
            _ => app.modal = None,
        },

        Some(Modal::Help(scroll)) => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => app.modal = None,
            KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
            KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
            KeyCode::PageDown => *scroll += 10,
            KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
            _ => {}
        },

        None => {}
    }
}

// ---------------------------------------------------------------------------
// Panes
// ---------------------------------------------------------------------------

fn handle_explorer(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.explorer.select_next(),
        KeyCode::Char('k') | KeyCode::Up => app.explorer.select_previous(),
        KeyCode::Char('g') | KeyCode::Home => app.explorer.select_first(),
        KeyCode::Char('G') | KeyCode::End => app.explorer.select_last(),
        KeyCode::Char('H') => app.explorer.collapse_all(&app.index),
        KeyCode::Char('L') => app.explorer.expand_all(&app.index),
        KeyCode::PageDown => app.explorer.page(10),
        KeyCode::PageUp => app.explorer.page(-10),
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            if let Some(id) = app.explorer.selected_note() {
                app.open_note(id);
            } else {
                app.explorer.toggle(&app.index);
            }
        }
        KeyCode::Char(' ') => {
            app.explorer.toggle(&app.index);
        }
        KeyCode::Char('h') | KeyCode::Left => {
            app.explorer.toggle(&app.index);
        }
        KeyCode::Char('/') => {
            app.modal = Some(Modal::Prompt(Prompt::new(
                "Filter files",
                app.explorer.filter.clone(),
                PromptIntent::FilterExplorer,
            )));
        }
        KeyCode::Esc => {
            if !app.explorer.filter.is_empty() {
                app.explorer.filter.clear();
                app.explorer.rebuild(&app.index);
            }
        }
        KeyCode::Char('?') => dispatch(app, Action::OpenHelp),
        KeyCode::Char('q') => dispatch(app, Action::Quit),
        KeyCode::Char('s') => dispatch(app, Action::CycleSortOrder),
        _ => {}
    }
}

fn handle_note(app: &mut App, key: KeyEvent) {
    let Some(mode) = app.active().map(|t| t.mode) else {
        // With no note open, the pane behaves like the empty state's hints.
        match key.code {
            KeyCode::Char('?') => dispatch(app, Action::OpenHelp),
            KeyCode::Char('q') => dispatch(app, Action::Quit),
            _ => {}
        }
        return;
    };

    match mode {
        Mode::Reading => handle_reading(app, key),
        Mode::Editing => handle_editing(app, key),
    }
}

fn handle_reading(app: &mut App, key: KeyEvent) {
    let step = |app: &mut App, delta: isize| {
        if let Some(tab) = app.active_mut() {
            tab.scroll = (tab.scroll as isize + delta).max(0) as usize;
        }
    };
    // Panning across a wide table. The draw pass clamps this to the content's
    // width, which is the only place the width is known.
    let pan = |app: &mut App, delta: isize| {
        if let Some(tab) = app.active_mut() {
            tab.hscroll = (tab.hscroll as isize + delta).max(0) as usize;
        }
    };

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => step(app, 1),
        KeyCode::Char('k') | KeyCode::Up => step(app, -1),
        KeyCode::Char('l') | KeyCode::Right => pan(app, PAN_STEP),
        KeyCode::Char('h') | KeyCode::Left => pan(app, -PAN_STEP),
        KeyCode::PageDown | KeyCode::Char(' ') => step(app, 15),
        KeyCode::PageUp => step(app, -15),
        KeyCode::Char('g') | KeyCode::Home => {
            if let Some(tab) = app.active_mut() {
                tab.scroll = 0;
                tab.hscroll = 0;
            }
        }
        KeyCode::Char('G') | KeyCode::End => step(app, 100_000),
        // Following the first link on the page is the reading-mode equivalent
        // of clicking one.
        KeyCode::Enter => follow_first_link(app),
        KeyCode::Char('?') => dispatch(app, Action::OpenHelp),
        KeyCode::Char('q') => dispatch(app, Action::Quit),
        _ => {}
    }
}

fn follow_first_link(app: &mut App) {
    let Some(id) = app.active_note() else { return };
    let target = app.index.note(id).and_then(|note| {
        note.links.iter().find_map(|link| match &link.target {
            emeraldian_core::index::LinkTarget::Note(target) => {
                app.index.note(*target).map(|n| n.meta.rel.clone())
            }
            emeraldian_core::index::LinkTarget::Unresolved(name) => Some(name.clone()),
            _ => None,
        })
    });

    match target {
        Some(target) => dispatch(app, Action::FollowLink(target)),
        None => app.info("no links in this note"),
    }
}

fn handle_editing(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    // Formatting shortcuts are checked before the editor sees the key.
    if ctrl {
        match key.code {
            KeyCode::Char('b' | 'B') => {
                if let Some(editor) = app.editor_mut() {
                    editor.toggle_wrap("**");
                }
                return;
            }
            KeyCode::Char('i' | 'I') => {
                if let Some(editor) = app.editor_mut() {
                    editor.toggle_wrap("*");
                }
                return;
            }
            // Redo is `Ctrl+Y` on Windows and `Ctrl+Shift+Z` on a Mac and on
            // Linux; both work everywhere.
            KeyCode::Char('z' | 'Z') => {
                if let Some(editor) = app.editor_mut() {
                    if shift {
                        editor.redo();
                    } else {
                        editor.undo();
                    }
                }
                return;
            }
            KeyCode::Char('y' | 'Y') => {
                if let Some(editor) = app.editor_mut() {
                    editor.redo();
                }
                return;
            }
            KeyCode::Char('a' | 'A') => {
                if let Some(editor) = app.editor_mut() {
                    editor.select_all();
                }
                return;
            }
            KeyCode::Char(ch @ ('c' | 'C' | 'x' | 'X')) => {
                let cut = ch.eq_ignore_ascii_case(&'x');
                let text = app.editor_mut().and_then(|editor| {
                    let text = editor.selected_text()?;
                    if cut {
                        editor.commit();
                        editor.delete_selection();
                        editor.commit();
                    }
                    Some(text)
                });
                match text {
                    Some(text) => {
                        crate::clipboard::copy(&text);
                        app.info(if cut { "cut" } else { "copied" });
                    }
                    None => app.info("select some text first — Shift+arrows or drag"),
                }
                return;
            }
            KeyCode::Char('v' | 'V') => {
                let text = crate::clipboard::paste();
                paste_into_editor(app, &text);
                return;
            }
            // Emacs-style set-mark, for selecting without holding shift.
            KeyCode::Char(' ') => {
                if let Some(editor) = app.editor_mut() {
                    editor.begin_selection();
                }
                return;
            }
            KeyCode::Char('k' | 'K') if shift => {
                if let Some(editor) = app.editor_mut() {
                    editor.delete_line();
                }
                return;
            }
            KeyCode::Char('k' | 'K') => {
                if let Some(editor) = app.editor_mut() {
                    editor.insert_link();
                }
                return;
            }
            _ => {}
        }
    }

    // Moving between rows has to follow the text as it was wrapped, so it needs
    // the geometry the last frame recorded — the same source of truth clicks
    // are resolved against.
    if matches!(
        key.code,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown
    ) || (!ctrl && matches!(key.code, KeyCode::Home | KeyCode::End))
    {
        move_in_editor(app, key.code, shift);
        return;
    }

    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let mac = Keyboard::of(app) == Keyboard::Mac;
    // The keys below that move or delete by word or by line are typing keys.
    // Vim's Normal and Visual modes have their own, and leave these alone.
    let typing = typing_in_note(app);
    // `⌘⌫` deletes to where `⌘←` goes, which on a wrapped line is the start
    // of the row on screen.
    if typing && ctrl && shift && key.code == KeyCode::Backspace {
        delete_to_row_start(app);
        return;
    }
    let Some(editor) = app.editor_mut() else {
        return;
    };

    match key.code {
        // Readline's word keys, which are also what a Mac terminal sends for
        // `⌥←`, `⌥→` and `⌥⌦`, over SSH as much as anywhere.
        KeyCode::Char('b' | 'B') if typing && alt && !ctrl => editor.move_word_left(shift),
        KeyCode::Char('f' | 'F') if typing && alt && !ctrl => editor.move_word_right(shift),
        KeyCode::Char('d') if typing && alt && !ctrl => editor.delete_word_forward(),
        // `Ctrl+Backspace`, as a terminal without the Kitty protocol sends it.
        // Nothing sends it for a plain Backspace any more.
        KeyCode::Char('h') if typing && ctrl => editor.delete_word_back(),
        // Readline's, and vim's while typing.
        KeyCode::Char('u') if typing && ctrl => editor.delete_to_line_start(),
        // A Mac moves by word with `⌥` and an arrow, where Windows and Linux
        // go back and forward through the notes visited.
        KeyCode::Left if typing && mac && alt => editor.move_word_left(shift),
        KeyCode::Right if typing && mac && alt => editor.move_word_right(shift),
        KeyCode::Char(ch) if !ctrl => editor.insert_char(ch),
        KeyCode::Enter => editor.newline(),
        KeyCode::Tab => editor.tab(true),
        KeyCode::BackTab => editor.tab(false),
        // Deleting by word is `Ctrl` with Backspace or Delete on Windows and
        // Linux, and `⌥` with them on a Mac.
        KeyCode::Backspace if typing && (ctrl || (mac && alt)) => editor.delete_word_back(),
        KeyCode::Delete if typing && (ctrl || (mac && alt)) => editor.delete_word_forward(),
        KeyCode::Backspace => editor.backspace(),
        KeyCode::Delete => editor.delete_forward(),
        KeyCode::Left if ctrl => editor.move_word_left(shift),
        KeyCode::Right if ctrl => editor.move_word_right(shift),
        KeyCode::Left => editor.move_left(shift),
        KeyCode::Right => editor.move_right(shift),
        KeyCode::Home if ctrl => editor.move_document_start(shift),
        KeyCode::End if ctrl => editor.move_document_end(shift),
        KeyCode::Esc => {
            editor.commit();
            dispatch(app, Action::ToggleMode);
        }
        _ => {}
    }
}

/// Handles text the terminal pasted in one piece.
///
/// Bracketed paste is on, so a paste arrives whole rather than as keystrokes.
/// That is what keeps a pasted newline from sending a half-written chat
/// message, and a pasted `d` from running as a vim command.
pub fn paste(app: &mut App, text: &str) {
    app.status.text.clear();
    let text = crate::clipboard::normalize(text);

    let typing_field = matches!(app.modal, Some(Modal::Picker(_) | Modal::Prompt(_)))
        || (app.modal.is_none() && app.focus == Focus::Chat);
    if typing_field {
        // A search, a name or a chat message is one line, so the paste is
        // typed into it as one. A line copied whole ends in a line break,
        // which is not part of what was meant.
        for ch in text.trim_end_matches('\n').chars() {
            let ch = if ch.is_whitespace() { ' ' } else { ch };
            handle(app, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    } else if app.modal.is_none() && app.focus == Focus::Note && app.editing() {
        paste_into_editor(app, &text);
    }
}

/// Inserts text as a single undo step, replacing any selection.
fn paste_into_editor(app: &mut App, text: &str) {
    let vim_normal = app.config.editor.vim && app.vim.mode.is_normal_like();
    if vim_normal {
        // A paste over a visual selection replaces it, and what is left is
        // no longer a selection to stay in.
        app.vim.reset();
    }
    let Some(editor) = app.editor_mut() else {
        return;
    };
    editor.commit();
    editor.insert_str(text);
    editor.commit();
    // Normal mode keeps the cursor on a character, never past the end.
    if vim_normal {
        editor.clamp_normal();
    }
}

/// Deletes back to the start of the row on screen, or the selection.
fn delete_to_row_start(app: &mut App) {
    let (width, _) = crate::ui::note::edit_viewport(app);
    let wrap = app.config.editor.wrap;
    if let Some(editor) = app.editor_mut() {
        let layout = editor.layout(width, wrap);
        editor.delete_to_row_start(&layout);
    }
}

/// Moves the cursor by what is on screen rather than by source line.
fn move_in_editor(app: &mut App, code: KeyCode, extend: bool) {
    let (width, height) = crate::ui::note::edit_viewport(app);
    let wrap = app.config.editor.wrap;
    let Some(editor) = app.editor_mut() else {
        return;
    };

    let layout = editor.layout(width, wrap);
    // A page is the viewport, less a row of overlap so the eye keeps its place.
    let page = height.saturating_sub(1).max(1) as isize;
    match code {
        KeyCode::Up => editor.move_row(&layout, -1, extend),
        KeyCode::Down => editor.move_row(&layout, 1, extend),
        KeyCode::PageUp => editor.move_row(&layout, -page, extend),
        KeyCode::PageDown => editor.move_row(&layout, page, extend),
        KeyCode::Home => editor.move_row_start(&layout, extend),
        KeyCode::End => editor.move_row_end(&layout, extend),
        _ => {}
    }
}

fn handle_sidebar(app: &mut App, key: KeyEvent) {
    let targets = sidebar_targets(app);

    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            app.side_selected = (app.side_selected + 1).min(targets.len().saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.side_selected = app.side_selected.saturating_sub(1);
        }
        KeyCode::Char('g') | KeyCode::Home => app.side_selected = 0,
        KeyCode::Char('G') | KeyCode::End => {
            app.side_selected = targets.len().saturating_sub(1);
        }
        KeyCode::Tab if key.modifiers.is_empty() => dispatch(app, Action::CycleSidePanel),
        KeyCode::Enter => match targets.get(app.side_selected).cloned() {
            Some(SidebarTarget::Heading(line)) => {
                // Jumping to a heading scrolls the reading view and moves the
                // editor's cursor, whichever mode is active.
                //
                // The outline names a line in the file, but the reading view
                // scrolls by rendered rows — prose wraps, headings gain a rule,
                // a picture takes a dozen rows — so the two have to be
                // translated through what the last frame actually drew.
                let row = crate::ui::note::anchor_row(&app.regions.anchors, line);
                if let Some(tab) = app.active_mut() {
                    tab.scroll = row;
                }
                if let Some(editor) = app.editor_mut() {
                    editor.goto(line, 0);
                }
                app.focus = Focus::Note;
            }
            Some(SidebarTarget::Note(id)) => app.open_note(id),
            Some(SidebarTarget::Tag(tag)) => {
                // Selecting a tag searches for it, which is the useful action.
                dispatch(app, Action::OpenSearch);
                if let Some(Modal::Picker(picker)) = app.modal.as_mut() {
                    for ch in format!("#{tag}").chars() {
                        picker.insert(ch);
                    }
                }
                actions::update_search(app);
            }
            None => {}
        },
        KeyCode::Char('?') => dispatch(app, Action::OpenHelp),
        KeyCode::Char('q') => dispatch(app, Action::Quit),
        _ => {}
    }
}

fn handle_chat(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // While a slash command is being typed, the arrow keys belong to the list of
    // commands rather than to the transcript — a menu on screen is what the keys
    // obviously act on, and it is the only way to read the list without knowing
    // the names already.
    let completions = if app.chat.busy {
        Vec::new()
    } else {
        crate::slash::completions(&app.chat.input)
    };
    if !completions.is_empty() && handle_completions(app, key, &completions) {
        return;
    }

    match key.code {
        KeyCode::Enter if !app.chat.busy && crate::slash::is_command(&app.chat.input) => {
            run_command(app);
        }
        KeyCode::Enter if !app.chat.busy => agent::send(app),
        KeyCode::Char('c' | 'C') if ctrl => {
            app.chat.cancel();
            app.info("stopping…");
        }
        KeyCode::Char('r' | 'R') if ctrl => {
            app.chat.reset();
            app.info("conversation cleared");
        }
        KeyCode::Char(ch) if !ctrl => app.chat.insert_char(ch),
        KeyCode::Backspace => app.chat.backspace(),
        KeyCode::Left => app.chat.move_cursor(-1),
        KeyCode::Right => app.chat.move_cursor(1),
        KeyCode::Up | KeyCode::PageUp => {
            // Scrolling up detaches from the bottom so streaming output doesn't
            // yank the view back.
            app.chat.follow = false;
            app.chat.scroll = app.chat.scroll.saturating_sub(3);
        }
        KeyCode::Down | KeyCode::PageDown => {
            app.chat.scroll += 3;
            app.chat.follow = true;
        }
        KeyCode::Esc => app.focus = Focus::Note,
        _ => {}
    }
}

/// Keys that belong to the slash-command list. Returns whether one was used.
fn handle_completions(
    app: &mut App,
    key: KeyEvent,
    completions: &[&'static crate::slash::SlashCommand],
) -> bool {
    let selected = || {
        completions
            .get(app.chat.completion)
            .or(completions.first())
            .map(|command| command.name)
    };

    match key.code {
        // Only when there is something to choose between: with the command
        // already named, the arrows still belong to the transcript.
        KeyCode::Up if completions.len() > 1 => app.chat.move_completion(-1, completions.len()),
        KeyCode::Down if completions.len() > 1 => app.chat.move_completion(1, completions.len()),
        // Tab fills in the highlighted command, the way a shell would.
        KeyCode::Tab => {
            if let Some(name) = selected() {
                app.chat.complete_with(name);
            }
        }
        // Enter takes the highlight when the name is still being typed, and runs
        // the command once it is settled. So `/` then arrows then Enter fills it
        // in, and Enter again runs it — while typing `/help` and pressing Enter
        // runs it outright, without a detour through the menu.
        KeyCode::Enter => match selected().filter(|name| !settled(&app.chat.input, name)) {
            Some(name) => app.chat.complete_with(name),
            None => run_command(app),
        },
        // Abandoning what you were typing is what Escape means here; the list
        // closes with it, and a second Escape leaves the panel.
        KeyCode::Esc => app.chat.clear_input(),
        _ => return false,
    }
    true
}

/// Whether the input already names this command, rather than a prefix of it.
fn settled(input: &str, name: &str) -> bool {
    crate::slash::parse(input).is_some_and(|(typed, _)| typed.eq_ignore_ascii_case(name))
}

fn run_command(app: &mut App) {
    let input = app.chat.input.trim().to_string();
    app.chat.clear_input();
    if let crate::slash::Outcome::Unknown(name) = crate::slash::run(app, &input) {
        app.error(format!("unknown command '/{name}'; /help lists them"));
    }
}

fn handle_graph(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(graph) = app.graph.as_mut() else {
        return;
    };
    // Pan speed scales with zoom so it feels constant on screen.
    let step = 20.0 / graph.zoom;

    match key.code {
        KeyCode::Char('h') => graph.center.x -= step,
        KeyCode::Char('l') => graph.center.x += step,
        KeyCode::Char('k') => graph.center.y += step,
        KeyCode::Char('j') => graph.center.y -= step,
        // Arrows walk the selection from node to node, which is how you read a
        // graph; `hjkl` pans the camera. Binding both to panning left no way to
        // step through the picture except Tab, which jumps by link count and so
        // teleports across the vault.
        KeyCode::Left => graph.select_in_direction(-1.0, 0.0),
        KeyCode::Right => graph.select_in_direction(1.0, 0.0),
        KeyCode::Up => graph.select_in_direction(0.0, 1.0),
        KeyCode::Down => graph.select_in_direction(0.0, -1.0),
        KeyCode::Char('+' | '=') => graph.zoom_by(1.25),
        KeyCode::Char('-' | '_') => graph.zoom_by(1.0 / 1.25),
        // `f` to fit and `0` to reset are the two conventions graph and image
        // viewers use; both frame the whole layout.
        KeyCode::Char('f' | '0') => graph.fit(),
        KeyCode::Tab | KeyCode::Char('n') => graph.cycle_selection(1),
        KeyCode::BackTab | KeyCode::Char('N') => graph.cycle_selection(-1),
        // Recentre on the selection without changing zoom.
        KeyCode::Char('c') => {
            if let Some(selected) = graph.selected {
                graph.focus_node(selected);
            }
        }
        KeyCode::Enter => {
            let target = graph.selected.and_then(|index| {
                graph
                    .simulation
                    .graph
                    .nodes
                    .get(index)
                    .map(|node| (node.kind.clone(), node.label.clone()))
            });
            match target {
                Some((emeraldian_core::graph::NodeKind::Note(id), _)) => {
                    dispatch(app, Action::OpenNote(id));
                }
                // Opening an unresolved node creates the note it stands for,
                // which is the whole point of showing them.
                Some((emeraldian_core::graph::NodeKind::Unresolved, label)) => {
                    dispatch(app, Action::FollowLink(label));
                }
                _ => app.info("select a node first; Tab cycles through them"),
            }
        }
        KeyCode::Char('L') => dispatch(app, Action::ToggleGraphLabels),
        KeyCode::Char('u') => dispatch(app, Action::ToggleGraphUnresolved),
        KeyCode::Char('t') => dispatch(app, Action::ToggleGraphTags),
        KeyCode::Char('a') => dispatch(app, Action::ToggleGraphAttachments),
        KeyCode::Char('r') if !ctrl => {
            app.refresh_graph();
            app.info("graph rebuilt");
        }
        KeyCode::Esc => dispatch(app, Action::OpenNotesView),
        KeyCode::Char('?') => dispatch(app, Action::OpenHelp),
        KeyCode::Char('q') => dispatch(app, Action::Quit),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn app() -> (TempVault, App) {
        let vault = TempVault::new("keys");
        vault.write("A.md", "# A\n\n[[B]]\n");
        vault.write("B.md", "# B\n");
        let app = App::new(vault.vault(), Config::default()).expect("app");
        (vault, app)
    }

    #[test]
    fn key_releases_are_ignored() {
        let (_v, mut app) = app();
        let mut event = ctrl(KeyCode::Char('g'));
        event.kind = KeyEventKind::Release;

        handle(&mut app, event);
        assert_eq!(
            app.view,
            View::Notes,
            "a release must not re-run the binding"
        );
    }

    #[test]
    fn global_bindings_work_from_any_pane() {
        let (_v, mut app) = app();
        app.focus = Focus::Explorer;
        handle(&mut app, ctrl(KeyCode::Char('p')));
        assert!(matches!(app.modal, Some(Modal::Picker(_))));
    }

    #[test]
    fn typing_in_the_editor_does_not_trigger_commands() {
        let (_v, mut app) = app();
        let b = app.index.id_of_rel("B.md").unwrap();
        app.open_note(b);
        app.active_mut().unwrap().mode = Mode::Editing;

        handle(&mut app, key(KeyCode::Char('n')));
        handle(&mut app, key(KeyCode::Char('g')));

        assert!(app.modal.is_none(), "plain letters are text, not commands");
        assert_eq!(app.view, View::Notes);
        assert!(app.editor_mut().unwrap().text().contains("ng"));
    }

    #[test]
    fn explorer_navigation_and_opening() {
        let (_v, mut app) = app();
        app.focus = Focus::Explorer;

        handle(&mut app, key(KeyCode::Char('j')));
        handle(&mut app, key(KeyCode::Enter));

        assert!(app.active_note().is_some(), "Enter opens the selected note");
        assert_eq!(app.focus, Focus::Note);
    }

    fn shift_tab() -> KeyEvent {
        // What terminals send for Shift+Tab: a key of its own, with Shift held.
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)
    }

    #[test]
    fn tab_moves_on_from_a_note_opened_in_the_explorer() {
        // Opening a note hands it the focus, in reading mode. The note pane
        // used to keep Tab and Shift+Tab for itself even there, where they did
        // nothing, so the keyboard had no way back to the explorer.
        let (_v, mut app) = app();
        app.focus = Focus::Explorer;
        handle(&mut app, key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Note);
        assert_eq!(app.active().unwrap().mode, Mode::Reading);

        handle(&mut app, shift_tab());
        assert_eq!(app.focus, Focus::Explorer, "Shift+Tab goes back");

        handle(&mut app, key(KeyCode::Tab));
        handle(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Sidebar, "Tab goes on past the note");
    }

    #[test]
    fn tab_moves_on_from_an_empty_note_pane() {
        let (_v, mut app) = app();
        app.focus = Focus::Note;
        assert!(app.active().is_none());

        handle(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Sidebar);
        handle(&mut app, shift_tab());
        handle(&mut app, shift_tab());
        assert_eq!(app.focus, Focus::Explorer);
    }

    #[test]
    fn tab_still_nests_a_list_item_while_editing() {
        let (vault, mut app) = app();
        vault.write("List.md", "- item\n");
        app.refresh();
        let list = app.index.id_of_rel("List.md").unwrap();
        app.open_note(list);
        app.active_mut().unwrap().mode = Mode::Editing;

        handle(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Note, "typing keeps the key");
        assert_eq!(app.editor_mut().unwrap().text(), "    - item\n");

        handle(&mut app, shift_tab());
        assert_eq!(app.focus, Focus::Note);
        assert_eq!(app.editor_mut().unwrap().text(), "- item\n");
    }

    #[test]
    fn escape_in_a_picker_closes_it() {
        let (_v, mut app) = app();
        handle(&mut app, ctrl(KeyCode::Char('o')));
        assert!(app.modal.is_some());

        handle(&mut app, key(KeyCode::Esc));
        assert!(app.modal.is_none());
    }

    #[test]
    fn the_quick_switcher_opens_the_chosen_note() {
        let (_v, mut app) = app();
        handle(&mut app, ctrl(KeyCode::Char('o')));
        for ch in "B".chars() {
            handle(&mut app, key(KeyCode::Char(ch)));
        }
        handle(&mut app, key(KeyCode::Enter));

        assert_eq!(app.note_title(app.active_note().expect("open")), "B");
        assert!(app.modal.is_none());
    }

    #[test]
    fn confirmation_requires_an_explicit_yes() {
        let (vault, mut app) = app();
        let b = app.index.id_of_rel("B.md").unwrap();
        app.open_note(b);

        dispatch(&mut app, Action::DeleteNote);
        handle(&mut app, key(KeyCode::Char('x')));
        assert!(vault.exists("B.md"), "anything but y cancels");
        assert!(app.modal.is_none());

        dispatch(&mut app, Action::DeleteNote);
        handle(&mut app, key(KeyCode::Char('y')));
        assert!(!vault.exists("B.md"));
    }

    #[test]
    fn reading_mode_scrolls_and_clamps_at_the_top() {
        let (_v, mut app) = app();
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);

        handle(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.active().unwrap().scroll, 1);

        handle(&mut app, key(KeyCode::Char('k')));
        handle(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.active().unwrap().scroll, 0, "must not go negative");
    }

    #[test]
    fn the_pane_toggles_respond_to_the_bytes_a_terminal_actually_sends() {
        let (_v, mut app) = app();

        // A terminal without the Kitty keyboard protocol sends 0x1C for Ctrl+\
        // and 0x1D for Ctrl+], which crossterm reports as Ctrl+4 and Ctrl+5.
        // Binding the punctuation alone left both toggles doing nothing.
        let right = app.config.ui.show_right_sidebar;
        handle(&mut app, ctrl(KeyCode::Char('5')));
        assert_ne!(
            app.config.ui.show_right_sidebar, right,
            "Ctrl+] must toggle the outline sidebar"
        );

        let left = app.config.ui.show_left_sidebar;
        handle(&mut app, ctrl(KeyCode::Char('4')));
        assert_ne!(
            app.config.ui.show_left_sidebar, left,
            "Ctrl+\\ must toggle the file explorer"
        );

        // The punctuation form still works, for terminals that do send it.
        handle(&mut app, ctrl(KeyCode::Char(']')));
        assert_eq!(app.config.ui.show_right_sidebar, right);
        handle(&mut app, ctrl(KeyCode::Char('\\')));
        assert_eq!(app.config.ui.show_left_sidebar, left);
    }

    #[test]
    fn a_plain_digit_is_not_mistaken_for_a_pane_toggle() {
        let (_v, mut app) = app();
        let before = (
            app.config.ui.show_left_sidebar,
            app.config.ui.show_right_sidebar,
        );
        handle(&mut app, key(KeyCode::Char('4')));
        handle(&mut app, key(KeyCode::Char('5')));
        assert_eq!(
            (
                app.config.ui.show_left_sidebar,
                app.config.ui.show_right_sidebar
            ),
            before,
            "only the control form is rewritten"
        );
    }

    #[test]
    fn reading_mode_pans_sideways_and_clamps_at_the_left() {
        let (_v, mut app) = app();
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);

        handle(&mut app, key(KeyCode::Right));
        assert_eq!(app.active().unwrap().hscroll, PAN_STEP as usize);

        handle(&mut app, key(KeyCode::Char('l')));
        assert_eq!(app.active().unwrap().hscroll, PAN_STEP as usize * 2);

        for _ in 0..5 {
            handle(&mut app, key(KeyCode::Left));
        }
        assert_eq!(app.active().unwrap().hscroll, 0, "must not go negative");

        // Jumping to the top comes back to the left edge too; leaving a note
        // scrolled sideways when you asked for the start of it is disorienting.
        handle(&mut app, key(KeyCode::Right));
        handle(&mut app, key(KeyCode::Char('g')));
        assert_eq!(app.active().unwrap().hscroll, 0);
    }

    #[test]
    fn enter_in_reading_mode_follows_a_link() {
        let (_v, mut app) = app();
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);

        handle(&mut app, key(KeyCode::Enter));
        assert_eq!(app.note_title(app.active_note().expect("open")), "B");
    }

    #[test]
    fn editing_shortcuts_wrap_the_selection() {
        let (_v, mut app) = app();
        let b = app.index.id_of_rel("B.md").unwrap();
        app.open_note(b);
        app.active_mut().unwrap().mode = Mode::Editing;

        let editor = app.editor_mut().unwrap();
        editor.goto(0, 2);
        editor.begin_selection();
        editor.move_line_end(true);

        handle(&mut app, ctrl(KeyCode::Char('b')));
        assert!(app.editor_mut().unwrap().text().contains("**B**"));
    }

    #[test]
    fn escape_leaves_the_editor_for_reading_mode() {
        let (_v, mut app) = app();
        let b = app.index.id_of_rel("B.md").unwrap();
        app.open_note(b);
        app.active_mut().unwrap().mode = Mode::Editing;

        handle(&mut app, key(KeyCode::Esc));
        assert_eq!(app.active().unwrap().mode, Mode::Reading);
    }

    #[test]
    fn graph_keys_pan_zoom_and_select() {
        let (_v, mut app) = app();
        dispatch(&mut app, Action::OpenGraph);

        let start = app.graph.as_ref().unwrap().center.x;
        handle(&mut app, key(KeyCode::Char('l')));
        assert!(app.graph.as_ref().unwrap().center.x > start);

        handle(&mut app, key(KeyCode::Char('+')));
        assert!(app.graph.as_ref().unwrap().zoom > 1.0);

        handle(&mut app, key(KeyCode::Tab));
        assert!(app.graph.as_ref().unwrap().selected.is_some());
    }

    #[test]
    fn enter_on_a_graph_node_opens_the_note() {
        let (_v, mut app) = app();
        dispatch(&mut app, Action::OpenGraph);
        handle(&mut app, key(KeyCode::Tab));
        handle(&mut app, key(KeyCode::Enter));

        assert_eq!(app.view, View::Notes);
        assert!(app.active_note().is_some());
    }

    #[test]
    fn escape_leaves_the_graph() {
        let (_v, mut app) = app();
        dispatch(&mut app, Action::OpenGraph);
        handle(&mut app, key(KeyCode::Esc));
        assert_eq!(app.view, View::Notes);
    }

    #[test]
    fn chat_input_accepts_text_and_ctrl_r_clears() {
        let (_v, mut app) = app();
        dispatch(&mut app, Action::ToggleChat);
        assert_eq!(app.focus, Focus::Chat);

        for ch in "hi".chars() {
            handle(&mut app, key(KeyCode::Char(ch)));
        }
        assert_eq!(app.chat.input, "hi");

        app.chat
            .transcript
            .push(crate::agent::Entry::User("x".into()));
        handle(&mut app, ctrl(KeyCode::Char('r')));
        assert!(app.chat.transcript.is_empty());
    }

    #[test]
    fn tab_cycles_only_through_visible_panes() {
        let (_v, mut app) = app();
        app.config.ui.show_right_sidebar = false;
        app.config.ui.show_chat = false;
        app.focus = Focus::Explorer;

        cycle_focus(&mut app, 1);
        assert_eq!(app.focus, Focus::Note);
        cycle_focus(&mut app, 1);
        assert_eq!(app.focus, Focus::Explorer, "wraps past the hidden panes");
    }

    #[test]
    fn the_help_overlay_scrolls_and_closes() {
        let (_v, mut app) = app();
        handle(&mut app, key(KeyCode::Char('?')));
        assert!(matches!(app.modal, Some(Modal::Help(0))));

        handle(&mut app, key(KeyCode::Char('j')));
        assert!(matches!(app.modal, Some(Modal::Help(1))));

        handle(&mut app, key(KeyCode::Esc));
        assert!(app.modal.is_none());
    }

    #[test]
    fn the_explorer_filter_prompt_applies_on_enter() {
        let (_v, mut app) = app();
        app.focus = Focus::Explorer;

        handle(&mut app, key(KeyCode::Char('/')));
        for ch in "B".chars() {
            handle(&mut app, key(KeyCode::Char(ch)));
        }
        handle(&mut app, key(KeyCode::Enter));

        assert_eq!(app.explorer.filter, "B");
        assert_eq!(app.explorer.len(), 1);
    }

    #[test]
    fn the_outline_scrolls_to_the_row_a_heading_was_drawn_on() {
        // A source line and a rendered row are different numbers: the paragraph
        // below wraps to several rows and the H1 gains a rule under it, so the
        // second heading is drawn far below the line it lives on.
        let vault = TempVault::new("outline-jump");
        let prose = "a ".repeat(120);
        vault.write("Deep.md", &format!("# One\n\n{prose}\n\n## Two\n\nend\n"));
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let deep = app.index.id_of_rel("Deep.md").expect("indexed");
        app.open_note(deep);

        // Draw once, so the outline has a rendered layout to translate through.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 30)).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("frame");

        app.side_panel = crate::app::SidePanel::Outline;
        app.focus = Focus::Sidebar;
        app.side_selected = 1;
        let line = match &sidebar_targets(&app)[1] {
            SidebarTarget::Heading(line) => *line,
            other => panic!("expected a heading, got {other:?}"),
        };
        handle(&mut app, key(KeyCode::Enter));

        let scroll = app.active().expect("tab").scroll;
        assert!(
            scroll > line,
            "`## Two` is on source line {line} but was drawn further down; \
             scrolled to {scroll}"
        );
    }

    #[test]
    fn the_outline_falls_back_to_the_line_before_anything_is_drawn() {
        let (_v, mut app) = app();
        let a = app.index.id_of_rel("A.md").expect("indexed");
        app.open_note(a);

        app.side_panel = crate::app::SidePanel::Outline;
        app.focus = Focus::Sidebar;
        handle(&mut app, key(KeyCode::Enter));

        assert_eq!(app.active().expect("tab").scroll, 0, "no frame, no crash");
    }
}

#[cfg(test)]
mod chat_command_tests {
    use super::*;
    use crate::agent::Entry;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    fn app() -> (TempVault, App) {
        let vault = TempVault::new("chat-keys");
        vault.write("A.md", "# A\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.focus = Focus::Chat;
        (vault, app)
    }

    fn press(app: &mut App, code: KeyCode) {
        handle(app, KeyEvent::new(code, KeyModifiers::empty()));
    }

    fn type_str(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch));
        }
    }

    #[test]
    fn a_slash_command_runs_locally_instead_of_being_sent() {
        let (_v, mut app) = app();
        type_str(&mut app, "/writes off");
        press(&mut app, KeyCode::Enter);

        assert!(!app.config.agent.allow_writes, "the command took effect");
        assert!(
            app.chat.conversation.is_empty(),
            "a command must never reach the model"
        );
        assert!(app.chat.input.is_empty(), "the input box is cleared");
    }

    #[test]
    fn a_message_mentioning_a_slash_is_still_a_message() {
        let (_v, mut app) = app();
        // Only a leading slash is a command; prose about paths is not.
        type_str(&mut app, "what is in /tmp");
        assert!(!crate::slash::is_command(&app.chat.input));
    }

    #[test]
    fn tab_completes_the_command_being_typed() {
        let (_v, mut app) = app();
        type_str(&mut app, "/resu");
        press(&mut app, KeyCode::Tab);

        assert_eq!(app.chat.input, "/resume ");
        assert_eq!(
            app.chat.cursor,
            app.chat.input.chars().count(),
            "the cursor follows the completion"
        );
    }

    #[test]
    fn arrows_walk_the_command_list_and_enter_takes_the_highlight() {
        let (_v, mut app) = app();
        type_str(&mut app, "/");

        let names: Vec<&str> = crate::slash::completions("/")
            .iter()
            .map(|command| command.name)
            .collect();
        assert!(names.len() > 3, "a bare slash offers everything");

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.chat.completion, 2, "the highlight moved");

        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.chat.input,
            format!("/{} ", names[2]),
            "Enter takes the highlighted command rather than running the first"
        );

        press(&mut app, KeyCode::Enter);
        assert!(
            app.chat.input.is_empty(),
            "and Enter again runs it, since the name is settled"
        );
    }

    #[test]
    fn the_highlight_wraps_and_resets_as_the_command_is_typed() {
        let (_v, mut app) = app();
        type_str(&mut app, "/");

        press(&mut app, KeyCode::Up);
        assert_eq!(
            app.chat.completion,
            crate::slash::completions("/").len() - 1,
            "up from the top wraps to the bottom"
        );

        type_str(&mut app, "s");
        assert_eq!(
            app.chat.completion, 0,
            "a keystroke changes the list, so the highlight starts over"
        );
    }

    #[test]
    fn a_named_command_leaves_the_arrows_to_the_transcript() {
        let (_v, mut app) = app();
        app.chat.scroll = 9;
        app.chat.follow = true;
        type_str(&mut app, "/model ");

        press(&mut app, KeyCode::Up);
        assert_eq!(app.chat.completion, 0, "there is nothing left to choose");
        assert!(
            !app.chat.follow && app.chat.scroll < 9,
            "so Up scrolls the conversation, as it does while writing a message"
        );
    }

    #[test]
    fn escape_abandons_a_half_typed_command_before_leaving_the_panel() {
        let (_v, mut app) = app();
        type_str(&mut app, "/sess");

        press(&mut app, KeyCode::Esc);
        assert!(app.chat.input.is_empty(), "the command is abandoned");
        assert_eq!(app.focus, Focus::Chat, "but the panel keeps focus");

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, Focus::Note, "a second Escape leaves");
    }

    #[test]
    fn tab_in_a_normal_message_does_not_complete() {
        let (_v, mut app) = app();
        type_str(&mut app, "hello");
        press(&mut app, KeyCode::Tab);
        assert_eq!(app.chat.input, "hello");
    }

    #[test]
    fn an_unknown_command_is_reported_and_not_sent() {
        let (_v, mut app) = app();
        type_str(&mut app, "/nope");
        press(&mut app, KeyCode::Enter);

        assert!(app.chat.conversation.is_empty());
        assert!(
            app.status.text.contains("unknown command"),
            "the user is told, rather than the typo silently vanishing"
        );
    }

    #[test]
    fn command_output_lands_in_the_transcript() {
        let (_v, mut app) = app();
        type_str(&mut app, "/vault");
        press(&mut app, KeyCode::Enter);

        assert!(
            matches!(app.chat.transcript.last(), Some(Entry::Context(_))),
            "output is part of the conversation's history"
        );
    }

    #[test]
    fn q_in_the_chat_box_types_rather_than_quits() {
        let (_v, mut app) = app();
        type_str(&mut app, "q");
        assert_eq!(app.chat.input, "q");
        assert!(app.modal.is_none(), "no quit prompt while typing");
    }
}

#[cfg(test)]
mod binding_tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    fn graph_app() -> (TempVault, App) {
        let vault = TempVault::new("bindings");
        vault.write("A.md", "# A\n\n[[B]] [[C]]\n");
        vault.write("B.md", "# B\n\n[[C]]\n");
        vault.write("C.md", "# C\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        dispatch(&mut app, Action::OpenGraph);
        (vault, app)
    }

    fn press(app: &mut App, ch: char) {
        handle(app, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()));
    }

    #[test]
    fn fit_frames_the_graph_rather_than_the_origin() {
        let (_v, mut app) = graph_app();
        {
            let graph = app.graph.as_mut().expect("graph");
            graph.center = emeraldian_core::graph::Vec2::new(9_999.0, 9_999.0);
            graph.zoom = 8.0;
        }

        press(&mut app, 'f');

        let graph = app.graph.as_ref().expect("graph");
        let (min_x, min_y, max_x, max_y) = graph.simulation.graph.bounds();
        assert!((graph.zoom - 1.0).abs() < f32::EPSILON);
        assert!(
            graph.center.x >= min_x && graph.center.x <= max_x,
            "fit must land inside the layout, not back at the origin"
        );
        assert!(graph.center.y >= min_y && graph.center.y <= max_y);
    }

    #[test]
    fn zoom_is_clamped_at_both_ends() {
        let (_v, mut app) = graph_app();
        for _ in 0..60 {
            press(&mut app, '+');
        }
        assert_eq!(app.graph.as_ref().unwrap().zoom, crate::app::MAX_ZOOM);
        for _ in 0..120 {
            press(&mut app, '-');
        }
        assert_eq!(app.graph.as_ref().unwrap().zoom, crate::app::MIN_ZOOM);
    }

    #[test]
    fn the_first_tab_selects_the_best_connected_node() {
        let (_v, mut app) = graph_app();
        handle(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()));

        let graph = app.graph.as_ref().expect("graph");
        let selected = graph.selected.expect("something is selected");
        let best = graph
            .simulation
            .graph
            .nodes
            .iter()
            .map(|n| n.degree)
            .max()
            .unwrap_or(0);
        assert_eq!(
            graph.simulation.graph.nodes[selected].degree, best,
            "the first Tab should land somewhere worth looking at"
        );
    }

    #[test]
    fn n_and_shift_n_step_the_selection_both_ways() {
        let (_v, mut app) = graph_app();
        press(&mut app, 'n');
        let first = app.graph.as_ref().unwrap().selected;
        press(&mut app, 'n');
        let second = app.graph.as_ref().unwrap().selected;
        assert_ne!(first, second);
        press(&mut app, 'N');
        assert_eq!(app.graph.as_ref().unwrap().selected, first, "N goes back");
    }

    #[test]
    fn c_recentres_on_the_selection() {
        let (_v, mut app) = graph_app();
        press(&mut app, 'n');
        let node = {
            let graph = app.graph.as_ref().unwrap();
            graph.simulation.graph.nodes[graph.selected.unwrap()].pos
        };
        app.graph.as_mut().unwrap().center = emeraldian_core::graph::Vec2::new(500.0, 500.0);

        press(&mut app, 'c');

        assert_eq!(app.graph.as_ref().unwrap().center, node);
    }

    #[test]
    fn uppercase_l_toggles_labels_and_lowercase_l_pans() {
        let (_v, mut app) = graph_app();
        let before = app.config.graph.show_labels;
        let x = app.graph.as_ref().unwrap().center.x;

        press(&mut app, 'L');
        assert_ne!(app.config.graph.show_labels, before, "L toggles labels");
        assert_eq!(app.graph.as_ref().unwrap().center.x, x, "L does not pan");

        press(&mut app, 'l');
        assert!(app.graph.as_ref().unwrap().center.x > x, "l pans right");
    }

    #[test]
    fn r_rebuilds_the_graph_without_leaving_the_view() {
        let (_v, mut app) = graph_app();
        press(&mut app, 'r');
        assert_eq!(app.view, View::Graph);
        assert!(app.graph.is_some());
    }

    #[test]
    fn arrows_move_the_selection_and_hjkl_moves_the_camera() {
        let (_v, mut app) = graph_app();
        {
            // Two nodes side by side, so "right" has an unambiguous answer.
            let graph = app.graph.as_mut().unwrap();
            graph
                .simulation
                .drag(0, emeraldian_core::graph::Vec2::new(0.0, 0.0));
            graph
                .simulation
                .drag(1, emeraldian_core::graph::Vec2::new(30.0, 0.0));
            graph.selected = Some(0);
        }
        let center = app.graph.as_ref().unwrap().center;

        handle(
            &mut app,
            KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        );
        let graph = app.graph.as_ref().unwrap();
        assert_eq!(graph.selected, Some(1), "→ walks to the next node");
        assert_eq!(graph.center, center, "→ does not pan");

        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::empty()),
        );
        let graph = app.graph.as_ref().unwrap();
        assert!(graph.center.x > center.x, "l still pans");
        assert_eq!(
            graph.selected,
            Some(1),
            "panning does not move the selection"
        );
    }

    #[test]
    fn a_toggles_attachments() {
        let (_v, mut app) = graph_app();
        let before = app.config.graph.show_attachments;
        press(&mut app, 'a');
        assert_ne!(app.config.graph.show_attachments, before);
    }

    #[test]
    fn q_asks_before_quitting_from_the_graph() {
        let (_v, mut app) = graph_app();
        press(&mut app, 'q');
        assert!(!app.quit);
        assert!(matches!(app.modal, Some(Modal::Confirm(_))));
    }

    #[test]
    fn enter_with_nothing_selected_says_so_instead_of_doing_nothing() {
        let (_v, mut app) = graph_app();
        app.graph.as_mut().unwrap().selected = None;
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert!(!app.status.text.is_empty(), "silence looks like a bug");
    }

    #[test]
    fn q_while_editing_types_rather_than_quits() {
        let (vault, mut app) = graph_app();
        let _ = &vault;
        dispatch(&mut app, Action::OpenNotesView);
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);
        dispatch(&mut app, Action::ToggleMode);
        assert_eq!(app.active().map(|t| t.mode), Some(Mode::Editing));

        press(&mut app, 'q');

        assert!(app.modal.is_none(), "q is a letter while editing");
        assert!(app.editor_mut().expect("editor").text().contains('q'));
    }

    #[test]
    fn s_cycles_the_sort_order_in_the_explorer() {
        let vault = TempVault::new("sort-key");
        vault.write("A.md", "a");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.focus = Focus::Explorer;

        let before = app.explorer.sort();
        press(&mut app, 's');
        let after = app.explorer.sort();

        assert_ne!(before, after, "s should change the order");
        assert_eq!(after, before.next());
        // The config follows, so "Save settings" keeps it.
        assert_eq!(app.config.ui.sort_order, after.key());
    }

    #[test]
    fn s_is_a_letter_when_typing_rather_than_a_sort_command() {
        let vault = TempVault::new("sort-key-editing");
        vault.write("A.md", "a");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.open_note(0);
        dispatch(&mut app, Action::ToggleMode);
        app.focus = Focus::Note;

        let before = app.explorer.sort();
        press(&mut app, 's');

        assert_eq!(app.explorer.sort(), before, "s must not sort while editing");
        assert!(app.editor_mut().expect("editor").text().contains('s'));
    }

    #[test]
    fn cycling_the_sort_order_returns_to_where_it_started() {
        let vault = TempVault::new("sort-cycle");
        vault.write("A.md", "a");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.focus = Focus::Explorer;

        let start = app.explorer.sort();
        for _ in 0..emeraldian_core::sort::SortOrder::ALL.len() {
            press(&mut app, 's');
        }
        assert_eq!(app.explorer.sort(), start, "a full cycle should come home");
    }
}

#[cfg(test)]
mod clipboard_tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn app() -> (TempVault, App) {
        let vault = TempVault::new("clip");
        vault.write("A.md", "# A\n");
        let app = App::new(vault.vault(), Config::default()).expect("app");
        (vault, app)
    }

    /// A note open for editing with vim off.
    fn editing(text: &str) -> (TempVault, App) {
        let vault = TempVault::new("clip");
        vault.write("N.md", text);
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let id = app.index.id_of_rel("N.md").expect("indexed");
        app.open_note(id);
        app.active_mut().expect("tab").mode = Mode::Editing;
        (vault, app)
    }

    fn note(app: &mut App) -> String {
        app.editor_mut().expect("editor").text()
    }

    #[test]
    fn ctrl_c_then_ctrl_v_copies_several_lines_intact() {
        let (_v, mut app) = editing("- [ ] one\n- [ ] two\nend\n");
        let editor = app.editor_mut().expect("editor");
        editor.goto(0, 0);
        editor.begin_selection();
        editor.goto_extend(2, 0);
        handle(&mut app, ctrl(KeyCode::Char('c')));
        assert_eq!(crate::clipboard::paste(), "- [ ] one\n- [ ] two\n");

        app.editor_mut().expect("editor").goto(2, 0);
        handle(&mut app, ctrl(KeyCode::Char('v')));
        assert_eq!(
            note(&mut app),
            "- [ ] one\n- [ ] two\n- [ ] one\n- [ ] two\nend\n",
            "pasted as written — no list marker added, no line lost"
        );

        handle(&mut app, ctrl(KeyCode::Char('z')));
        assert_eq!(
            note(&mut app),
            "- [ ] one\n- [ ] two\nend\n",
            "one undo takes the whole paste back"
        );
    }

    #[test]
    fn ctrl_x_cuts_the_selection() {
        let (_v, mut app) = editing("keep cut\n");
        let editor = app.editor_mut().expect("editor");
        editor.goto(0, 4);
        editor.begin_selection();
        editor.goto_extend(0, 8);
        handle(&mut app, ctrl(KeyCode::Char('x')));
        assert_eq!(note(&mut app), "keep\n");
        assert_eq!(crate::clipboard::paste(), " cut");
    }

    #[test]
    fn copying_nothing_says_how_to_select() {
        let (_v, mut app) = editing("text\n");
        crate::clipboard::copy("before");
        handle(&mut app, ctrl(KeyCode::Char('c')));
        assert!(app.status.text.contains("select"));
        assert_eq!(
            crate::clipboard::paste(),
            "before",
            "the clipboard is left alone"
        );
    }

    #[test]
    fn a_terminal_paste_goes_into_the_note_whole() {
        let (_v, mut app) = editing("");
        // Terminals send bracketed paste with `\r` for each newline.
        paste(&mut app, "# Title\r\rbody");
        assert_eq!(note(&mut app), "# Title\n\nbody\n");
    }

    #[test]
    fn a_terminal_paste_into_the_chat_does_not_send_it() {
        let (_v, mut app) = app();
        app.focus = Focus::Chat;
        paste(&mut app, "first line\nsecond");
        assert_eq!(app.chat.input, "first line second");
    }

    #[test]
    fn a_terminal_paste_into_the_palette_filters_it() {
        let (_v, mut app) = app();
        handle(&mut app, ctrl(KeyCode::Char('p')));
        paste(&mut app, "sav");
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("the palette is still open");
        };
        assert_eq!(picker.query, "sav");
    }

    #[test]
    fn a_terminal_paste_while_reading_changes_nothing() {
        let (_v, mut app) = app();
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);
        paste(&mut app, "q");
        assert!(app.modal.is_none(), "pasted text is not a command");
        assert_eq!(app.view, View::Notes);
    }
}

#[cfg(test)]
mod keymap_tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn app() -> (TempVault, App) {
        let vault = TempVault::new("keymap");
        vault.write("A.md", "# A\n");
        let app = App::new(vault.vault(), Config::default()).expect("app");
        (vault, app)
    }

    fn remapped(pairs: &[(&str, &str)]) -> (TempVault, App) {
        let (vault, mut app) = app();
        let keys = pairs
            .iter()
            .map(|(name, key)| ((*name).to_string(), (*key).to_string()))
            .collect();
        app.keymap = crate::keymap::Keymap::new(&keys);
        app.config.keys = keys;
        (vault, app)
    }

    #[test]
    fn a_remapped_key_works_and_the_default_still_does() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        let shown = app.config.ui.show_left_sidebar;
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT),
        );
        assert_eq!(app.config.ui.show_left_sidebar, !shown);
        handle(&mut app, ctrl(KeyCode::Char('\\')));
        assert_eq!(app.config.ui.show_left_sidebar, shown);
    }

    #[test]
    fn a_remapped_key_beats_vim_normal_mode() {
        let (_v, mut app) = remapped(&[("toggle_right_sidebar", "alt+o")]);
        let a = app.index.id_of_rel("A.md").unwrap();
        app.open_note(a);
        app.active_mut().unwrap().mode = Mode::Editing;
        app.config.editor.vim = true;
        app.focus = Focus::Note;
        let text = app.editor_mut().unwrap().text();
        let shown = app.config.ui.show_right_sidebar;

        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT),
        );
        assert_eq!(app.config.ui.show_right_sidebar, !shown);
        assert_eq!(
            app.editor_mut().unwrap().text(),
            text,
            "not read as vim's o"
        );
    }

    #[test]
    fn an_overlay_keeps_its_keys() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        handle(&mut app, ctrl(KeyCode::Char('p')));
        let shown = app.config.ui.show_left_sidebar;
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT),
        );
        assert_eq!(app.config.ui.show_left_sidebar, shown);
        assert!(matches!(app.modal, Some(Modal::Picker(_))));
    }

    #[test]
    fn the_palette_shows_the_remapped_key() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        handle(&mut app, ctrl(KeyCode::Char('p')));
        let Some(Modal::Picker(picker)) = &app.modal else {
            panic!("palette open");
        };
        let entry = picker
            .visible()
            .map(|(entry, _)| entry)
            .find(|e| e.action == Action::ToggleLeftSidebar)
            .expect("listed");
        assert_eq!(entry.detail, "Alt+E");
    }

    #[test]
    fn reset_puts_every_key_back_after_asking() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        let dir =
            std::env::temp_dir().join(format!("emeraldian-keys-reset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        app.config_dir = Some(dir.clone());

        dispatch(&mut app, Action::ResetKeys);
        assert!(matches!(app.modal, Some(Modal::Confirm(_))), "asks first");
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE),
        );

        assert!(app.config.keys.is_empty());
        let shown = app.config.ui.show_left_sidebar;
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT),
        );
        assert_eq!(app.config.ui.show_left_sidebar, shown, "Alt+E is gone");
        handle(&mut app, ctrl(KeyCode::Char('\\')));
        assert_eq!(app.config.ui.show_left_sidebar, !shown, "the default works");

        let written = std::fs::read_to_string(dir.join("config.toml")).expect("saved");
        assert!(!written.contains("[keys]"), "and the reset is on disk");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn declining_the_reset_keeps_the_keys() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        dispatch(&mut app, Action::ResetKeys);
        handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
        );
        assert!(app.modal.is_none());
        assert_eq!(app.config.keys.len(), 1);
    }

    #[test]
    fn reset_with_nothing_remapped_just_says_so() {
        let (_v, mut app) = app();
        dispatch(&mut app, Action::ResetKeys);
        assert!(app.modal.is_none());
        assert!(app.status.text.contains("already the default"));
    }

    #[test]
    fn reset_removes_only_the_keys_section() {
        let (_v, mut app) = remapped(&[("toggle_left_sidebar", "alt+e")]);
        let dir =
            std::env::temp_dir().join(format!("emeraldian-keys-inplace-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        app.config_dir = Some(dir.clone());
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# mine\ntheme = \"nord\"\n\n[keys]\ntoggle_left_sidebar = \"alt+e\"\n",
        )
        .expect("seed");

        dispatch(&mut app, Action::ClearRemappedKeys);
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "# mine\ntheme = \"nord\"\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod familiar_tests {
    //! The shortcuts people already know from other apps, on each platform.

    use super::*;
    use crate::config::Config;
    use crate::modal::PickerKind;
    use emeraldian_core::test_support::TempVault;

    const CMD: KeyModifiers = KeyModifiers::SUPER;
    const NONE: KeyModifiers = KeyModifiers::NONE;

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        handle(app, KeyEvent::new(code, modifiers));
    }

    fn type_str(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch), NONE);
        }
    }

    /// A note open for editing with vim off, on the keyboard named, and drawn
    /// once so the editor knows its rows.
    fn editing(keyboard: &str, text: &str) -> (TempVault, App) {
        let vault = TempVault::new("familiar");
        vault.write("N.md", text);
        vault.write("Other.md", "# Other\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.ui.keyboard = keyboard.into();
        let id = app.index.id_of_rel("N.md").expect("indexed");
        app.open_note(id);
        app.active_mut().expect("tab").mode = Mode::Editing;
        app.focus = Focus::Note;
        let _ = app.editor_mut();
        draw(&mut app);
        (vault, app)
    }

    fn draw(app: &mut App) {
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, app))
            .expect("draw");
    }

    fn text(app: &mut App) -> String {
        app.editor_mut().expect("editor").text()
    }

    fn cursor(app: &mut App) -> (usize, usize) {
        let at = app.editor_mut().expect("editor").cursor();
        (at.line, at.col)
    }

    fn goto(app: &mut App, line: usize, col: usize) {
        app.editor_mut().expect("editor").goto(line, col);
    }

    fn picker(app: &App) -> &crate::modal::Picker {
        match &app.modal {
            Some(Modal::Picker(picker)) => picker,
            other => panic!("expected a picker, have {other:?}"),
        }
    }

    fn other(app: &App) -> emeraldian_core::index::NoteId {
        app.index.id_of_rel("Other.md").expect("indexed")
    }

    #[test]
    fn the_keyboard_follows_the_setting_and_otherwise_the_machine() {
        let (_v, mut app) = editing("auto", "x\n");
        let machine = if cfg!(target_os = "macos") {
            Keyboard::Mac
        } else {
            Keyboard::Pc
        };
        assert_eq!(Keyboard::of(&app), machine);
        for (setting, expected) in [
            ("mac", Keyboard::Mac),
            ("macOS", Keyboard::Mac),
            (" Windows ", Keyboard::Pc),
            ("linux", Keyboard::Pc),
            ("a typo", machine),
        ] {
            app.config.ui.keyboard = setting.into();
            assert_eq!(Keyboard::of(&app), expected, "{setting:?}");
        }
    }

    #[test]
    fn command_does_what_ctrl_does_on_a_mac() {
        let (_v, mut app) = editing("mac", "hello\n");
        goto(&mut app, 0, 5);
        type_str(&mut app, "!");

        press(&mut app, KeyCode::Char('z'), CMD);
        assert_eq!(text(&mut app), "hello\n", "⌘Z undoes");
        press(&mut app, KeyCode::Char('z'), CMD | KeyModifiers::SHIFT);
        assert_eq!(text(&mut app), "hello!\n", "⌘⇧Z redoes");

        press(&mut app, KeyCode::Char('p'), CMD);
        assert_eq!(picker(&app).kind, PickerKind::Commands, "⌘P is the palette");
    }

    #[test]
    fn command_is_never_a_vim_command() {
        let (_v, mut app) = editing("mac", "hello\n");
        app.config.editor.vim = true;
        app.vim.mode = crate::vim::VimMode::Normal;

        // `Ctrl+F` is a page down in vim's Normal mode; `⌘F` is still find.
        press(&mut app, KeyCode::Char('f'), CMD);
        assert_eq!(picker(&app).kind, PickerKind::Find);
    }

    #[test]
    fn command_right_in_normal_mode_rests_on_the_last_character() {
        let (_v, mut app) = editing("mac", "hello\n");
        app.config.editor.vim = true;
        app.vim.mode = crate::vim::VimMode::Normal;
        goto(&mut app, 0, 0);
        press(&mut app, KeyCode::Right, CMD);
        assert_eq!(cursor(&mut app), (0, 4), "on the o, as $ would leave it");
    }

    #[test]
    fn the_super_key_types_nothing_off_a_mac() {
        let (_v, mut app) = editing("linux", "hello\n");
        goto(&mut app, 0, 5);
        press(&mut app, KeyCode::Char('p'), CMD);
        assert!(app.modal.is_none(), "not the palette");
        assert_eq!(text(&mut app), "hello\n", "and not a p either");
    }

    #[test]
    fn command_arrows_go_to_the_ends_of_the_line_and_the_note() {
        let (_v, mut app) = editing("mac", "one two\nthree four");
        goto(&mut app, 0, 3);

        press(&mut app, KeyCode::Right, CMD);
        assert_eq!(cursor(&mut app), (0, 7));
        press(&mut app, KeyCode::Left, CMD);
        assert_eq!(cursor(&mut app), (0, 0));
        press(&mut app, KeyCode::Down, CMD);
        assert_eq!(cursor(&mut app), (1, 10));
        press(&mut app, KeyCode::Up, CMD);
        assert_eq!(cursor(&mut app), (0, 0));

        // With Shift, each one selects as it goes.
        press(&mut app, KeyCode::Right, CMD | KeyModifiers::SHIFT);
        let selected = app.editor_mut().expect("editor").selected_text();
        assert_eq!(selected.as_deref(), Some("one two"));
    }

    #[test]
    fn option_moves_by_word_while_typing_on_a_mac() {
        let (_v, mut app) = editing("mac", "one two three");
        let by_word = {
            let editor = app.editor_mut().expect("editor");
            editor.goto(0, 0);
            editor.move_word_right(false);
            editor.move_word_right(false);
            editor.cursor().col
        };
        goto(&mut app, 0, 0);

        // `⌥→` as an arrow, and as the `Esc f` Terminal sends for it.
        press(&mut app, KeyCode::Right, KeyModifiers::ALT);
        press(&mut app, KeyCode::Char('f'), KeyModifiers::ALT);
        assert_eq!(cursor(&mut app), (0, by_word));
        assert_eq!(text(&mut app), "one two three\n", "nothing typed");

        press(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(cursor(&mut app), (0, 0));
    }

    #[test]
    fn alt_arrows_go_back_and_forward_while_typing_on_windows_and_linux() {
        let (_v, mut app) = editing("windows", "one two three");
        let note = app.active_note();
        let other = other(&app);
        app.open_note(other);

        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(app.active_note(), note, "Alt+← goes back");
        assert!(app.editing(), "to the note being edited");
        press(&mut app, KeyCode::Right, KeyModifiers::ALT);
        assert_eq!(app.active_note(), Some(other), "and Alt+→ forward again");
    }

    #[test]
    fn option_arrows_still_go_back_when_not_typing_on_a_mac() {
        let (_v, mut app) = editing("mac", "text\n");
        let note = app.active_note();
        app.open_note(other(&app));
        assert!(!app.editing(), "a note opens to be read");

        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(app.active_note(), note);
    }

    #[test]
    fn deleting_a_word_is_ctrl_on_windows_and_linux_and_option_on_a_mac() {
        for (keyboard, modifier) in [("linux", KeyModifiers::CONTROL), ("mac", KeyModifiers::ALT)] {
            let (_v, mut app) = editing(keyboard, "one two three");
            goto(&mut app, 0, 7);
            press(&mut app, KeyCode::Backspace, modifier);
            assert_eq!(text(&mut app), "one  three\n", "{keyboard}: a word back");
            press(&mut app, KeyCode::Delete, modifier);
            assert_eq!(
                text(&mut app),
                "one three\n",
                "{keyboard}: forwards, to the next word"
            );
        }

        // Elsewhere `Alt` isn't a word key, so it deletes a character.
        let (_v, mut app) = editing("windows", "one two");
        goto(&mut app, 0, 7);
        press(&mut app, KeyCode::Backspace, KeyModifiers::ALT);
        assert_eq!(text(&mut app), "one tw\n");
    }

    #[test]
    fn ctrl_backspace_works_from_a_terminal_that_sends_ctrl_h() {
        let (_v, mut app) = editing("linux", "one two");
        goto(&mut app, 0, 7);
        press(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "one \n");
    }

    #[test]
    fn command_backspace_and_ctrl_u_delete_to_the_start_of_the_line() {
        for (code, modifiers) in [
            (KeyCode::Backspace, CMD),
            (KeyCode::Char('u'), KeyModifiers::CONTROL),
        ] {
            let (_v, mut app) = editing("mac", "keep\none two\n");
            goto(&mut app, 1, 4);
            press(&mut app, code, modifiers);
            assert_eq!(text(&mut app), "keep\ntwo\n", "{code:?} {modifiers:?}");
        }
    }

    #[test]
    fn redo_is_ctrl_y_or_ctrl_shift_z() {
        let (_v, mut app) = editing("linux", "a\n");
        goto(&mut app, 0, 1);
        type_str(&mut app, "b");

        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "a\n");
        press(
            &mut app,
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(text(&mut app), "ab\n");
        press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "ab\n");
    }

    #[test]
    fn ctrl_k_makes_a_link_while_typing_and_cycles_the_sidebar_otherwise() {
        let (_v, mut app) = editing("linux", "see docs");
        {
            let editor = app.editor_mut().expect("editor");
            editor.goto(0, 4);
            editor.begin_selection();
            editor.goto_extend(0, 8);
        }
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "see [docs]()\n");
        assert_eq!(cursor(&mut app), (0, 11), "waiting for the address");

        press(&mut app, KeyCode::Esc, NONE);
        assert!(!app.editing());
        let panel = app.side_panel;
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_ne!(app.side_panel, panel, "reading, it moves the sidebar on");
    }

    #[test]
    fn ctrl_f_finds_in_the_note_and_enter_selects_the_match() {
        let (_v, mut app) = editing("linux", "alpha\nNeedle here\nbeta needle\n");
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert_eq!(picker(&app).kind, PickerKind::Find);

        // Lowercase ignores case.
        type_str(&mut app, "needle");
        assert_eq!(picker(&app).len(), 2);
        assert_eq!(
            picker(&app).selected_entry().expect("entry").detail,
            "line 2"
        );

        press(&mut app, KeyCode::Enter, NONE);
        assert!(app.modal.is_none());
        let selected = app.editor_mut().expect("editor").selected_text();
        assert_eq!(selected.as_deref(), Some("Needle"));

        // A capital makes it exact.
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        type_str(&mut app, "Needle");
        assert_eq!(picker(&app).len(), 1);
    }

    #[test]
    fn ctrl_f_again_searches_every_note_for_the_same_text() {
        let (_v, mut app) = editing("linux", "needle\n");
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        type_str(&mut app, "needle");
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);

        let search = picker(&app);
        assert_eq!(search.kind, PickerKind::Search);
        assert_eq!(search.query, "needle", "carried over, not typed as an f");
        assert_eq!(search.len(), 1, "and already searched for");
    }

    #[test]
    fn a_search_box_pastes_with_ctrl_v_and_types_no_other_ctrl_key() {
        let (_v, mut app) = editing("linux", "x\n");
        crate::clipboard::copy("two\nlines");
        press(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('a'), CMD);
        press(&mut app, KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(picker(&app).query, "two lines");
    }

    #[test]
    fn ctrl_f_while_reading_scrolls_to_the_match() {
        // One paragraph of many rows, as a note written without line breaks
        // is: the block starts at the top, and the match is far below it.
        let words: String = (0..400).map(|i| format!("word{i} ")).collect();
        let (_v, mut app) = editing("linux", &format!("{words}needle at the end\n"));
        press(&mut app, KeyCode::Esc, NONE);
        draw(&mut app);

        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        type_str(&mut app, "needle");
        press(&mut app, KeyCode::Enter, NONE);
        assert_eq!(app.focus, Focus::Note);
        draw(&mut app);

        let screen = screen(&mut app);
        assert!(screen.contains("needle at the end"), "{screen}");
        assert!(!screen.contains("word0 "), "scrolled down to it:\n{screen}");
    }

    fn screen(app: &mut App) -> String {
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn ctrl_t_opens_the_switcher_and_ctrl_shift_t_reopens_a_closed_tab() {
        let (_v, mut app) = editing("linux", "x\n");
        press(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert_eq!(picker(&app).kind, PickerKind::Notes);
        press(&mut app, KeyCode::Esc, NONE);

        let other = other(&app);
        app.open_note(other);
        press(&mut app, KeyCode::F(3), NONE);
        assert_eq!(app.tabs.len(), 1);

        let reopen = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        press(&mut app, KeyCode::Char('t'), reopen);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.active_note(), Some(other));

        press(&mut app, KeyCode::Char('t'), reopen);
        assert_eq!(app.tabs.len(), 2, "nothing else was closed");
    }

    #[test]
    fn the_tab_and_function_keys_other_apps_use() {
        let (_v, mut app) = editing("windows", "x\n");
        let first = app.active_note();
        app.open_note(other(&app));

        press(&mut app, KeyCode::PageUp, KeyModifiers::CONTROL);
        assert_eq!(app.active_note(), first, "Ctrl+PgUp");
        press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
        assert_eq!(app.active_note(), Some(other(&app)), "Ctrl+PgDn");

        press(&mut app, KeyCode::F(5), NONE);
        assert_eq!(app.status.text, "reloaded from disk");

        let vim = app.config.editor.vim;
        press(&mut app, KeyCode::F(4), KeyModifiers::CONTROL);
        assert_eq!(app.tabs.len(), 1, "Ctrl+F4 closes the tab");
        assert_eq!(app.config.editor.vim, vim, "rather than switching vim");

        press(&mut app, KeyCode::F(1), NONE);
        assert!(matches!(app.modal, Some(Modal::Help(_))));
    }

    // ---- found in review ----------------------------------------------------

    /// `Ctrl+[`, as a terminal speaking the Kitty protocol sends it.
    fn ctrl_bracket() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL)
    }

    fn vim(app: &mut App) {
        app.config.editor.vim = true;
        app.vim.mode = crate::vim::VimMode::Normal;
    }

    #[test]
    fn ctrl_bracket_is_escape_everywhere() {
        let (_v, mut app) = editing("linux", "hello\n");
        press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
        handle(&mut app, ctrl_bracket());
        assert!(app.modal.is_none(), "it closes the palette");
        handle(&mut app, ctrl_bracket());
        assert!(!app.editing(), "and leaves the editor");

        let (_v, mut app) = editing("linux", "hello\n");
        vim(&mut app);
        goto(&mut app, 0, 0);
        type_str(&mut app, "r");
        handle(&mut app, ctrl_bracket());
        assert_eq!(
            text(&mut app),
            "hello\n",
            "a pending r is cancelled, not given a ["
        );
        type_str(&mut app, "vl");
        handle(&mut app, ctrl_bracket());
        assert_eq!(app.vim.mode, crate::vim::VimMode::Normal, "Visual ends");
    }

    #[test]
    fn typing_keys_stay_out_of_vims_visual_mode() {
        let (_v, mut app) = editing("linux", "hello world\n");
        vim(&mut app);
        goto(&mut app, 0, 0);
        type_str(&mut app, "vlll");
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "hello world\n");

        // And `Ctrl+Backspace` in Normal mode moves, as Backspace does.
        press(&mut app, KeyCode::Esc, NONE);
        vim(&mut app);
        goto(&mut app, 0, 5);
        press(&mut app, KeyCode::Backspace, KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "hello world\n");
        assert_eq!(cursor(&mut app), (0, 4));
    }

    #[test]
    fn a_note_left_in_edit_mode_is_not_typed_in_from_another_pane() {
        let (_v, mut app) = editing("linux", "x\n");
        app.focus = Focus::Explorer;
        let panel = app.side_panel;
        press(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_ne!(app.side_panel, panel, "Ctrl+K cycles the sidebar");

        let (_v, mut app) = editing("mac", "x\n");
        let first = app.active_note();
        app.open_note(other(&app));
        app.active_mut().expect("tab").mode = Mode::Editing;
        app.focus = Focus::Explorer;
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(app.active_note(), first, "and ⌥← goes back");
    }

    #[test]
    fn forward_never_opens_a_note_that_was_not_there() {
        let (vault, mut app) = editing("linux", "x\n");
        app.open_note(other(&app));
        press(&mut app, KeyCode::Left, KeyModifiers::ALT);
        // A note sorted in between shifts every id after it.
        vault.write("M.md", "# M\n");
        press(&mut app, KeyCode::F(5), NONE);
        let before = app.active_note();
        press(&mut app, KeyCode::Right, KeyModifiers::ALT);
        assert_eq!(app.active_note(), before);
    }

    #[test]
    fn a_closed_tab_is_not_reopened_in_another_vault() {
        let (_v, mut app) = editing("linux", "x\n");
        press(&mut app, KeyCode::F(3), NONE);
        let other = TempVault::new("familiar-other");
        other.write("N.md", "# Not the one that was closed\n");
        app.open_vault(other.path().to_path_buf());
        press(
            &mut app,
            KeyCode::Char('t'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert!(app.tabs.is_empty());
    }

    #[test]
    fn deleting_to_the_start_of_the_line_never_deletes_forwards() {
        let (_v, mut app) = editing("linux", "    - item\n");
        goto(&mut app, 0, 2);
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "  - item\n");
        goto(&mut app, 0, 0);
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(text(&mut app), "  - item\n", "nothing behind the cursor");
    }

    #[test]
    fn command_backspace_stops_where_command_left_does() {
        let words: String = (0..40).map(|i| format!("word{i} ")).collect();
        let (_v, mut app) = editing("mac", &format!("{words}\n"));
        goto(&mut app, 0, 150);
        press(&mut app, KeyCode::Left, CMD);
        let (_, row_start) = cursor(&mut app);
        assert!(
            row_start > 0,
            "the line wraps, so the row starts partway in"
        );

        goto(&mut app, 0, 150);
        press(&mut app, KeyCode::Backspace, CMD);
        let expected: String = words
            .chars()
            .take(row_start)
            .chain(words.chars().skip(150))
            .collect();
        assert_eq!(text(&mut app), format!("{expected}\n"));
    }

    #[test]
    fn find_while_reading_scrolls_to_the_match_picked_not_the_first() {
        // One hard-wrapped paragraph, with the same word near each end.
        let mut lines = vec!["the needle at the top".to_string()];
        lines.extend((0..300).map(|i| format!("filler line {i}")));
        lines.push("the needle at the bottom".to_string());
        let (_v, mut app) = editing("linux", &format!("{}\n", lines.join("\n")));
        press(&mut app, KeyCode::Esc, NONE);
        draw(&mut app);

        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        type_str(&mut app, "needle");
        press(&mut app, KeyCode::Down, NONE);
        assert_eq!(
            picker(&app).selected_entry().expect("entry").detail,
            "line 302"
        );
        press(&mut app, KeyCode::Enter, NONE);

        let screen = screen(&mut app);
        assert!(screen.contains("needle at the bottom"), "{screen}");
        assert!(!screen.contains("needle at the top"), "{screen}");
    }

    #[test]
    fn find_in_vims_normal_mode_lands_like_a_search() {
        let (_v, mut app) = editing("linux", "foo bar\nbaz bar\n");
        vim(&mut app);
        dispatch(&mut app, Action::OpenFind);
        type_str(&mut app, "bar");
        press(&mut app, KeyCode::Enter, NONE);
        assert_eq!(cursor(&mut app), (0, 4), "on the match");
        assert!(app.editor_mut().expect("editor").selection().is_none());
        assert_eq!(app.vim.mode, crate::vim::VimMode::Normal);
        type_str(&mut app, "n");
        assert_eq!(cursor(&mut app), (1, 4), "n goes on to the next one");
    }

    #[test]
    fn command_keys_in_visual_mode_do_what_vims_ctrl_keys_do() {
        let (_v, mut app) = editing("mac", "abcdef\n");
        vim(&mut app);
        goto(&mut app, 0, 0);
        type_str(&mut app, "vll");
        press(&mut app, KeyCode::Char('x'), CMD);
        assert_eq!(text(&mut app), "def\n", "the selection, cursor and all");
        assert_eq!(app.vim.mode, crate::vim::VimMode::Normal);

        goto(&mut app, 0, 0);
        type_str(&mut app, "v");
        press(&mut app, KeyCode::Right, CMD);
        assert_eq!(app.vim.mode, crate::vim::VimMode::Visual);
        assert_eq!(cursor(&mut app), (0, 2), "out to the end, as $ goes");
        assert!(app.editor_mut().expect("editor").selection().is_some());
    }

    #[test]
    fn a_copied_line_pasted_into_find_finds_it() {
        let (_v, mut app) = editing("linux", "needle here\n");
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        paste(&mut app, "needle here\n");
        assert_eq!(picker(&app).query, "needle here");
        assert_eq!(picker(&app).len(), 1);
    }

    #[test]
    fn a_cmd_key_in_keys_is_matched_before_the_mac_ones() {
        let (_v, mut app) = editing("mac", "x\n");
        let keys = [("back".to_string(), "cmd+left".to_string())]
            .into_iter()
            .collect();
        app.keymap = crate::keymap::Keymap::new(&keys);
        let first = app.active_note();
        app.open_note(other(&app));
        press(&mut app, KeyCode::Left, CMD);
        assert_eq!(app.active_note(), first);
    }

    #[test]
    fn option_b_and_f_go_back_and_forward_away_from_the_text_on_a_mac() {
        let (_v, mut app) = editing("mac", "x\n");
        let first = app.active_note();
        app.open_note(other(&app));
        app.focus = Focus::Explorer;
        press(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(app.active_note(), first, "Esc b is ⌥←");
        // Going back put the focus on the note, which is being edited.
        app.focus = Focus::Explorer;
        press(&mut app, KeyCode::Char('f'), KeyModifiers::ALT);
        assert_eq!(app.active_note(), Some(other(&app)), "and Esc f is ⌥→");
    }

    #[test]
    fn find_results_keep_their_line_number_on_a_small_screen() {
        let words: String = (0..100).map(|i| format!("word{i} ")).collect();
        let (_v, mut app) = editing("linux", &format!("{words}needle {words}\n"));
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("draw");
        press(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        type_str(&mut app, "needle");
        terminal
            .draw(|frame| crate::ui::draw(frame, &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect()
            })
            .collect();
        let row = rows
            .iter()
            .find(|row| row.contains("needle") && row.contains('…'));
        let row = row.unwrap_or_else(|| panic!("no result row:\n{}", rows.join("\n")));
        assert!(row.contains("line 1"), "{row}");
    }

    #[test]
    fn command_brackets_indent_while_typing_and_go_back_elsewhere() {
        let (_v, mut app) = editing("mac", "- item\n");
        goto(&mut app, 0, 2);
        press(&mut app, KeyCode::Char(']'), CMD);
        assert!(text(&mut app).starts_with(|ch: char| ch.is_whitespace()));
        press(&mut app, KeyCode::Char('['), CMD);
        assert_eq!(text(&mut app), "- item\n");

        let note = app.active_note();
        app.open_note(other(&app));
        press(&mut app, KeyCode::Char('['), CMD);
        assert_eq!(app.active_note(), note, "⌘[ is back while reading");

        press(&mut app, KeyCode::Char(']'), CMD | KeyModifiers::SHIFT);
        assert_eq!(app.active_note(), Some(other(&app)), "⌘⇧] is the next tab");
    }
}

#[cfg(test)]
mod altgr_tests {
    use super::*;
    use crate::config::Config;
    use emeraldian_core::test_support::TempVault;

    const ALTGR: KeyModifiers = KeyModifiers::CONTROL.union(KeyModifiers::ALT);

    #[test]
    fn altgr_types_its_character_as_windows_reports_it() {
        let vault = TempVault::new("altgr");
        vault.write("N.md", "mail\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        let id = app.index.id_of_rel("N.md").expect("indexed");
        app.open_note(id);
        app.active_mut().expect("tab").mode = Mode::Editing;
        app.focus = Focus::Note;
        app.editor_mut().expect("editor").goto(0, 4);
        let sidebar = app.config.ui.show_right_sidebar;

        for ch in ['@', ']', '€'] {
            handle(&mut app, KeyEvent::new(KeyCode::Char(ch), ALTGR));
        }
        assert_eq!(app.editor_mut().expect("editor").text(), "mail@]€\n");
        assert_eq!(
            app.config.ui.show_right_sidebar, sidebar,
            "] is typed, not Ctrl+]"
        );

        // In a search box too.
        dispatch(&mut app, Action::OpenSwitcher);
        handle(&mut app, KeyEvent::new(KeyCode::Char('@'), ALTGR));
        match &app.modal {
            Some(Modal::Picker(picker)) => assert_eq!(picker.query, "@"),
            other => panic!("expected the switcher, have {other:?}"),
        }
    }

    #[test]
    fn ctrl_alt_with_a_letter_is_still_a_chord() {
        let key = KeyEvent::new(KeyCode::Char('o'), ALTGR);
        assert_eq!(altgr_as_text(key), key);
    }
}
