//! OASIS REST API Server — Axum HTTP server for UE5 client, mobile app, web dashboard.
//!
//! ## Endpoints
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET    | /health | Health check |
//! | GET    | /api/v1/oasis/player/:address | Get player profile |
//! | POST   | /api/v1/oasis/player/:address/xp | Award XP to player |
//! | GET    | /api/v1/oasis/leaderboard | Top players by XP |
//! | POST   | /api/v1/oasis/guild | Create a guild |
//! | GET    | /api/v1/oasis/guild/:id | Get guild info |
//! | POST   | /api/v1/oasis/guild/:id/join | Join a guild |
//! | GET    | /api/v1/oasis/map | Full territory map |
//! | GET    | /api/v1/oasis/rewards/pools | Reward pool status |

use crate::api::ApiResponse;
use crate::combat::{ActionType, CombatAction, CombatEngine, Combatant};
use crate::config::OasisConfig;
use crate::db::OasisDb;
use crate::guild::Guild;
use crate::hiran_bridge::OasisHiranBridge;
use crate::metrics::{serve_metrics, OasisMetrics};
use crate::quests::QuestManager;
use crate::rate_limit::{rate_limit_middleware, RateLimiter};
use crate::rewards::{RewardPool, RewardSlot};
use crate::territory::TerritoryMap;
use crate::websocket::{ws_events_handler, ws_leaderboard_handler, WsHub};
use crate::xp::{XpSource, XpSystem};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

/// Shared application state.
#[derive(Clone)]
pub struct OasisState {
    pub db: OasisDb,
    pub config: OasisConfig,
    pub xp_sys: Arc<XpSystem>,
    pub quest_mgr: Arc<QuestManager>,
    pub metrics: Arc<OasisMetrics>,
    pub ws_hub: Option<Arc<WsHub>>,
    pub hiran: Arc<OasisHiranBridge>,
}

impl OasisState {
    pub fn new(
        db: OasisDb,
        config: OasisConfig,
        quest_mgr: Arc<QuestManager>,
        metrics: Arc<OasisMetrics>,
        ws_hub: Option<Arc<WsHub>>,
    ) -> Self {
        let daily_cap = config.daily_xp_cap;
        let hiran = Arc::new(OasisHiranBridge::new(&config));
        Self {
            db,
            config,
            xp_sys: Arc::new(XpSystem { daily_cap }),
            quest_mgr,
            metrics,
            ws_hub,
            hiran,
        }
    }
}

/// Build the Axum router with all OASIS routes.
pub fn build_router(state: OasisState) -> Router {
    let limiter = RateLimiter::new(30, 60);

    // Sensitive POST endpoints under rate limit
    let sensitive = Router::new()
        .route("/api/v1/oasis/player/:address/xp", post(award_xp))
        .route("/api/v1/oasis/guild", post(create_guild))
        .route("/api/v1/oasis/guild/:id/join", post(join_guild))
        .route("/api/v1/oasis/raid-team", post(create_raid_team))
        .route("/api/v1/oasis/raid-team/:id/join", post(join_raid_team))
        .route(
            "/api/v1/oasis/player/:address/quests/:quest_id/complete",
            post(complete_quest),
        )
        .route("/api/v1/oasis/combat/resolve", post(resolve_combat))
        .layer(middleware::from_fn(rate_limit_middleware))
        .layer(Extension(limiter));

    Router::new()
        .merge(sensitive)
        .route("/health", get(health))
        .route("/api/v1/oasis/player/:address", get(get_player))
        .route("/api/v1/oasis/leaderboard", get(leaderboard))
        .route("/api/v1/oasis/leaderboard/top100", get(top_100_leaderboard))
        .route("/api/v1/oasis/guild/:id", get(get_guild))
        .route("/api/v1/oasis/map", get(territory_map))
        .route("/api/v1/oasis/rewards/pools", get(reward_pools))
        // Golden Egg
        .route(
            "/api/v1/oasis/golden-egg/progress/:address",
            get(golden_egg_progress),
        )
        .route(
            "/api/v1/oasis/golden-egg/leaderboard",
            get(golden_egg_leaderboard),
        )
        .route("/api/v1/oasis/prize-tiers", get(prize_tiers))
        // Raid Team
        .route("/api/v1/oasis/raid-team/:id", get(get_raid_team))
        .route("/api/v1/oasis/raid-leaderboard", get(raid_leaderboard))
        // Quests
        .route("/api/v1/oasis/quests", get(list_quests))
        .route("/api/v1/oasis/player/:address/quests", get(player_quests))
        // Avatars
        .route("/api/v1/oasis/avatars", get(list_avatars))
        .route("/api/v1/oasis/avatars/:id", get(get_avatar))
        .route("/api/v1/oasis/avatars/:id/quests", get(avatar_quests))
        // WebSocket feeds
        .route("/api/v1/oasis/ws/leaderboard", get(ws_leaderboard_handler))
        .route("/api/v1/oasis/ws/events", get(ws_events_handler))
        // ── Hiran AI endpoints ──────────────────────────────────────────────
        .route("/api/v1/oasis/ai/quest-narrative", post(ai_quest_narrative))
        .route(
            "/api/v1/oasis/ai/consciousness-eval",
            post(ai_consciousness_eval),
        )
        .route("/api/v1/oasis/ai/npc-dialogue", post(ai_npc_dialogue))
        .route("/api/v1/oasis/ai/hiran-health", get(ai_hiran_health))
        .with_state(state)
}

