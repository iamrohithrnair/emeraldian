//! The ACP client side of crow-term.
//!
//! crow-term is an ACP **client** (an editor-shaped host); the agent is a separate
//! process — by default `crow-cli acp`, exactly like Zed or the crow-cli Textual TUI
//! spawn it. Everything the UI needs arrives as `session/update` notifications on a
//! channel, so the renderer never touches JSON-RPC.
//!
//! We speak **protocol v1** because that is what `crow-cli acp` serves today, and
//! rust-sdk refuses to convert between protocol versions (a `Client::v2()` cannot
//! talk to a v1 agent at all). See `README.md` § Protocol version.

mod client;
pub mod config;
mod mcp;

pub use client::{AcpClient, AgentLaunch, UpdateStream};
pub use mcp::McpServer;
