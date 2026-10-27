//! HTTP API for team mode (M7): the same services as the CLI and MCP server, behind a bearer
//! token, with a minimal read-only dashboard.
//!
//! - A token (`QUALYX_API_TOKEN`) is mandatory, including on loopback.
//! - Every project path must sit under a configured root.
//! - Recovery is an explicit offline command; another process may still own a run.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::extract::{DefaultBodyLimit, Path as AxPath, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use qualyx_app::TestRequest;
use qualyx_app::jobs::{Jobs, Roots};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

/// Maximum simultaneous runs in one API process.
pub const MAX_CONCURRENT_RUNS: usize = 4;
const MAX_ARTIFACT_BYTES: usize = 200_000;

#[derive(Clone)]
struct AppState {
    roots: Roots,
    jobs: Jobs,
    token: Option<String>,
    queue_url: Option<String>,
}

/// Listener address, authorized project roots and bearer credential.
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub roots: Vec<PathBuf>,
    pub token: Option<String>,
    /// Use durable PostgreSQL dispatch instead of local execution.
    pub team: bool,
}

fn error(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({"error": msg.into()}))).into_response()
}

async fn auth(
    State(st): State<AppState>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    if headers.contains_key(header::ORIGIN) {
        return error(
            StatusCode::FORBIDDEN,
            "browser Origin requests are not supported",
        );
    }
    if let Some(expected) = &st.token {
        let given = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if !bool::from(given.as_bytes().ct_eq(expected.as_bytes())) {
            return error(StatusCode::UNAUTHORIZED, "missing or invalid bearer token");
        }
    }
    next.run(req).await
}

#[derive(Deserialize)]
struct StartRun {
    project: String,
    changed_since: Option<String>,
    jobs: Option<usize>,
    timeout_secs: Option<u64>,
}