/// Start the HTTP server on the configured port.
pub async fn start_server(state: OasisState) -> anyhow::Result<()> {
    let bind = format!("{}:{}", state.config.bind, state.config.port);
    let metrics_port = state.config.metrics_port;
    let metrics = Arc::clone(&state.metrics);
    let db = state.db.clone();
    let router = build_router(state);
    let listener = TcpListener::bind(&bind).await?;
    info!("OASIS API server listening on http://{}", bind);

    // Spawn Prometheus metrics endpoint in background
    tokio::spawn(serve_metrics(metrics.clone(), metrics_port));

    // Spawn periodic gauge refresh from DB + daily XP reset at midnight UTC
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        let mut last_reset_day: u64 = 0;
        loop {
            interval.tick().await;
            if let Ok(count) = db.player_count() {
                metrics
                    .player_count
                    .store(count, std::sync::atomic::Ordering::Relaxed);
            }
            if let Ok(count) = db.guild_count() {
                metrics
                    .active_guilds
                    .store(count, std::sync::atomic::Ordering::Relaxed);
            }
            // Daily XP reset at UTC midnight
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let today = now / 86_400;
            if today != last_reset_day {
                last_reset_day = today;
                if let Err(e) = db.reset_all_daily_xp() {
                    tracing::warn!("Daily XP reset failed: {}", e);
                } else {
                    info!("Daily XP reset completed for UTC day {}", today);
                }
            }
        }
    });

    axum::serve(listener, router).await?;
    Ok(())
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// GET /health
async fn health() -> impl IntoResponse {
    #[derive(Serialize)]
    struct Health {
        status: &'static str,
        service: &'static str,
        version: &'static str,
    }
    Json(ApiResponse::ok(Health {
        status: "ok",
        service: "zion-oasis",
        version: env!("CARGO_PKG_VERSION"),
    }))
}

