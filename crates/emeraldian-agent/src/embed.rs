//! Embeddings: text in, vectors out.
//!
//! Semantic search needs a model that turns a passage into a vector, and the
//! natural home for one is a local server — Ollama, by default — which keeps
//! the model loaded between questions and keeps a tensor library out of this
//! binary. Any server that speaks OpenAI's embeddings API serves as well: LM
//! Studio, llama.cpp's server, vLLM, or OpenAI itself.
//!
//! Ollama also fetches a model on request, which is how the first use of
//! semantic search downloads the model instead of asking someone to.

use std::io::{BufRead, BufReader};

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::provider::{get_json, post_json, post_stream};

/// Which API an embeddings server speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// Ollama's own: `/api/embed`, and `/api/pull` to download a model.
    Ollama,
    /// OpenAI's `/v1/embeddings`, which most other servers copy.
    OpenAi,
}

impl Api {
    /// Reads a `provider` setting. The names of servers that speak OpenAI's
    /// API are accepted for it, so whatever someone calls their server works.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "ollama" => Some(Self::Ollama),
            "openai" | "openai-compatible" | "lmstudio" | "lm-studio" | "llamacpp"
            | "llama.cpp" | "vllm" => Some(Self::OpenAi),
            _ => None,
        }
    }
}

/// A connection to an embeddings server, for one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Embedder {
    pub api: Api,
    pub base_url: String,
    pub model: String,
    /// How many dimensions to ask for, from a model that can give fewer than
    /// its full size. Servers that can't oblige ignore it, so callers cut the
    /// vectors themselves too.
    pub dimensions: Option<usize>,
    /// Sent as a bearer token where there is one. A local server needs none.
    pub key: Option<String>,
    /// How long Ollama keeps the model loaded after a request, as Ollama
    /// reads a duration: `30s`, `5m`, `0` to unload at once. The model holds
    /// a few hundred megabytes while loaded and takes about a second to load
    /// again, so this is the trade between the two. `None` leaves Ollama's.
    pub keep_alive: Option<String>,
}

/// One step of a model download, as Ollama reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullStatus {
    pub status: String,
    pub completed: u64,
    pub total: u64,
}

/// How long one batch of passages may take. The first request also loads the
/// model into memory, which from a slow disk is a matter of seconds.
const EMBED_TIMEOUT_SECS: u64 = 300;

/// How long a question about the server itself may take: whether it is there,
/// whether it has the model.
const QUERY_TIMEOUT_SECS: u64 = 15;

/// A download takes as long as the connection makes it; this only stops one
/// that has stalled for good.
const PULL_TIMEOUT_SECS: u64 = 6 * 60 * 60;

impl Embedder {
    fn url(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url.trim_end_matches('/'))
    }

    /// One vector per text, in order.
    pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let (path, mut body) = match self.api {
            // `truncate` lets a passage longer than the model's window through,
            // cut, rather than failing the whole batch over it.
            Api::Ollama => (
                "api/embed",
                json!({ "model": self.model, "input": texts, "truncate": true }),
            ),
            Api::OpenAi => (
                "v1/embeddings",
                json!({ "model": self.model, "input": texts }),
            ),
        };
        if let Some(dimensions) = self.dimensions {
            body["dimensions"] = json!(dimensions);
        }
        if let (Api::Ollama, Some(keep_alive)) = (self.api, &self.keep_alive) {
            body["keep_alive"] = json!(keep_alive);
        }
        let value = post_json(
            &self.url(path),
            self.key.as_deref(),
            &body,
            EMBED_TIMEOUT_SECS,
        )?;
        let vectors = match self.api {
            Api::Ollama => parse_ollama(&value)?,
            Api::OpenAi => parse_openai(&value)?,
        };
        if vectors.len() != texts.len() {
            return Err(Error::Protocol(format!(
                "{} vectors came back for {} texts",
                vectors.len(),
                texts.len()
            )));
        }
        Ok(vectors)
    }

    /// Whether the server answers at all.
    pub fn reachable(&self) -> Result<()> {
        let path = match self.api {
            Api::Ollama => "api/version",
            Api::OpenAi => "v1/models",
        };
        get_json(&self.url(path), &[], self.key.as_deref()).map(|_| ())
    }

    /// Whether the server has the model. Only Ollama can be asked; any other
    /// server is taken to have what it was configured with.
    pub fn installed(&self) -> Result<bool> {
        match self.api {
            Api::OpenAi => Ok(true),
            Api::Ollama => match post_json(
                &self.url("api/show"),
                None,
                &json!({ "model": self.model }),
                QUERY_TIMEOUT_SECS,
            ) {
                Ok(_) => Ok(true),
                Err(Error::Http { status: 404, .. }) => Ok(false),
                Err(err) => Err(err),
            },
        }
    }

    /// Downloads the model through Ollama, reporting each step.
    pub fn pull(&self, progress: &mut dyn FnMut(&PullStatus)) -> Result<()> {
        if self.api != Api::Ollama {
            return Err(Error::Protocol(
                "only Ollama can download a model; install it on the server instead".into(),
            ));
        }
        let reader = post_stream(
            &self.url("api/pull"),
            &[],
            &json!({ "model": self.model, "stream": true }),
            PULL_TIMEOUT_SECS,
        )?;
        for line in BufReader::new(reader).lines() {
            let line = line.map_err(|err| Error::Transport(err.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            let status = parse_pull(&line)?;
            progress(&status);
            if status.status == "success" {
                return Ok(());
            }
        }
        Err(Error::Protocol(
            "the download stopped before Ollama said it had finished".into(),
        ))
    }
}

/// `{"embeddings": [[…], …]}`.
fn parse_ollama(value: &Value) -> Result<Vec<Vec<f32>>> {
    value
        .get("embeddings")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Protocol("no `embeddings` in the reply".into()))?
        .iter()
        .map(vector)
        .collect()
}

