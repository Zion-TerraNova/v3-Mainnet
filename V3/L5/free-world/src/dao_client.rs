//! DAO client — submits grant/project proposals to the L2 DAO for governance approval.
//!
//! This is a placeholder integration. In production it would call the DAO REST API
//! at `ZION_DAO_API_ADDR` to create treasury proposals.

use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct DaoClientConfig {
    pub dao_api_url: String,
    pub api_key: String,
}

impl Default for DaoClientConfig {
    fn default() -> Self {
        Self {
            dao_api_url: std::env::var("ZION_DAO_API_ADDR")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
            api_key: std::env::var("ZION_DAO_API_KEY").unwrap_or_default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaoProposalRequest {
    pub title: String,
    pub description: String,
    pub amount_zion: u64,
    pub recipient_address: String,
    pub proposal_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaoProposalResponse {
    pub proposal_id: u64,
    pub status: String,
}

pub struct DaoClient {
    config: DaoClientConfig,
    http: reqwest::Client,
}

impl DaoClient {
    pub fn new(config: DaoClientConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, http }
    }

    /// Submit a grant disbursement request to the DAO treasury.
    pub async fn submit_grant_proposal(
        &self,
        req: &DaoProposalRequest,
    ) -> anyhow::Result<DaoProposalResponse> {
        let url = format!("{}/api/v1/proposals", self.config.dao_api_url);
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("DAO API error: {}", e))?;

        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("DAO API returned {}", resp.status()));
        }

        let body = resp
            .json::<DaoProposalResponse>()
            .await
            .map_err(|e| anyhow::anyhow!("DAO parse error: {}", e))?;

        Ok(body)
    }
}
