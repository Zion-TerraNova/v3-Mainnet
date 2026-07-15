//! DAO HTTP API — Axum REST server
//!
//! Exposes DAO governance functions over HTTP so that the desktop agent,
//! mobile app, and external clients can interact with the DAO without
//! needing direct database access.
//!
//! ## Endpoints
//!
//! | Method | Path                       | Description                     |
//! |--------|----------------------------|---------------------------------|
//! | GET    | /api/dao/health            | Service health check            |
//! | GET    | /api/dao/proposals         | List all proposals (paginated)  |
//! | GET    | /api/dao/proposals/:id     | Get single proposal             |
//! | POST   | /api/dao/proposals         | Create new proposal             |
//! | GET    | /api/dao/proposals/:id/votes | Get vote breakdown            |
//! | POST   | /api/dao/proposals/:id/vote | Cast vote (API key auth)       |
//! | GET    | /api/dao/treasury          | Treasury overview               |
//! | GET    | /api/dao/stats             | Global DAO statistics           |
//!
//! ## Auth
//!
//! Write operations (create proposal, vote) require `X-DAO-Key` header.
//! Value must match `ZION_DAO_API_KEY` env var. Read operations are public.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::info;

use crate::config::DaoConfig;
use crate::db::DaoDb;
use crate::metrics::DaoMetrics;
use crate::proposal::{Proposal, ProposalType};
use crate::treasury::{Treasury, TreasuryOperation};
use crate::types::{VoteChoice, DAO_TREASURY_TOTAL, FLOWERS_PER_ZION, PROPOSAL_THRESHOLD};

// ─────────────────────────────────────────────────────────────────────────────
// App State
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<DaoDb>>,
    pub config: Arc<DaoConfig>,
    pub api_key: String,
    pub metrics: Arc<DaoMetrics>,
    pub treasury: Arc<Mutex<Treasury>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// API Response wrappers
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[allow(dead_code)]
struct ApiOk<T: Serialize> {
    success: bool,
    data: T,
}

#[derive(Serialize)]
#[allow(dead_code)]
struct ApiErr {
    success: bool,
    error: String,
}

fn ok<T: Serialize>(data: T) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "success": true, "data": data }))
}

