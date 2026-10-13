//! MCP servers handed to the agent at `session/new`.
//!
//! In ACP the **client owns tool supply**: an agent starts with exactly the MCP
//! servers the client passes and nothing else (`new_session` in
//! `crow-cli/src/crow_cli/agent/main.py` — "empty = zero tools"). A client that
//! sends none gets a model that can only talk, which is how it ends up printing a
//! hallucinated tool call as prose.
//!
//! These are passed through untouched — crow-term never connects to them itself.

use std::path::PathBuf;

use agent_client_protocol::schema::v1::{
    EnvVariable, HttpHeader, McpServer as WireMcpServer, McpServerHttp, McpServerSse,
    McpServerStdio,
};

/// A server to hand the agent. Mirrors the `mcpServers` entries in crow-cli's
/// config: `transport: http|sse` with a url, or stdio with a command.
#[derive(Debug, Clone)]
pub enum McpServer {
    Stdio {
        name: String,
        command: PathBuf,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
    Sse {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
}

impl McpServer {
    pub fn name(&self) -> &str {
        match self {
            Self::Stdio { name, .. } | Self::Http { name, .. } | Self::Sse { name, .. } => name,
        }
    }

    fn into_wire(self) -> WireMcpServer {
        match self {
            Self::Stdio {
                name,
                command,
                args,
                env,
            } => WireMcpServer::Stdio(
                McpServerStdio::new(name, command).args(args).env(
                    env.into_iter()
                        .map(|(name, value)| EnvVariable::new(name, value))
                        .collect(),
                ),
            ),
            Self::Http { name, url, headers } => {
                WireMcpServer::Http(McpServerHttp::new(name, url).headers(wire_headers(headers)))
            }
            Self::Sse { name, url, headers } => {
                WireMcpServer::Sse(McpServerSse::new(name, url).headers(wire_headers(headers)))
            }
        }
    }
}

fn wire_headers(pairs: Vec<(String, String)>) -> Vec<HttpHeader> {
    pairs
        .into_iter()
        .map(|(name, value)| HttpHeader::new(name, value))
        .collect()
}

pub(crate) fn to_wire(servers: Vec<McpServer>) -> Vec<WireMcpServer> {
    servers.into_iter().map(McpServer::into_wire).collect()
}
