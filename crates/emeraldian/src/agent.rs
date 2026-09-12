//! The agent chat panel's state and its wiring to the ACP wire.
//!
//! The panel keeps one record: a **transcript** for the user, which includes
//! tool calls and errors. There is no second record any more — crow-cli owns
//! the conversation server-side, so emeraldian keeps no model history of its
//! own; resuming a session is the agent's job (`session/load`), not ours.

use std::sync::mpsc::TryRecvError;

use crow_term_acp as wire;
use serde::{Deserialize, Serialize};

use crate::acp::{Connection, Event};
use crate::app::App;
use crate::config::{AgentConfig, Config};
use crate::editor::Editor;

/// One entry in the visible transcript.
///
/// Serializable because `/save` persists the transcript alongside the model's
/// message list — restoring only the latter would resume a conversation the
/// user can no longer read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    User(String),
    Assistant(String),
    /// Summarized model reasoning, shown dimmed.
    Reasoning(String),
    Tool {
        name: String,
        detail: String,
        status: ToolStatus,
    },
    Error(String),
    /// A note attached as context for the next message.
    Context(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolStatus {
    Running,
    Ok,
    Failed,
}

pub struct Chat {
    /// What the user sees.
    pub transcript: Vec<Entry>,
    /// The input box is emeraldian's own editor — the same wrapped, styled,
    /// multi-line one the notes use — not a bespoke single-line field. Text
    /// past the pane width wraps onto the next row instead of vanishing.
    pub input: Editor,
    /// Columns the input's text was last laid out across, recorded while
    /// drawing so key handling can move within it by visual rows.
    pub input_cols: usize,
    /// Rows of the input box that were on screen at the last draw, for paging.
    pub input_rows: usize,
    /// The editor settings the input was built with, so it can be replaced
    /// whole (completion, clearing) without reaching for the config again.
    tab_width: usize,
    expand_tabs: bool,
    /// Which slash command the completion list has highlighted.
    ///
    /// Reset whenever the input changes, since the list it indexes into is
    /// recomputed from what has been typed.
    pub completion: usize,
    pub scroll: usize,
    /// True while a turn is in flight.
    pub busy: bool,
    /// Set once the wire reports the turn over (`Done`).
    ///
    /// Scripted runs wait on this rather than on `busy`, which is false both
    /// before the first event arrives and after the last one — those two
    /// states are otherwise indistinguishable, and waiting on `busy` alone
    /// races straight past a turn that has not started yet.
    pub turn_done: bool,
    /// Context-window usage as the agent reported it (`ctx used/size`).
    pub context: Option<String>,
    /// Slash commands the agent advertised (`available_commands/update`) —
    /// `/compact`, `/stop`, … — which route to the wire instead of erroring.
    pub available_commands: Vec<(String, String)>,
    /// Set when the panel should stick to the bottom as output streams in.
    pub follow: bool,

    /// The ACP connection, once the first message needs one. The agent
    /// subprocess lives inside it; dropping it ends both.
    connection: Option<Connection>,
    /// Snapshot of the agent settings this session started with.
    pub settings: AgentConfig,
}

impl Chat {
    #[must_use]
    pub fn new(config: &Config) -> Self {
        Self {
            transcript: Vec::new(),
            input: Editor::new("", config.editor.tab_width, config.editor.expand_tabs),
            input_cols: 0,
            input_rows: 1,
            tab_width: config.editor.tab_width,
            expand_tabs: config.editor.expand_tabs,
            completion: 0,
            scroll: 0,
            busy: false,
            turn_done: false,
            context: None,
            available_commands: Vec::new(),
            follow: true,
            connection: None,
            settings: config.agent.clone(),
        }
    }

    /// Clears the session, keeping settings. The connection goes with it: a
    /// fresh conversation means a fresh agent subprocess and session.
    pub fn reset(&mut self) {
        self.transcript.clear();
        self.context = None;
        self.available_commands.clear();
        self.scroll = 0;
        self.connection = None;
        self.busy = false;
        self.turn_done = false;
    }

    /// Replaces the input's text whole, cursor at the end.
    ///
    /// Completion and `/resume`-style flows put a whole line in at once; the
    /// editor is rebuilt rather than edited so the change reads as one step.
    pub fn set_input(&mut self, text: &str) {
        let mut editor = Editor::new(text, self.tab_width, self.expand_tabs);
        editor.move_document_end(false);
        self.input = editor;
        self.completion = 0;
    }

    /// What is typed, without the editor's trailing newline.
    #[must_use]
    pub fn input_text(&self) -> String {
        self.input.lines().join("\n")
    }

    /// Whether there is nothing typed yet.
    #[must_use]
    pub fn input_is_empty(&self) -> bool {
        self.input.lines().iter().all(|line| line.is_empty())
    }

    /// Puts a slash command in the input, ready for its argument.
    pub fn complete_with(&mut self, name: &str) {
        self.set_input(&format!("/{name} "));
    }

    /// Moves the completion highlight, wrapping at both ends.
    ///
    /// Wrapping because the list is short and reaching the end by holding a key
    /// is a normal way to look through it.
    pub fn move_completion(&mut self, delta: isize, len: usize) {
        if len == 0 {
            return;
        }
        let len = len as isize;
        self.completion = ((self.completion as isize + delta).rem_euclid(len)) as usize;
    }

    /// Empties the input, closing the completion list with it.
    pub fn clear_input(&mut self) {
        self.set_input("");
    }

    /// Appends streaming text to the last assistant entry, starting one if the
    /// previous entry was something else.
    fn push_text(&mut self, text: &str) {
        match self.transcript.last_mut() {
            Some(Entry::Assistant(existing)) => existing.push_str(text),
            _ => self.transcript.push(Entry::Assistant(text.to_string())),
        }
    }

    fn push_reasoning(&mut self, text: &str) {
        match self.transcript.last_mut() {
            Some(Entry::Reasoning(existing)) => existing.push_str(text),
            _ => self.transcript.push(Entry::Reasoning(text.to_string())),
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Started => {
                self.busy = true;
                self.turn_done = false;
            }
            Event::Text(text) => self.push_text(&text),
            Event::Reasoning(text) => self.push_reasoning(&text),
            Event::ToolCall { name, summary, .. } => self.transcript.push(Entry::Tool {
                name,
                detail: summary,
                status: ToolStatus::Running,
            }),
            Event::ToolResult { ok, summary, .. } => {
                // Update the most recent running entry for this tool.
                if let Some(Entry::Tool { status, detail, .. }) =
                    self.transcript.iter_mut().rev().find(|e| {
                        matches!(
                            e,
                            Entry::Tool {
                                status: ToolStatus::Running,
                                ..
                            }
                        )
                    })
                {
                    *status = if ok {
                        ToolStatus::Ok
                    } else {
                        ToolStatus::Failed
                    };
                    if !summary.is_empty() {
                        *detail = summary;
                    }
                }
            }
            Event::Context(context) => self.context = Some(context),
            Event::Commands(commands) => self.available_commands = commands,
            Event::Failed(message) => self.transcript.push(Entry::Error(message)),
            Event::TurnEnd => {}
            Event::Done => {
                self.busy = false;
                self.turn_done = true;
            }
        }
    }

    /// Asks the running turn to stop.
    pub fn cancel(&mut self) {
        if let Some(connection) = self.connection.as_ref() {
            connection.cancel();
        }
    }
}

/// Starts a turn with the text currently in the input box.
///
/// A free function rather than a method because sending needs the vault (the
/// session's working directory) while the chat lives inside the app. The
/// connection is created lazily: the first message spawns `crow-cli acp`, opens
/// a session rooted at the vault, and only then prompts. Later messages reuse
/// both — crow-cli keeps the conversation server-side, so the client keeps no
/// history of its own.
pub fn send(app: &mut App) {
    let text = app.chat.input_text();
    let text = text.trim().to_string();
    if text.is_empty() || app.chat.busy {
        return;
    }
    app.chat.clear_input();
    app.chat.follow = true;
    app.chat.transcript.push(Entry::User(text.clone()));

    if app.chat.connection.is_none() {
        // The agent's tool supply is crow-cli's MCP config, passed through at
        // `session/new` — ACP gives the client this job, and emeraldian has no
        // tools of its own.
        let servers = wire::config::load(None).unwrap_or_default();
        app.chat.connection = Some(Connection::spawn(app.index.vault.path.clone(), servers));
    }
    if let Some(connection) = app.chat.connection.as_ref() {
        connection.prompt(text);
    }
}

/// Drains wire events into the transcript.
///
/// Called once per frame. The connection is taken out of the app first, because
/// applying events needs `&mut Chat` and it lives inside it. A closed channel
/// means the connection thread exited — a failed spawn, or an app that moved on
/// — and the next message will try again.
pub fn poll(app: &mut App) -> bool {
    let Some(mut connection) = app.chat.connection.take() else {
        return false;
    };
    let mut changed = false;
    let mut dead = false;

    loop {
        match connection.try_recv() {
            Ok(event) => {
                app.chat.apply(event);
                changed = true;
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
            Err(_) => {
                dead = true;
                break;
            }
        }
    }

    if dead {
        // The thread is gone; the connection drops here and a fresh one is
        // spawned on the next message.
        app.chat.busy = false;
    } else {
        app.chat.connection = Some(connection);
    }

    changed
}

/// A model list being fetched from a provider.
///
/// Asking costs a network round trip, and a local server that isn't running costs
/// the whole timeout, so it happens on a thread and the reader stays usable while
/// it does. One at a time: the answer feeds a menu that is about to open, and a
/// second request would only race the first to fill it.
#[derive(Default)]
pub struct Lookup {
    pending: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    /// Which provider was asked, so the picker can say so.
    pub provider: String,
}

impl Lookup {
    /// Whether an answer is still outstanding.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Asks a provider for its models.
    ///
    /// Replaces any request already in flight; the old one's answer is dropped
    /// when its channel closes.
    pub fn start(&mut self, provider: &str, api_key: Option<String>, base_url: Option<String>) {
        let Some(preset) = emeraldian_agent::catalog::find(provider) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending = Some(rx);
        self.provider = provider.to_string();

        std::thread::Builder::new()
            // Linux caps a thread name at 15 bytes and refuses anything longer,
            // so this is deliberately shorter than the crate name.
            .name("emerald-models".into())
            .spawn(move || {
                let result = emeraldian_agent::catalog::models(
                    preset,
                    api_key.as_deref(),
                    base_url.as_deref(),
                )
                .map_err(|err| err.to_string());
                // Nobody left to tell means the app moved on, which is fine.
                let _ = tx.send(result);
            })
            .ok();
    }

    /// The answer, once it arrives.
    pub fn take(&mut self) -> Option<Result<Vec<String>, String>> {
        let pending = self.pending.as_ref()?;
        match pending.try_recv() {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            // The thread died without answering. Reporting that is better than
            // waiting for a reply that will never come.
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                Some(Err("the request went away".into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_editing_is_character_aware() {
        let mut chat = Chat::new(&Config::default());
        for ch in "héllo".chars() {
            chat.input.insert_char(ch);
        }
        assert_eq!(chat.input_text(), "héllo");
        assert_eq!(chat.input.cursor().col, 5);

        for _ in 0..3 {
            chat.input.move_left(false);
        }
        chat.input.insert_char('X');
        assert_eq!(chat.input_text(), "héXllo");

        chat.input.backspace();
        assert_eq!(chat.input_text(), "héllo");
    }

    #[test]
    fn cursor_movement_stays_inside_the_input() {
        let mut chat = Chat::new(&Config::default());
        chat.input.insert_char('a');
        for _ in 0..10 {
            chat.input.move_left(false);
        }
        assert_eq!(chat.input.cursor().col, 0);
        for _ in 0..10 {
            chat.input.move_right(false);
        }
        assert_eq!(chat.input.cursor().col, 1);
    }

    #[test]
    fn set_input_puts_the_cursor_at_its_end() {
        let mut chat = Chat::new(&Config::default());
        chat.set_input("/resume ");
        assert_eq!(chat.input_text(), "/resume ");
        assert_eq!(
            chat.input.cursor(),
            crate::editor::Cursor { line: 0, col: 8 },
            "the cursor follows the text"
        );
    }

    #[test]
    fn input_text_joins_lines_without_a_trailing_newline() {
        let mut chat = Chat::new(&Config::default());
        chat.set_input("first");
        chat.input.newline();
        chat.input.insert_char('s');
        assert_eq!(chat.input_text(), "first\ns");
        assert!(!chat.input_is_empty());

        chat.clear_input();
        assert!(chat.input_is_empty());
    }

    #[test]
    fn streamed_text_accumulates_into_one_entry() {
        let mut chat = Chat::new(&Config::default());
        chat.apply(Event::Text("Hel".into()));
        chat.apply(Event::Text("lo".into()));

        assert_eq!(chat.transcript, vec![Entry::Assistant("Hello".into())]);
    }

    #[test]
    fn a_tool_call_updates_in_place_when_it_finishes() {
        let mut chat = Chat::new(&Config::default());
        chat.apply(Event::ToolCall {
            id: "t1".into(),
            name: "create_note".into(),
            summary: "name=Ideas".into(),
        });
        assert!(matches!(
            chat.transcript[0],
            Entry::Tool {
                status: ToolStatus::Running,
                ..
            }
        ));

        chat.apply(Event::ToolResult {
            id: "t1".into(),
            ok: true,
            summary: "created Ideas.md".into(),
        });

        assert_eq!(
            chat.transcript.len(),
            1,
            "the entry is updated, not appended"
        );
        assert!(matches!(
            &chat.transcript[0],
            Entry::Tool { status: ToolStatus::Ok, detail, .. } if detail == "created Ideas.md"
        ));
    }

    #[test]
    fn reasoning_and_text_stay_separate_entries() {
        let mut chat = Chat::new(&Config::default());
        chat.apply(Event::Reasoning("thinking".into()));
        chat.apply(Event::Text("answer".into()));
        chat.apply(Event::Reasoning("more".into()));

        assert_eq!(chat.transcript.len(), 3);
    }

    #[test]
    fn done_clears_the_busy_flag() {
        let mut chat = Chat::new(&Config::default());
        chat.apply(Event::Started);
        assert!(chat.busy);
        chat.apply(Event::Done);
        assert!(!chat.busy);
    }

    #[test]
    fn the_turn_done_flag_waits_for_the_wire_not_the_busy_race() {
        let mut chat = Chat::new(&Config::default());
        // Before the first event, both busy and turn_done are false — which is
        // why waiting on `busy` alone raced past whole turns.
        assert!(!chat.busy);
        assert!(!chat.turn_done);

        chat.apply(Event::Started);
        assert!(chat.busy);
        assert!(!chat.turn_done, "a turn that started is not over");

        chat.apply(Event::Done);
        assert!(chat.turn_done, "and now the scripted run may print");

        chat.apply(Event::Started);
        assert!(!chat.turn_done, "the next turn re-arms the wait");
    }

    #[test]
    fn reset_clears_the_transcript() {
        let mut chat = Chat::new(&Config::default());
        chat.transcript.push(Entry::User("hi".into()));
        chat.reset();

        assert!(chat.transcript.is_empty());
    }
}