fn err(msg: impl Into<String>, code: StatusCode) -> (StatusCode, Json<serde_json::Value>) {
    (
        code,
        Json(serde_json::json!({ "success": false, "error": msg.into() })),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Auth helper
// ─────────────────────────────────────────────────────────────────────────────

fn check_api_key(headers: &HeaderMap, expected: &str) -> bool {
    // Empty expected key means authentication is disabled in config; do not
    // allow the empty-header bypass to be used as an auth credential.
    if expected.is_empty() {
        return false;
    }
    headers
        .get("X-DAO-Key")
        .and_then(|v| v.to_str().ok())
        .map(|k| k == expected)
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

pub fn dao_router(state: AppState) -> Router {
    Router::new()
        .route("/api/dao/health", get(health))
        .route(
            "/api/dao/proposals",
            get(list_proposals).post(create_proposal),
        )
        .route("/api/dao/proposals/:id", get(get_proposal))
        .route("/api/dao/proposals/:id/votes", get(get_votes))
        .route("/api/dao/proposals/:id/vote", post(cast_vote))
        // Consent endpoints (L5 governance)
        .route(
            "/api/dao/proposals/:id/consent",
            get(get_consent_status).post(post_consent),
        )
        // Co-Admin endpoints
        .route("/api/dao/co-admins", get(list_co_admins))
        .route("/api/dao/co-admins/:layer", get(list_co_admins_by_layer))
        // Cross-layer endpoints
        .route("/api/dao/cross-layer/:id", get(get_cross_layer_state))
        .route("/api/dao/cross-layer/:id/veto", post(post_cross_layer_veto))
        .route(
            "/api/dao/cross-layer/:id/consent",
            post(post_cross_layer_consent),
        )
        // Treasury
        .route("/api/dao/treasury", get(treasury_overview))
        .route("/api/dao/treasury/submit", post(treasury_submit))
        .route("/api/dao/treasury/:op_id/sign", post(treasury_sign))
        .route("/api/dao/treasury/:op_id/execute", post(treasury_execute))
        .route("/api/dao/stats", get(dao_stats))
        .route("/metrics", get(prometheus_metrics))
        .with_state(state)
}

// ─────────────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────────────

/// GET /metrics — Prometheus text exposition format (compatible with Grafana/Prometheus)
async fn prometheus_metrics(State(state): State<AppState>) -> impl axum::response::IntoResponse {
    let body = state.metrics.render_prometheus();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        body,
    )
}

/// GET /api/dao/health
async fn health() -> Json<serde_json::Value> {
    ok(serde_json::json!({
        "service": "zion-dao",
        "version": "2.9.6",
        "status": "ok",
        "timestamp": Utc::now().to_rfc3339(),
    }))
}

/// GET /api/dao/stats
async fn dao_stats(State(state): State<AppState>) -> Json<serde_json::Value> {
    let db = state.db.lock().await;
    let proposals = db.load_all_proposals().unwrap_or_default();

    let total = proposals.len();
    let active = proposals.iter().filter(|p| p.status == "Active").count();
    let passed = proposals.iter().filter(|p| p.status == "Passed").count();
    let executed = proposals.iter().filter(|p| p.status == "Executed").count();

    ok(serde_json::json!({
        "total_proposals": total,
        "active": active,
        "passed": passed,
        "executed": executed,
        "treasury_total_zion": 4_000_000_000u64,
        "voting_period_days": 7u64,
        "quorum_percent": 10,
        "multisig": "5-of-7",
    }))
}

// ── Proposals ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Pagination {
    limit: Option<i64>,
    offset: Option<i64>,
    status: Option<String>,
}

/// GET /api/dao/proposals
async fn list_proposals(
    State(state): State<AppState>,
    Query(params): Query<Pagination>,
) -> Json<serde_json::Value> {
    let db = state.db.lock().await;
    let mut rows = db.load_all_proposals().unwrap_or_default();

    // Filter by status
    if let Some(ref status) = params.status {
        rows.retain(|r| r.status.to_lowercase() == status.to_lowercase());
    }

    // Paginate
    let total = rows.len();
    let offset = params.offset.unwrap_or(0) as usize;
    let limit = params.limit.unwrap_or(20) as usize;
    let page: Vec<_> = rows.into_iter().skip(offset).take(limit).collect();

    ok(serde_json::json!({
        "proposals": page,
        "total": total,
        "offset": offset,
        "limit": limit,
    }))
}

/// GET /api/dao/proposals/:id
async fn get_proposal(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;
    match db.get_proposal(id) {
        Ok(Some(row)) => Ok(ok(row)),
        Ok(None) => Err(err(
            format!("Proposal {} not found", id),
            StatusCode::NOT_FOUND,
        )),
        Err(e) => Err(err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)),
    }
}

// ── Create Proposal ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CreateProposalRequest {
    title: String,
    description: String,
    proposer: String,
    /// JSON-serialized ProposalType variant
    proposal_type: serde_json::Value,
}

/// POST /api/dao/proposals
async fn create_proposal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateProposalRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &state.api_key) {
        return Err(err(
            "Unauthorized — X-DAO-Key required",
            StatusCode::UNAUTHORIZED,
        ));
    }

    // Validate inputs
    if req.title.is_empty() || req.description.is_empty() || req.proposer.is_empty() {
        return Err(err(
            "title, description, proposer are required",
            StatusCode::BAD_REQUEST,
        ));
    }
    if !req.proposer.starts_with("zion1") {
        return Err(err(
            "proposer must be a valid ZION L1 address (zion1...)",
            StatusCode::BAD_REQUEST,
        ));
    }

    // Parse ProposalType from JSON
    let proposal_type: ProposalType =
        serde_json::from_value(req.proposal_type.clone()).map_err(|e| {
            err(
                format!("Invalid proposal_type: {}", e),
                StatusCode::BAD_REQUEST,
            )
        })?;

    // Assign sequential ID (simple: max(id)+1)
    let next_id = {
        let db = state.db.lock().await;
        let rows = db.load_all_proposals().unwrap_or_default();
        rows.iter().map(|r| r.id).max().unwrap_or(0) + 1
    };

    let proposal = Proposal::new(
        next_id,
        req.title.clone(),
        req.description.clone(),
        proposal_type,
        req.proposer.clone(),
        PROPOSAL_THRESHOLD, // assume threshold balance for API-created proposals
        0,                  // snapshot_block (0 = created via API, not L1 scan)
    );

    {
        let db = state.db.lock().await;
        db.insert_proposal(&proposal)
            .map_err(|e| err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;
    }

    info!(
        "[DAO-API] Proposal #{} created by {}",
        next_id, req.proposer
    );

    Ok(ok(serde_json::json!({
        "id": next_id,
        "status": "Active",
        "voting_ends_at": proposal.voting_ends_at.to_rfc3339(),
    })))
}