/// GET /api/v1/oasis/player/:address
async fn get_player(
    State(state): State<OasisState>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    match state.db.get_or_create_player(&address) {
        Ok(player) => (StatusCode::OK, Json(ApiResponse::ok(player))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

/// Request body for POST /api/v1/oasis/player/:address/xp
#[derive(Debug, Deserialize)]
pub struct AwardXpRequest {
    pub source: String,
    pub amount: Option<u64>,
    pub details: Option<serde_json::Value>,
}

/// Response for XP award
#[derive(Debug, Serialize)]
pub struct AwardXpResponse {
    pub address: String,
    pub xp_awarded: u64,
    pub total_xp: u64,
    pub level: String,
    pub leveled_up: bool,
}

/// POST /api/v1/oasis/player/:address/xp
async fn award_xp(
    State(state): State<OasisState>,
    Path(address): Path<String>,
    Json(req): Json<AwardXpRequest>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut player = match state.db.get_or_create_player(&address) {
        Ok(p) => p,
        Err(e) => {
            state
                .metrics
                .errors_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(&e.to_string())),
            )
                .into_response();
        }
    };

    // Build XP source from request
    let amount = req.amount.unwrap_or(10);
    let source = match req.source.as_str() {
        "block_mined" => XpSource::BlockMined {
            block_height: 0,
            shares: amount,
        },
        "meditation" => XpSource::Meditation {
            duration_minutes: amount as u32,
        },
        "tithe" => XpSource::Tithe {
            category: "general".to_string(),
            amount,
        },
        "guild_quest" => XpSource::GuildQuest {
            quest_id: "quest-0".to_string(),
        },
        "referral" => XpSource::Referral {
            referred_address: "unknown".to_string(),
        },
        _ => XpSource::BlockMined {
            block_height: 0,
            shares: amount.min(100),
        },
    };

    let award = state
        .xp_sys
        .award(player.total_xp, player.level, &source, player.daily_xp);

    player.total_xp = award.new_total_xp;
    player.daily_xp += award.actual_amount;
    player.level = award.new_level;
    // Update daily streak on every XP award (marks player as active)
    player.touch();

    if let Err(e) = state.db.save_player(&player) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response();
    }

    let resp = AwardXpResponse {
        address: address.clone(),
        xp_awarded: award.actual_amount,
        total_xp: award.new_total_xp,
        level: award.new_level.name().to_string(),
        leveled_up: award.leveled_up,
    };
    state
        .metrics
        .xp_awards_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    // Broadcast XP award to WebSocket subscribers
    if let Some(ref hub) = state.ws_hub {
        hub.broadcast(crate::websocket::WsEvent::XpAward {
            address,
            amount: award.actual_amount,
            total_xp: award.new_total_xp,
        });
    }

    (StatusCode::OK, Json(ApiResponse::ok(resp))).into_response()
}

/// GET /api/v1/oasis/leaderboard
async fn leaderboard(State(state): State<OasisState>) -> impl IntoResponse {
    match state.db.top_players(100) {
        Ok(entries) => (StatusCode::OK, Json(ApiResponse::ok(entries))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

/// Request for POST /api/v1/oasis/guild
#[derive(Debug, Deserialize)]
pub struct CreateGuildRequest {
    pub name: String,
    pub founder: String,
    pub description: Option<String>,
}

/// POST /api/v1/oasis/guild
async fn create_guild(
    State(state): State<OasisState>,
    Json(req): Json<CreateGuildRequest>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Check founder XP requirement (Mental level = 5000 XP)
    match state.db.get_player(&req.founder) {
        Ok(Some(player)) if player.total_xp < crate::guild::MIN_LEVEL_CREATE => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiResponse::<()>::error(
                    "Insufficient level to create a guild (requires Mental level, 5000 XP)",
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(&e.to_string())),
            )
                .into_response();
        }
        _ => {}
    }

    let id = uuid::Uuid::new_v4().to_string();
    let mut guild = Guild::new(id, req.name, req.founder);
    if let Some(desc) = req.description {
        guild.description = desc;
    }

    if let Err(e) = state.db.save_guild(&guild) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response();
    }

    state
        .metrics
        .guild_creations_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Broadcast guild creation
    if let Some(ref hub) = state.ws_hub {
        hub.broadcast(crate::websocket::WsEvent::GuildCreate {
            guild_id: guild.id.clone(),
            name: guild.name.clone(),
        });
    }
    (StatusCode::CREATED, Json(ApiResponse::ok(guild))).into_response()
}

