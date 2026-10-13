//! Tool supply: the agent's MCP servers, read from crow-cli's config and handed to it
//! at `session/new`.
//!
//! ACP gives the client this job — an agent gets exactly the servers the client sends.
//! crow-term has no tools of its own, so without this the agent is mute-but-talkative:
//! it will happily print a hallucinated tool call instead of running anything. This
//! mirrors `crow_cli/tui/mcp.py`: read `mcpServers` out of `~/.agents/crow/config.yaml`
//! and pass it through; crow-term never connects to these servers itself.

use crate::McpServer;
use anyhow::{Context as _, Result, anyhow};
use std::path::{Path, PathBuf};
use yaml_rust2::Yaml;

/// Where crow-cli keeps its config, and therefore where this client reads tools from.
pub const DEFAULT_CONFIG: &str = "~/.agents/crow/config.yaml";

/// Loads `mcpServers` in ACP wire shape. A missing file means no servers, not an
/// error — the agent runs toolless, which is a legitimate (if dull) configuration.
pub fn load(path: Option<&Path>) -> Result<Vec<McpServer>> {
    let path = match path {
        Some(path) => path.to_path_buf(),
        None => expand_home(DEFAULT_CONFIG)?,
    };
    if !path.exists() {
        return Ok(Vec::new());
    }

    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let documents = yaml_rust2::YamlLoader::load_from_str(&text)
        .with_context(|| format!("parsing {}", path.display()))?;
    let Some(document) = documents.first() else {
        return Ok(Vec::new());
    };

    let mut servers = Vec::new();
    if let Some(entries) = document["mcpServers"].as_hash() {
        for (name, entry) in entries {
            let Some(name) = name.as_str() else { continue };
            match to_server(name, entry) {
                Some(server) => servers.push(server),
                None => eprintln!(
                    "warning: skipping mcpServers.{name} — no url (http/sse) or command (stdio)"
                ),
            }
        }
    }
    Ok(servers)
}

/// One `mcpServers` entry. HTTP/SSE when a transport says so or a url is present;
/// otherwise stdio with a command. Same branch order as `_to_wire`.
fn to_server(name: &str, entry: &Yaml) -> Option<McpServer> {
    let transport = entry["transport"].as_str();
    let url = entry["url"].as_str().map(str::to_owned);

    match (transport, url) {
        (Some("http") | None, Some(url)) => Some(McpServer::Http {
            name: name.to_owned(),
            url,
            headers: pairs(&entry["headers"]),
        }),
        (Some("sse"), Some(url)) => Some(McpServer::Sse {
            name: name.to_owned(),
            url,
            headers: pairs(&entry["headers"]),
        }),
        _ => {
            let command = entry["command"].as_str()?;
            Some(McpServer::Stdio {
                name: name.to_owned(),
                command: PathBuf::from(command),
                args: strings(&entry["args"]),
                env: pairs(&entry["env"]),
            })
        }
    }
}

/// A `{key: value}` mapping flattened to pairs, as the wire format wants them.
fn pairs(yaml: &Yaml) -> Vec<(String, String)> {
    yaml.as_hash()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| {
                    Some((key.as_str()?.to_owned(), value.as_str()?.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn strings(yaml: &Yaml) -> Vec<String> {
    yaml.as_vec()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn expand_home(path: &str) -> Result<PathBuf> {
    let home = std::env::var("HOME").context("$HOME is unset")?;
    path.strip_prefix("~")
        .map(|rest| PathBuf::from(home).join(rest.trim_start_matches('/')))
        .ok_or_else(|| anyhow!("not a home-relative path: {path}"))
}
