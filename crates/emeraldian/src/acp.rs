//! The ACP wire: a background connection to an agent subprocess.
//!
//! The chat panel used to run the model in-process (`emeraldian-agent`: providers,
//! SSE, a tool loop on a thread). It now speaks the Agent Client Protocol instead:
//! emeraldian is a *client*, and the agent — `crow-cli acp` by default — is a
//! subprocess, exactly like Zed spawns agents. The client supplies the tools at
//! `session/new`; crow-cli's MCP config is passed through untouched.
//!
//! The split keeps emeraldian synchronous: commands go out on an unbounded channel
//! (sendable from key handlers), events come back on one that
//! [`crate::agent::poll`] drains between frames. A tokio runtime lives inside the
//! connection's thread because the wire client is async; nothing else here is.

use std::collections::HashSet;
use std::path::PathBuf;

use agent_client_protocol::schema::v1::{
    ContentBlock, SessionNotification, SessionUpdate, ToolCallContent, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};
use tokio::sync::mpsc;

use crow_term_acp::{self as wire, McpServer};

/// What the UI asks the connection to do.
#[derive(Debug, Clone)]
pub enum Command {
    /// Send one user message; the turn's updates stream back as events.
    Prompt(String),
    /// Ask the agent to stop the running turn (`session/cancel`).
    Cancel,
}

/// What the wire reports, shaped for the transcript.
///
/// The same event set the in-process agent used, so the panel and its renderer
/// are untouched by the swap. Tool calls pair up by recency on the panel side:
/// crow-cli runs tools one at a time, so "the most recent running call" is
/// always the right one to update.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A turn began.
    Started,
    /// Incremental reasoning (`thinking` channel).
    Reasoning(String),
    /// Incremental assistant text.
    Text(String),
    /// The agent started a tool call.
    ToolCall {
        id: String,
        name: String,
        summary: String,
    },
    /// A tool finished.
    ToolResult {
        id: String,
        ok: bool,
        summary: String,
    },
    /// Context-window usage (`usage/update`), as `used/size`.
    Context(String),
    /// The slash commands the agent advertises (`available_commands/update`).
    Commands(Vec<(String, String)>),
    /// The turn ended in failure. Carries a message meant for the user.
    Failed(String),
    /// One assistant response completed; more may follow if tools ran.
    TurnEnd,
    /// The turn is over, successfully or not.
    Done,
}

/// A live connection to an agent subprocess.
///
/// Dropping it drops the command sender; the thread sees that and shuts down,
/// which ends the tokio runtime and kills the subprocess with it.
pub struct Connection {
    commands: mpsc::UnboundedSender<Command>,
    events: mpsc::UnboundedReceiver<Event>,
}

impl Connection {
    /// Spawns the agent subprocess and its connection thread.
    ///
    /// Returns immediately: connecting, `initialize` and `session/new` happen on
    /// the thread, and commands sent meanwhile queue until the session exists.
    pub fn spawn(cwd: PathBuf, servers: Vec<McpServer>) -> Self {
        let (commands_tx, commands_rx) = mpsc::unbounded_channel();
        let (events_tx, events_rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("emerald-acp".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("a tokio runtime for the ACP connection");
                runtime.block_on(run(commands_rx, events_tx, cwd, servers));
            })
            .expect("spawn the ACP connection thread");
        Self {
            commands: commands_tx,
            events: events_rx,
        }
    }

    /// Queues one user message.
    pub fn prompt(&self, text: String) {
        let _ = self.commands.send(Command::Prompt(text));
    }

    /// Asks the running turn to stop. Safe mid-turn by design.
    pub fn cancel(&self) {
        let _ = self.commands.send(Command::Cancel);
    }

    /// Takes the next event, if one has arrived.
    ///
    /// A closed channel means the connection thread exited — a failed spawn or
    /// an app that moved on — and the next message will try again.
    pub fn try_recv(&mut self) -> Result<Event, mpsc::error::TryRecvError> {
        self.events.try_recv()
    }
}

async fn run(
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    cwd: PathBuf,
    servers: Vec<McpServer>,
) {
    let launch = wire::AgentLaunch::default();
    let (client, mut updates) = match wire::AcpClient::connect(launch).await {
        Ok(pair) => pair,
        Err(error) => {
            let _ = events.send(Event::Failed(format!(
                "could not start the agent: {error:#}"
            )));
            let _ = events.send(Event::Done);
            return;
        }
    };
    let session = match client.new_session(cwd, servers).await {
        Ok(session) => session,
        Err(error) => {
            let _ = events.send(Event::Failed(format!(
                "could not open a session: {error:#}"
            )));
            let _ = events.send(Event::Done);
            return;
        }
    };

    // crow-cli announces each call once at its start; later updates for the same
    // id are progress, and re-announcing them would duplicate panel entries.
    let mut announced = HashSet::new();

    loop {
        tokio::select! {
            command = commands.recv() => match command {
                // The UI dropped the connection: shut down, which ends the agent subprocess.
                None => break,
                Some(Command::Cancel) => {
                    if let Err(error) = client.cancel(&session) {
                        let _ = events.send(Event::Failed(format!("cancel failed: {error:#}")));
                    }
                }
                Some(Command::Prompt(text)) => {
                    let _ = events.send(Event::Started);
                    match client.prompt(&session, text).await {
                        Ok(_reason) => {
                            let _ = events.send(Event::TurnEnd);
                            let _ = events.send(Event::Done);
                        }
                        Err(error) => {
                            let _ = events.send(Event::Failed(format!("error · {error:#}")));
                            let _ = events.send(Event::Done);
                        }
                    }
                }
            },
            notification = updates.recv() => match notification {
                None => break,
                Some(note) => {
                    for event in map_notification(&note, &mut announced) {
                        let _ = events.send(event);
                    }
                }
            },
        }
    }
}