// ── Votes ─────────────────────────────────────────────────────────────────────

/// GET /api/dao/proposals/:id/votes
async fn get_votes(
    Path(id): Path<u64>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let db = state.db.lock().await;

    // Verify proposal exists
    match db.get_proposal(id) {
        Ok(None) => {
            return Err(err(
                format!("Proposal {} not found", id),
                StatusCode::NOT_FOUND,
            ))
        }
        Err(e) => return Err(err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)),
        _ => {}
    }

    let (yes, no, abstain) = db
        .vote_totals(id)
        .map_err(|e| err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

    let total = yes + no + abstain;
    let yes_pct = if total > 0 {
        yes as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    Ok(ok(serde_json::json!({
        "proposal_id": id,
        "yes":  { "weight": yes,     "pct": format!("{:.1}", yes_pct) },
        "no":   { "weight": no,      "pct": format!("{:.1}", if total > 0 { no     as f64 / total as f64 * 100.0 } else { 0.0 }) },
        "abstain": { "weight": abstain, "pct": format!("{:.1}", if total > 0 { abstain as f64 / total as f64 * 100.0 } else { 0.0 }) },
        "total_weight": total,
    })))
}

/// POST /api/dao/proposals/:id/vote
#[derive(Deserialize)]
struct CastVoteRequest {
    voter: String,
    choice: String, // "yes" | "no" | "abstain"
    weight: u64,    // ZION balance in atomic units (verified by scanner; API trusts it for now)
    l1_tx_hash: Option<String>,
}

async fn cast_vote(
    Path(id): Path<u64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CastVoteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &state.api_key) {
        return Err(err("Unauthorized", StatusCode::UNAUTHORIZED));
    }

    let choice = match req.choice.as_str() {
        "yes" => VoteChoice::Yes,
        "no" => VoteChoice::No,
        "abstain" => VoteChoice::Abstain,
        other => {
            return Err(err(
                format!("Invalid choice '{}' — use yes/no/abstain", other),
                StatusCode::BAD_REQUEST,
            ))
        }
    };

    if !req.voter.starts_with("zion1") {
        return Err(err(
            "voter must be a valid ZION L1 address",
            StatusCode::BAD_REQUEST,
        ));
    }

    let db = state.db.lock().await;

    // Check proposal is active
    match db.get_proposal(id) {
        Ok(None) => {
            return Err(err(
                format!("Proposal {} not found", id),
                StatusCode::NOT_FOUND,
            ))
        }
        Ok(Some(ref p)) if p.status != "Active" => {
            return Err(err(
                format!("Proposal {} is {}, not Active", id, p.status),
                StatusCode::CONFLICT,
            ));
        }
        Err(e) => return Err(err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR)),
        _ => {}
    }

    let recorded = db
        .record_vote(
            id,
            &req.voter,
            choice,
            req.weight,
            req.l1_tx_hash.as_deref(),
        )
        .map_err(|e| err(e.to_string(), StatusCode::INTERNAL_SERVER_ERROR))?;

    if !recorded {
        return Err(err(
            format!("{} already voted on proposal {}", req.voter, id),
            StatusCode::CONFLICT,
        ));
    }

    info!(
        "[DAO-API] Vote recorded: proposal={} voter={}",
        id, req.voter
    );

    Ok(ok(serde_json::json!({
        "proposal_id": id,
        "voter": req.voter,
        "choice": req.choice,
        "weight": req.weight,
    })))
}

// ── Treasury ──────────────────────────────────────────────────────────────────

/// GET /api/dao/treasury
async fn treasury_overview(State(state): State<AppState>) -> Json<serde_json::Value> {
    let treasury = state.treasury.lock().await;
    let config = &state.config;
    ok(serde_json::json!({
        "total_zion": (DAO_TREASURY_TOTAL / FLOWERS_PER_ZION as u128) as u64,
        "available_flowers": treasury.balance().to_string(),
        "available_zion": (treasury.balance() / FLOWERS_PER_ZION as u128) as u64,
        "addresses": config.treasury_addresses,
        "multisig": format!("{}-of-{}", treasury.threshold(), treasury.guardian_count()),
        "pending_operations": treasury.pending_count(),
        "daily_spend_limit_zion": config.daily_spend_limit,
        "note": "Treasury operations require guardian signatures",
    }))
}