/// GET /api/v1/oasis/guild/:id
async fn get_guild(State(state): State<OasisState>, Path(id): Path<String>) -> impl IntoResponse {
    match state.db.get_guild(&id) {
        Ok(Some(guild)) => (StatusCode::OK, Json(ApiResponse::ok(guild))).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error("Guild not found")),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

/// Request for POST /api/v1/oasis/guild/:id/join
#[derive(Debug, Deserialize)]
pub struct JoinGuildRequest {
    pub address: String,
}

/// POST /api/v1/oasis/guild/:id/join
async fn join_guild(
    State(state): State<OasisState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGuildRequest>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut guild = match state.db.get_guild(&id) {
        Ok(Some(g)) => g,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::<()>::error("Guild not found")),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(&e.to_string())),
            )
                .into_response()
        }
    };

    // Check player XP for Emotional level
    match state.db.get_player(&req.address) {
        Ok(Some(player)) if player.total_xp < crate::guild::MIN_LEVEL_JOIN => {
            return (
                StatusCode::FORBIDDEN,
                Json(ApiResponse::<()>::error(
                    "Insufficient level to join a guild (requires Emotional level, 1000 XP)",
                )),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(&e.to_string())),
            )
                .into_response();
        }
        _ => {}
    }

    if let Err(e) = guild.add_member(&req.address) {
        return (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response();
    }

    // Update player's guild_id
    if let Ok(mut player) = state.db.get_or_create_player(&req.address) {
        player.guild_id = Some(id.clone());
        let _ = state.db.save_player(&player);
    }

    if let Err(e) = state.db.save_guild(&guild) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response();
    }

    state
        .metrics
        .guild_joins_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (StatusCode::OK, Json(ApiResponse::ok(guild))).into_response()
}

/// GET /api/v1/oasis/map
async fn territory_map() -> impl IntoResponse {
    let map = TerritoryMap::genesis_map();
    Json(ApiResponse::ok(map))
}

/// GET /api/v1/oasis/rewards/pools
async fn reward_pools() -> impl IntoResponse {
    #[derive(Serialize)]
    struct PoolsResponse {
        pools: Vec<RewardPool>,
        total_allocated: u64,
        total_distributed: u64,
    }

    let pools: Vec<RewardPool> = RewardSlot::all().into_iter().map(RewardPool::new).collect();
    let total_allocated: u64 = pools.iter().map(|p| p.total).sum();
    let total_distributed: u64 = pools.iter().map(|p| p.distributed).sum();
    let resp = PoolsResponse {
        pools,
        total_allocated,
        total_distributed,
    };
    Json(ApiResponse::ok(resp))
}

// ── Top 100 Leaderboard ───────────────────────────────────────────────────────

