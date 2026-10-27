//! Optional LLM providers behind one small interface.
//!
//! - `openai-compatible` covers DeepSeek, OpenAI, Ollama, vLLM and any `/chat/completions`
//!   endpoint; `anthropic` covers the Messages API.
//! - Model names are configuration, never hard-coded.
//! - API keys are read from an environment variable at call time, never stored, logged or
//!   echoed in errors (threat model T10).
//! - `probe` reports what an endpoint actually does instead of assuming every provider
//!   supports the same features.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    OpenaiCompatible,
    Anthropic,
}

/// `[llm]` section of `.qualyx/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    /// e.g. `https://api.deepseek.com`, `http://localhost:11434/v1`, `https://api.anthropic.com`.
    pub base_url: String,
    pub model: String,
    /// Name of the environment variable holding the key (none for local servers).
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

/// Known endpoints, so users only pick a model. The model itself is always required.
pub fn preset(name: &str, model: &str) -> Option<ProviderConfig> {
    let (kind, base, env) = match name {
        "deepseek" => (
            ProviderKind::OpenaiCompatible,
            "https://api.deepseek.com",
            Some("DEEPSEEK_API_KEY"),
        ),
        "openai" => (
            ProviderKind::OpenaiCompatible,
            "https://api.openai.com/v1",
            Some("OPENAI_API_KEY"),
        ),
        "anthropic" => (
            ProviderKind::Anthropic,
            "https://api.anthropic.com",
            Some("ANTHROPIC_API_KEY"),
        ),
        "ollama" => (
            ProviderKind::OpenaiCompatible,
            "http://localhost:11434/v1",
            None,
        ),
        "vllm" => (
            ProviderKind::OpenaiCompatible,
            "http://localhost:8000/v1",
            None,
        ),
        _ => return None,
    };
    Some(ProviderConfig {
        kind,
        base_url: base.into(),
        model: model.into(),
        api_key_env: env.map(String::from),
        timeout_secs: default_timeout(),
    })
}

#[derive(Debug)]
pub enum LlmError {
    MissingKey(String),
    Http { status: u16, body: String },
    Transport(String),
    Protocol(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::MissingKey(env) => write!(f, "environment variable {env} is not set"),
            LlmError::Http { status, body } => write!(f, "provider returned HTTP {status}: {body}"),
            LlmError::Transport(e) => write!(f, "cannot reach provider: {e}"),
            LlmError::Protocol(e) => write!(f, "unexpected provider response: {e}"),
        }
    }
}

impl std::error::Error for LlmError {}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completion {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub reachable: bool,
    /// `None` when the endpoint has no model listing.
    pub model_listed: Option<bool>,
    pub completion: bool,
    pub structured_json: bool,
    pub errors: Vec<String>,
}

fn key(cfg: &ProviderConfig) -> Result<Option<String>, LlmError> {
    match &cfg.api_key_env {
        None => Ok(None),
        Some(env) => std::env::var(env)
            .ok()
            .filter(|k| !k.is_empty())
            .map(Some)
            .ok_or_else(|| LlmError::MissingKey(env.clone())),
    }
}

fn agent(cfg: &ProviderConfig) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(cfg.timeout_secs)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn url(cfg: &ProviderConfig, path: &str) -> String {
    format!("{}/{}", cfg.base_url.trim_end_matches('/'), path)
}

/// Keep provider error bodies short and free of anything that looks like our key.
fn scrub(body: &str, key: Option<&str>) -> String {
    let mut s: String = body.chars().take(500).collect();
    if let Some(k) = key.filter(|k| k.len() >= 4) {
        s = s.replace(k, "[REDACTED]");
    }
    s
}

fn with_auth<B>(
    r: ureq::RequestBuilder<B>,
    kind: ProviderKind,
    key: Option<&str>,
) -> ureq::RequestBuilder<B> {
    match (kind, key) {
        (ProviderKind::Anthropic, k) => {
            let r = r.header("anthropic-version", "2023-06-01");
            match k {
                Some(k) => r.header("x-api-key", k),
                None => r,
            }
        }
        (ProviderKind::OpenaiCompatible, Some(k)) => {
            r.header("authorization", format!("Bearer {k}"))
        }
        (ProviderKind::OpenaiCompatible, None) => r,
    }
}