async fn start_run(State(st): State<AppState>, Json(body): Json<StartRun>) -> Response {
    let project = match st.roots.check(&body.project) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::FORBIDDEN, e),
    };
    let req = TestRequest {
        timeout_secs: body.timeout_secs,
        isolation: None,
        changed_since: body.changed_since,
        jobs: body.jobs.unwrap_or(1).clamp(1, 8),
        compose: None,
    };
    if let Some(url) = st.queue_url {
        return match tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
            Ok(qualyx_store::queue::Queue::connect(&url)?
                .enqueue(&project, &serde_json::to_string(&req)?)?)
        })
        .await
        {
            Ok(Ok(id)) => (
                StatusCode::ACCEPTED,
                Json(json!({"run_id": id, "state": "queued"})),
            )
                .into_response(),
            Ok(Err(e)) => error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }
    let jobs = st.jobs.clone();
    match tokio::task::spawn_blocking(move || jobs.start(&project, &req)).await {
        Ok(Ok((run_id, ecosystems))) => (
            StatusCode::ACCEPTED,
            Json(json!({"run_id": run_id, "state": "running", "ecosystems": ecosystems})),
        )
            .into_response(),
        Ok(Err(e)) => error(StatusCode::CONFLICT, format!("{e:#}")),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct ProjectQuery {
    project: Option<String>,
    limit: Option<u32>,
}

async fn list_runs(State(st): State<AppState>, Query(q): Query<ProjectQuery>) -> Response {
    if let Some(url) = st.queue_url.clone() {
        let roots = st.roots.clone();
        return match tokio::task::spawn_blocking(move || -> anyhow::Result<Value> {
            let project = q
                .project
                .map(|p| roots.check(&p).map_err(anyhow::Error::msg))
                .transpose()?;
            let jobs: Vec<_> = qualyx_store::queue::Queue::connect(&url)?
                .list()?
                .into_iter()
                .filter(|j| {
                    roots.check(&j.project.to_string_lossy()).is_ok()
                        && project.as_ref().is_none_or(|p| p == &j.project)
                })
                .take(q.limit.unwrap_or(20).min(200) as usize)
                .collect();
            let active: Vec<_> = jobs
                .iter()
                .filter(|j| j.state == "queued" || j.state == "running")
                .collect();
            Ok(json!({"in_flight": active, "jobs": jobs}))
        })
        .await
        {
            Ok(Ok(v)) => Json(v).into_response(),
            Ok(Err(e)) => error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
            Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        };
    }
    let in_flight: Vec<Value> = st
        .jobs
        .list()
        .into_iter()
        .filter(|j| j.state == "running")
        .map(|j| json!({"run_id": j.run_id, "project": j.project, "state": j.state}))
        .collect();
    let Some(project) = q.project else {
        return Json(json!({"in_flight": in_flight})).into_response();
    };
    let project = match st.roots.check(&project) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::FORBIDDEN, e),
    };
    let limit = q.limit.unwrap_or(20).min(200);
    match tokio::task::spawn_blocking(move || qualyx_app::list_runs(&project, limit)).await {
        Ok(Ok(runs)) => Json(json!({
            "in_flight": in_flight,
            "runs": runs.iter().map(|r| json!({"run_id": r.id, "status": r.status.as_str(), "total": r.total, "passed": r.passed, "failed": r.failed, "started_at": r.started_at})).collect::<Vec<_>>(),
        }))
        .into_response(),
        Ok(Err(e)) => error(StatusCode::NOT_FOUND, format!("{e:#}")),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_run(
    State(st): State<AppState>,
    AxPath(id): AxPath<String>,
    Query(q): Query<ProjectQuery>,
) -> Response {
    if let Some(url) = st.queue_url.clone() {
        return team_job(url, st.roots, id, false).await;
    }
    if let Some(j) = st.jobs.get(&id) {
        return Json(serde_json::to_value(j).unwrap_or_default()).into_response();
    }
    let Some(project) = q.project else {
        return error(
            StatusCode::NOT_FOUND,
            "unknown in-flight run; pass ?project= for stored runs",
        );
    };
    let project = match st.roots.check(&project) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::FORBIDDEN, e),
    };
    match tokio::task::spawn_blocking(move || qualyx_app::load_run(&project, Some(&id))).await {
        Ok(Ok((m, _))) => Json(json!({"state": "finished", "manifest": m})).into_response(),
        Ok(Err(e)) => error(StatusCode::NOT_FOUND, format!("{e:#}")),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn cancel_run(State(st): State<AppState>, AxPath(id): AxPath<String>) -> Response {
    if let Some(url) = st.queue_url.clone() {
        return team_job(url, st.roots, id, true).await;
    }
    match st.jobs.cancel(&id) {
        Some(true) => (
            StatusCode::ACCEPTED,
            Json(json!({"run_id": id, "state": "cancelling"})),
        )
            .into_response(),
        Some(false) => Json(json!({"run_id": id, "state": "already-finished"})).into_response(),
        None => error(StatusCode::NOT_FOUND, "unknown run"),
    }
}

async fn team_job(url: String, roots: Roots, id: String, cancel: bool) -> Response {
    match tokio::task::spawn_blocking(move || -> anyhow::Result<Response> {
        let mut queue = qualyx_store::queue::Queue::connect(&url)?;
        let Some(job) = queue.get(&id)? else {
            return Ok(error(StatusCode::NOT_FOUND, "unknown run"));
        };
        if roots.check(&job.project.to_string_lossy()).is_err() {
            return Ok(error(StatusCode::NOT_FOUND, "unknown run"));
        }
        if cancel {
            let requested = queue.cancel(&id)?.unwrap_or(false);
            Ok((
                if requested {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::OK
                },
                Json(json!({"run_id": id, "cancel_requested": requested})),
            )
                .into_response())
        } else {
            Ok(Json(serde_json::to_value(job)?).into_response())
        }
    })
    .await
    {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => error(StatusCode::SERVICE_UNAVAILABLE, e.to_string()),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn get_artifact(
    State(st): State<AppState>,
    AxPath((id, name)): AxPath<(String, String)>,
    Query(q): Query<ProjectQuery>,
) -> Response {
    let Some(project) = q.project else {
        return error(StatusCode::BAD_REQUEST, "missing ?project=");
    };
    let project = match st.roots.check(&project) {
        Ok(p) => p,
        Err(e) => return error(StatusCode::FORBIDDEN, e),
    };
    match tokio::task::spawn_blocking(move || {
        qualyx_app::read_artifact(&project, &id, &name, MAX_ARTIFACT_BYTES)
    })
    .await
    {
        Ok(Ok(text)) => {
            ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
        }
        Ok(Err(e)) => error(StatusCode::NOT_FOUND, format!("{e:#}")),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")}))
}

const DASHBOARD: &str = include_str!("dashboard.html");

async fn dashboard() -> Html<&'static str> {
    Html(DASHBOARD)
}

fn router(roots: Roots, token: Option<String>, queue_url: Option<String>) -> Router {
    let st = AppState {
        roots,
        jobs: Jobs::new(MAX_CONCURRENT_RUNS),
        token,
        queue_url,
    };
    let api = Router::new()
        .route("/v1/runs", post(start_run).get(list_runs))
        .route("/v1/runs/{id}", get(get_run))
        .route("/v1/runs/{id}/cancel", post(cancel_run))
        .route("/v1/runs/{id}/artifacts/{name}", get(get_artifact))
        .route_layer(middleware::from_fn_with_state(st.clone(), auth));
    Router::new()
        .route("/health", get(health))
        .route("/", get(dashboard))
        .merge(api)
        .layer(DefaultBodyLimit::max(16_384))
        .with_state(st)
}

/// Validate credentials and roots, then serve until interrupted.
pub fn serve(cfg: ApiConfig) -> anyhow::Result<()> {
    if cfg.token.as_deref().is_none_or(str::is_empty) {
        anyhow::bail!(
            "refusing to listen on {} without QUALYX_API_TOKEN",
            cfg.bind
        );
    }
    if cfg
        .token
        .as_ref()
        .is_some_and(|t| t.len() < 32 || !t.bytes().all(|b| b.is_ascii_graphic()))
    {
        anyhow::bail!(
            "QUALYX_API_TOKEN must contain at least 32 printable ASCII characters without spaces"
        );
    }
    let roots = Roots::new(&cfg.roots)?;
    let queue_url = if cfg.team {
        let url = std::env::var("QUALYX_DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("--team needs QUALYX_DATABASE_URL"))?;
        qualyx_store::queue::Queue::connect(&url)?;
        Some(url)
    } else {
        None
    };
    let app = router(roots, cfg.token.filter(|t| !t.is_empty()), queue_url);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
        eprintln!("qualyx api listening on http://{}", listener.local_addr()?);
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
        Ok(())
    })
}
