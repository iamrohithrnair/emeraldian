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

use crate::app::{Action, App, Focus};
use crate::modal::{Modal, Prompt, PromptIntent};

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

/// What an operator does to the span a motion picks out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
}

impl Operator {
    /// The key that names it, and the doubled key that means "this line".
    #[must_use]
    fn from_key(ch: char) -> Option<Self> {
        Some(match ch {
            'd' => Self::Delete,
            'c' => Self::Change,
            'y' => Self::Yank,
            '>' => Self::Indent,
            '<' => Self::Outdent,
            _ => return None,
        })
    }

    #[must_use]
    fn key(self) -> char {
        match self {
            Self::Delete => 'd',
            Self::Change => 'c',
            Self::Yank => 'y',
            Self::Indent => '>',
            Self::Outdent => '<',
        }
    }
}

/// How a motion's span is measured.
///
/// The distinction is vim's and it is not cosmetic: `dw` stops before the next
/// word while `de` eats the last letter of this one, and the only difference
/// between them is which of these they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    /// Up to but not including the target — `w`, `b`, `0`.
    Exclusive,
    /// Including the character landed on — `e`, `$`, `f`.
    Inclusive,
    /// Whole lines, however far along them the ends sit — `j`, `G`, `}`.
    Linewise,
}

/// Where a motion ended up, and how much of the way there it covers.
struct Motion {
    to: crate::editor::Cursor,
    span: Span,
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
    /// An operator typed, waiting for the motion or object to apply it to.
    Operator(Operator),
    /// `dg` typed, waiting for the `g` of `dgg`.
    OperatorG(Operator),
    /// `di` or `da` typed, waiting for the object — the `w` of `diw`.
    Object(Operator, bool),
    /// `f`, `F`, `t` or `T` typed, waiting for the character to search for.
    /// The flags are forward, and whether it stops short.
    Find {
        operator: Option<Operator>,
        forward: bool,
        till: bool,
    },
    /// `Ctrl+W` typed, waiting for the pane to move to.
    Window,
    /// `<Space>` typed, waiting for a leader key. The which-key list is on
    /// screen while this is set.
    Leader,
    /// `<Space>f` typed — the "find" group.
    LeaderFind,
    /// `[` or `]` typed, waiting for what to step through.
    Bracket(bool),
}

/// The leader map, which is also exactly what the which-key popup lists.
///
/// One table so the two can never disagree: a key that works but isn't shown is
/// undiscoverable, and one that is shown but doesn't work is worse.
pub const LEADER: &[(&str, &str, Action)] = &[
    ("ff", "Find a note", Action::OpenSwitcher),
    ("fg", "Grep the vault", Action::OpenSearch),
    ("e", "File explorer", Action::FocusPane(Focus::Explorer)),
    ("p", "Command palette", Action::OpenPalette),
    ("n", "New note", Action::NewNote),
    ("d", "Daily note", Action::DailyNote),
    ("w", "Save", Action::Save),
    ("x", "Close tab", Action::CloseTab),
    ("g", "Graph", Action::OpenGraph),
    ("G", "Local graph", Action::OpenLocalGraph),
    ("a", "Assistant", Action::ToggleChat),
    ("o", "Outline", Action::FocusPane(Focus::Sidebar)),
    ("t", "Theme", Action::OpenThemePicker),
    ("r", "Reload vault", Action::Refresh),
    ("?", "Help", Action::OpenHelp),
    ("q", "Quit", Action::Quit),
];

/// The last `f`/`t` search, so `;` and `,` can repeat it.
#[derive(Debug, Clone, Copy)]
pub struct FindTarget {
    ch: char,
    forward: bool,
    till: bool,
}

/// An active in-buffer search, kept so `n`, `N` and the highlight all agree.
#[derive(Debug, Clone)]
pub struct Search {
    pub pattern: String,
    forward: bool,
}

/// Everything vim mode remembers.
#[derive(Debug, Clone, Default)]
pub struct Vim {
    pub mode: VimMode,
    /// The count being typed, as in the `3` of `3dd`.
    count: Option<usize>,
    pending: Pending,
    /// Survives `reset`: `;` should still work after switching notes, the way
    /// the register does.
    last_find: Option<FindTarget>,
    /// The live search, or `None` once `:noh` or `Esc` has cleared it. Read by
    /// the renderer to highlight every match.
    pub search: Option<Search>,
    /// Keys of the command being typed, kept only while it might turn out to
    /// have changed something.
    recording: Option<Vec<KeyEvent>>,
    /// The buffer's revision when that recording started.
    recording_at: u64,
    /// The last command that did change something — what `.` replays.
    last_change: Vec<KeyEvent>,
    /// Set while `.` is feeding those keys back in, so the replay is not itself
    /// recorded as a new change.
    replaying: bool,
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

