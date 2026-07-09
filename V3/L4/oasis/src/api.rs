//! OASIS REST API — endpoints for UE5 client, mobile app, and web dashboard.
//!
//! ⚠️ LAYER BOUNDARY: This API is the ONLY external interface for L4.
//! UE5 (Unreal Engine 5) client and mobile apps connect here.
//! This module does NOT start a server — it defines route handlers
//! that will be mounted on a Tokio/Axum server in the future.
//!
//! ## API Endpoints (planned)
//!
//! ### Player
//! - `GET  /api/v1/oasis/player/:address`        — Get player profile
//! - `POST /api/v1/oasis/player/:address/xp`      — Award XP (internal)
//! - `GET  /api/v1/oasis/player/:address/achievements` — Get achievements
//!
//! ### Leaderboard
//! - `GET  /api/v1/oasis/leaderboard/:type`       — Get leaderboard
//! - `GET  /api/v1/oasis/leaderboard/:type/rank/:address` — Player rank
//!
//! ### Guild
//! - `POST /api/v1/oasis/guild`                   — Create guild
//! - `GET  /api/v1/oasis/guild/:id`               — Get guild info
//! - `POST /api/v1/oasis/guild/:id/join`           — Join guild
//! - `POST /api/v1/oasis/guild/:id/quest`          — Start guild quest
//!
//! ### Territory
//! - `GET  /api/v1/oasis/map`                     — Full territory map
//! - `POST /api/v1/oasis/territory/:id/claim`      — Claim territory
//! - `POST /api/v1/oasis/territory/:id/enter`      — Enter territory
//!
//! ### Challenges
//! - `GET  /api/v1/oasis/challenges`              — Available challenges
//! - `GET  /api/v1/oasis/challenges/daily`         — Today's daily challenges
//! - `POST /api/v1/oasis/challenges/:id/submit`    — Submit challenge result
//!
//! ### Tithe
//! - `POST /api/v1/oasis/tithe`                   — Make a tithe
//! - `GET  /api/v1/oasis/tithe/stats`             — Tithe statistics
//!
//! ### Rewards
//! - `GET  /api/v1/oasis/rewards/pools`           — Pool status
//!
//! ### Health
//! - `GET  /api/v1/oasis/health`                  — Service health

use serde::{Deserialize, Serialize};

/// API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: u64,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

impl ApiResponse<()> {
    pub fn error(msg: &str) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(msg.to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub service: String,
    pub version: String,
    pub layer: String,
    pub status: String,
    pub total_players: u64,
    pub total_guilds: u64,
    pub uptime_seconds: u64,
}

/// OASIS API configuration
pub const API_VERSION: &str = "v1";
pub const API_PREFIX: &str = "/api/v1/oasis";
pub const DEFAULT_PORT: u16 = 8094;
