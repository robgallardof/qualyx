//! Performance engineering (M5): k6 workloads with authorization, ceilings and abort
//! conditions, raw per-request latencies, and statistically defended regression checks.
//!
//! k6 is AGPL-3.0 and is only ever run as an external process (integration matrix).

pub mod stats;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Workload file (TOML). Every field except `name` and `target` has a conservative default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workload {
    pub name: String,
    pub target: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_vus")]
    pub vus: u32,
    #[serde(default = "default_duration")]
    pub duration_secs: u32,
    /// Fixed arrival rate. Required for targets that aren't loopback, so traffic is bounded.
    #[serde(default)]
    pub rate_per_sec: Option<u32>,
    #[serde(default = "default_warmup")]
    pub warmup_secs: u32,
    #[serde(default)]
    pub think_time_ms: u64,
    #[serde(default)]
    pub thresholds: Thresholds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    /// Fail the run when p95 latency (ms) exceeds this.
    #[serde(default)]
    pub p95_ms: Option<f64>,
    /// Abort the run as soon as the failed-request rate exceeds this (0..1).
    #[serde(default = "default_error_rate")]
    pub abort_error_rate: f64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            p95_ms: None,
            abort_error_rate: default_error_rate(),
        }
    }
}

fn default_method() -> String {
    "GET".into()
}
fn default_vus() -> u32 {
    5
}
fn default_duration() -> u32 {
    10
}
fn default_warmup() -> u32 {
    2
}
fn default_error_rate() -> f64 {
    0.10
}

/// Safety ceilings (`[performance]` in `.qualyx/config.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub max_vus: u32,
    pub max_duration_secs: u32,
    pub max_rps: u32,
    /// Origins (`https://staging.example.com`) explicitly authorized besides loopback.
    pub allowed_targets: Vec<String>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_vus: 50,
            max_duration_secs: 300,
            max_rps: 200,
            allowed_targets: Vec::new(),
        }
    }
}

fn host_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    Some(if host.starts_with('[') {
        host.split(']').next()?.trim_start_matches('[')
    } else {
        host.split(':').next()?
    })
}

pub fn is_loopback(url: &str) -> bool {
    matches!(host_of(url), Some("127.0.0.1" | "localhost" | "::1"))
}

/// Refuse anything that could hurt a system nobody authorized (threat model T8).
pub fn authorize(w: &Workload, l: &Limits) -> Result<(), String> {
    if !(w.target.starts_with("http://") || w.target.starts_with("https://")) {
        return Err(format!("target must be http(s): {}", w.target));
    }
    let loopback = is_loopback(&w.target);
    let listed = l.allowed_targets.iter().any(|a| {
        let a = a.trim_end_matches('/');
        w.target == a || w.target.starts_with(&format!("{a}/"))
    });
    if !loopback && !listed {
        return Err(format!(
            "{} is not loopback and not in [performance] allowed_targets",
            w.target
        ));
    }
    if !loopback && w.rate_per_sec.is_none() {
        return Err("non-loopback targets need rate_per_sec so traffic is bounded".into());
    }
    if w.vus == 0 || w.vus > l.max_vus {
        return Err(format!("vus {} outside 1..={}", w.vus, l.max_vus));
    }
    if w.duration_secs == 0 || w.duration_secs + w.warmup_secs > l.max_duration_secs {
        return Err(format!(
            "duration + warmup must be 1..={}s",
            l.max_duration_secs
        ));
    }
    if let Some(r) = w.rate_per_sec
        && (r == 0 || r > l.max_rps)
    {
        return Err(format!("rate_per_sec {r} outside 1..={}", l.max_rps));
    }
    if !(0.0..=1.0).contains(&w.thresholds.abort_error_rate) {
        return Err("abort_error_rate must be within 0..1".into());
    }
    Ok(())
}

fn js(v: &impl Serialize) -> String {
    serde_json::to_string(v).expect("serializable")
}

