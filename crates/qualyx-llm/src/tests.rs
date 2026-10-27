//! Contract tests against an in-process HTTP server that speaks each provider's wire format.
//! The network boundary is the only thing replaced; request shapes are asserted exactly.

use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

#[derive(Debug)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Value,
}

/// Serve `n` requests with `reply(path) -> (status, body)` and report what was received.
fn server(n: usize, reply: fn(&str, &Value) -> (u16, String)) -> (String, mpsc::Receiver<Seen>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for _ in 0..n {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut parts = line.split_whitespace();
            let (method, path) = (
                parts.next().unwrap().to_string(),
                parts.next().unwrap().to_string(),
            );
            let mut headers = Vec::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                if h.trim().is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap();
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
            let len: usize = headers
                .iter()
                .find(|(k, _)| k == "content-length")
                .map(|(_, v)| v.parse().unwrap())
                .unwrap_or(0);
            let mut buf = vec![0; len];
            r.read_exact(&mut buf).unwrap();
            let body: Value = serde_json::from_slice(&buf).unwrap_or(Value::Null);
            let (status, out) = reply(&path, &body);
            let mut s = s;
            write!(s, "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{out}", out.len()).unwrap();
            tx.send(Seen {
                method,
                path,
                headers,
                body,
            })
            .unwrap();
        }
    });
    (base, rx)
}

fn header<'a>(s: &'a Seen, k: &str) -> Option<&'a str> {
    s.headers
        .iter()
        .find(|(h, _)| h == k)
        .map(|(_, v)| v.as_str())
}

fn cfg(kind: ProviderKind, base: String, env: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        kind,
        base_url: base,
        model: "test-model".into(),
        api_key_env: env.map(String::from),
        timeout_secs: 10,
    }
}

#[test]
fn openai_compatible_request_and_response() {
    let (base, rx) = server(1, |_, _| {
        (200, json!({"choices": [{"message": {"content": "hello"}}], "usage": {"prompt_tokens": 7, "completion_tokens": 2}}).to_string())
    });
    // PATH is always set, so it stands in for a key variable without mutating the environment.
    let c = complete(
        &cfg(ProviderKind::OpenaiCompatible, base, Some("PATH")),
        "sys",
        "hi",
        64,
        true,
    )
    .unwrap();
    assert_eq!(c.text, "hello");
    assert_eq!((c.input_tokens, c.output_tokens), (Some(7), Some(2)));
    let s = rx.recv().unwrap();
    assert_eq!(
        (s.method.as_str(), s.path.as_str()),
        ("POST", "/chat/completions")
    );
    assert!(header(&s, "authorization").unwrap().starts_with("Bearer "));
    assert_eq!(s.body["model"], "test-model");
    assert_eq!(s.body["messages"][0]["role"], "system");
    assert_eq!(s.body["messages"][1]["content"], "hi");
    assert_eq!(s.body["response_format"]["type"], "json_object");
}

#[test]
fn anthropic_request_and_response() {
    let (base, rx) = server(1, |_, _| {
        (200, json!({"content": [{"type": "text", "text": "hola"}], "usage": {"input_tokens": 5, "output_tokens": 1}}).to_string())
    });
    let c = complete(
        &cfg(ProviderKind::Anthropic, base, Some("PATH")),
        "sys",
        "hi",
        64,
        false,
    )
    .unwrap();
    assert_eq!(c.text, "hola");
    let s = rx.recv().unwrap();
    assert_eq!(s.path, "/v1/messages");
    assert!(header(&s, "x-api-key").is_some());
    assert_eq!(header(&s, "anthropic-version"), Some("2023-06-01"));
    assert_eq!(s.body["system"], "sys");
    assert_eq!(s.body["messages"][0]["role"], "user");
}

#[test]
fn local_server_needs_no_key_and_sends_none() {
    let (base, rx) = server(1, |_, _| {
        (
            200,
            json!({"choices": [{"message": {"content": "x"}}]}).to_string(),
        )
    });
    complete(
        &cfg(ProviderKind::OpenaiCompatible, base, None),
        "s",
        "u",
        8,
        false,
    )
    .unwrap();
    assert!(header(&rx.recv().unwrap(), "authorization").is_none());
}

#[test]
fn missing_key_fails_before_any_request() {
    let c = cfg(
        ProviderKind::OpenaiCompatible,
        "http://127.0.0.1:9".into(),
        Some("QUALYX_TEST_KEY_THAT_DOES_NOT_EXIST"),
    );
    let e = complete(&c, "s", "u", 8, false).unwrap_err();
    assert!(matches!(e, LlmError::MissingKey(_)));
}

#[test]
fn http_errors_never_echo_the_key() {
    // The fake provider reflects the Authorization header back, as some proxies do.
    let (base, _rx) = server(1, |_, _| (401, "{\"error\":\"bad key\"}".into()));
    let path_value = std::env::var("PATH").unwrap();
    let e = complete(
        &cfg(ProviderKind::OpenaiCompatible, base, Some("PATH")),
        "s",
        "u",
        8,
        false,
    )
    .unwrap_err();
    let msg = e.to_string();
    assert!(msg.contains("401"));
    assert!(!msg.contains(&path_value));
}

#[test]
fn probe_reports_real_capabilities() {
    let (base, _rx) = server(3, |path, body| match path {
        "/models" => (
            200,
            json!({"data": [{"id": "test-model"}, {"id": "other"}]}).to_string(),
        ),
        _ if body.get("response_format").is_some() => (
            200,
            json!({"choices": [{"message": {"content": "```json\n{\"ok\": true}\n```"}}]})
                .to_string(),
        ),
        _ => (
            200,
            json!({"choices": [{"message": {"content": "Pong."}}]}).to_string(),
        ),
    });
    let caps = probe(&cfg(ProviderKind::OpenaiCompatible, base, None));
    assert!(
        caps.reachable && caps.completion && caps.structured_json,
        "{caps:?}"
    );
    assert_eq!(caps.model_listed, Some(true));
}

#[test]
fn probe_of_dead_endpoint_is_honest() {
    let caps = probe(&cfg(
        ProviderKind::OpenaiCompatible,
        "http://127.0.0.1:9".into(),
        None,
    ));
    assert!(!caps.reachable && !caps.completion && !caps.structured_json);
    assert_eq!(caps.errors.len(), 2);
}

#[test]
fn presets_require_a_model_and_know_their_key_env() {
    let d = preset("deepseek", "my-model").unwrap();
    assert_eq!(d.kind, ProviderKind::OpenaiCompatible);
    assert_eq!(d.api_key_env.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert_eq!(d.model, "my-model");
    assert!(preset("ollama", "m").unwrap().api_key_env.is_none());
    assert!(preset("unknown", "m").is_none());
}

#[test]
fn extracts_json_from_prose() {
    assert_eq!(
        extract_json("Sure! ```json\n{\"a\": 1}\n``` done").unwrap()["a"],
        1
    );
    assert!(extract_json("no json").is_none());
}
