//! Deliberately small adapter registry. Schemas and dispatch share the same typed inputs.
use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Serialize)]
pub struct Capability {
    #[serde(skip)]
    validate: fn(Value) -> Result<()>,
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub version: u32,
    pub permission: &'static str,
    pub read_only: bool,
    pub asynchronous: bool,
    pub categories: Vec<&'static str>,
    pub input_schema: Value,
    pub output_schema: Value,
    pub constraints: Value,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    pub path: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Run {
    pub path: String,
    /// Native test timeout, 1..=1800 seconds. Test execution uses existing jobs.
    pub timeout_secs: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Id {
    pub run_id: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stored {
    pub path: String,
    pub run_id: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Artifact {
    pub path: String,
    pub run_id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunReply {
    pub run_id: String,
    pub state: String,
    pub ecosystems: Option<Vec<String>>,
}
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StatusReply {
    pub run_id: String,
    pub state: String,
    pub cancel_requested: bool,
}
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelReply {
    pub run_id: String,
    pub cancel_requested: bool,
}
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactList {
    pub artifacts: Vec<String>,
}
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactText {
    pub text: String,
}

fn validate<T: serde::de::DeserializeOwned>(value: Value) -> Result<()> {
    serde_json::from_value::<T>(value)?;
    Ok(())
}

fn entry<T: JsonSchema, O: JsonSchema + serde::de::DeserializeOwned>(
    id: &'static str,
    description: &'static str,
    read_only: bool,
    asynchronous: bool,
) -> Capability {
    Capability {
        validate: validate::<O>,
        id,
        name: id,
        description,
        version: 1,
        permission: if read_only { "read" } else { id },
        read_only,
        asynchronous,
        categories: vec![
            "tests", "rust", "cargo", "python", "pytest", "node", "dotnet",
        ],
        input_schema: serde_json::to_value(schemars::schema_for!(T)).expect("schema serialization"),
        output_schema: serde_json::to_value(schemars::schema_for!(O))
            .expect("schema serialization"),
        constraints: json!({"max_result_bytes":crate::FRAME_BYTES - 1024,
            "native_timeout_secs":if id == "tests.run" {Some(1800)} else {None},
            "artifact_tail_bytes":if id == "artifacts.read" {Some(8192)} else {None}}),
    }
}

pub fn registry() -> Vec<Capability> {
    vec![
        entry::<Project, qualyx_core::ProjectProfile>(
            "tests.discover",
            "Discover project ecosystems and native test runners (not individual test cases).",
            true,
            false,
        ),
        entry::<Run, RunReply>(
            "tests.run",
            "Submit an asynchronous native test job; requires operator approval grant.",
            false,
            true,
        ),
        entry::<Id, StatusReply>(
            "tests.status",
            "Poll a job submitted through this host or the shared team queue.",
            true,
            false,
        ),
        entry::<Stored, qualyx_core::RunManifest>(
            "tests.results",
            "Read a persisted run manifest including failures and artifacts.",
            true,
            false,
        ),
        entry::<Id, CancelReply>(
            "tests.cancel",
            "Request cancellation of an existing job; requires operator approval grant.",
            false,
            false,
        ),
        entry::<Stored, ArtifactList>(
            "artifacts.list",
            "List native evidence names from a stored run.",
            true,
            false,
        ),
        entry::<Artifact, ArtifactText>(
            "artifacts.read",
            "Read up to 8192 bytes of a known, integrity-checked artifact.",
            true,
            false,
        ),
    ]
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
    /// Literal case-insensitive match across identifiers, descriptions, categories and schemas.
    #[serde(default)]
    pub query: String,
    pub permission: Option<String>,
    pub read_only: Option<bool>,
    /// At most 7 entries per page.
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub fn search(args: &Search) -> Result<Value> {
    if args.query.len() > 256 || args.permission.as_ref().is_some_and(|s| s.len() > 64) {
        bail!("search input limit exceeded");
    }
    let query = args.query.to_lowercase();
    let matches: Vec<_> = registry()
        .into_iter()
        .filter(|c| {
            args.permission.as_deref().is_none_or(|p| p == c.permission)
                && args.read_only.is_none_or(|r| r == c.read_only)
                && serde_json::to_string(c)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
        })
        .collect();
    let total = matches.len();
    let offset = args.offset.unwrap_or(0).min(total);
    let end = offset
        .saturating_add(args.limit.unwrap_or(3).clamp(1, 7))
        .min(total);
    let result = json!({"capabilities": &matches[offset..end], "total":total,
        "next_offset": if end < total { Some(end) } else { None },
        "truncated":false});
    if serde_json::to_vec(&result)?.len() > crate::FRAME_BYTES - 1024 {
        bail!("search output limit exceeded; request a smaller page");
    }
    Ok(result)
}

pub(crate) fn validate_output(capability: &Capability, result: &Value) -> Result<()> {
    (capability.validate)(result.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_search_and_schemas() {
        let entries = registry();
        let mut ids: Vec<_> = entries.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), entries.len());
        assert_eq!(
            search(&Search {
                query: "pytest".into(),
                ..Default::default()
            })
            .unwrap()["total"],
            7
        );
        assert_eq!(
            search(&Search {
                permission: Some("tests.run".into()),
                ..Default::default()
            })
            .unwrap()["total"],
            1
        );
        assert!(serde_json::from_value::<Run>(json!({"path":".","approval":true})).is_err());
        assert!(validate_output(&entries[1], &json!({"run_id":"x"})).is_err());
        assert!(validate_output(&entries[1], &json!({"run_id":1,"state":"queued"})).is_err());
        assert!(validate_output(&entries[1], &json!({"run_id":"x","state":"queued"})).is_ok());
        assert!(validate_output(&entries[6], &json!({"text":false})).is_err());
        assert!(
            search(&Search {
                query: "x".repeat(257),
                ..Default::default()
            })
            .is_err()
        );
    }
}
