use std::path::PathBuf;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, InitializeRequest, LoadSessionRequest, NewSessionRequest,
    PromptRequest, SessionId, SessionNotification, StopReason, TextContent,
};
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, ConnectionTo};
use anyhow::{Context, Result};
use tokio::sync::{mpsc, oneshot};

use crate::mcp::{self, McpServer};

/// The ACP version we speak. `crow-cli acp` serves v1, and rust-sdk never converts
/// between protocol versions, so this cannot be bumped until the agent speaks v2.
const PROTOCOL: ProtocolVersion = ProtocolVersion::V1;

/// Incoming `session/update` stream. Unbounded because dropping an update loses it —
/// the SDK does not buffer unhandled notifications per session.
pub type UpdateStream = mpsc::UnboundedReceiver<SessionNotification>;

/// How to spawn the agent subprocess.
#[derive(Debug, Clone)]
pub struct AgentLaunch {
    pub command: String,
    pub args: Vec<String>,
    /// `--model` for the agent process. crow-cli resolves it against config.yaml and,
    /// on `session/load`, it supersedes the session's saved model — an explicit "use
    /// THIS model for this run".
    pub model: Option<String>,
    /// `--config-file` for the agent process: a YAML config override.
    pub config_file: Option<PathBuf>,
}

impl Default for AgentLaunch {
    /// `crow-cli acp` — the same launch command the Textual TUI uses
    /// (`crow_cli/cli/tui_cmd.py`).
    fn default() -> Self {
        Self {
            command: "crow-cli".to_owned(),
            args: vec!["acp".to_owned()],
            model: None,
            config_file: None,
        }
    }
}

impl AgentLaunch {
    /// The full agent argv: base args, then the flag passthroughs in a fixed order.
    fn argv(&self) -> Vec<String> {
        let mut argv = self.args.clone();
        if let Some(model) = &self.model {
            argv.push("--model".to_owned());
            argv.push(model.clone());
        }
        if let Some(config_file) = &self.config_file {
            argv.push("--config-file".to_owned());
            argv.push(config_file.to_string_lossy().into_owned());
        }
        argv
    }

    fn into_agent(self) -> AcpAgent {
        let args = self.argv();
        AcpAgent::new(AcpAgentConfig::new(self.command).args(args))
    }
}

/// A connected, initialized ACP client. Cheap to clone; the agent subprocess lives
/// until the last clone is dropped.
#[derive(Clone)]
pub struct AcpClient {
    connection: ConnectionTo<Agent>,
    /// Whether the agent advertised `loadSession` at `initialize` — the gate on
    /// [`Self::load_session`]. Read from the wire, never assumed.
    can_load_session: bool,
    /// Dropping the last clone drops this sender, which ends the connection task and
    /// tears down the agent subprocess. Never read — held for its lifetime only.
    _keepalive: mpsc::Sender<()>,
}

impl AcpClient {
    /// Spawns the agent, connects, and performs the `initialize` handshake.
    ///
    /// Returns the client handle plus the `session/update` stream. The stream is a
    /// separate value on purpose: prompting borrows the client immutably while the
    /// UI drains updates, and `cancel` must stay callable mid-turn.
    pub async fn connect(launch: AgentLaunch) -> Result<(Self, UpdateStream)> {
        let (update_tx, update_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel::<Result<(Self, UpdateStream)>>();
        let (keepalive, mut shutdown) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            let agent = launch.into_agent();
            let _ = agent_client_protocol::Client
                .builder()
                .on_receive_notification(
                    async move |notification: SessionNotification, _cx| {
                        // The only receiver is the UI; if it is gone, we are shutting down.
                        let _ = update_tx.send(notification);
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                // No `on_receive_request` handler for `session/request_permission`:
                // crow-cli's agent never asks (it has no permissions surface), and an
                // unhandled request is answered with method-not-found — the same
                // posture as both crow-cli clients. Conscious decision, not a TODO.
                .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
                    let client = Self {
                        connection: connection.clone(),
                        can_load_session: false,
                        _keepalive: keepalive,
                    };
                    let client = match client.initialized().await {
                        Ok(client) => client,
                        Err(error) => {
                            let _ = ready_tx.send(Err(error));
                            return Ok(());
                        }
                    };
                    // The UI owns the update stream for the life of the connection.
                    if ready_tx.send(Ok((client, update_rx))).is_err() {
                        return Ok(());
                    }
                    shutdown.recv().await;
                    Ok(())
                })
                .await;
        });