/// The k6 script: a warm-up scenario, then the measured `main` scenario. Thresholds and the
/// abort condition apply to `main` only.
pub fn script(w: &Workload) -> String {
    let main = match w.rate_per_sec {
        Some(rate) => format!(
            "{{ executor: 'constant-arrival-rate', rate: {rate}, timeUnit: '1s', duration: '{}s', preAllocatedVUs: {}, maxVUs: {}, startTime: '{}s', tags: {{ phase: 'main' }} }}",
            w.duration_secs, w.vus, w.vus, w.warmup_secs
        ),
        None => format!(
            "{{ executor: 'constant-vus', vus: {}, duration: '{}s', startTime: '{}s', tags: {{ phase: 'main' }} }}",
            w.vus, w.duration_secs, w.warmup_secs
        ),
    };
    let warmup = if w.warmup_secs > 0 {
        format!(
            "warmup: {{ executor: 'constant-vus', vus: {}, duration: '{}s', tags: {{ phase: 'warmup' }} }},",
            w.vus.min(2),
            w.warmup_secs
        )
    } else {
        String::new()
    };
    let p95 = w
        .thresholds
        .p95_ms
        .map(|p| format!("'http_req_duration{{phase:main}}': ['p(95)<{p}'],"))
        .unwrap_or_default();
    format!(
        r#"// Generated by Qualyx. Do not edit; change the workload file instead.
import http from 'k6/http';
import {{ sleep }} from 'k6';

export const options = {{
  scenarios: {{
    {warmup}
    main: {main},
  }},
  thresholds: {{
    'http_req_failed{{phase:main}}': [{{ threshold: 'rate<{err}', abortOnFail: true, delayAbortEval: '2s' }}],
    {p95}
  }},
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
}};

const TARGET = {target};
const METHOD = {method};
const BODY = {body};
const PARAMS = {{ headers: {headers} }};

export default function () {{
  http.request(METHOD, TARGET, BODY, PARAMS);
  {think}
}}

export function handleSummary(data) {{
  return {{ [__ENV.QUALYX_SUMMARY]: JSON.stringify(data) }};
}}
"#,
        err = w.thresholds.abort_error_rate,
        target = js(&w.target),
        method = js(&w.method),
        body = w.body.as_ref().map(js).unwrap_or_else(|| "null".into()),
        headers = js(&w.headers),
        think = if w.think_time_ms > 0 {
            format!("sleep({});", w.think_time_ms as f64 / 1000.0)
        } else {
            String::new()
        },
    )
}

/// One k6 execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub requests: u64,
    pub rps: f64,
    pub error_rate: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    pub thresholds_failed: Vec<String>,
    pub exit_code: Option<i32>,
    /// File (in the result dir) with one measured latency in ms per line.
    pub latencies_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfRun {
    pub run_id: String,
    pub workload: Workload,
    pub k6_version: Option<String>,
    pub os: String,
    pub cpus: usize,
    pub samples: Vec<Sample>,
    /// Per-repeat p95 spread; above `NOISY_CV` the run is flagged as unreliable.
    pub p95_cv: Option<f64>,
    pub noisy: bool,
}

pub const NOISY_CV: f64 = 0.25;

/// Extract `main`-phase request durations from k6's `--out json` stream.
pub fn parse_latencies(raw: &str) -> Vec<f64> {
    raw.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| {
            v["type"] == "Point"
                && v["metric"] == "http_req_duration"
                && v["data"]["tags"]["phase"] == "main"
        })
        .filter_map(|v| v["data"]["value"].as_f64())
        .collect()
}

fn summary_value(s: &Value, metric: &str, key: &str) -> f64 {
    s["metrics"][metric]["values"][key].as_f64().unwrap_or(0.0)
}

fn failed_thresholds(s: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(metrics) = s["metrics"].as_object() {
        for (name, m) in metrics {
            if let Some(th) = m["thresholds"].as_object() {
                for (expr, r) in th {
                    if r["ok"] == false {
                        out.push(format!("{name}: {expr}"));
                    }
                }
            }
        }
    }
    out
}