/// Turns one `session/update` into transcript events. Unknown updates are
/// ignored rather than shouted about — the same posture crow-term takes.
fn map_notification(note: &SessionNotification, announced: &mut HashSet<String>) -> Vec<Event> {
    let mut out = Vec::new();
    match &note.update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            if let ContentBlock::Text(text) = &chunk.content {
                out.push(Event::Text(text.text.clone()));
            }
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            if let ContentBlock::Text(text) = &chunk.content {
                out.push(Event::Reasoning(text.text.clone()));
            }
        }
        SessionUpdate::ToolCall(call) => {
            let update = ToolCallUpdate::from(call.clone());
            map_tool_call(
                update.tool_call_id.0.to_string(),
                &update.fields,
                announced,
                &mut out,
            );
        }
        SessionUpdate::ToolCallUpdate(update) => {
            map_tool_call(
                update.tool_call_id.0.to_string(),
                &update.fields,
                announced,
                &mut out,
            );
        }
        SessionUpdate::UsageUpdate(usage) => {
            out.push(Event::Context(format!("ctx {}/{}", usage.used, usage.size)));
        }
        SessionUpdate::AvailableCommandsUpdate(commands) => {
            out.push(Event::Commands(
                commands
                    .available_commands
                    .iter()
                    .map(|command| (command.name.clone(), command.description.clone()))
                    .collect(),
            ));
        }
        _ => {}
    }
    out
}

/// One tool-call event: the start announces (once per id), a settled status
/// reports. In-flight updates for an already-announced call change nothing.
fn map_tool_call(
    id: String,
    fields: &ToolCallUpdateFields,
    announced: &mut HashSet<String>,
    out: &mut Vec<Event>,
) {
    match fields.status {
        Some(ToolCallStatus::Completed) => out.push(Event::ToolResult {
            id,
            ok: true,
            summary: content_summary(fields),
        }),
        Some(ToolCallStatus::Failed) => out.push(Event::ToolResult {
            id,
            ok: false,
            summary: content_summary(fields),
        }),
        _ => {
            if announced.insert(id.clone()) {
                out.push(Event::ToolCall {
                    id,
                    name: fields.title.clone().unwrap_or_else(|| "tool".into()),
                    summary: String::new(),
                });
            }
        }
    }
}

/// The text the agent attached to a finished call, flattened for the panel's
/// one-line entry. Diffs and terminals are the boxed-rendering story, not this.
fn content_summary(fields: &ToolCallUpdateFields) -> String {
    let Some(contents) = &fields.content else {
        return String::new();
    };
    let mut out = String::new();
    for content in contents {
        if let ToolCallContent::Content(block) = content
            && let ContentBlock::Text(text) = &block.content
        {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&text.text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{Content, TextContent};

    fn fields(status: Option<ToolCallStatus>, title: Option<&str>) -> ToolCallUpdateFields {
        // `#[non_exhaustive]` from another crate: built from `default()` and
        // mutated rather than written as a struct literal.
        let mut fields = ToolCallUpdateFields::default();
        fields.status = status;
        fields.title = title.map(str::to_owned);
        fields
    }

    #[test]
    fn a_call_announces_once_per_id() {
        let mut announced = HashSet::new();
        let mut out = Vec::new();
        map_tool_call(
            "t1".into(),
            &fields(None, Some("Read A.md")),
            &mut announced,
            &mut out,
        );
        assert!(matches!(out[0], Event::ToolCall { ref name, .. } if name == "Read A.md"));

        // A second in-flight update for the same id is progress, not a new call.
        map_tool_call(
            "t1".into(),
            &fields(None, Some("Read A.md")),
            &mut announced,
            &mut out,
        );
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_settled_status_reports_even_without_a_start() {
        let mut announced = HashSet::new();
        let mut out = Vec::new();
        map_tool_call(
            "t1".into(),
            &fields(Some(ToolCallStatus::Completed), None),
            &mut announced,
            &mut out,
        );
        assert!(matches!(out[0], Event::ToolResult { ok: true, .. }));
    }

    #[test]
    fn text_content_flattens_into_the_summary() {
        let block = ToolCallContent::Content(Content::new(ContentBlock::Text(TextContent::new(
            "wrote the note",
        ))));
        let mut fields = ToolCallUpdateFields::default();
        fields.content = Some(vec![block]);
        assert_eq!(content_summary(&fields), "wrote the note");
    }
}