#[derive(Deserialize)]
struct SubmitTreasuryRequest {
    op_id: String,
    guardian: String,
    operation: TreasuryOperation,
}

#[derive(Deserialize)]
struct SignTreasuryRequest {
    guardian: String,
}

#[derive(Deserialize)]
struct ExecuteTreasuryRequest {
    guardian: String,
}

/// POST /api/dao/treasury/submit
async fn treasury_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SubmitTreasuryRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &state.api_key) {
        return Err(err(
            "Unauthorized — X-DAO-Key required",
            StatusCode::UNAUTHORIZED,
        ));
    }
    if req.op_id.trim().is_empty() {
        return Err(err("op_id is required", StatusCode::BAD_REQUEST));
    }

    let mut treasury = state.treasury.lock().await;
    treasury
        .submit_operation(req.op_id.clone(), req.operation, &req.guardian)
        .map_err(|e| err(e.to_string(), StatusCode::BAD_REQUEST))?;

    state
        .metrics
        .treasury_operations_submitted
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state
        .metrics
        .guardian_signatures_collected
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let signatures = treasury.pending_signatures(&req.op_id).unwrap_or(0);
    let threshold = treasury.threshold() as usize;
    Ok(ok(serde_json::json!({
        "op_id": req.op_id,
        "signatures": signatures,
        "threshold": threshold,
        "ready": signatures >= threshold,
    })))
}

/// POST /api/dao/treasury/:op_id/sign
async fn treasury_sign(
    Path(op_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SignTreasuryRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &state.api_key) {
        return Err(err(
            "Unauthorized — X-DAO-Key required",
            StatusCode::UNAUTHORIZED,
        ));
    }

    let mut treasury = state.treasury.lock().await;
    let before = treasury.pending_signatures(&op_id).unwrap_or(0);
    let threshold_reached = treasury
        .add_signature(&op_id, &req.guardian)
        .map_err(|e| err(e.to_string(), StatusCode::BAD_REQUEST))?;
    let after = treasury.pending_signatures(&op_id).unwrap_or(before);

    if after > before {
        state
            .metrics
            .guardian_signatures_collected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    Ok(ok(serde_json::json!({
        "op_id": op_id,
        "signatures": after,
        "threshold": treasury.threshold(),
        "ready": threshold_reached,
    })))
}

/// POST /api/dao/treasury/:op_id/execute
async fn treasury_execute(
    Path(op_id): Path<String>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ExecuteTreasuryRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &state.api_key) {
        return Err(err(
            "Unauthorized — X-DAO-Key required",
            StatusCode::UNAUTHORIZED,
        ));
    }

    let mut treasury = state.treasury.lock().await;
    if !treasury.is_guardian_address(&req.guardian) {
        return Err(err(
            "guardian is not in active guardian set",
            StatusCode::UNAUTHORIZED,
        ));
    }

    let operation = treasury
        .execute(&op_id)
        .map_err(|e| err(e.to_string(), StatusCode::BAD_REQUEST))?;

    state
        .metrics
        .treasury_operations_executed
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let amount = match &operation {
        TreasuryOperation::Spend { amount, .. } => *amount,
        TreasuryOperation::HumanitarianGrant { amount, .. } => *amount,
        TreasuryOperation::Rebalance { amount, .. } => *amount,
        TreasuryOperation::GoldenEggPrize { amount, .. } => *amount,
    };
    state.metrics.treasury_total_disbursed_zion.fetch_add(
        amount / FLOWERS_PER_ZION,
        std::sync::atomic::Ordering::Relaxed,
    );

    Ok(ok(serde_json::json!({
        "op_id": op_id,
        "executed_by": req.guardian,
        "amount_flowers": amount,
        "amount_zion": amount / FLOWERS_PER_ZION,
        "operation": operation,
    })))
}

// ── Co-Admin Handlers ────────────────────────────────────────────────────────

/// GET /api/dao/co-admins — List all Co-Admins (mock response, state expansion needed)
async fn list_co_admins(State(state): State<AppState>) -> Json<serde_json::Value> {
    ok(serde_json::json!({
        "co_admins": state.config.co_admins,
        "total": state.config.co_admins.len(),
        "cross_layer_veto_enabled": state.config.cross_layer_veto_enabled,
        "cross_layer_consent_threshold": state.config.cross_layer_consent_threshold,
    }))
}