/// GET /api/v1/oasis/leaderboard/top100
async fn top_100_leaderboard(State(state): State<OasisState>) -> impl IntoResponse {
    match state.db.top_players(100) {
        Ok(entries) => (StatusCode::OK, Json(ApiResponse::ok(entries))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

// ── Golden Egg ──────────────────────────────────────────────────────────────────

use crate::golden_egg::GoldenEggProgress;
use crate::prize_tiers::PrizeConfig;
use crate::raid_team::RaidTeam;

/// GET /api/v1/oasis/golden-egg/progress/:address
async fn golden_egg_progress(Path(address): Path<String>) -> impl IntoResponse {
    let progress = GoldenEggProgress::new(address);
    Json(ApiResponse::ok(progress))
}

/// GET /api/v1/oasis/golden-egg/leaderboard
async fn golden_egg_leaderboard() -> impl IntoResponse {
    // Placeholder - will be wired to DB in future iteration
    let leaderboard: Vec<GoldenEggProgress> = Vec::new();
    Json(ApiResponse::ok(leaderboard))
}

/// GET /api/v1/oasis/prize-tiers
async fn prize_tiers() -> impl IntoResponse {
    let config = PrizeConfig::from_embedded_json();
    Json(ApiResponse::ok(config))
}

// ── Raid Team ───────────────────────────────────────────────────────────────────

/// Request for POST /api/v1/oasis/raid-team
#[derive(Debug, Deserialize)]
pub struct CreateRaidRequest {
    pub name: String,
    pub leader_address: String,
}

/// POST /api/v1/oasis/raid-team
async fn create_raid_team(
    State(state): State<OasisState>,
    Json(req): Json<CreateRaidRequest>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let id = uuid::Uuid::new_v4().to_string();
    let raid = RaidTeam::new(id.clone(), req.name, req.leader_address);
    state
        .metrics
        .raid_team_creations_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (StatusCode::CREATED, Json(ApiResponse::ok(raid))).into_response()
}

/// GET /api/v1/oasis/raid-team/:id
async fn get_raid_team(Path(id): Path<String>) -> impl IntoResponse {
    // Placeholder - returns not found
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::<()>::error(&format!(
            "Raid team {} not found",
            id
        ))),
    )
        .into_response()
}

/// Request for POST /api/v1/oasis/raid-team/:id/join
#[derive(Debug, Deserialize)]
pub struct JoinRaidRequest {
    pub address: String,
    pub consciousness_level: u8,
    pub role: String,
}

/// POST /api/v1/oasis/raid-team/:id/join
async fn join_raid_team(
    Path(id): Path<String>,
    Json(_req): Json<JoinRaidRequest>,
) -> impl IntoResponse {
    // Placeholder - would add member to raid team in DB
    (
        StatusCode::OK,
        Json(ApiResponse::ok(format!(
            "Join request for raid {} received",
            id
        ))),
    )
        .into_response()
}

/// GET /api/v1/oasis/raid-leaderboard
async fn raid_leaderboard() -> impl IntoResponse {
    let leaderboard: Vec<&str> = Vec::new();
    Json(ApiResponse::ok(leaderboard))
}

// ── Quests ────────────────────────────────────────────────────────────────────

/// GET /api/v1/oasis/quests
async fn list_quests(State(state): State<OasisState>) -> impl IntoResponse {
    Json(ApiResponse::ok(state.quest_mgr.registry.all().to_vec()))
}

/// GET /api/v1/oasis/player/:address/quests
async fn player_quests(
    State(state): State<OasisState>,
    Path(address): Path<String>,
) -> impl IntoResponse {
    match state.quest_mgr.player_progress(&state.db, &address) {
        Ok(progress) => (StatusCode::OK, Json(ApiResponse::ok(progress))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

/// POST /api/v1/oasis/player/:address/quests/:quest_id/complete
async fn complete_quest(
    State(state): State<OasisState>,
    Path((address, quest_id)): Path<(String, String)>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let def = match state.quest_mgr.registry.get(&quest_id) {
        Some(d) => d.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(ApiResponse::<()>::error("quest not found")),
            )
                .into_response();
        }
    };

    let mut player = match state.db.get_or_create_player(&address) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()>::error(&e.to_string())),
            )
                .into_response()
        }
    };

    match state
        .quest_mgr
        .complete_quest(&state.db, &address, &quest_id)
    {
        Ok(progress) => {
            // Award XP via the standard XP system
            let source = XpSource::AvatarQuest {
                quest_id: quest_id.clone(),
                avatar_id: def.avatar_id,
            };
            let award = state
                .xp_sys
                .award(player.total_xp, player.level, &source, player.daily_xp);
            player.total_xp = award.new_total_xp;
            player.daily_xp += award.actual_amount;
            player.level = award.new_level;
            if let Err(e) = state.db.save_player(&player) {
                tracing::warn!("Quest XP save failed: {}", e);
            }
            state
                .metrics
                .quest_completions_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            // Broadcast quest completion
            if let Some(ref hub) = state.ws_hub {
                hub.broadcast(crate::websocket::WsEvent::QuestComplete { address, quest_id });
            }
            (StatusCode::OK, Json(ApiResponse::ok(progress))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

// ── Avatars ───────────────────────────────────────────────────────────────────

/// GET /api/v1/oasis/avatars?ray=Blue&min_cl=4&rarity=Epic
async fn list_avatars(
    State(state): State<OasisState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ray = params.get("ray").map(|s| s.as_str());
    let min_cl = params.get("min_cl").and_then(|s| s.parse::<u8>().ok());
    let rarity = params.get("rarity").map(|s| s.as_str());
    let filtered = state.quest_mgr.registry.filter_avatars(ray, min_cl, rarity);
    (StatusCode::OK, Json(ApiResponse::ok(filtered))).into_response()
}

/// GET /api/v1/oasis/avatars/:id
async fn get_avatar(State(state): State<OasisState>, Path(id): Path<u16>) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    match state.quest_mgr.registry.get_avatar(id) {
        Some(avatar) => (StatusCode::OK, Json(ApiResponse::ok(avatar))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()>::error("avatar not found")),
        )
            .into_response(),
    }
}

/// GET /api/v1/oasis/avatars/:id/quests
async fn avatar_quests(State(state): State<OasisState>, Path(id): Path<u16>) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let quests = state.quest_mgr.registry.by_avatar(id);
    (StatusCode::OK, Json(ApiResponse::ok(quests))).into_response()
}