/// `{"data": [{"index": 0, "embedding": […]}, …]}`, put back in input order:
/// the API numbers each vector, and doesn't promise to send them in order.
fn parse_openai(value: &Value) -> Result<Vec<Vec<f32>>> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Protocol("no `data` in the reply".into()))?;
    let mut numbered: Vec<(u64, Vec<f32>)> = data
        .iter()
        .enumerate()
        .map(|(position, item)| {
            let index = item
                .get("index")
                .and_then(Value::as_u64)
                .unwrap_or(position as u64);
            let embedding = item
                .get("embedding")
                .ok_or_else(|| Error::Protocol("an item has no `embedding`".into()))?;
            Ok((index, vector(embedding)?))
        })
        .collect::<Result<_>>()?;
    numbered.sort_by_key(|(index, _)| *index);
    Ok(numbered.into_iter().map(|(_, vector)| vector).collect())
}

fn vector(value: &Value) -> Result<Vec<f32>> {
    value
        .as_array()
        .ok_or_else(|| Error::Protocol("an embedding is not a list of numbers".into()))?
        .iter()
        .map(|number| {
            number
                .as_f64()
                .map(|n| n as f32)
                .ok_or_else(|| Error::Protocol("an embedding holds something not a number".into()))
        })
        .collect()
}

/// One line of Ollama's download stream. A line with `error` in it is the
/// download failing, with the reason.
fn parse_pull(line: &str) -> Result<PullStatus> {
    let value: Value =
        serde_json::from_str(line).map_err(|err| Error::Protocol(err.to_string()))?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(Error::Protocol(error.to_string()));
    }
    Ok(PullStatus {
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        completed: value.get("completed").and_then(Value::as_u64).unwrap_or(0),
        total: value.get("total").and_then(Value::as_u64).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_map_to_the_api_they_speak() {
        assert_eq!(Api::parse("Ollama"), Some(Api::Ollama));
        for name in ["openai", "lmstudio", "llama.cpp", "vllm"] {
            assert_eq!(Api::parse(name), Some(Api::OpenAi), "{name}");
        }
        assert_eq!(Api::parse("anthropic"), None, "no embeddings there");
    }

    #[test]
    fn urls_join_whatever_the_base_ends_with() {
        let mut embedder = Embedder {
            api: Api::Ollama,
            base_url: "http://localhost:11434/".into(),
            model: "m".into(),
            dimensions: None,
            key: None,
            keep_alive: None,
        };
        assert_eq!(
            embedder.url("api/embed"),
            "http://localhost:11434/api/embed"
        );
        embedder.base_url = "http://localhost:11434".into();
        assert_eq!(
            embedder.url("api/embed"),
            "http://localhost:11434/api/embed"
        );
    }

    #[test]
    fn ollama_vectors_are_read_in_order() {
        let reply = json!({ "model": "m", "embeddings": [[0.5, -0.5], [1, 0]] });
        assert_eq!(
            parse_ollama(&reply).expect("parses"),
            vec![vec![0.5, -0.5], vec![1.0, 0.0]]
        );
        assert!(parse_ollama(&json!({ "error": "x" })).is_err());
        assert!(parse_ollama(&json!({ "embeddings": [["a"]] })).is_err());
    }

    #[test]
    fn openai_vectors_are_put_back_in_input_order() {
        let reply = json!({ "data": [
            { "index": 1, "embedding": [0.0, 1.0] },
            { "index": 0, "embedding": [1.0, 0.0] }
        ] });
        assert_eq!(
            parse_openai(&reply).expect("parses"),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
    }

    #[test]
    fn a_download_reports_its_progress_and_its_failure() {
        assert_eq!(
            parse_pull(r#"{"status":"pulling 111e84992a04","digest":"sha256:x","total":300,"completed":150}"#)
                .expect("parses"),
            PullStatus {
                status: "pulling 111e84992a04".into(),
                completed: 150,
                total: 300
            }
        );
        let error = parse_pull(r#"{"error":"pull model manifest: file does not exist"}"#)
            .expect_err("a failure");
        assert!(error.to_string().contains("file does not exist"));
    }
}