        ready_rx
            .await
            .context("agent task ended before connecting")?
            .context("failed to initialize agent")
    }

    /// The `initialize` handshake, run once per connection. The response's
    /// capabilities are kept — `loadSession` gates session resume.
    async fn initialized(mut self) -> Result<Self> {
        let response = self
            .connection
            .send_request(InitializeRequest::new(PROTOCOL))
            .block_task()
            .await
            .context("initialize")?;
        self.can_load_session = response.agent_capabilities.load_session;
        Ok(self)
    }

    /// Opens a session. `servers` is the agent's entire tool supply — pass an empty
    /// list and it has no tools at all (see [`McpServer`]).
    pub async fn new_session(&self, cwd: PathBuf, servers: Vec<McpServer>) -> Result<SessionId> {
        let request = NewSessionRequest::new(cwd).mcp_servers(mcp::to_wire(servers));
        let response = self
            .connection
            .send_request(request)
            .block_task()
            .await
            .context("session/new")?;
        Ok(response.session_id)
    }

    /// Re-attaches to an existing session — crash recovery, "back in business where you
    /// left off". The session id is a passthrough: crow-cli's ids are the human-named
    /// ones (`lumpy-energetic-hyrax-of-opportunity-77bcbd`), bare id = trunk HEAD,
    /// three-part id = exact fork; the agent resolves it. Like `session/new`, the
    /// client re-supplies the tool belt — and by design nothing is replayed back: the
    /// agent restores its own state and the transcript starts fresh.
    pub async fn load_session(
        &self,
        cwd: PathBuf,
        servers: Vec<McpServer>,
        session_id: &SessionId,
    ) -> Result<()> {
        if !self.can_load_session {
            anyhow::bail!("agent does not advertise loadSession — it cannot resume sessions");
        }
        let request =
            LoadSessionRequest::new(session_id.clone(), cwd).mcp_servers(mcp::to_wire(servers));
        self.connection
            .send_request(request)
            .block_task()
            .await
            .context("session/load")?;
        Ok(())
    }

    /// Sends `session/prompt` and resolves with the agent's stop reason. Updates for
    /// the turn arrive on [`UpdateStream`] while this is in flight.
    pub async fn prompt(
        &self,
        session_id: &SessionId,
        text: impl Into<String>,
    ) -> Result<StopReason> {
        let request = PromptRequest::new(
            session_id.clone(),
            vec![ContentBlock::Text(TextContent::new(text.into()))],
        );
        let response = self
            .connection
            .send_request(request)
            .block_task()
            .await
            .context("session/prompt")?;
        Ok(response.stop_reason)
    }

    /// Fires `session/cancel`. Synchronous by design: it has to be callable from a key
    /// handler while a prompt request is still in flight.
    pub fn cancel(&self, session_id: &SessionId) -> Result<()> {
        self.connection
            .send_notification(CancelNotification::new(session_id.clone()))
            .context("session/cancel")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_launch_is_crow_cli_acp_with_no_flags() {
        let argv = AgentLaunch::default().argv();
        assert_eq!(argv, ["acp"]);
    }

    #[test]
    fn model_and_config_file_append_after_the_base_args() {
        let launch = AgentLaunch {
            model: Some("dev-favorite".to_owned()),
            config_file: Some(PathBuf::from("/tmp/overrides.yaml")),
            ..AgentLaunch::default()
        };
        let argv = launch.argv();
        assert_eq!(
            argv,
            [
                "acp",
                "--model",
                "dev-favorite",
                "--config-file",
                "/tmp/overrides.yaml"
            ]
        );
    }

    #[test]
    fn either_flag_alone_keeps_a_clean_argv() {
        let launch = AgentLaunch {
            config_file: Some(PathBuf::from("o.yaml")),
            ..AgentLaunch::default()
        };
        assert_eq!(launch.argv(), ["acp", "--config-file", "o.yaml"]);
    }
}