/// Run the workload `repeat` times with k6, keeping script, summaries and latencies in `dir`.
pub fn run(
    w: &Workload,
    k6: &Path,
    dir: &Path,
    run_id: &str,
    repeat: u32,
) -> Result<PerfRun, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let script_path = dir.join("script.js");
    fs::write(&script_path, script(w)).map_err(|e| e.to_string())?;
    let env = qualyx_runners::process::child_env(&[]);
    let budget = Duration::from_secs(u64::from(w.duration_secs + w.warmup_secs) + 60);
    let mut samples = Vec::new();
    for i in 1..=repeat.max(1) {
        let summary = dir.join(format!("summary-{i}.json"));
        let raw = dir.join(format!("raw-{i}.json"));
        let args: Vec<String> = vec![
            "run".into(),
            "--quiet".into(),
            "--no-usage-report".into(),
            format!("--out=json={}", raw.display()),
            script_path.display().to_string(),
        ];
        let mut cmd = qualyx_runners::process::command(k6, &args, dir, &env);
        cmd.env("QUALYX_SUMMARY", &summary);
        let outcome = qualyx_runners::process::run(
            cmd,
            &dir.join(format!("k6-{i}.log")),
            budget,
            &Default::default(),
        )
        .map_err(|e| e.to_string())?;
        let s: Value = fs::read(&summary)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| {
                format!(
                    "k6 run {i} produced no summary (exit {:?}); see k6-{i}.log",
                    outcome.exit_code
                )
            })?;
        let lat = parse_latencies(&fs::read_to_string(&raw).unwrap_or_default());
        let lat_file = format!("latencies-{i}.txt");
        fs::write(
            dir.join(&lat_file),
            lat.iter()
                .map(|v| format!("{v:.3}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .map_err(|e| e.to_string())?;
        let _ = fs::remove_file(&raw); // large; latencies + summary keep the evidence
        let pct = |q: f64| stats::percentile(&lat, q).unwrap_or(0.0);
        samples.push(Sample {
            requests: lat.len() as u64,
            rps: summary_value(&s, "http_reqs", "rate"),
            error_rate: summary_value(&s, "http_req_failed", "rate"),
            p50_ms: pct(50.0),
            p90_ms: pct(90.0),
            p95_ms: pct(95.0),
            p99_ms: pct(99.0),
            max_ms: lat.iter().copied().fold(0.0, f64::max),
            thresholds_failed: failed_thresholds(&s),
            exit_code: outcome.exit_code,
            latencies_file: lat_file,
        });
    }
    let p95s: Vec<f64> = samples.iter().map(|s| s.p95_ms).collect();
    let p95_cv = stats::coefficient_of_variation(&p95s);
    let k6_version = qualyx_runners::process::version(k6, &["version"], dir);
    let r = PerfRun {
        run_id: run_id.into(),
        workload: w.clone(),
        k6_version,
        os: std::env::consts::OS.into(),
        cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        noisy: p95_cv.is_some_and(|c| c > NOISY_CV),
        p95_cv,
        samples,
    };
    fs::write(
        dir.join("result.json"),
        serde_json::to_vec_pretty(&r).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(r)
}

pub fn load_latencies(dir: &Path, run: &PerfRun) -> Vec<f64> {
    run.samples
        .iter()
        .flat_map(|s| {
            fs::read_to_string(dir.join(&s.latencies_file))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect::<Vec<f64>>()
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    pub baseline_run: String,
    pub candidate_run: String,
    pub baseline_median_ms: f64,
    pub candidate_median_ms: f64,
    pub baseline_p95_ms: f64,
    pub candidate_p95_ms: f64,
    pub median_ratio: f64,
    pub p_value: f64,
    /// "regression", "no-regression" or "inconclusive" (noisy or too few samples).
    pub verdict: String,
    pub reasons: Vec<String>,
}

/// Minimum requests per side before a verdict is given.
pub const MIN_SAMPLES: usize = 100;

/// Regression only when the slowdown is both significant (p < alpha) and material
/// (median ratio > 1 + min_effect). A single run is never enough.
pub fn compare(
    base: &PerfRun,
    base_lat: &[f64],
    cand: &PerfRun,
    cand_lat: &[f64],
    alpha: f64,
    min_effect: f64,
) -> Comparison {
    let bm = stats::median(base_lat).unwrap_or(0.0);
    let cm = stats::median(cand_lat).unwrap_or(0.0);
    let p = stats::mann_whitney_greater(base_lat, cand_lat).unwrap_or(1.0);
    let ratio = if bm > 0.0 { cm / bm } else { 0.0 };
    let mut reasons = Vec::new();
    let verdict = if base_lat.len() < MIN_SAMPLES || cand_lat.len() < MIN_SAMPLES {
        reasons.push(format!(
            "need at least {MIN_SAMPLES} requests per side ({} vs {})",
            base_lat.len(),
            cand_lat.len()
        ));
        "inconclusive"
    } else if base.samples.len() < 2 || cand.samples.len() < 2 {
        reasons.push("need at least 2 repeats per side to estimate noise".into());
        "inconclusive"
    } else if base.noisy || cand.noisy {
        reasons.push(format!(
            "p95 varies more than {:.0}% between repeats (noisy environment)",
            NOISY_CV * 100.0
        ));
        "inconclusive"
    } else if p < alpha && ratio > 1.0 + min_effect {
        reasons.push(format!(
            "median {bm:.2} ms -> {cm:.2} ms (x{ratio:.2}), one-sided Mann-Whitney p = {p:.2e}"
        ));
        "regression"
    } else {
        reasons.push(format!(
            "median x{ratio:.2}, p = {p:.3}: below the alpha {alpha} / +{:.0}% bar",
            min_effect * 100.0
        ));
        "no-regression"
    };
    Comparison {
        baseline_run: base.run_id.clone(),
        candidate_run: cand.run_id.clone(),
        baseline_median_ms: bm,
        candidate_median_ms: cm,
        baseline_p95_ms: stats::percentile(base_lat, 95.0).unwrap_or(0.0),
        candidate_p95_ms: stats::percentile(cand_lat, 95.0).unwrap_or(0.0),
        median_ratio: ratio,
        p_value: p,
        verdict: verdict.into(),
        reasons,
    }
}

/// `.qualyx/perf/<workload>/<run-id>/`
pub fn run_dir(project: &Path, workload: &str, run_id: &str) -> PathBuf {
    project
        .join(".qualyx")
        .join("perf")
        .join(sanitize(workload))
        .join(run_id)
}

pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wl(target: &str) -> Workload {
        toml::from_str(&format!("name = \"w\"\ntarget = \"{target}\"\n")).unwrap()
    }

    #[test]
    fn loopback_is_allowed_by_default() {
        assert!(authorize(&wl("http://127.0.0.1:8080/api"), &Limits::default()).is_ok());
        assert!(authorize(&wl("http://localhost/"), &Limits::default()).is_ok());
        assert!(authorize(&wl("http://[::1]:9/"), &Limits::default()).is_ok());
    }

    #[test]
    fn unsafe_targets_are_blocked() {
        let l = Limits::default();
        for t in [
            "https://example.com/",
            "http://169.254.169.254/latest",
            "http://127.0.0.1.evil.com/",
            "http://user@evil.com/",
            "file:///etc/passwd",
        ] {
            assert!(authorize(&wl(t), &l).is_err(), "{t}");
        }
    }

    #[test]
    fn allowed_targets_still_need_bounded_traffic() {
        let l = Limits {
            allowed_targets: vec!["https://staging.example.com".into()],
            ..Limits::default()
        };
        let mut w = wl("https://staging.example.com/health");
        assert!(authorize(&w, &l).unwrap_err().contains("rate_per_sec"));
        w.rate_per_sec = Some(20);
        assert!(authorize(&w, &l).is_ok());
        w.target = "https://staging.example.com.evil.io/".into();
        assert!(authorize(&w, &l).is_err());
    }

    #[test]
    fn ceilings_are_enforced() {
        let l = Limits::default();
        let mut w = wl("http://127.0.0.1/");
        w.vus = 500;
        assert!(authorize(&w, &l).is_err());
        w.vus = 5;
        w.duration_secs = 3600;
        assert!(authorize(&w, &l).is_err());
        w.duration_secs = 10;
        w.rate_per_sec = Some(10_000);
        assert!(authorize(&w, &l).is_err());
    }

    #[test]
    fn workload_files_are_strict() {
        assert!(
            toml::from_str::<Workload>("name='w'\ntarget='http://127.0.0.1/'\nvu=5\n").is_err()
        );
    }

    #[test]
    fn script_escapes_inputs_and_scopes_thresholds() {
        let mut w = wl("http://127.0.0.1/a'b");
        w.thresholds.p95_ms = Some(250.0);
        let s = script(&w);
        assert!(
            s.contains(r#"const TARGET = "http://127.0.0.1/a'b";"#),
            "{s}"
        );
        assert!(s.contains("'http_req_failed{phase:main}'"));
        assert!(s.contains("'http_req_duration{phase:main}': ['p(95)<250']"));
        assert!(s.contains("abortOnFail: true"));
    }

    #[test]
    fn latencies_only_from_main_phase() {
        let raw = r#"{"type":"Point","metric":"http_req_duration","data":{"value":5.5,"tags":{"phase":"warmup"}}}
{"type":"Point","metric":"http_req_duration","data":{"value":7.25,"tags":{"phase":"main"}}}
{"type":"Point","metric":"http_req_waiting","data":{"value":1,"tags":{"phase":"main"}}}
{"type":"Metric","metric":"http_req_duration","data":{}}"#;
        assert_eq!(parse_latencies(raw), vec![7.25]);
    }
}
