//! Hiran v2.2 AI bridge for ZION Free World (L5 Humanitarian Layer).

use crate::config::FreeWorldConfig;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

pub struct FreeWorldHiranBridge {
    client: Client,
    base_url: String,
    enabled: bool,
}

impl FreeWorldHiranBridge {
    pub fn new(cfg: &FreeWorldConfig) -> Self {
        let base_url = cfg
            .hiran_endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:8002".to_string());
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client build failed");
        Self {
            client,
            base_url,
            enabled: cfg.hiran_enabled,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Analyze a grant proposal — returns AI recommendation (approve/reject/modify + reason).
    pub async fn analyze_grant_proposal(
        &self,
        title: &str,
        description: &str,
        amount_zion: u64,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Enable via FREE_WORLD_HIRAN_ENABLED=true.".to_string(),
            );
        }
        let prompt = format!(
            "Zhodnoť následující grantovou žádost pro ZION Free World humanitární fond.\n\
            Název: {title}\n\
            Popis: {description}\n\
            Požadovaná částka: {amount_zion} ZION\n\n\
            Poskytni doporučení: SCHVÁLIT / ZAMÍTNOUT / UPRAVIT. \
            Uveď konkrétní důvody, hodnocení dopadu (1-10), hodnocení transparentnosti (1-10) \
            a souladu se ZION filozofií."
        );
        self.chat(&prompt).await
    }

    /// Suggest matching community projects for a humanitarian need.
    pub async fn suggest_community_projects(
        &self,
        need: &str,
        region: &str,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Enable via FREE_WORLD_HIRAN_ENABLED=true.".to_string(),
            );
        }
        let prompt = format!(
            "Na základě následující humanitární potřeby navrhni 3-5 komunitních projektů \
            vhodných pro financování z ZION Free World fondu.\n\
            Potřeba: {need}\n\
            Region: {region}\n\n\
            Pro každý projekt uveď: název, popis, odhadované náklady v ZION, \
            očekávaný dopad a časový rámec realizace."
        );
        self.chat(&prompt).await
    }

    /// Generate impact report for a completed grant.
    pub async fn generate_impact_report(
        &self,
        project_name: &str,
        outcomes: &str,
        zion_spent: u64,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Enable via FREE_WORLD_HIRAN_ENABLED=true.".to_string(),
            );
        }
        let prompt = format!(
            "Vytvoř závěrečnou zprávu o dopadu dokončeného projektu ZION Free World.\n\
            Název projektu: {project_name}\n\
            Dosažené výsledky: {outcomes}\n\
            Celkové náklady: {zion_spent} ZION\n\n\
            Zpráva má obsahovat: shrnutí výsledků, kvantifikaci dopadu, \
            hodnocení efektivity vynaložených prostředků a doporučení pro budoucí projekty."
        );
        self.chat(&prompt).await
    }

    /// Health check — returns true if Hiran endpoint is reachable.
    pub async fn health(&self) -> bool {
        if !self.enabled {
            return false;
        }
        self.client
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    // ── internal ──────────────────────────────────────────────────────────────

    async fn chat(&self, user_prompt: &str) -> anyhow::Result<String> {
        let body = json!({
            "model": "hiran-v2.2",
            "messages": [
                {
                    "role": "system",
                    "content": "Jsi Hiran v2.2, AI poradce humanitárního fondu ZION Free World. \
                                Hodnotíš granty a projekty s ohledem na dopad, transparentnost \
                                a soulad s ZION filozofií."
                },
                {
                    "role": "user",
                    "content": user_prompt
                }
            ],
            "temperature": 0.7,
            "max_tokens": 512
        });

        let resp = match self
            .client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Ok(format!(
                    "Hiran nedosažitelný ({}). Pokračujte bez AI doporučení.",
                    e
                ));
            }
        };

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return Ok(format!(
                    "Hiran vrátil neplatnou odpověď ({}). Pokračujte bez AI doporučení.",
                    e
                ));
            }
        };

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("Hiran nevrátil žádný obsah.")
            .to_string();

        Ok(content)
    }
}