/// GET /api/dao/co-admins/:layer — List Co-Admins by layer
async fn list_co_admins_by_layer(
    Path(layer): Path<u8>,
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let filtered: Vec<_> = state
        .config
        .co_admins
        .iter()
        .filter(|c| c.layer == layer)
        .cloned()
        .collect();
    ok(serde_json::json!({
        "layer": layer,
        "co_admins": filtered,
        "total": filtered.len(),
    }))
}

// ── Consent Handlers (L5 Governance) ───────────────────────────────────────────

#[derive(Deserialize)]
struct ConsentRequest {
    voter: String,
    attestation: String, // "witness", "object", "abstain"
    reason_hash: Option<String>,
}

/// POST /api/dao/proposals/:id/consent — Cast attestation for consent proposal
async fn post_consent(
    Path(id): Path<u64>,
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ConsentRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // In production: load proposal, verify it uses consent model, store attestation
    if !check_api_key(&headers, &_state.config.api_key) {
        return Err(err(
            "Missing or invalid X-DAO-Key",
            StatusCode::UNAUTHORIZED,
        ));
    }

    let _attestation = match req.attestation.as_str() {
        "witness" => crate::consent::Attestation::Witness,
        "object" => crate::consent::Attestation::Object,
        "abstain" => crate::consent::Attestation::Abstain,
        other => {
            return Err(err(
                format!(
                    "Invalid attestation '{}' — use witness/object/abstain",
                    other
                ),
                StatusCode::BAD_REQUEST,
            ))
        }
    };

    Ok(ok(serde_json::json!({
        "proposal_id": id,
        "voter": req.voter,
        "attestation": req.attestation,
        "reason_hash": req.reason_hash,
        "status": "recorded",
        "note": "Consent engine integration pending DAO state persistence",
    })))
}

/// GET /api/dao/proposals/:id/consent — Get consent status
async fn get_consent_status(
    Path(id): Path<u64>,
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    ok(serde_json::json!({
        "proposal_id": id,
        "status": "pending",
        "witnesses": 0,
        "objections": 0,
        "abstentions": 0,
        "missing": 0,
        "note": "Consent engine integration pending DAO state persistence",
    }))
}

// ── Cross-Layer Handlers ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CrossLayerVetoRequest {
    layer: u8,
    reason_hash: String,
}

/// POST /api/dao/cross-layer/:id/veto — Veto a cross-layer proposal
async fn post_cross_layer_veto(
    Path(id): Path<u64>,
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CrossLayerVetoRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &_state.config.api_key) {
        return Err(err(
            "Missing or invalid X-DAO-Key",
            StatusCode::UNAUTHORIZED,
        ));
    }
    Ok(ok(serde_json::json!({
        "proposal_id": id,
        "layer": req.layer,
        "action": "veto",
        "reason_hash": req.reason_hash,
        "status": "recorded",
        "note": "Cross-layer registry integration pending DAO state persistence",
    })))
}

#[derive(Deserialize)]
struct CrossLayerConsentRequest {
    layer: u8,
}

/// POST /api/dao/cross-layer/:id/consent — Consent to a cross-layer proposal
async fn post_cross_layer_consent(
    Path(id): Path<u64>,
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CrossLayerConsentRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if !check_api_key(&headers, &_state.config.api_key) {
        return Err(err(
            "Missing or invalid X-DAO-Key",
            StatusCode::UNAUTHORIZED,
        ));
    }
    Ok(ok(serde_json::json!({
        "proposal_id": id,
        "layer": req.layer,
        "action": "consent",
        "status": "recorded",
        "note": "Cross-layer registry integration pending DAO state persistence",
    })))
}

