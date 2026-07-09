//! Hiran v2.2 AI bridge for ZION OASIS (L4 Consciousness Gaming Layer).

use crate::config::OasisConfig;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

pub struct OasisHiranBridge {
    client: Client,
    base_url: String,
    enabled: bool,
}

impl OasisHiranBridge {
    pub fn new(cfg: &OasisConfig) -> Self {
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

    /// Generate AI quest narrative for a player at a given consciousness level.
    pub async fn generate_quest_narrative(
        &self,
        player_address: &str,
        consciousness_level: &str,
        quest_theme: &str,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Set OASIS_HIRAN_ENABLED=true to activate.".to_string(),
            );
        }
        let prompt = format!(
            "Vygeneruj narativní popis questu pro hráče ZION OASIS.\n\
            Hráčova adresa: {player_address}\n\
            Úroveň vědomí: {consciousness_level}\n\
            Téma questu: {quest_theme}\n\n\
            Napiš inspirativní, mytologicky laděný popis questu (3-5 vět) \
            v souladu s ZION filosofií vědomého těžení kryptoměny. \
            Zmiň cestu vědomí a propojení s TerraNova světem."
        );
        self.chat(&prompt).await
    }

    /// Evaluate player consciousness evolution — provide personalized guidance.
    pub async fn evaluate_consciousness(
        &self,
        player_address: &str,
        total_xp: u64,
        current_level: &str,
        blocks_mined: u64,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Set OASIS_HIRAN_ENABLED=true to activate.".to_string(),
            );
        }
        let prompt = format!(
            "Zhodnoť vývoj vědomí OASIS hráče.\n\
            Adresa hráče: {player_address}\n\
            Celkové XP: {total_xp}\n\
            Aktuální level: {current_level}\n\
            Vytěžené bloky: {blocks_mined}\n\n\
            Poskytni: hodnocení pokroku (silné stránky, oblasti ke zlepšení), \
            doporučení pro postup na vyšší level vědomí, \
            a motivační zprávu v duchu ZION TerraNova filosofie."
        );
        self.chat(&prompt).await
    }

    /// Generate NPC dialogue for in-game AI characters.
    pub async fn npc_dialogue(
        &self,
        npc_name: &str,
        npc_role: &str,
        player_question: &str,
    ) -> anyhow::Result<String> {
        if !self.enabled {
            return Ok(
                "Hiran AI not enabled. Set OASIS_HIRAN_ENABLED=true to activate.".to_string(),
            );
        }
        let prompt = format!(
            "Zahraj roli NPC postavy v ZION OASIS hře.\n\
            Jméno NPC: {npc_name}\n\
            Role NPC: {npc_role}\n\
            Otázka hráče: {player_question}\n\n\
            Odpověz jako tato NPC postava — v souladu s ZION filosofií, \
            mystickým ale přátelským tónem. Buď nápomocný a inspirativní. \
            Délka: 2-4 věty."
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
                    "content": "Jsi Hiran v2.2, AI průvodce světem ZION OASIS — \
                                hry vědomého těžení kryptoměny. Pomáháš hráčům \
                                procházet devíti úrovněmi vědomí (od Physical do OnTheStar), \
                                generuješ questy a dialogy NPC postav v duchu ZION TerraNova filosofie."
                },
                {
                    "role": "user",
                    "content": user_prompt
                }
            ],
            "temperature": 0.8,
            "max_tokens": 400
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
                return Ok(format!("Hiran nedosažitelný ({}). Pokračujte bez AI.", e));
            }
        };

        let json: serde_json::Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                return Ok(format!(
                    "Hiran vrátil neplatnou odpověď ({}). Pokračujte bez AI.",
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
