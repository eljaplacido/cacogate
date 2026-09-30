//! HTTP API (loopback only). P0 exposes decisions and records; approvals
//! arrive in P1 together with authentication.
//!
//! - `POST /v1/check`            `ActionEnvelope` → `CheckResponse`
//! - `POST /v1/execution`        `ExecutionRecord` → `ExecutionRecord`
//! - `GET  /v1/health`
//! - `GET  /v1/status`
//! - `GET  /v1/decisions?limit=N`
//! - `GET  /v1/decisions/{id}`
//! - `GET  /v1/sessions/{id}/verify`
//!
//! Every reply carries the per-boot nonce in `x-cacogate-daemon` (see
//! [`crate::identity`]), so adapters can tell the daemon from a squatter.

use crate::engine::{Engine, EngineError};
use crate::identity::{self, DaemonIdentity, IDENTITY_HEADER};
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use cacogate_contracts::{ActionEnvelope, ExecutionRecord};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

/// Shared server state.
#[derive(Debug)]
pub struct AppState {
    /// Engine.
    pub engine: Engine,
    /// Start time.
    pub started: Instant,
    /// Per-boot nonce stamped on every reply.
    pub nonce: String,
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> Self {
        let code = match e {
            EngineError::DigestMismatch | EngineError::Version(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError(code, e.to_string())
    }
}

/// Build the router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/status", get(status))
        .route("/v1/check", post(check))
        .route("/v1/execution", post(execution))
        .route("/v1/decisions", get(decisions))
        .route("/v1/decisions/{id}", get(decision))
        .route("/v1/sessions/{id}/verify", get(verify))
        .layer(axum::middleware::map_response_with_state(
            state.clone(),
            stamp_identity,
        ))
        .with_state(state)
}

async fn stamp_identity(State(s): State<Arc<AppState>>, mut res: Response) -> Response {
    if let Ok(v) = HeaderValue::from_str(&s.nonce) {
        res.headers_mut().insert(IDENTITY_HEADER, v);
    }
    res
}

async fn health(State(s): State<Arc<AppState>>) -> Json<Value> {
    let p = s.engine.policy();
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "mode": s.engine.mode(),
        "policy": p.policy.name,
        "policy_digest": p.digest,
        "ledger_dir": s.engine.ledger().dir(),
        "uptime_s": s.started.elapsed().as_secs(),
    }))
}

async fn status(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let st = s.engine.status()?;
    Ok(Json(serde_json::to_value(st).map_err(|e| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

async fn check(
    State(s): State<Arc<AppState>>,
    Json(env): Json<ActionEnvelope>,
) -> Result<Json<Value>, ApiError> {
    let r = s.engine.check(&env)?;
    Ok(Json(serde_json::to_value(r).map_err(|e| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

async fn execution(
    State(s): State<Arc<AppState>>,
    Json(rec): Json<ExecutionRecord>,
) -> Result<Json<Value>, ApiError> {
    let r = s.engine.execution(rec)?;
    Ok(Json(serde_json::to_value(r).map_err(|e| {
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?))
}

#[derive(Deserialize)]
struct ListQuery {
    limit: Option<usize>,
}

async fn decisions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let rows = s
        .engine
        .ledger()
        .read_index(q.limit.unwrap_or(50))
        .map_err(EngineError::from)?;
    Ok(Json(json!(rows)))
}

async fn decision(
    State(s): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let ledger = s.engine.ledger();
    let rows = ledger.read_index(0).map_err(EngineError::from)?;
    let Some(row) = rows.into_iter().rev().find(|r| r.decision_id == id) else {
        return Err(ApiError(StatusCode::NOT_FOUND, format!("no decision {id}")));
    };
    let entries = ledger
        .read_session(&row.session_id)
        .map_err(EngineError::from)?;
    let related: Vec<_> = entries
        .into_iter()
        .filter(|e| {
            e.seq + 1 == row.seq
                || e.seq == row.seq
                || e.body.as_ref().is_some_and(|b| {
                    b.get("tool_use_id").and_then(Value::as_str) == Some(row.tool_use_id.as_str())
                        && e.seq > row.seq
                })
        })
        .collect();
    Ok(Json(json!({ "row": row, "entries": related })))
}

async fn verify(State(s): State<Arc<AppState>>, Path(id): Path<String>) -> Json<Value> {
    Json(json!(s.engine.ledger().verify_session(&id)))
}

/// Serve until Ctrl-C / SIGTERM. After the port is bound (and only then),
/// publish the daemon's identity in `data_dir`; remove it on shutdown.
pub async fn serve(
    addr: &str,
    state: Arc<AppState>,
    data_dir: &std::path::Path,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let id = DaemonIdentity {
        pid: std::process::id(),
        addr: listener.local_addr()?.to_string(),
        nonce: state.nonce.clone(),
        policy_digest: state.engine.policy().digest.clone(),
        started: crate::engine::now_rfc3339(),
    };
    identity::write(data_dir, &id)?;
    let result = axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    identity::remove_if_ours(data_dir, &id.nonce);
    result
}