/// GET /api/dao/cross-layer/:id — Get cross-layer state
async fn get_cross_layer_state(
    Path(id): Path<u64>,
    State(_state): State<AppState>,
) -> Json<serde_json::Value> {
    ok(serde_json::json!({
        "proposal_id": id,
        "required_layers": [],
        "layer_status": {},
        "veto_reasons": {},
        "is_ready": false,
        "has_veto": false,
        "note": "Cross-layer registry integration pending DAO state persistence",
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::HeaderMap;
    use axum::Json;
    use tokio::sync::Mutex;

    use crate::config::GuardianConfig;
    use crate::db::DaoDb;
    use crate::metrics::DaoMetrics;
    use crate::types::Guardian;

    #[allow(clippy::field_reassign_with_default)]
    fn test_state() -> AppState {
        let mut cfg = DaoConfig::default();
        cfg.api_key = "test-key".to_string();
        cfg.guardians = vec![
            GuardianConfig {
                name: "G1".to_string(),
                address: "zion1guardian1".to_string(),
                public_key: "pk1".to_string(),
            },
            GuardianConfig {
                name: "G2".to_string(),
                address: "zion1guardian2".to_string(),
                public_key: "pk2".to_string(),
            },
            GuardianConfig {
                name: "G3".to_string(),
                address: "zion1guardian3".to_string(),
                public_key: "pk3".to_string(),
            },
            GuardianConfig {
                name: "G4".to_string(),
                address: "zion1guardian4".to_string(),
                public_key: "pk4".to_string(),
            },
            GuardianConfig {
                name: "G5".to_string(),
                address: "zion1guardian5".to_string(),
                public_key: "pk5".to_string(),
            },
        ];

        let db = DaoDb::in_memory().expect("in-memory dao db must open");
        let treasury_guardians: Vec<Guardian> = cfg
            .guardians
            .iter()
            .map(|g| Guardian {
                name: g.name.clone(),
                address: g.address.clone(),
                public_key: g.public_key.clone(),
                is_active: true,
            })
            .collect();
        let treasury = Treasury::new(treasury_guardians, DAO_TREASURY_TOTAL);

        AppState {
            db: Arc::new(Mutex::new(db)),
            config: Arc::new(cfg.clone()),
            api_key: cfg.api_key.clone(),
            metrics: DaoMetrics::new(),
            treasury: Arc::new(Mutex::new(treasury)),
        }
    }

    #[test]
    fn test_api_key_check() {
        let mut headers = HeaderMap::new();
        headers.insert("X-DAO-Key", "secret123".parse().unwrap());
        assert!(check_api_key(&headers, "secret123"));
        assert!(!check_api_key(&headers, "wrong"));
    }

    #[test]
    fn test_api_key_missing() {
        let headers = HeaderMap::new();
        assert!(!check_api_key(&headers, "any"));
    }

    #[tokio::test]
    async fn test_treasury_multisig_submit_sign_execute_flow() {
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert("X-DAO-Key", "test-key".parse().unwrap());

        let submit = SubmitTreasuryRequest {
            op_id: "op-multisig-1".to_string(),
            guardian: "zion1guardian1".to_string(),
            operation: TreasuryOperation::Spend {
                recipient: "zion1recipientxyz".to_string(),
                amount: 1_000_000,
                purpose: "integration test spend".to_string(),
                proposal_id: 42,
            },
        };

        let submit_res = treasury_submit(State(state.clone()), headers.clone(), Json(submit))
            .await
            .expect("submit should succeed");
        let submit_json = submit_res.0;
        assert_eq!(submit_json["data"]["signatures"], 1);
        assert_eq!(submit_json["data"]["ready"], false);

        for guardian in [
            "zion1guardian2",
            "zion1guardian3",
            "zion1guardian4",
            "zion1guardian5",
        ] {
            let sign_res = treasury_sign(
                Path("op-multisig-1".to_string()),
                State(state.clone()),
                headers.clone(),
                Json(SignTreasuryRequest {
                    guardian: guardian.to_string(),
                }),
            )
            .await
            .expect("sign should succeed");

            let sign_json = sign_res.0;
            if guardian == "zion1guardian5" {
                assert_eq!(sign_json["data"]["ready"], true);
                assert_eq!(sign_json["data"]["signatures"], 5);
            }
        }

        let exec_res = treasury_execute(
            Path("op-multisig-1".to_string()),
            State(state.clone()),
            headers,
            Json(ExecuteTreasuryRequest {
                guardian: "zion1guardian1".to_string(),
            }),
        )
        .await
        .expect("execute should succeed after threshold");

        let exec_json = exec_res.0;
        assert_eq!(exec_json["data"]["executed_by"], "zion1guardian1");
        assert_eq!(exec_json["data"]["amount_flowers"], 1_000_000);
    }

    #[tokio::test]
    async fn test_treasury_submit_unauthorized_without_api_key() {
        let state = test_state();
        let headers = HeaderMap::new();

        let res = treasury_submit(
            State(state),
            headers,
            Json(SubmitTreasuryRequest {
                op_id: "op-noauth".to_string(),
                guardian: "zion1guardian1".to_string(),
                operation: TreasuryOperation::Spend {
                    recipient: "zion1recipientxyz".to_string(),
                    amount: 1_000_000,
                    purpose: "unauthorized".to_string(),
                    proposal_id: 1,
                },
            }),
        )
        .await;

        assert!(res.is_err());
        let (code, body) = res.err().unwrap();
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        assert_eq!(body.0["success"], false);
    }
}