// ── Combat ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CombatRequest {
    pub action: String,
    pub attacker_level: u8,
    pub defender_level: u8,
    pub base_damage: u32,
}

#[derive(Debug, Serialize)]
pub struct CombatResponse {
    pub damage_dealt: u32,
    pub healing_done: u32,
    pub energy_cost: u32,
}

/// POST /api/v1/oasis/combat/resolve
async fn resolve_combat(
    State(state): State<OasisState>,
    Json(req): Json<CombatRequest>,
) -> impl IntoResponse {
    state
        .metrics
        .requests_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let action_type = match req.action.as_str() {
        "strike" => ActionType::Strike,
        "meditate" => ActionType::Meditate,
        "soul_shield" => ActionType::SoulShield,
        "dharma_blast" => ActionType::DharmaBlast,
        "cosmic_ray" => ActionType::CosmicRay,
        "unity_pulse" => ActionType::UnityPulse,
        "keter_beam" => ActionType::KeterBeam,
        _ => ActionType::Strike,
    };

    let attacker_level = match req.attacker_level {
        1 => crate::consciousness::ConsciousnessLevel::Physical,
        2 => crate::consciousness::ConsciousnessLevel::Emotional,
        3 => crate::consciousness::ConsciousnessLevel::Mental,
        4 => crate::consciousness::ConsciousnessLevel::Intuitional,
        5 => crate::consciousness::ConsciousnessLevel::Spiritual,
        6 => crate::consciousness::ConsciousnessLevel::Cosmic,
        7 => crate::consciousness::ConsciousnessLevel::Divine,
        8 => crate::consciousness::ConsciousnessLevel::Unity,
        9 => crate::consciousness::ConsciousnessLevel::OnTheStar,
        _ => crate::consciousness::ConsciousnessLevel::Physical,
    };

    let defender_level = match req.defender_level {
        1 => crate::consciousness::ConsciousnessLevel::Physical,
        2 => crate::consciousness::ConsciousnessLevel::Emotional,
        3 => crate::consciousness::ConsciousnessLevel::Mental,
        4 => crate::consciousness::ConsciousnessLevel::Intuitional,
        5 => crate::consciousness::ConsciousnessLevel::Spiritual,
        6 => crate::consciousness::ConsciousnessLevel::Cosmic,
        7 => crate::consciousness::ConsciousnessLevel::Divine,
        8 => crate::consciousness::ConsciousnessLevel::Unity,
        9 => crate::consciousness::ConsciousnessLevel::OnTheStar,
        _ => crate::consciousness::ConsciousnessLevel::Physical,
    };

    let action = CombatAction {
        action_type: action_type.clone(),
        attacker_level,
        defender_level,
        base_damage: req.base_damage,
    };

    let mut attacker = Combatant {
        address: "attacker".into(),
        display_name: "Attacker".into(),
        level: attacker_level,
        current_hp: CombatEngine::base_hp(attacker_level),
        max_hp: CombatEngine::base_hp(attacker_level),
        energy: CombatEngine::base_energy(attacker_level),
    };

    let mut defender = Combatant {
        address: "defender".into(),
        display_name: "Defender".into(),
        level: defender_level,
        current_hp: CombatEngine::base_hp(defender_level),
        max_hp: CombatEngine::base_hp(defender_level),
        energy: CombatEngine::base_energy(defender_level),
    };

    let result = CombatEngine::resolve(&action, &mut attacker, &mut defender);

    let resp = CombatResponse {
        damage_dealt: result.damage_dealt,
        healing_done: result.healing_done,
        energy_cost: result.energy_cost,
    };

    state
        .metrics
        .combat_resolutions_total
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (StatusCode::OK, Json(ApiResponse::ok(resp)))
}

