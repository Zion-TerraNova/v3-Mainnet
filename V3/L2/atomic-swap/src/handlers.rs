//! Axum HTTP handlers for the atomic-swap API.
//!
//! # Endpoints
//!
//! | Method | Path                  | Auth? | Description                        |
//! |--------|-----------------------|-------|------------------------------------|
//! | GET    | /health               | No    | Liveness check                     |
//! | GET    | /swap/:hash           | No    | HTLC status query                  |
//! | GET    | /swap/escrow-address  | No    | Escrow address (for LOCK TX)       |
//! | POST   | /swap/claim           | Yes   | Reveal preimage → release ZION     |
//! | POST   | /swap/refund          | Yes   | Refund expired HTLC (manual)       |
//! | GET    | /swap/pending         | Yes   | List pending HTLCs (admin)         |

use crate::db::SwapDb;
use crate::error::SwapError;
use crate::executor::SwapExecutor;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

// ─── Shared app state ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<SwapDb>,
    pub executor: Arc<SwapExecutor>,
    pub escrow_address: String,
    pub bearer_token: Option<String>,
}

// ─── Request / response types ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ClaimRequest {
    /// SHA-256 hash (64-char hex) that was used in SWAP:LOCK
    pub hash_hex: String,
    /// 32-byte preimage (64-char hex) such that SHA256(preimage)==hash
    pub preimage_hex: String,
    /// L1 address of the claimer (where released ZION should go)
    pub recipient: String,
}

#[derive(Deserialize)]
pub struct RefundRequest {
    /// Hash of the HTLC to refund
    pub hash_hex: String,
}

fn err_json(msg: impl Into<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "error", "message": msg.into() }))
}

fn ok_json(v: serde_json::Value) -> Json<serde_json::Value> {
    Json(v)
}

// ─── Auth helper ─────────────────────────────────────────────────────────────

fn require_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let Some(ref expected) = state.bearer_token else {
        return Ok(()); // open access
    };
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let provided = auth.strip_prefix("Bearer ").unwrap_or("");
    if provided != expected {
        return Err((StatusCode::UNAUTHORIZED, err_json("Unauthorized")));
    }
    Ok(())
}

// ─── Handlers ────────────────────────────────────────────────────────────────

pub async fn health() -> &'static str {
    "ok"
}

/// GET /swap/escrow-address
pub async fn escrow_address(State(state): State<AppState>) -> Json<serde_json::Value> {
    ok_json(serde_json::json!({
        "status": "ok",
        "escrow_address": state.escrow_address,
        "memo_format": "SWAP:LOCK:<hash64>:<timeout_min>:<chain>:<counterparty_addr>",
    }))
}

/// GET /swap/:hash
pub async fn swap_status(
    State(state): State<AppState>,
    Path(hash_hex): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.db.get_htlc(&hash_hex) {
        Ok(Some(rec)) => (
            StatusCode::OK,
            ok_json(serde_json::json!({
                "status": "ok",
                "htlc": rec,
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            err_json(format!("HTLC not found: {hash_hex}")),
        ),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_json(e.to_string())),
    }
}

/// POST /swap/claim
pub async fn claim(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ClaimRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    // Basic input validation
    if req.hash_hex.len() != 64 {
        return (
            StatusCode::BAD_REQUEST,
            err_json("hash_hex must be 64 hex chars"),
        );
    }
    if req.preimage_hex.len() != 64 {
        return (
            StatusCode::BAD_REQUEST,
            err_json("preimage_hex must be 64 hex chars"),
        );
    }
    if req.recipient.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            err_json("recipient address required"),
        );
    }

    match state
        .executor
        .execute_claim(&state.db, &req.hash_hex, &req.preimage_hex, &req.recipient)
        .await
    {
        Ok(()) => {
            let rec = state.db.get_htlc(&req.hash_hex).ok().flatten();
            (
                StatusCode::OK,
                ok_json(serde_json::json!({
                    "status": "claimed",
                    "hash_hex": req.hash_hex,
                    "recipient": req.recipient,
                    "release_tx_id": rec.as_ref().and_then(|r| r.release_tx_id.clone()),
                })),
            )
        }
        Err(SwapError::PreimageMismatch) => (
            StatusCode::BAD_REQUEST,
            err_json("Preimage does not match hash"),
        ),
        Err(SwapError::AlreadySettled { .. }) => {
            (StatusCode::CONFLICT, err_json("HTLC already settled"))
        }
        Err(SwapError::TimelockExpired { .. }) => (
            StatusCode::GONE,
            err_json("HTLC timelock expired — claim rejected"),
        ),
        Err(SwapError::HtlcNotFound { .. }) => (
            StatusCode::NOT_FOUND,
            err_json(format!("HTLC not found: {}", req.hash_hex)),
        ),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_json(e.to_string())),
    }
}

/// POST /swap/refund
pub async fn refund(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RefundRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    match state
        .executor
        .execute_refund(&state.db, &req.hash_hex)
        .await
    {
        Ok(()) => {
            let rec = state.db.get_htlc(&req.hash_hex).ok().flatten();
            (
                StatusCode::OK,
                ok_json(serde_json::json!({
                    "status": "refunded",
                    "hash_hex": req.hash_hex,
                    "release_tx_id": rec.as_ref().and_then(|r| r.release_tx_id.clone()),
                })),
            )
        }
        Err(SwapError::TimelockActive { expires_at, .. }) => (
            StatusCode::CONFLICT,
            err_json(format!("Timelock active until UNIX {expires_at}")),
        ),
        Err(SwapError::AlreadySettled { .. }) => {
            (StatusCode::CONFLICT, err_json("HTLC already settled"))
        }
        Err(SwapError::HtlcNotFound { .. }) => (
            StatusCode::NOT_FOUND,
            err_json(format!("HTLC not found: {}", req.hash_hex)),
        ),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_json(e.to_string())),
    }
}

/// GET /swap/pending
pub async fn list_pending(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Err(e) = require_auth(&state, &headers) {
        return e;
    }
    match state.db.list_pending(100) {
        Ok(list) => (
            StatusCode::OK,
            ok_json(serde_json::json!({
                "status": "ok",
                "count": list.len(),
                "htlcs": list,
            })),
        ),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, err_json(e.to_string())),
    }
}
