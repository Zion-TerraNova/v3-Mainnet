//! REST API handlers for zion-free-world.

use crate::dao_client::{DaoClient, DaoClientConfig, DaoProposalRequest};
use crate::db::{FreeWorldDb, GrantRecord, ProjectRecord};
use crate::hiran_bridge::FreeWorldHiranBridge;
use crate::metrics::serve_metrics_text;
use crate::metrics::FreeWorldMetrics;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<FreeWorldDb>>,
    pub api_key: String,
    pub metrics: Arc<FreeWorldMetrics>,
    pub hiran: Arc<FreeWorldHiranBridge>,
}

/// Generic API response wrapper using serde_json::Value for data.
#[derive(Serialize)]
pub struct ApiResponse {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
}

impl ApiResponse {
    pub fn ok<T: Serialize>(data: T) -> Self {
        Self {
            success: true,
            data: serde_json::to_value(data).ok(),
            error: None,
        }
    }
    pub fn err(msg: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.to_string()),
        }
    }
}

// ── Routes ──

pub fn free_world_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics_handler))
        .route("/api/v1/grants", get(list_grants).post(create_grant))
        .route("/api/v1/grants/:id/approve", post(approve_grant))
        .route(
            "/api/v1/grants/:id/submit-to-dao",
            post(submit_grant_to_dao),
        )
        .route("/api/v1/projects", get(list_projects).post(create_project))
        .route("/api/v1/fund/balance", get(fund_balance))
        .route("/ai/analyze-grant", post(ai_analyze_grant))
        .route("/ai/suggest-projects", post(ai_suggest_projects))
        .with_state(state)
}

// ── Handlers ──

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db.get_fund_balance() {
        Ok(_) => (StatusCode::OK, Json(ApiResponse::ok("ok"))),
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let text = serve_metrics_text(&state.metrics);
    ([("content-type", "text/plain; charset=utf-8")], text)
}

async fn list_grants(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db.list_grants(None) {
        Ok(grants) => (StatusCode::OK, Json(ApiResponse::ok(grants))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

#[derive(Deserialize)]
pub struct CreateGrantRequest {
    pub title: String,
    pub category: String,
    pub amount_zion: u64,
    pub description: Option<String>,
    pub applicant_name: Option<String>,
    pub applicant_address: Option<String>,
}

async fn create_grant(
    State(state): State<AppState>,
    Json(req): Json<CreateGrantRequest>,
) -> impl IntoResponse {
    let mut grant = GrantRecord::new(&req.title, &req.category, req.amount_zion);
    grant.description = req.description;
    grant.applicant_name = req.applicant_name;
    grant.applicant_address = req.applicant_address;

    let db = state.db.lock().unwrap();
    match db.insert_grant(&grant) {
        Ok(_) => {
            state
                .metrics
                .grants_pending
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (StatusCode::CREATED, Json(ApiResponse::ok(grant)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

#[derive(Deserialize)]
pub struct ApproveGrantRequest {
    pub notes: Option<String>,
}

async fn approve_grant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ApproveGrantRequest>,
) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db.update_grant_status(&id, "approved", req.notes.as_deref()) {
        Ok(_) => {
            state
                .metrics
                .grants_pending
                .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            state
                .metrics
                .grants_approved
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (StatusCode::OK, Json(ApiResponse::ok("approved")))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

async fn submit_grant_to_dao(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    let grant = {
        let db = state.db.lock().unwrap();
        match db.list_grants(None) {
            Ok(grants) => grants.into_iter().find(|g| g.id == id),
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiResponse::err(&e.to_string())),
                )
            }
        }
    };
    match grant {
        Some(grant) => {
            let client = DaoClient::new(DaoClientConfig::default());
            let req = DaoProposalRequest {
                title: format!("Grant: {}", grant.title),
                description: grant.description.clone().unwrap_or_default(),
                amount_zion: grant.amount_zion,
                recipient_address: grant.applicant_address.clone().unwrap_or_default(),
                proposal_type: "treasury".to_string(),
            };
            match client.submit_grant_proposal(&req).await {
                Ok(resp) => (StatusCode::OK, Json(ApiResponse::ok(resp))),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(ApiResponse::err(&e.to_string())),
                ),
            }
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::err("Grant not found")),
        ),
    }
}

async fn list_projects(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db.list_projects(None) {
        Ok(projects) => (StatusCode::OK, Json(ApiResponse::ok(projects))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub category: String,
    pub budget_zion: u64,
    pub description: Option<String>,
    pub location: Option<String>,
}

async fn create_project(
    State(state): State<AppState>,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    let mut project = ProjectRecord::new(&req.name, &req.category, req.budget_zion);
    project.description = req.description;
    project.location = req.location;

    let db = state.db.lock().unwrap();
    match db.insert_project(&project) {
        Ok(_) => {
            state
                .metrics
                .projects_active
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (StatusCode::CREATED, Json(ApiResponse::ok(project)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

async fn fund_balance(State(state): State<AppState>) -> impl IntoResponse {
    let db = state.db.lock().unwrap();
    match db.get_fund_balance() {
        Ok(balance) => (StatusCode::OK, Json(ApiResponse::ok(balance))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

// ── AI / Hiran endpoints ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AiAnalyzeGrantRequest {
    pub title: String,
    pub description: String,
    pub amount_zion: u64,
}

async fn ai_analyze_grant(
    State(state): State<AppState>,
    Json(req): Json<AiAnalyzeGrantRequest>,
) -> impl IntoResponse {
    match state
        .hiran
        .analyze_grant_proposal(&req.title, &req.description, req.amount_zion)
        .await
    {
        Ok(analysis) => (StatusCode::OK, Json(ApiResponse::ok(analysis))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}

#[derive(Deserialize)]
pub struct AiSuggestProjectsRequest {
    pub need: String,
    pub region: String,
}

async fn ai_suggest_projects(
    State(state): State<AppState>,
    Json(req): Json<AiSuggestProjectsRequest>,
) -> impl IntoResponse {
    match state
        .hiran
        .suggest_community_projects(&req.need, &req.region)
        .await
    {
        Ok(suggestions) => (StatusCode::OK, Json(ApiResponse::ok(suggestions))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::err(&e.to_string())),
        ),
    }
}