fn send(
    cfg: &ProviderConfig,
    method: &str,
    path: &str,
    body: Option<&Value>,
) -> Result<Value, LlmError> {
    let key = key(cfg)?;
    let a = agent(cfg);
    let u = url(cfg, path);
    let result = match (method, body) {
        ("GET", _) => {
            let mut r = a.get(&u);
            r = with_auth(r, cfg.kind, key.as_deref());
            r.call()
        }
        (_, b) => {
            let mut r = a.post(&u).header("content-type", "application/json");
            r = with_auth(r, cfg.kind, key.as_deref());
            r.send(b.map(|v| v.to_string()).unwrap_or_default())
        }
    };
    let mut resp =
        result.map_err(|e| LlmError::Transport(scrub(&e.to_string(), key.as_deref())))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| LlmError::Transport(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(LlmError::Http {
            status,
            body: scrub(&text, key.as_deref()),
        });
    }
    serde_json::from_str(&text).map_err(|e| LlmError::Protocol(e.to_string()))
}

/// One system+user turn. `json_only` asks for a JSON object where the API has a switch for it.
pub fn complete(
    cfg: &ProviderConfig,
    system: &str,
    user: &str,
    max_tokens: u32,
    json_only: bool,
) -> Result<Completion, LlmError> {
    let t = Instant::now();
    let (path, body) = match cfg.kind {
        ProviderKind::OpenaiCompatible => {
            let mut b = json!({
                "model": cfg.model,
                "max_tokens": max_tokens,
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
            });
            if json_only {
                b["response_format"] = json!({"type": "json_object"});
            }
            ("chat/completions", b)
        }
        ProviderKind::Anthropic => (
            "v1/messages",
            json!({
                "model": cfg.model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            }),
        ),
    };
    let v = send(cfg, "POST", path, Some(&body))?;
    let latency_ms = t.elapsed().as_millis() as u64;
    let (text, input, output) = match cfg.kind {
        ProviderKind::OpenaiCompatible => (
            v["choices"][0]["message"]["content"].as_str(),
            v["usage"]["prompt_tokens"].as_u64(),
            v["usage"]["completion_tokens"].as_u64(),
        ),
        ProviderKind::Anthropic => (
            v["content"]
                .as_array()
                .and_then(|c| c.iter().find(|b| b["type"] == "text"))
                .and_then(|b| b["text"].as_str()),
            v["usage"]["input_tokens"].as_u64(),
            v["usage"]["output_tokens"].as_u64(),
        ),
    };
    let text = text
        .ok_or_else(|| LlmError::Protocol("no text in response".into()))?
        .to_string();
    Ok(Completion {
        text,
        input_tokens: input,
        output_tokens: output,
        latency_ms,
    })
}

/// Probe what this endpoint really supports. Costs at most two tiny completions.
pub fn probe(cfg: &ProviderConfig) -> Capabilities {
    let mut caps = Capabilities {
        reachable: false,
        model_listed: None,
        completion: false,
        structured_json: false,
        errors: vec![],
    };
    let models_path = match cfg.kind {
        ProviderKind::OpenaiCompatible => "models",
        ProviderKind::Anthropic => "v1/models",
    };
    match send(cfg, "GET", models_path, None) {
        Ok(v) => {
            caps.reachable = true;
            let ids: Vec<&str> = v["data"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| m["id"].as_str())
                .collect();
            caps.model_listed = Some(ids.contains(&cfg.model.as_str()));
        }
        Err(LlmError::Http { status: 404, .. }) => caps.reachable = true,
        Err(e) => caps.errors.push(format!("models: {e}")),
    }
    match complete(cfg, "Reply with the single word: pong", "ping", 16, false) {
        Ok(c) => {
            caps.reachable = true;
            caps.completion = c.text.to_ascii_lowercase().contains("pong");
        }
        Err(e) => caps.errors.push(format!("completion: {e}")),
    }
    if caps.completion {
        match complete(
            cfg,
            "Answer only with a JSON object.",
            "Return {\"ok\": true}",
            32,
            cfg.kind == ProviderKind::OpenaiCompatible,
        ) {
            Ok(c) => caps.structured_json = extract_json(&c.text).is_some_and(|v| v["ok"] == true),
            Err(e) => caps.errors.push(format!("json: {e}")),
        }
    }
    caps
}

/// First JSON object in a model reply (models often wrap JSON in prose or code fences).
pub fn extract_json(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    serde_json::from_str(text.get(start..=end)?).ok()
}

#[cfg(test)]
mod tests;