// ── Hiran AI Handlers ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AiQuestNarrativeRequest {
    pub player_address: String,
    pub consciousness_level: String,
    pub quest_theme: String,
}

#[derive(Debug, Serialize)]
pub struct AiTextResponse {
    pub result: String,
}

/// POST /api/v1/oasis/ai/quest-narrative
async fn ai_quest_narrative(
    State(state): State<OasisState>,
    Json(req): Json<AiQuestNarrativeRequest>,
) -> impl IntoResponse {
    match state
        .hiran
        .generate_quest_narrative(
            &req.player_address,
            &req.consciousness_level,
            &req.quest_theme,
        )
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(ApiResponse::ok(AiTextResponse { result })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct AiConsciousnessEvalRequest {
    pub player_address: String,
    pub total_xp: u64,
    pub current_level: String,
    pub blocks_mined: u64,
}

/// POST /api/v1/oasis/ai/consciousness-eval
async fn ai_consciousness_eval(
    State(state): State<OasisState>,
    Json(req): Json<AiConsciousnessEvalRequest>,
) -> impl IntoResponse {
    match state
        .hiran
        .evaluate_consciousness(
            &req.player_address,
            req.total_xp,
            &req.current_level,
            req.blocks_mined,
        )
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(ApiResponse::ok(AiTextResponse { result })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct AiNpcDialogueRequest {
    pub npc_name: String,
    pub npc_role: String,
    pub player_question: String,
}

/// POST /api/v1/oasis/ai/npc-dialogue
async fn ai_npc_dialogue(
    State(state): State<OasisState>,
    Json(req): Json<AiNpcDialogueRequest>,
) -> impl IntoResponse {
    match state
        .hiran
        .npc_dialogue(&req.npc_name, &req.npc_role, &req.player_question)
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(ApiResponse::ok(AiTextResponse { result })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<()>::error(&e.to_string())),
        )
            .into_response(),
    }
}

/// GET /api/v1/oasis/ai/hiran-health
async fn ai_hiran_health(State(state): State<OasisState>) -> impl IntoResponse {
    let enabled = state.hiran.is_enabled();
    let reachable = state.hiran.health().await;
    Json(ApiResponse::ok(serde_json::json!({
        "enabled": enabled,
        "reachable": reachable,
    })))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Player;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::util::ServiceExt;

    fn test_state() -> OasisState {
        let db = OasisDb::in_memory().expect("db");
        let config = OasisConfig::default();
        let quest_mgr = Arc::new(QuestManager::new(crate::quests::QuestRegistry::default()));
        let metrics = OasisMetrics::new();
        let ws_hub = Some(crate::websocket::WsHub::new());
        OasisState::new(db, config, quest_mgr, metrics, ws_hub)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_player_creates_if_missing() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/player/zion1newplayer")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_leaderboard_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/leaderboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_territory_map_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/map")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_reward_pools_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/rewards/pools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_award_xp_endpoint() {
        let state = test_state();
        // Pre-create player first
        state
            .db
            .save_player(&Player::new("zion1miner".to_string()))
            .unwrap();
        let app = build_router(state);
        let body = serde_json::json!({
            "source": "block_mined",
            "amount": 10
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/oasis/player/zion1miner/xp")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_guild_not_found() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/guild/nonexistent-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_top100_leaderboard_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/leaderboard/top100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_golden_egg_progress_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/golden-egg/progress/zion1test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_prize_tiers_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/oasis/prize-tiers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_create_raid_team_endpoint() {
        let state = test_state();
        let app = build_router(state);
        let body = serde_json::json!({
            "name": "Golden Egg Raiders",
            "leader_address": "zion1leader"
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/oasis/raid-team")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
}