    /// Whether the leader menu should be on screen.
    #[must_use]
    pub fn showing_leader(&self) -> bool {
        matches!(self.pending, Pending::Leader | Pending::LeaderFind)
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
    // `.` replays keys rather than re-running a parsed command. Recording what
    // was typed is the one representation that covers every case uniformly —
    // an operator with a motion, a lone `x`, and the literal text typed during
    // an insert are all just keys.
    if !app.vim.replaying {
        start_or_continue_recording(app, key);
    }

    let used = match app.vim.mode {
        VimMode::Insert => insert(app, key),
        VimMode::Normal => normal(app, key),
        VimMode::Visual | VimMode::VisualLine => visual(app, key),
    };

    if !app.vim.replaying {
        finish_recording(app);
    }
    used
}

/// Begins recording a candidate change, or adds to one already in progress.
fn start_or_continue_recording(app: &mut App, key: KeyEvent) {
    if app.vim.recording.is_none() {
        // Only a command begun in Normal mode is repeatable; there is no
        // meaningful `.` for "the last thing typed mid-sentence".
        if app.vim.mode != VimMode::Normal {
            return;
        }
        app.vim.recording_at = with_editor_out(app, |editor| editor.revision()).unwrap_or(0);
        app.vim.recording = Some(Vec::new());
    }
    if let Some(keys) = app.vim.recording.as_mut() {
        keys.push(key);
    }
}

/// Decides what the recorded keys turned out to be.
fn finish_recording(app: &mut App) {
    // Still mid-command: a pending operator, or an insert that has not been
    // left yet. Either way there is more to come.
    if app.vim.pending != Pending::None || app.vim.mode == VimMode::Insert {
        return;
    }
    let Some(keys) = app.vim.recording.take() else {
        return;
    };
    let now = with_editor_out(app, |editor| editor.revision()).unwrap_or(0);
    // A motion is not a change, and repeating one would be a surprise.
    if now != app.vim.recording_at && !keys.is_empty() {
        app.vim.last_change = keys;
    }
}

/// `.` — runs the last change again.
fn repeat_change(app: &mut App) {
    // `.` must never be recorded as the change it just made, or the next `.`
    // replays a replay — which recurses until the stack runs out. Dropping the
    // in-progress recording here is what keeps `.` meaning the last *edit*
    // however many times it is pressed.
    app.vim.recording = None;

    let keys = app.vim.last_change.clone();
    if keys.is_empty() {
        app.info("nothing to repeat");
        return;
    }
    app.vim.replaying = true;
    for key in keys {
        // Through the full key handler, not `handle` directly: Insert mode
        // deliberately declines ordinary characters so the shared editing path
        // types them, and a replay has to take that same route or the text
        // typed during a `ciw` never reappears.
        crate::keys::handle(app, key);
    }
    app.vim.replaying = false;
    // A replay that ended mid-insert would leave the editor typing.
    if app.vim.mode == VimMode::Insert {
        leave_insert(app);
    }
}

// ---------------------------------------------------------------------------
// Navigation — the app, not the text
// ---------------------------------------------------------------------------

/// Whether keys are currently going into text rather than commands.
///
/// While typing, the app's own bindings apply unchanged; a navigation layer
/// that stole keys mid-sentence would be worse than not having one.
#[must_use]
pub fn is_typing(app: &App) -> bool {
    (app.focus == Focus::Note && app.editing() && app.vim.mode == VimMode::Insert)
        || app.focus == Focus::Chat
}

/// Vim-style movement around the application, from any pane.
///
/// Deliberately outside the mode machine: `Ctrl+W` and the jumplist are about
/// where you are, not about what you are editing, so they work in the explorer
/// and the sidebar too. Returns whether the key was used.
pub fn navigation(app: &mut App, key: KeyEvent) -> bool {
    // The pane waiting to be named after `Ctrl+W`.
    if app.vim.pending == Pending::Window {
        app.vim.clear_pending();
        let action = match key.code {
            KeyCode::Char('h') | KeyCode::Left => Some(Action::FocusPane(Focus::Explorer)),
            KeyCode::Char('l') | KeyCode::Right => Some(Action::FocusPane(Focus::Sidebar)),
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('k') | KeyCode::Up => {
                Some(Action::FocusPane(Focus::Note))
            }
            KeyCode::Char('p') => Some(Action::FocusPane(Focus::Chat)),
            KeyCode::Char('c' | 'q') => Some(Action::CloseTab),
            KeyCode::Char('w') => {
                crate::keys::cycle_focus(app, 1);
                None
            }
            KeyCode::Char('W') => {
                crate::keys::cycle_focus(app, -1);
                None
            }
            _ => None,
        };
        if let Some(action) = action {
            crate::actions::dispatch(app, action);
        }
        return true;
    }

    if !key.modifiers.contains(KeyModifiers::CONTROL) || key.modifiers.contains(KeyModifiers::SHIFT)
    {
        return false;
    }
    match key.code {
        KeyCode::Char('w') => {
            app.vim.pending = Pending::Window;
            app.vim.showcmd.push_str("^W");
            true
        }
        // Vim's jumplist, which here is the note history `Alt+←` already walks.
        // `Ctrl+I` and `Tab` are the same byte without the Kitty protocol, so
        // forward is best-effort; `Tab` is unaffected because it is not a Ctrl
        // key and never reaches this.
        KeyCode::Char('o') => {
            crate::actions::dispatch(app, Action::Back);
            true
        }
        KeyCode::Char('i') => {
            crate::actions::dispatch(app, Action::Forward);
            true
        }
        _ => false,
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

    // The two line-editing keys vim defines while typing. Everything else falls
    // through to the ordinary editor, including every shifted combination.
    if ctrl && !key.modifiers.contains(KeyModifiers::SHIFT) {
        match key.code {
            KeyCode::Char('w') => {
                with_editor(app, |editor| {
                    let at = editor.cursor();
                    let start = editor.word_back(at, 1, false);
                    editor.delete_range(start, at);
                });
                return true;
            }
            KeyCode::Char('u') => {
                with_editor(app, |editor| {
                    let at = editor.cursor();
                    let start = crate::editor::Cursor {
                        line: at.line,
                        col: 0,
                    };
                    editor.delete_range(start, at);
                });
                return true;
            }
            _ => {}
        }
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
        Pending::Find {
            operator,
            forward,
            till,
        } => {
            app.vim.pending = Pending::None;
            if let KeyCode::Char(ch) = key.code {
                app.vim.last_find = Some(FindTarget { ch, forward, till });
                run_find(app, ch, forward, till, operator);
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        // Owned by the navigation layer, which runs before this and consumes
        // the key while it is set.
        Pending::Window => return false,
        Pending::Leader => {
            app.vim.pending = Pending::None;
            match key.code {
                KeyCode::Char('f') => {
                    app.vim.pending = Pending::LeaderFind;
                    app.vim.showcmd.push('f');
                    return true;
                }
                KeyCode::Esc => app.vim.clear_pending(),
                KeyCode::Char(ch) => {
                    run_leader(app, &ch.to_string());
                }
                _ => app.vim.clear_pending(),
            }
            app.vim.clear_pending();
            return true;
        }
        Pending::LeaderFind => {
            app.vim.pending = Pending::None;
            if let KeyCode::Char(ch) = key.code {
                run_leader(app, &format!("f{ch}"));
            }
            app.vim.clear_pending();
            return true;
        }
        Pending::Bracket(forward) => {
            app.vim.clear_pending();
            // `[b` and `]b` step through tabs, the way vim-unimpaired steps
            // through buffers.
            if key.code == KeyCode::Char('b') {
                app.cycle_tab(if forward { 1 } else { -1 });
            }
            return true;
        }
        Pending::Object(operator, around) => {
            app.vim.pending = Pending::None;
            if let KeyCode::Char(ch) = key.code {
                run_object(app, operator, around, ch);
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        Pending::OperatorG(operator) => {
            app.vim.pending = Pending::None;
            // `dgg` — delete from here to the top of the note.
            if key.code == KeyCode::Char('g') {
                let to = crate::editor::Cursor {
                    line: app.vim.count.map_or(0, |n| n.saturating_sub(1)),
                    col: 0,
                };
                apply(
                    app,
                    operator,
                    &Motion {
                        to,
                        span: Span::Linewise,
                    },
                );
            }
            app.vim.clear_pending();
            clamp(app);
            return true;
        }
        Pending::Operator(operator) => {
            app.vim.pending = Pending::None;
            let count = app.vim.count();

            match key.code {
                // The doubled form means "this line", whichever operator it is.
                KeyCode::Char(ch) if ch == operator.key() => {
                    let line = with_editor_out(app, |editor| editor.cursor().line).unwrap_or(0);
                    let to = crate::editor::Cursor {
                        line: line + count - 1,
                        col: 0,
                    };
                    apply(
                        app,
                        operator,
                        &Motion {
                            to,
                            span: Span::Linewise,
                        },
                    );
                }
                // `i`/`a` start a text object rather than being motions here.
                KeyCode::Char(ch @ ('i' | 'a')) => {
                    app.vim.pending = Pending::Object(operator, ch == 'a');
                    app.vim.showcmd.push(ch);
                    return true;
                }
                KeyCode::Char('g') => {
                    app.vim.pending = Pending::OperatorG(operator);
                    app.vim.showcmd.push('g');
                    return true;
                }
                KeyCode::Char(ch @ ('f' | 'F' | 't' | 'T')) => {
                    app.vim.pending = Pending::Find {
                        operator: Some(operator),
                        forward: ch == 'f' || ch == 't',
                        till: ch == 't' || ch == 'T',
                    };
                    app.vim.showcmd.push(ch);
                    return true;
                }
                // A digit here is a second count: `d3w`, which multiplies.
                KeyCode::Char(ch @ '1'..='9') => {
                    app.vim.pending = Pending::Operator(operator);
                    app.vim.push_count(ch.to_digit(10).unwrap_or(0));
                    app.vim.showcmd.push(ch);
                    return true;
                }
                // Not a motion: vim abandons the operator rather than guessing,
                // which is what stops a stray key from deleting something
                // nobody asked about.
                code => {
                    if let Some(motion) = motion_for(app, code, count) {
                        apply(app, operator, &motion);
                    }
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
                KeyCode::Char('e') => {
                    let count = app.vim.count();
                    with_editor(app, |editor| {
                        let to = editor.word_back(editor.cursor(), count, false);
                        let to = editor.word_end_from(to, false);
                        editor.set_cursor(to);
                    });
                }
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

    let count = app.vim.count();

    match key.code {
        KeyCode::Char(ch @ '1'..='9') => {
            app.vim.push_count(ch.to_digit(10).unwrap_or(0));
            app.vim.showcmd.push(ch);
            return true;
        }
        KeyCode::Char(ch @ '0') if app.vim.count.is_some() => {
            app.vim.push_count(ch.to_digit(10).unwrap_or(0));
            app.vim.showcmd.push(ch);
            return true;
        }

        // ---- operators ----------------------------------------------------
        // An operator does nothing on its own — it waits to be told what to act
        // on, which is the `w` of `dw` or the second `d` of `dd`.
        KeyCode::Char(ch @ ('d' | 'c' | 'y' | '>' | '<')) => {
            if let Some(operator) = Operator::from_key(ch) {
                app.vim.pending = Pending::Operator(operator);
                app.vim.showcmd.push(ch);
            }
            return true;
        }

        KeyCode::Char('g') => {
            app.vim.pending = Pending::G;
            app.vim.showcmd.push('g');
            return true;
        }
        KeyCode::Char(ch @ ('f' | 'F' | 't' | 'T')) => {
            app.vim.pending = Pending::Find {
                operator: None,
                forward: ch == 'f' || ch == 't',
                till: ch == 't' || ch == 'T',
            };
            app.vim.showcmd.push(ch);
            return true;
        }
        // `;` and `,` repeat the last f/t, forwards and backwards.
        KeyCode::Char(ch @ (';' | ',')) => {
            if let Some(find) = app.vim.last_find {
                let forward = if ch == ';' {
                    find.forward
                } else {
                    !find.forward
                };
                run_find(app, find.ch, forward, find.till, None);
            }
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

        // ---- shorthands ----------------------------------------------------
        // Each of these is an operator and a motion in one key, which is how
        // vim spells the combinations worth a single keystroke.
        KeyCode::Char('D') => {
            let to = with_editor_out(app, |editor| crate::editor::Cursor {
                line: editor.cursor().line,
                col: editor.line_len_at(editor.cursor().line),
            });
            if let Some(to) = to {
                apply(
                    app,
                    Operator::Delete,
                    &Motion {
                        to,
                        span: Span::Exclusive,
                    },
                );
            }
        }
        KeyCode::Char('C') => {
            let to = with_editor_out(app, |editor| crate::editor::Cursor {
                line: editor.cursor().line,
                col: editor.line_len_at(editor.cursor().line),
            });
            if let Some(to) = to {
                apply(
                    app,
                    Operator::Change,
                    &Motion {
                        to,
                        span: Span::Exclusive,
                    },
                );
            }
            return true;
        }
        KeyCode::Char('Y') => {
            let line = with_editor_out(app, |editor| editor.cursor().line).unwrap_or(0);
            apply(
                app,
                Operator::Yank,
                &Motion {
                    to: crate::editor::Cursor {
                        line: line + count - 1,
                        col: 0,
                    },
                    span: Span::Linewise,
                },
            );
        }
        KeyCode::Char('S') => {
            let line = with_editor_out(app, |editor| editor.cursor().line).unwrap_or(0);
            apply(
                app,
                Operator::Change,
                &Motion {
                    to: crate::editor::Cursor {
                        line: line + count - 1,
                        col: 0,
                    },
                    span: Span::Linewise,
                },
            );
            return true;
        }
        KeyCode::Char('s') => {
            let text = with_editor_out(app, |editor| editor.take_chars(count));
            if let Some(text) = text.filter(|t| !t.is_empty()) {
                app.vim.register = Register {
                    text,
                    linewise: false,
                };
            }
            enter_insert(app);
            return true;
        }
        KeyCode::Char('X') => {
            let text = with_editor_out(app, |editor| {
                let cursor = editor.cursor();
                let start = cursor.col.saturating_sub(count);
                let text = editor.delete_range(
                    crate::editor::Cursor {
                        line: cursor.line,
                        col: start,
                    },
                    cursor,
                );
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
        KeyCode::Char('J') => {
            with_editor(app, |editor| {
                editor.join_lines(count);
                editor.commit();
            });
        }
        KeyCode::Char('~') => {
            with_editor(app, |editor| {
                editor.toggle_case(count);
                editor.commit();
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

        // Space is the leader, as every nvim distribution has settled on. Vim's
        // own meaning for it — move right — is the one motion nobody uses,
        // which is why it was free to take.
        KeyCode::Char(' ') => {
            app.vim.pending = Pending::Leader;
            app.vim.showcmd.push('␣');
            return true;
        }
        KeyCode::Char(ch @ ('[' | ']')) => {
            app.vim.pending = Pending::Bracket(ch == ']');
            app.vim.showcmd.push(ch);
            return true;
        }

        // ---- the command and search lines ----------------------------------
        KeyCode::Char(':') => {
            app.modal = Some(Modal::Prompt(Prompt::new(":", "", PromptIntent::VimEx)));
            app.vim.clear_pending();
            return true;
        }
        KeyCode::Char(ch @ ('/' | '?')) => {
            let forward = ch == '/';
            app.modal = Some(Modal::Prompt(Prompt::new(
                ch.to_string(),
                "",
                PromptIntent::VimSearch(forward),
            )));
            app.vim.clear_pending();
            return true;
        }
        KeyCode::Char('n' | 'N') => {
            let same = key.code == KeyCode::Char('n');
            let forward = app.vim.search.as_ref().is_some_and(|s| s.forward) == same;
            jump_to_match(app, forward);
        }
        KeyCode::Char('.') => {
            repeat_change(app);
            return true;
        }

        // Everything else is either a motion or nothing. Going through the same
        // resolver the operators use is what keeps `w` and `dw` agreeing about
        // where a word ends.
        code => match motion_for(app, code, count) {
            Some(motion) => move_to(app, code, count, &motion),
            None => return false,
        },
    }

    app.vim.clear_pending();
    clamp(app);
    true
}

/// Where a motion key lands, and how the span up to there is measured.
///
/// The single definition of every motion. An operator applies to the span this
/// returns, and a bare keypress moves to its target — so `dw` cannot disagree
/// with `w` about where the next word starts.
fn motion_for(app: &mut App, code: KeyCode, count: usize) -> Option<Motion> {
    // Read before the editor is borrowed: `G` needs the raw count to tell
    // "go to line 5" from a bare "go to the end".
    let explicit = app.vim.count;
    let editor = app.editor_mut()?;
    let at = editor.cursor();
    let line_len = editor.line_len_at(at.line);

    let (to, span) = match code {
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => (
            crate::editor::Cursor {
                line: at.line,
                col: at.col.saturating_sub(count),
            },
            Span::Exclusive,
        ),
        // Space is not listed here: it is the leader, claimed before the
        // motions are consulted.
        KeyCode::Char('l') | KeyCode::Right => (
            crate::editor::Cursor {
                line: at.line,
                col: (at.col + count).min(line_len),
            },
            Span::Exclusive,
        ),
        KeyCode::Char('j') | KeyCode::Down => (
            crate::editor::Cursor {
                line: at.line + count,
                col: at.col,
            },
            Span::Linewise,
        ),
        KeyCode::Char('k') | KeyCode::Up => (
            crate::editor::Cursor {
                line: at.line.saturating_sub(count),
                col: at.col,
            },
            Span::Linewise,
        ),
        KeyCode::Char('0') => (
            crate::editor::Cursor {
                line: at.line,
                col: 0,
            },
            Span::Exclusive,
        ),
        KeyCode::Char('^') | KeyCode::Home => {
            let col = editor.lines()[at.line]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .count();
            (
                crate::editor::Cursor {
                    line: at.line,
                    col: col.min(line_len),
                },
                Span::Exclusive,
            )
        }
        // `$` is inclusive, which is why `d$` clears to the end of the line
        // rather than leaving the last character behind.
        KeyCode::Char('$') | KeyCode::End => (
            crate::editor::Cursor {
                line: at.line,
                col: line_len.saturating_sub(1),
            },
            Span::Inclusive,
        ),
        KeyCode::Char('w') => (editor.word_forward(at, count, false), Span::Exclusive),
        KeyCode::Char('W') => (editor.word_forward(at, count, true), Span::Exclusive),
        KeyCode::Char('b') => (editor.word_back(at, count, false), Span::Exclusive),
        KeyCode::Char('B') => (editor.word_back(at, count, true), Span::Exclusive),
        KeyCode::Char('e') => (editor.word_end(at, count, false), Span::Inclusive),
        KeyCode::Char('E') => (editor.word_end(at, count, true), Span::Inclusive),
        KeyCode::Char('{') => (editor.paragraph(at, false, count), Span::Exclusive),
        KeyCode::Char('}') => (editor.paragraph(at, true, count), Span::Exclusive),
        KeyCode::Char('G') => {
            let line = match explicit {
                Some(n) => n.saturating_sub(1),
                None => editor.line_count().saturating_sub(1),
            };
            (crate::editor::Cursor { line, col: 0 }, Span::Linewise)
        }
        _ => return None,
    };
    Some(Motion { to, span })
}

/// Moves the cursor to a motion's target.
///
/// Three keys go through the editor's own methods rather than a bare jump,
/// because they carry the column the cursor is aiming for: `j` and `k` keep it
/// across short lines, and `$` sticks to the end of every line it passes.
fn move_to(app: &mut App, code: KeyCode, count: usize, motion: &Motion) {
    let to = motion.to;
    with_editor(app, |editor| match code {
        KeyCode::Char('j') | KeyCode::Down => editor.move_line(count as isize, false),
        KeyCode::Char('k') | KeyCode::Up => editor.move_line(-(count as isize), false),
        KeyCode::Char('$') | KeyCode::End => editor.move_line_end_sticky(false),
        _ => editor.set_cursor(to),
    });
}

/// Runs an operator over the span between the cursor and a motion's target.
fn apply(app: &mut App, operator: Operator, motion: &Motion) {
    let Some(at) = with_editor_out(app, |editor| editor.cursor()) else {
        return;
    };
    let to = motion.to;

    if motion.span == Span::Linewise {
        let first = at.line.min(to.line);
        let last = at.line.max(to.line);
        operate_lines(app, operator, first, last - first + 1);
        return;
    }

    let (start, mut end) = if at <= to { (at, to) } else { (to, at) };
    if motion.span == Span::Inclusive {
        // The buffer's ranges stop short of their end; vim's inclusive motions
        // cover the character landed on, so it has to be added back.
        let len = with_editor_out(app, |editor| editor.line_len_at(end.line)).unwrap_or(0);
        end.col = (end.col + 1).min(len);
    }
    operate_chars(app, operator, start, end);
}

/// The linewise form of every operator.
fn operate_lines(app: &mut App, operator: Operator, first: usize, count: usize) {
    match operator {
        Operator::Yank => {
            if let Some(text) = with_editor_out(app, |editor| editor.copy_lines(first, count)) {
                let lines = text.lines().count();
                app.vim.register = Register {
                    text,
                    linewise: true,
                };
                app.info_yank(lines);
            }
        }
        Operator::Delete => {
            if let Some(text) = with_editor_out(app, |editor| {
                let text = editor.take_lines(first, count);
                editor.move_first_nonblank(false);
                editor.commit();
                text
            }) {
                app.vim.register = Register {
                    text,
                    linewise: true,
                };
            }
        }
        Operator::Change => {
            // A linewise change leaves an empty line to type on rather than
            // closing the gap, which is the whole difference from `d`.
            if let Some(text) = with_editor_out(app, |editor| {
                let text = editor.take_lines(first, count);
                editor.open_line(true);
                text
            }) {
                app.vim.register = Register {
                    text,
                    linewise: true,
                };
            }
            enter_insert(app);
        }
        Operator::Indent | Operator::Outdent => {
            let last = first + count.saturating_sub(1);
            with_editor(app, |editor| {
                editor.goto(first, 0);
                editor.begin_selection();
                editor.goto_extend(last, 0);
                editor.indent(operator == Operator::Indent);
                // Vim leaves the cursor on the first line of the range. Left on
                // the last, a following `<j` would shift a different pair of
                // lines from the `>j` that preceded it.
                editor.goto(first, 0);
                editor.move_first_nonblank(false);
                editor.commit();
            });
        }
    }
}

/// The charwise form. `end` is exclusive.
fn operate_chars(
    app: &mut App,
    operator: Operator,
    start: crate::editor::Cursor,
    end: crate::editor::Cursor,
) {
    match operator {
        Operator::Yank => {
            if let Some(text) = with_editor_out(app, |editor| {
                let text = editor.copy_range(start, end);
                // Vim leaves the cursor at the start of what was yanked.
                editor.set_cursor(start);
                text
            }) {
                if !text.is_empty() {
                    app.info_yank(text.lines().count().max(1));
                }
                app.vim.register = Register {
                    text,
                    linewise: false,
                };
            }
        }
        Operator::Delete | Operator::Change => {
            if let Some(text) = with_editor_out(app, |editor| {
                let text = editor.delete_range(start, end);
                editor.commit();
                text
            }) {
                app.vim.register = Register {
                    text,
                    linewise: false,
                };
            }
            if operator == Operator::Change {
                enter_insert(app);
            }
        }
        // Indenting is a line operation however the span was measured.
        Operator::Indent | Operator::Outdent => {
            operate_lines(app, operator, start.line, end.line - start.line + 1);
        }
    }
}

/// `f`, `F`, `t`, `T` — and the operator forms `df,` and friends.
fn run_find(app: &mut App, ch: char, forward: bool, till: bool, operator: Option<Operator>) {
    let count = app.vim.count();
    let Some(to) = with_editor_out(app, |editor| {
        editor.find_in_line(editor.cursor(), ch, forward, till, count)
    })
    .flatten() else {
        return;
    };

    // Searching backwards stops before the character; forwards covers it.
    let span = if forward {
        Span::Inclusive
    } else {
        Span::Exclusive
    };
    match operator {
        Some(operator) => apply(app, operator, &Motion { to, span }),
        None => with_editor(app, |editor| editor.set_cursor(to)),
    }
}

// ---------------------------------------------------------------------------
// The `:` line
// ---------------------------------------------------------------------------

/// Runs an ex command.
///
/// Most of these are an existing [`Action`] under a different name, which is
/// the point: `:w` and `Ctrl+S` should not be two implementations of saving.
pub fn run_ex(app: &mut App, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    // `:42` jumps to a line, which is why the number is checked before the
    // names — nobody has a command called `42`.
    if let Ok(number) = line.parse::<usize>() {
        with_editor(app, |editor| {
            editor.goto(number.saturating_sub(1), 0);
            editor.move_first_nonblank(false);
        });
        clamp(app);
        return;
    }

    let (name, argument) = match line.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, rest.trim()),
        None => (line, ""),
    };

    // Writing and quitting compose — `:wq` is both — so they are read as a
    // pair of intentions rather than a list of literal spellings.
    let force = name.ends_with('!');
    let base = name.trim_end_matches('!');
    let all = base.ends_with('a') && base != "a";
    let verb = base.trim_end_matches('a');

    match verb {
        "w" | "wq" | "x" | "q" => {
            if verb != "q" {
                crate::actions::dispatch(app, if all { Action::SaveAll } else { Action::Save });
            }
            if matches!(verb, "q" | "wq" | "x") {
                let action = match (all, force) {
                    (true, true) => Action::ForceQuit,
                    (true, false) => Action::Quit,
                    // `:q` closes the note, as it closes a window in vim; the
                    // whole app is `:qa`.
                    (false, _) => Action::CloseTab,
                };
                crate::actions::dispatch(app, action);
            }
        }
        "e" | "edit" => {
            if argument.is_empty() {
                crate::actions::dispatch(app, Action::Refresh);
            } else {
                app.open_or_create(argument);
            }
        }
        "h" | "help" => crate::actions::dispatch(app, Action::OpenHelp),
        "set" => run_set(app, argument),
        "mkconfig" => crate::actions::dispatch(app, Action::SaveSettings),
        "noh" | "nohl" | "nohlsearch" => {
            app.vim.search = None;
        }
        other => app.error(format!("not a command: :{other}")),
    }
}

/// `:set nu`, `:set nowrap`, `:set ts=2`.
///
/// A deliberately short list — the settings someone would plausibly change
/// mid-session. Everything else lives in `config.toml`, and `:mkconfig` writes
/// what is set here into it.
fn run_set(app: &mut App, argument: &str) {
    if argument.is_empty() {
        app.info("try :set nu / nonu / wrap / nowrap / et / noet / ts=4 / vim / novim");
        return;
    }
    if let Some(width) = argument
        .strip_prefix("ts=")
        .or_else(|| argument.strip_prefix("tabstop="))
        .and_then(|value| value.parse::<usize>().ok())
    {
        app.config.editor.tab_width = width.clamp(1, 16);
        app.info(format!("tabstop {}", app.config.editor.tab_width));
        return;
    }

    let (name, on) = match argument.strip_prefix("no") {
        Some(rest) => (rest, false),
        None => (argument, true),
    };
    match name {
        "nu" | "number" => {
            app.config.ui.line_numbers = on;
            app.info(format!("number {}", if on { "on" } else { "off" }));
        }
        "wrap" => {
            app.config.editor.wrap = on;
            app.info(format!("wrap {}", if on { "on" } else { "off" }));
        }
        "et" | "expandtab" => {
            app.config.editor.expand_tabs = on;
            app.info(format!("expandtab {}", if on { "on" } else { "off" }));
        }
        // The one `:set` that persists, because it is the one that changes what
        // every other key does. See `actions::toggle_vim`.
        "vim" => {
            if app.config.editor.vim != on {
                crate::actions::toggle_vim(app);
            }
        }
        other => app.error(format!("not an option: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Runs a `/` or `?` search and jumps to the first match.
pub fn run_search(app: &mut App, pattern: &str, forward: bool) {
    if pattern.is_empty() {
        return;
    }
    app.vim.search = Some(Search {
        pattern: pattern.to_string(),
        forward,
    });
    jump_to_match(app, forward);
}

/// Steps to the next match in a direction, wrapping at the ends.
fn jump_to_match(app: &mut App, forward: bool) {
    let Some(search) = app.vim.search.clone() else {
        app.info("no previous search");
        return;
    };
    let found = with_editor_out(app, |editor| {
        editor.find_next(editor.cursor(), &search.pattern, forward)
    })
    .flatten();

    match found {
        Some(at) => with_editor(app, |editor| editor.set_cursor(at)),
        // Saying so beats a key that silently does nothing.
        None => app.info(format!("no match for {}", search.pattern)),
    }
    clamp(app);
}

/// Runs a leader sequence, if it names one.
///
/// Reads the same table the which-key popup draws, so nothing can be bound
/// without also being listed.
fn run_leader(app: &mut App, keys: &str) {
    let Some((_, _, action)) = LEADER.iter().find(|(binding, _, _)| *binding == keys) else {
        app.info(format!("no leader binding for {keys}"));
        return;
    };
    crate::actions::dispatch(app, action.clone());
}

/// `diw`, `ci"`, `da(` and the rest.
fn run_object(app: &mut App, operator: Operator, around: bool, ch: char) {
    let range = with_editor_out(app, |editor| {
        let at = editor.cursor();
        match ch {
            'w' => editor.word_object(at, around, false),
            'W' => editor.word_object(at, around, true),
            '"' | '\'' | '`' => editor.quoted_object(at, ch, around),
            '(' | ')' | 'b' => editor.bracket_object(at, '(', ')', around),
            '[' | ']' => editor.bracket_object(at, '[', ']', around),
            '{' | '}' | 'B' => editor.bracket_object(at, '{', '}', around),
            '<' | '>' => editor.bracket_object(at, '<', '>', around),
            _ => None,
        }
    })
    .flatten();

    let Some((start, end)) = range else {
        return;
    };
    // Objects come back inclusive of both ends.
    let len = with_editor_out(app, |editor| editor.line_len_at(end.line)).unwrap_or(0);
    let end = crate::editor::Cursor {
        line: end.line,
        col: (end.col + 1).min(len),
    };
    with_editor(app, |editor| editor.set_cursor(start));
    operate_chars(app, operator, start, end);
}

/// The Ctrl keys vim defines. Anything else is declined and reaches the app.
fn normal_ctrl(app: &mut App, key: KeyEvent) -> bool {
    // Vim binds none of these with Shift and the app does — `Ctrl+Shift+F` is
    // the vault search, `Ctrl+Shift+G` the local graph. Declining them here is
    // what lets those through.
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        return false;
    }

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
        // Increment and decrement the number under the cursor. Worth having in
        // a notes app for the same reason as anywhere else: renumbering a list
        // by hand is exactly the sort of thing to get wrong.
        KeyCode::Char(ch @ ('a' | 'x')) => {
            let step = app.vim.count() as i64 * if ch == 'a' { 1 } else { -1 };
            let found = with_editor_out(app, |editor| {
                let found = editor.adjust_number(step);
                editor.commit();
                found
            });
            if found != Some(true) {
                app.info("no number on this line");
            }
        }
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

        // `f`/`t` work here too, and are the quickest way to stretch a
        // selection to a punctuation mark you can see.
        KeyCode::Char(ch @ ('f' | 'F' | 't' | 'T')) => {
            app.vim.pending = Pending::Find {
                operator: None,
                forward: ch == 'f' || ch == 't',
                till: ch == 't' || ch == 'T',
            };
            app.vim.showcmd.push(ch);
            return true;
        }
        KeyCode::Char('g') => {
            app.vim.pending = Pending::G;
            app.vim.showcmd.push('g');
            return true;
        }

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

        // Motions extend the selection rather than replacing it, through the
        // same resolver Normal mode uses — so `vw` and `dw` cover exactly the
        // same text.
        code => {
            let count = app.vim.count();
            match motion_for(app, code, count) {
                Some(motion) => {
                    let to = motion.to;
                    with_editor(app, |editor| editor.goto_extend(to.line, to.col));
                }
                None => return false,
            }
        }
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

        // A regular file standing where the directory would have to be.
        // Creating anything underneath it fails on every platform — unlike a
        // made-up absolute path, which Windows resolves against the current
        // drive and then cheerfully creates, leaving the write to succeed and
        // a stray directory at the root of C:.
        let blocker =
            std::env::temp_dir().join(format!("emeraldian-not-a-dir-{}", std::process::id()));
        std::fs::write(&blocker, b"not a directory").expect("blocker");
        app.config_dir = Some(blocker.clone());

        f4(&mut app);
        std::fs::remove_file(&blocker).ok();

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
        // Insert mode is the ordinary editor with an exit, so the keys Normal
        // mode claims are given back — Ctrl+R reloads rather than redoing.
        let (_v, mut app) = app("text\n");
        press(&mut app, 'i');

        let tabs = app.tabs.len();
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.tabs.len(), tabs);
        assert_eq!(
            app.active().expect("tab").mode,
            Mode::Reading,
            "Ctrl+E still toggles the reading view"
        );
    }

    #[test]
    fn insert_mode_keeps_vims_two_line_editing_keys() {
        // Ctrl+W and Ctrl+U are the exceptions: reflexive enough while typing
        // that leaving them as the window prefix and a half-page scroll would
        // be a papercut every day.
        let (_v, mut app) = app("");
        press(&mut app, 'i');
        for ch in "alpha beta".chars() {
            press(&mut app, ch);
        }
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "alpha \n", "Ctrl+W takes the word back");

        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "\n", "Ctrl+U takes the rest of the line");
        assert_eq!(app.vim.mode, VimMode::Insert, "and typing continues");
    }

    // ---- motions ---------------------------------------------------------

    #[test]
    fn w_and_b_step_by_word_treating_punctuation_as_its_own() {
        // `foo.bar` is three words in vim, not one — the letters and the dot
        // are different kinds of character.
        let (_v, mut app) = app("foo.bar baz\n");
        press(&mut app, 'w');
        assert_eq!(cursor(&mut app), (0, 3), "the dot is a word of its own");
        press(&mut app, 'w');
        assert_eq!(cursor(&mut app), (0, 4));
        press(&mut app, 'w');
        assert_eq!(cursor(&mut app), (0, 8), "on to baz");

        press(&mut app, 'b');
        assert_eq!(cursor(&mut app), (0, 4));
    }

    #[test]
    fn capital_w_and_b_step_over_punctuation() {
        let (_v, mut app) = app("foo.bar baz\n");
        press(&mut app, 'W');
        assert_eq!(cursor(&mut app), (0, 8), "a WORD stops only at blanks");
        press(&mut app, 'B');
        assert_eq!(cursor(&mut app), (0, 0));
    }

    #[test]
    fn e_lands_on_the_last_character_of_a_word() {
        let (_v, mut app) = app("alpha beta\n");
        press(&mut app, 'e');
        assert_eq!(cursor(&mut app), (0, 4), "the a of alpha, not the space");
        press(&mut app, 'e');
        assert_eq!(cursor(&mut app), (0, 9));
    }

    #[test]
    fn word_motions_cross_lines() {
        let (_v, mut app) = app("one\ntwo\n");
        press(&mut app, 'w');
        assert_eq!(cursor(&mut app), (1, 0));
        press(&mut app, 'b');
        assert_eq!(cursor(&mut app), (0, 0));
    }

    #[test]
    fn paragraph_motions_jump_between_blocks() {
        let (_v, mut app) = app("one\ntwo\n\nthree\nfour\n\nfive\n");
        press(&mut app, '}');
        assert_eq!(cursor(&mut app).0, 2, "the blank line after the block");
        press(&mut app, '}');
        assert_eq!(cursor(&mut app).0, 5);
        press(&mut app, '{');
        assert_eq!(cursor(&mut app).0, 2);
    }

    #[test]
    fn f_and_t_aim_at_a_character_on_the_line() {
        let (_v, mut app) = app("a,b,c\n");
        type_str(&mut app, "f,");
        assert_eq!(cursor(&mut app), (0, 1));
        press(&mut app, ';');
        assert_eq!(cursor(&mut app), (0, 3), "; repeats the search");
        press(&mut app, ',');
        assert_eq!(cursor(&mut app), (0, 1), ", repeats it backwards");
    }

    #[test]
    fn t_stops_one_short_of_its_target() {
        let (_v, mut app) = app("a,b\n");
        type_str(&mut app, "t,");
        assert_eq!(cursor(&mut app), (0, 0));
    }

    #[test]
    fn f_stays_on_its_own_line() {
        // It is a motion you aim by eye, so running onto the next line would
        // take the cursor somewhere the user never looked.
        let (_v, mut app) = app("abc\nx,y\n");
        type_str(&mut app, "f,");
        assert_eq!(cursor(&mut app), (0, 0), "no comma here, so nothing moves");
    }

    // ---- operators -------------------------------------------------------

    #[test]
    fn dw_deletes_to_the_start_of_the_next_word() {
        let (_v, mut app) = app("alpha beta gamma\n");
        type_str(&mut app, "dw");
        assert_eq!(text(&mut app), "beta gamma\n");
        assert_eq!(app.vim.register.text, "alpha ");
        assert!(!app.vim.register.linewise);
    }

    #[test]
    fn de_stops_on_the_last_character_rather_than_before_the_next_word() {
        // The exclusive/inclusive distinction: `dw` takes the trailing space
        // and `de` leaves it.
        let (_v, mut app) = app("alpha beta\n");
        type_str(&mut app, "de");
        assert_eq!(text(&mut app), " beta\n");
    }

    #[test]
    fn a_count_multiplies_wherever_it_is_typed() {
        for keys in ["d3w", "3dw"] {
            let (_v, mut app) = app("one two three four\n");
            type_str(&mut app, keys);
            assert_eq!(text(&mut app), "four\n", "{keys} should take three words");
        }
    }

    #[test]
    fn d_dollar_clears_to_the_end_of_the_line() {
        let (_v, mut app) = app("keep this\n");
        type_str(&mut app, "ll");
        type_str(&mut app, "d$");
        assert_eq!(text(&mut app), "ke\n", "$ is inclusive");
    }

    #[test]
    fn dj_takes_both_lines_because_j_is_linewise() {
        let (_v, mut app) = app("one\ntwo\nthree\n");
        type_str(&mut app, "dj");
        assert_eq!(text(&mut app), "three\n");
        assert!(app.vim.register.linewise);
    }

    #[test]
    fn cw_deletes_the_word_and_starts_typing() {
        let (_v, mut app) = app("alpha beta\n");
        type_str(&mut app, "cw");
        assert_eq!(app.vim.mode, VimMode::Insert);
        for ch in "omega".chars() {
            press(&mut app, ch);
        }
        assert_eq!(text(&mut app), "omegabeta\n");
    }

    #[test]
    fn yw_copies_without_changing_anything() {
        let (_v, mut app) = app("alpha beta\n");
        type_str(&mut app, "yw");
        assert_eq!(text(&mut app), "alpha beta\n");
        assert_eq!(app.vim.register.text, "alpha ");
        assert_eq!(cursor(&mut app), (0, 0), "the cursor stays at the start");
    }

    #[test]
    fn an_operator_over_a_line_range_indents_it() {
        let (_v, mut app) = app("one\ntwo\nthree\n");
        type_str(&mut app, ">j");
        assert_eq!(text(&mut app), "    one\n    two\nthree\n");
        type_str(&mut app, "<j");
        assert_eq!(text(&mut app), "one\ntwo\nthree\n");
    }

    #[test]
    fn df_deletes_up_to_and_including_the_character() {
        let (_v, mut app) = app("keep,drop rest\n");
        type_str(&mut app, "df,");
        assert_eq!(text(&mut app), "drop rest\n");
    }

    #[test]
    fn dgg_deletes_back_to_the_top() {
        let (_v, mut app) = app("one\ntwo\nthree\n");
        press(&mut app, 'j');
        type_str(&mut app, "dgg");
        assert_eq!(text(&mut app), "three\n");
    }

    #[test]
    fn an_operator_followed_by_nonsense_does_nothing() {
        // Guessing at what a stray key meant is how an editor eats a paragraph
        // nobody asked it to touch.
        let (_v, mut app) = app("untouched\n");
        type_str(&mut app, "dZ");
        assert_eq!(text(&mut app), "untouched\n");
        assert!(app.vim.showcmd.is_empty(), "and the operator is abandoned");
    }

    // ---- text objects ----------------------------------------------------

    #[test]
    fn diw_takes_the_word_under_the_cursor_from_anywhere_in_it() {
        for start in 0..5 {
            let (_v, mut app) = app("alpha beta\n");
            for _ in 0..start {
                press(&mut app, 'l');
            }
            type_str(&mut app, "diw");
            assert_eq!(text(&mut app), " beta\n", "from column {start}");
        }
    }

    #[test]
    fn daw_takes_the_trailing_space_too() {
        let (_v, mut app) = app("alpha beta\n");
        type_str(&mut app, "daw");
        assert_eq!(text(&mut app), "beta\n");
    }

    #[test]
    fn ciw_replaces_a_word_in_place() {
        let (_v, mut app) = app("the quick fox\n");
        type_str(&mut app, "w");
        type_str(&mut app, "ciw");
        for ch in "slow".chars() {
            press(&mut app, ch);
        }
        assert_eq!(text(&mut app), "the slow fox\n");
    }

    #[test]
    fn a_quoted_object_takes_what_is_between_the_quotes() {
        let (_v, mut app) = app("say \"hello there\" now\n");
        type_str(&mut app, "ci\"");
        for ch in "bye".chars() {
            press(&mut app, ch);
        }
        assert_eq!(text(&mut app), "say \"bye\" now\n");
    }

    #[test]
    fn a_bracketed_object_takes_what_is_between_the_brackets() {
        let (_v, mut app) = app("call(alpha, beta)\n");
        type_str(&mut app, "wci(");
        press(&mut app, 'x');
        assert_eq!(text(&mut app), "call(x)\n");
    }

    #[test]
    fn an_around_object_takes_the_delimiters_as_well() {
        let (_v, mut app) = app("call(alpha)\n");
        type_str(&mut app, "wda(");
        assert_eq!(text(&mut app), "call\n");
    }

    #[test]
    fn a_nested_bracket_takes_the_pair_the_cursor_is_actually_in() {
        let (_v, mut app) = app("f(g(x))\n");
        // On the `x`, inside the inner pair.
        type_str(&mut app, "llll");
        type_str(&mut app, "di(");
        assert_eq!(text(&mut app), "f(g())\n");
    }

    #[test]
    fn an_object_with_no_match_leaves_the_note_alone() {
        let (_v, mut app) = app("no brackets here\n");
        type_str(&mut app, "di(");
        assert_eq!(text(&mut app), "no brackets here\n");
    }

    // ---- the remaining single keys ---------------------------------------

    #[test]
    fn d_capital_clears_the_rest_of_the_line() {
        let (_v, mut app) = app("keep this\n");
        type_str(&mut app, "llD");
        assert_eq!(text(&mut app), "ke\n");
    }

    #[test]
    fn c_capital_replaces_the_rest_of_the_line() {
        let (_v, mut app) = app("keep this\n");
        type_str(&mut app, "llC");
        press(&mut app, 'y');
        assert_eq!(text(&mut app), "key\n");
    }

    #[test]
    fn capital_x_deletes_backwards() {
        let (_v, mut app) = app("abc\n");
        press(&mut app, 'l');
        press(&mut app, 'X');
        assert_eq!(text(&mut app), "bc\n");
    }

    #[test]
    fn s_deletes_a_character_and_starts_typing() {
        let (_v, mut app) = app("abc\n");
        press(&mut app, 's');
        press(&mut app, 'X');
        assert_eq!(text(&mut app), "Xbc\n");
    }

    #[test]
    fn j_joins_the_next_line_with_a_single_space() {
        let (_v, mut app) = app("one\n   two\nthree\n");
        press(&mut app, 'J');
        assert_eq!(
            text(&mut app),
            "one two\nthree\n",
            "the indent is absorbed rather than left mid-sentence"
        );
    }

    #[test]
    fn a_count_joins_several_lines() {
        let (_v, mut app) = app("a\nb\nc\n");
        type_str(&mut app, "3J");
        assert_eq!(text(&mut app), "a b c\n");
    }

    #[test]
    fn tilde_flips_the_case_and_moves_on() {
        let (_v, mut app) = app("abc\n");
        press(&mut app, '~');
        assert_eq!(text(&mut app), "Abc\n");
        assert_eq!(cursor(&mut app), (0, 1), "and steps past what it changed");
    }

    #[test]
    fn ctrl_a_and_ctrl_x_adjust_the_number_on_the_line() {
        let (_v, mut app) = app("item 41 here\n");
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "item 42 here\n");

        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        );
        assert_eq!(text(&mut app), "item 41 here\n");
    }

    #[test]
    fn decrementing_past_zero_goes_negative_rather_than_mangling_digits() {
        let (_v, mut app) = app("n = 1\n");
        for _ in 0..3 {
            crate::keys::handle(
                &mut app,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            );
        }
        assert_eq!(text(&mut app), "n = -2\n");
    }

    #[test]
    fn ctrl_a_says_so_when_there_is_no_number() {
        let (_v, mut app) = app("no digits\n");
        crate::keys::handle(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        assert!(!app.status.text.is_empty(), "silence looks like a bug");
    }

    #[test]
    fn visual_motions_land_where_normal_mode_motions_do() {
        // The shared resolver's actual guarantee. Note that `vwd` and `dw` do
        // *not* delete the same text, in this editor or in vim: a visual
        // selection covers the character the cursor ends on and an exclusive
        // motion stops before it. Asserting the cursor rather than the text is
        // what tests the thing that is really shared.
        for keys in ["w", "b", "e", "$", "0", "}"] {
            let plain = {
                let (_v, mut app) = app("alpha beta gamma\n\nnext\n");
                type_str(&mut app, "ll");
                type_str(&mut app, keys);
                cursor(&mut app)
            };
            let visual = {
                let (_v, mut app) = app("alpha beta gamma\n\nnext\n");
                type_str(&mut app, "ll");
                press(&mut app, 'v');
                type_str(&mut app, keys);
                cursor(&mut app)
            };
            assert_eq!(plain, visual, "{keys} disagrees between the two modes");
        }
    }

    #[test]
    fn a_visual_selection_covers_the_character_it_ends_on() {
        // Vim's rule, and the reason `vld` deletes two characters where `dl`
        // deletes one.
        let visual = {
            let (_v, mut app) = app("abcdef\n");
            type_str(&mut app, "vld");
            text(&mut app)
        };
        assert_eq!(visual, "cdef\n", "v l d takes two characters");

        let operator = {
            let (_v, mut app) = app("abcdef\n");
            type_str(&mut app, "dl");
            text(&mut app)
        };
        assert_eq!(operator, "bcdef\n", "d l takes one");
    }

    // ---- the global remap -------------------------------------------------

    fn ctrl(app: &mut App, ch: char) {
        crate::keys::handle(app, KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    }

    #[test]
    fn ctrl_w_moves_between_the_apps_panes() {
        let (_v, mut app) = app("text\n");
        app.focus = crate::app::Focus::Note;

        ctrl(&mut app, 'w');
        press(&mut app, 'h');
        assert_eq!(app.focus, crate::app::Focus::Explorer, "h is the left pane");

        app.focus = crate::app::Focus::Note;
        ctrl(&mut app, 'w');
        press(&mut app, 'l');
        assert_eq!(app.focus, crate::app::Focus::Sidebar, "l is the right one");
    }

    #[test]
    fn ctrl_w_says_so_rather_than_focusing_a_closed_pane() {
        // Focus in a pane that isn't drawn is a keyboard talking to nothing.
        let (_v, mut app) = app("text\n");
        app.config.ui.show_chat = false;
        app.focus = crate::app::Focus::Note;

        ctrl(&mut app, 'w');
        press(&mut app, 'p');
        assert_eq!(app.focus, crate::app::Focus::Note);
        assert!(!app.status.text.is_empty());
    }

    #[test]
    fn space_opens_the_leader_menu_and_the_next_key_runs_a_binding() {
        let (_v, mut app) = app("text\n");
        press(&mut app, ' ');
        assert!(app.vim.showing_leader(), "the menu is on screen");

        press(&mut app, 'p');
        assert!(
            matches!(app.modal, Some(crate::modal::Modal::Picker(_))),
            "Space p opens the palette"
        );
        assert!(!app.vim.showing_leader(), "and the menu is dismissed");
    }

    #[test]
    fn a_two_key_leader_binding_works() {
        let (_v, mut app) = app("text\n");
        type_str(&mut app, " ff");
        assert!(matches!(app.modal, Some(crate::modal::Modal::Picker(_))));
    }

    #[test]
    fn escape_dismisses_the_leader_menu_without_doing_anything() {
        let (_v, mut app) = app("text\n");
        press(&mut app, ' ');
        esc(&mut app);
        assert!(!app.vim.showing_leader());
        assert!(app.modal.is_none());
    }

    #[test]
    fn every_leader_binding_is_listed_in_the_menu_it_draws() {
        // The table is both the keymap and the popup, so a binding cannot exist
        // without being shown. This guards the table itself staying sane.
        for (keys, label, _) in LEADER {
            assert!(!keys.is_empty(), "a binding with no key");
            assert!(!label.is_empty(), "{keys} has no label");
            assert!(keys.len() <= 2, "{keys} is longer than the menu shows");
        }
        let mut seen: Vec<&str> = LEADER.iter().map(|(keys, _, _)| *keys).collect();
        seen.sort_unstable();
        let count = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), count, "two bindings share a key");
    }

    #[test]
    fn a_leader_prefix_cannot_also_be_a_binding() {
        // `f` opens the find group, so a bare `<Space>f` must not also do
        // something — one of the two would be unreachable.
        assert!(
            !LEADER.iter().any(|(keys, _, _)| *keys == "f"),
            "f is a group prefix, so it cannot be a binding too"
        );
        assert!(LEADER.iter().any(|(keys, _, _)| keys.starts_with('f')));
    }

    #[test]
    fn bracket_b_steps_through_tabs() {
        let vault = TempVault::new("vim-brackets");
        vault.write("A.md", "a\n");
        vault.write("B.md", "b\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        for rel in ["A.md", "B.md"] {
            let id = app.index.id_of_rel(rel).expect("indexed");
            app.open_note(id);
            app.active_mut().expect("tab").mode = Mode::Editing;
        }
        assert_eq!(app.active_tab, Some(1));

        type_str(&mut app, "[b");
        assert_eq!(app.active_tab, Some(0));
        type_str(&mut app, "]b");
        assert_eq!(app.active_tab, Some(1));
    }

    #[test]
    fn ctrl_o_and_ctrl_i_walk_the_note_history() {
        let vault = TempVault::new("vim-jumps");
        vault.write("A.md", "a\n");
        vault.write("B.md", "b\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        let a = app.index.id_of_rel("A.md").expect("indexed");
        let b = app.index.id_of_rel("B.md").expect("indexed");
        app.open_note(a);
        app.open_note(b);
        app.active_mut().expect("tab").mode = Mode::Editing;

        ctrl(&mut app, 'o');
        assert_eq!(app.active_note(), Some(a), "Ctrl+O jumps back");

        ctrl(&mut app, 'i');
        assert_eq!(app.active_note(), Some(b), "Ctrl+I returns");
    }

    #[test]
    fn going_somewhere_new_ends_the_forward_trail() {
        // The browser rule. Without it, Ctrl+I after a fresh jump would take
        // you somewhere you never came from.
        let vault = TempVault::new("vim-forward");
        for rel in ["A.md", "B.md", "C.md"] {
            vault.write(rel, "x\n");
        }
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        let ids: Vec<_> = ["A.md", "B.md", "C.md"]
            .iter()
            .map(|rel| app.index.id_of_rel(rel).expect("indexed"))
            .collect();

        app.open_note(ids[0]);
        app.open_note(ids[1]);
        crate::actions::dispatch(&mut app, Action::Back);
        assert_eq!(app.forward.len(), 1);

        app.open_note(ids[2]);
        assert!(app.forward.is_empty(), "a new jump ends the trail");
    }

    #[test]
    fn the_leader_is_only_the_leader_in_normal_mode() {
        // Space has to stay a space while typing, or the editor is unusable.
        let (_v, mut app) = app("");
        press(&mut app, 'i');
        press(&mut app, 'a');
        press(&mut app, ' ');
        press(&mut app, 'b');
        assert_eq!(text(&mut app), "a b\n");
    }

    #[test]
    fn with_vim_off_ctrl_w_still_closes_the_tab() {
        let (_v, mut app) = app("text\n");
        app.config.editor.vim = false;
        let tabs = app.tabs.len();

        ctrl(&mut app, 'w');
        assert!(app.tabs.len() < tabs);
    }

    // ---- reading mode: vim stays out of it --------------------------------

    /// The same app, left in reading mode.
    fn reading_app(text: &str) -> (TempVault, App) {
        let (vault, mut app) = app(text);
        app.active_mut().expect("tab").mode = Mode::Reading;
        (vault, app)
    }

    #[test]
    fn vim_does_not_touch_a_note_being_read() {
        // Vim mode is about editing text. A note being read has no buffer to
        // act on, so the reading pane keeps every key it always had — and
        // `Ctrl+E` is still the way into the editor.
        let (_v, mut app) = reading_app("text\n");

        press(&mut app, 'i');
        assert_eq!(
            app.active().expect("tab").mode,
            Mode::Reading,
            "i is not an editing command on a page you are reading"
        );
        assert_eq!(app.vim.mode, VimMode::Normal, "and no phantom INSERT");
    }

    #[test]
    fn reading_mode_keeps_its_own_keys_exactly() {
        let (_v, mut app) = reading_app("a\nb\nc\n");
        press(&mut app, 'j');
        assert_eq!(app.active().expect("tab").scroll, 1, "j still scrolls");

        // `g` jumps to the top in one press, as it always has — vim mode does
        // not make it wait for a second `g` here.
        press(&mut app, 'g');
        assert_eq!(app.active().expect("tab").scroll, 0);
    }

    #[test]
    fn reading_mode_keeps_the_global_bindings_vim_does_not_define() {
        // The regression that started all this: keys claimed by vim and then
        // handled by nobody, silently doing nothing.
        let (_v, mut app) = reading_app("text\n");
        ctrl(&mut app, 'd');
        assert!(
            app.tabs.len() > 1 || !app.status.text.is_empty(),
            "Ctrl+D still reaches the daily note while reading"
        );

        let (_v2, mut app2) = reading_app("text\n");
        ctrl(&mut app2, 'r');
        assert!(
            !app2.status.is_error,
            "Ctrl+R still reaches the vault reload: {}",
            app2.status.text
        );
    }

    #[test]
    fn ctrl_w_is_the_window_prefix_in_every_pane() {
        // The trap this replaces: `Ctrl+W h` reached the explorer, and pressing
        // `Ctrl+W` again to come back closed the tab instead. A navigation key
        // that destroys work when used from the pane it just took you to is
        // worse than no navigation key.
        let (_v, mut app) = app("text\n");
        app.focus = crate::app::Focus::Note;

        ctrl(&mut app, 'w');
        press(&mut app, 'h');
        assert_eq!(app.focus, crate::app::Focus::Explorer);

        let tabs = app.tabs.len();
        ctrl(&mut app, 'w');
        press(&mut app, 'l');
        assert_eq!(app.tabs.len(), tabs, "and coming back costs nothing");
        assert_eq!(app.focus, crate::app::Focus::Sidebar);
    }

    #[test]
    fn ctrl_w_c_is_how_the_tab_closes_once_the_prefix_owns_the_key() {
        let (_v, mut app) = app("text\n");
        let tabs = app.tabs.len();
        ctrl(&mut app, 'w');
        press(&mut app, 'c');
        assert!(app.tabs.len() < tabs);
    }

    #[test]
    fn the_jumplist_works_from_any_pane() {
        // Navigation, not editing: it should not stop at the editor's edge.
        let vault = TempVault::new("vim-jump-panes");
        vault.write("A.md", "a\n");
        vault.write("B.md", "b\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        let a = app.index.id_of_rel("A.md").expect("indexed");
        let b = app.index.id_of_rel("B.md").expect("indexed");
        app.open_note(a);
        app.open_note(b);

        app.focus = crate::app::Focus::Explorer;
        ctrl(&mut app, 'o');
        assert_eq!(app.active_note(), Some(a), "back, from the explorer");
        ctrl(&mut app, 'i');
        assert_eq!(app.active_note(), Some(b), "and forward again");
    }

    #[test]
    fn vim_stays_out_when_no_note_is_open_at_all() {
        // There is nothing to be in Normal mode *of*.
        let vault = TempVault::new("vim-empty");
        vault.write("A.md", "a\n");
        let mut app = App::new(vault.vault(), Config::default()).expect("app");
        app.config.editor.vim = true;
        app.focus = crate::app::Focus::Note;

        press(&mut app, 'i');
        assert_eq!(app.vim.mode, VimMode::Normal, "no phantom INSERT mode");
        assert!(app.tabs.is_empty());
    }

    /// Enough of the app's state to tell "something happened" from "nothing did".
    #[derive(PartialEq)]
    struct Snapshot {
        text: String,
        cursor: (usize, usize),
        scroll: usize,
        tabs: usize,
        note: Option<usize>,
        mode: Option<Mode>,
        vim: VimMode,
        /// A prefix key is doing something: it shows in the status bar and
        /// changes what the next key means.
        showcmd: String,
        view: crate::app::View,
        focus: crate::app::Focus,
        modal: bool,
        status: String,
        config: String,
    }

    impl Snapshot {
        fn of(app: &mut App) -> Self {
            Self {
                text: app.editor_mut().map(|e| e.text()).unwrap_or_default(),
                cursor: app
                    .editor_mut()
                    .map(|e| (e.cursor().line, e.cursor().col))
                    .unwrap_or_default(),
                scroll: app.active().map_or(0, |t| t.scroll),
                tabs: app.tabs.len(),
                note: app.active_note(),
                mode: app.active().map(|t| t.mode),
                vim: app.vim.mode,
                showcmd: app.vim.showcmd.clone(),
                view: app.view,
                focus: app.focus,
                modal: app.modal.is_some(),
                status: app.status.text.clone(),
                config: format!("{:?}", app.config.ui),
            }
        }
    }

    #[test]
    fn vim_never_makes_a_working_key_dead() {
        // The bug class, rather than the keys that happened to hit it: vim
        // taking a key from the global map and then not implementing it, so it
        // silently does nothing at all.
        //
        // The question is not "is every key bound" — Ctrl+C is unbound in the
        // editor either way — but "did turning vim on take something away". So
        // each key is tried twice and the two are compared.
        //
        // What this does *not* catch is a key that still does something, but
        // the wrong thing: `Ctrl+W` closing a tab when it should be a window
        // prefix looks identical from here. That needs a test that names the
        // expected behaviour, which is `ctrl_w_is_the_window_prefix_in_every_pane`.
        // Focus is an axis anyway, since a key can be live in one pane and dead
        // in another.
        let long: String = (0..200).map(|i| format!("line {i} of text\n")).collect();

        let press_key = |focus: crate::app::Focus, editing: bool, vim: bool, ch: char| {
            let (_vault, mut app) = app(&long);
            app.config.editor.vim = vim;
            if !editing {
                app.active_mut().expect("tab").mode = Mode::Reading;
            }
            // Partway down, so a key that scrolls has somewhere to go in either
            // direction; at a boundary a working key looks like a dead one.
            app.active_mut().expect("tab").scroll = 50;
            if editing {
                with_editor(&mut app, |editor| {
                    // An edit, undone: otherwise Ctrl+R has an empty redo stack
                    // and reads as dead when it is merely finished.
                    editor.goto(100, 3);
                    editor.insert_str("zz");
                    editor.commit();
                    editor.undo();
                    editor.goto(100, 3);
                });
            }
            app.focus = focus;

            let before = Snapshot::of(&mut app);
            ctrl(&mut app, ch);
            before != Snapshot::of(&mut app)
        };

        for focus in [
            crate::app::Focus::Note,
            crate::app::Focus::Explorer,
            crate::app::Focus::Sidebar,
        ] {
            for editing in [true, false] {
                for ch in 'a'..='z' {
                    let without = press_key(focus, editing, false, ch);
                    let with = press_key(focus, editing, true, ch);
                    assert!(
                        with || !without,
                        "Ctrl+{ch} works in {focus:?} while {} with vim off and \
                         does nothing at all with it on — vim claimed the key \
                         and then ignored it",
                        if editing { "editing" } else { "reading" }
                    );
                }
            }
        }
    }

    #[test]
    fn an_older_config_without_the_key_starts_in_emeraldian_mode() {
        // Upgrading must not silently put someone into a modal editor.
        let config: Config = toml::from_str("theme = \"nord\"\n").expect("parse");
        assert!(!config.editor.vim);
    }
}
