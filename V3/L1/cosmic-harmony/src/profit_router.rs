//! Profit router — external coin definitions and Blake3-compatible revenue targets
//!
//! ZION's CosmicHarmony pipeline (Keccak→SHA3→Matrix→Fusion) produces ZION blocks.
//! The revenue system also supports mining external coins that share compatible
//! algorithms. Decred (DCR) uses standard Blake3 (DCP-0011, since Oct 2022),
//! and Alephium (ALPH) also uses Blake3.
//!
//! This module defines:
//! - `ExternalCoin` — enumeration of mineable external coins
//! - `CoinProfile` — per-coin metadata (algorithm, default pool, protocol)
//! - `ProfitEntry` — snapshot of per-coin estimated profitability
//! - `select_best_coin` — pick the most profitable coin from a list, with hysteresis

use serde::{Deserialize, Serialize};
use std::fmt;

/// Pool routing preference, compatible with legacy revenue system semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PoolPreference {
    NiceHash,
    HeroMiners,
    ZPool,
    Default,
}

impl PoolPreference {
    pub fn from_str_loose(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "nicehash" | "nh" => Self::NiceHash,
            "herominers" | "hm" => Self::HeroMiners,
            "zpool" => Self::ZPool,
            _ => Self::Default,
        }
    }
}

// ── External coin enumeration ────────────────────────────────────────

/// Coins that ZION miners can profit-switch to for the 25% multi-algo revenue slot.
///
/// Listed in rough priority order. Only coins with a live, tested pool endpoint
/// are `Enabled` by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalCoin {
    /// Decred — standard Blake3 (DCP-0011, since Oct 2022).
    /// High-profit Blake3 coin, GPU+ASIC. 2miners pool, BTC payout.
    DCR,
    /// Alephium — Blake3. GPU coin. 2miners pool, BTC payout.
    ALPH,
    /// Kaspa — kHeavyHash. GPU coin. 2miners, BTC payout.
    KAS,
    /// Ergo — Autolykos v2. GPU coin. 2miners, BTC payout.
    ERG,
    /// Ravencoin — KawPow. GPU coin. 2miners, BTC payout.
    RVN,
    /// Ethereum Classic — Ethash. GPU coin. 2miners, BTC payout.
    ETC,
    /// Evrmore — EvrProgPow. GPU coin. ZPool, BTC payout.
    EVR,
    /// MeowCoin — MeowPow. GPU coin. ZPool, BTC payout.
    MEWC,
    /// Flux — ZelHash (Equihash variant). GPU coin. WoolyPooly.
    FLUX,
    /// Clore.AI — KawPow. GPU coin. WoolyPooly.
    CLORE,
    /// Monero — RandomX. CPU coin. MoneroOcean, XMR→BTC.
    XMR,
    /// Verus — VerusHash v2.2 (Haraka+CLHash). CPU coin. LuckPool.
    /// B2b revenue stream: ASIC/GPU resistant, PBaaS merge mining.
    VRSC,
}

impl ExternalCoin {
    /// Canonical ticker string.
    pub fn ticker(self) -> &'static str {
        match self {
            Self::DCR => "DCR",
            Self::ALPH => "ALPH",
            Self::KAS => "KAS",
            Self::ERG => "ERG",
            Self::RVN => "RVN",
            Self::ETC => "ETC",
            Self::EVR => "EVR",
            Self::MEWC => "MEWC",
            Self::FLUX => "FLUX",
            Self::CLORE => "CLORE",
            Self::XMR => "XMR",
            Self::VRSC => "VRSC",
        }
    }

    /// Mining algorithm identifier string.
    pub fn algorithm(self) -> &'static str {
        match self {
            Self::DCR => "blake3",
            Self::ALPH => "blake3",
            Self::KAS => "kheavyhash",
            Self::ERG => "autolykos",
            Self::RVN => "kawpow",
            Self::ETC => "ethash",
            Self::EVR => "evrprogpow",
            Self::MEWC => "meowpow",
            Self::FLUX => "zelhash",
            Self::CLORE => "kawpow",
            Self::XMR => "randomx",
            Self::VRSC => "verushash",
        }
    }

    /// Whether this coin uses the Blake3 hash function (same family as ZION's
    /// CosmicHarmony uses internally for hashing utilities).
    pub fn is_blake3(self) -> bool {
        matches!(self, Self::DCR | Self::ALPH)
    }

    /// Whether this coin is CPU-minable (no GPU required).
    pub fn is_cpu(self) -> bool {
        matches!(self, Self::XMR | Self::VRSC)
    }

    /// Parse from a case-insensitive string. Accepts ticker, full name, and
    /// common aliases.
    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "dcr" | "decred" | "blake3-dcr" | "blake3dcr" => Some(Self::DCR),
            "alph" | "alephium" | "blake3-alph" | "blake3alph" => Some(Self::ALPH),
            "kas" | "kaspa" | "kheavyhash" => Some(Self::KAS),
            "erg" | "ergo" | "autolykos" => Some(Self::ERG),
            "rvn" | "ravencoin" | "kawpow" => Some(Self::RVN),
            "etc" | "ethereum-classic" | "ethash" => Some(Self::ETC),
            "evr" | "evrmore" | "evrprogpow" => Some(Self::EVR),
            "mewc" | "meowcoin" | "meowpow" => Some(Self::MEWC),
            "flux" | "zelhash" => Some(Self::FLUX),
            "clore" | "clore.ai" => Some(Self::CLORE),
            "xmr" | "monero" | "randomx" => Some(Self::XMR),
            "vrsc" | "verus" | "verushash" => Some(Self::VRSC),
            _ => None,
        }
    }

    /// Default Stratum pool endpoint (host:port) for this coin.
    /// Uses 2miners where available (BTC payout), falls back to ZPool/WoolyPooly.
    pub fn default_pool(self) -> &'static str {
        match self {
            Self::DCR => "pool.woolypooly.com:3152",
            Self::ALPH => "pool.woolypooly.com:3106",
            Self::KAS => "kas.2miners.com:2020",
            Self::ERG => "erg.2miners.com:3056",
            Self::RVN => "rvn.2miners.com:6060",
            Self::ETC => "etc.2miners.com:1010",
            Self::EVR => "evrprogpow.eu.mine.zpool.ca:1330",
            Self::MEWC => "meowpow.eu.mine.zpool.ca:1327",
            Self::FLUX => "flux.woolypooly.com:3000",
            Self::CLORE => "clore.woolypooly.com:3090",
            Self::XMR => "gulf.moneroocean.stream:10001",
            Self::VRSC => "eu.luckpool.net:3956",
        }
    }

    /// NiceHash endpoint for supported algos.
    ///
    /// Note: NiceHash currently does not expose Blake3 endpoints, so DCR/ALPH
    /// return `None` and should fall back to HeroMiners/ZPool/default.
    pub fn nicehash_pool(self, region: &str) -> Option<String> {
        let (algo, port): (&str, u16) = match self {
            Self::ETC => ("etchash", 9013),
            Self::RVN => ("kawpow", 9017),
            Self::ERG => ("autolykos", 9018),
            Self::KAS => ("kheavyhash", 9024),
            // NH does not provide Blake3 stratum endpoints for these at present.
            Self::DCR | Self::ALPH => return None,
            _ => return None,
        };
        let nh_region = match region.to_ascii_lowercase().as_str() {
            "eu" => "eu",
            "na" | "us" => "usa",
            _ => "auto",
        };
        Some(format!("{}.{}.nicehash.com:{}", algo, nh_region, port))
    }

    /// HeroMiners endpoints for supported coins.
    pub fn herominers_pool(self, region: &str) -> Option<String> {
        let (subdomain, port): (&str, u16) = match self {
            Self::ETC => ("etc", 1150),
            Self::KAS => ("kaspa", 1206),
            Self::ALPH => ("alephium", 1220),
            Self::ERG => ("ergo", 1180),
            Self::RVN => ("ravencoin", 1140),
            _ => return None,
        };

        let hm_region = match region.to_ascii_lowercase().as_str() {
            "eu" => "de",
            "na" | "us" => "us",
            "hk" | "sg" | "asia" => "hk",
            _ => "de",
        };

        Some(format!(
            "{}.{}.herominers.com:{}",
            hm_region, subdomain, port
        ))
    }

    /// ZPool endpoints for supported coins.
    pub fn zpool_pool(self, region: &str) -> Option<String> {
        let (algo, port): (&str, u16) = match self {
            Self::EVR => ("evrprogpow", 1330),
            Self::MEWC => ("meowpow", 1327),
            _ => return None,
        };
        let zp_region = match region.to_ascii_lowercase().as_str() {
            "na" | "us" => "na",
            _ => "eu",
        };
        Some(format!("{}.{}.mine.zpool.ca:{}", algo, zp_region, port))
    }

    /// Best pool endpoint using the legacy fallback hierarchy:
    /// nicehash -> herominers -> zpool -> default.
    pub fn best_pool(self, preference: PoolPreference, region: &str) -> String {
        match preference {
            PoolPreference::NiceHash => {
                if let Some(url) = self.nicehash_pool(region) {
                    return url;
                }
                if let Some(url) = self.herominers_pool(region) {
                    return url;
                }
                if let Some(url) = self.zpool_pool(region) {
                    return url;
                }
                self.default_pool().to_string()
            }
            PoolPreference::HeroMiners => {
                if let Some(url) = self.herominers_pool(region) {
                    return url;
                }
                if let Some(url) = self.zpool_pool(region) {
                    return url;
                }
                self.default_pool().to_string()
            }
            PoolPreference::ZPool => {
                if let Some(url) = self.zpool_pool(region) {
                    return url;
                }
                self.default_pool().to_string()
            }
            PoolPreference::Default => self.default_pool().to_string(),
        }
    }

    /// Stratum protocol variant used by this coin's pool.
    pub fn protocol(self) -> StratumProtocol {
        match self {
            Self::DCR => StratumProtocol::Stratum,
            Self::ALPH => StratumProtocol::Stratum,
            Self::KAS => StratumProtocol::Stratum,
            Self::ERG => StratumProtocol::EthStratum,
            Self::RVN => StratumProtocol::EthStratum,
            Self::ETC => StratumProtocol::EthStratum,
            Self::EVR => StratumProtocol::EthStratum,
            Self::MEWC => StratumProtocol::EthStratum,
            Self::FLUX => StratumProtocol::Stratum,
            Self::CLORE => StratumProtocol::EthStratum,
            Self::XMR => StratumProtocol::Stratum,
            Self::VRSC => StratumProtocol::ZcashStratum,
        }
    }

    /// All known coins.
    pub fn all() -> &'static [ExternalCoin] {
        &[
            Self::DCR,
            Self::ALPH,
            Self::KAS,
            Self::ERG,
            Self::RVN,
            Self::ETC,
            Self::EVR,
            Self::MEWC,
            Self::FLUX,
            Self::CLORE,
            Self::XMR,
            Self::VRSC,
        ]
    }

    /// Only Blake3-compatible coins.
    pub fn blake3_coins() -> &'static [ExternalCoin] {
        &[Self::DCR, Self::ALPH]
    }

    /// Map this external coin to the canonical revenue source used by the
    /// pool-side revenue collector.
    pub fn revenue_source(self) -> crate::revenue::RevenueSource {
        use crate::revenue::RevenueSource;
        match self {
            Self::DCR | Self::ALPH => RevenueSource::Blake3External,
            Self::KAS => RevenueSource::KHeavyHashExternal,
            Self::ETC | Self::EVR | Self::MEWC => RevenueSource::EthashExternal,
            Self::RVN | Self::CLORE => RevenueSource::KawPowExternal,
            Self::ERG => RevenueSource::AutolykosExternal,
            Self::XMR => RevenueSource::RandomXExternal,
            Self::FLUX => RevenueSource::ZelHashExternal,
            Self::VRSC => RevenueSource::VerusHashExternal,
        }
    }
}

impl fmt::Display for ExternalCoin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.ticker())
    }
}

// ── Stratum protocol variant ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StratumProtocol {
    /// Standard Stratum v1 (mining.subscribe / mining.authorize / mining.submit)
    Stratum,
    /// EthStratum / ETH-proxy variant (eth_submitWork, eth_getWork)
    EthStratum,
    /// Zcash/Equihash-style Stratum — used by VRSC/VerusHash pools (LuckPool).
    /// Uses mining.subscribe/authorize/notify/set_target and 5-param submit.
    ZcashStratum,
}

impl StratumProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stratum => "stratum",
            Self::EthStratum => "ethstratum",
            Self::ZcashStratum => "zcashstratum",
        }
    }
}

// ── Coin profile (full metadata snapshot) ────────────────────────────

/// Complete profile for an external coin — enough to connect and mine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinProfile {
    pub coin: ExternalCoin,
    pub ticker: String,
    pub algorithm: String,
    pub pool_host: String,
    pub pool_port: u16,
    pub protocol: StratumProtocol,
    pub worker_name: String,
    pub enabled: bool,
}

impl CoinProfile {
    /// Build a default profile for a coin, splitting `default_pool()` into host:port.
    pub fn default_for(coin: ExternalCoin) -> Self {
        let (host, port) = split_host_port(coin.default_pool());
        Self {
            coin,
            ticker: coin.ticker().to_string(),
            algorithm: coin.algorithm().to_string(),
            pool_host: host,
            pool_port: port,
            protocol: coin.protocol(),
            worker_name: "zion_dynamic".to_string(),
            enabled: true,
        }
    }

    /// Build profile with pool preference + region fallback chain.
    pub fn for_preference(coin: ExternalCoin, preference: PoolPreference, region: &str) -> Self {
        let pool = coin.best_pool(preference, region);
        let (host, port) = split_host_port(&pool);
        Self {
            coin,
            ticker: coin.ticker().to_string(),
            algorithm: coin.algorithm().to_string(),
            pool_host: host,
            pool_port: port,
            protocol: coin.protocol(),
            worker_name: "zion_dynamic".to_string(),
            enabled: true,
        }
    }

    /// Stratum address as "host:port" string.
    pub fn pool_address(&self) -> String {
        format!("{}:{}", self.pool_host, self.pool_port)
    }
}

// ── Profitability snapshot ───────────────────────────────────────────

/// A single profitability estimate for a coin (e.g. from WhatToMine or fallback).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitEntry {
    pub coin: ExternalCoin,
    pub revenue_per_day_usd: f64,
    pub power_cost_usd: f64,
}

impl ProfitEntry {
    pub fn profit_per_day_usd(&self) -> f64 {
        self.revenue_per_day_usd - self.power_cost_usd
    }
}

/// Static fallback profitability estimates when WhatToMine is unavailable.
/// Values are approximate daily USD revenue per 100 MH/s reference hashrate.
pub fn fallback_estimates() -> Vec<ProfitEntry> {
    vec![
        ProfitEntry {
            coin: ExternalCoin::KAS,
            revenue_per_day_usd: 0.85,
            power_cost_usd: 0.10,
        },
        ProfitEntry {
            coin: ExternalCoin::ETC,
            revenue_per_day_usd: 0.60,
            power_cost_usd: 0.12,
        },
        ProfitEntry {
            coin: ExternalCoin::ALPH,
            revenue_per_day_usd: 0.55,
            power_cost_usd: 0.08,
        },
        ProfitEntry {
            coin: ExternalCoin::FLUX,
            revenue_per_day_usd: 0.50,
            power_cost_usd: 0.10,
        },
        ProfitEntry {
            coin: ExternalCoin::DCR,
            revenue_per_day_usd: 0.45,
            power_cost_usd: 0.08,
        },
        ProfitEntry {
            coin: ExternalCoin::ERG,
            revenue_per_day_usd: 0.40,
            power_cost_usd: 0.10,
        },
        ProfitEntry {
            coin: ExternalCoin::RVN,
            revenue_per_day_usd: 0.35,
            power_cost_usd: 0.12,
        },
        ProfitEntry {
            coin: ExternalCoin::CLORE,
            revenue_per_day_usd: 0.30,
            power_cost_usd: 0.10,
        },
        ProfitEntry {
            coin: ExternalCoin::EVR,
            revenue_per_day_usd: 0.20,
            power_cost_usd: 0.08,
        },
        ProfitEntry {
            coin: ExternalCoin::MEWC,
            revenue_per_day_usd: 0.15,
            power_cost_usd: 0.06,
        },
        ProfitEntry {
            coin: ExternalCoin::XMR,
            revenue_per_day_usd: 0.12,
            power_cost_usd: 0.03,
        },
        ProfitEntry {
            coin: ExternalCoin::VRSC,
            revenue_per_day_usd: 0.08,
            power_cost_usd: 0.01,
        },
    ]
}

// ── Live profit fetching ─────────────────────────────────────────────

/// Fetch live profitability estimates from WhatToMine API.
///
/// WhatToMine provides `https://whattomine.com/coins.json` with per-coin
/// revenue estimates in USD per GH/s-day.  We map the coin tags to our
/// `ExternalCoin` enum and return `Vec<ProfitEntry>`.
///
/// On any error (network, parse, empty), falls back to `fallback_estimates()`.
pub fn fetch_live_profit_estimates() -> Vec<ProfitEntry> {
    let url = "https://whattomine.com/coins.json";
    match fetch_url_blocking_internal(url, 10) {
        Ok(body) => parse_whattomine_for_external_coins(&body),
        Err(e) => {
            eprintln!("profit_router: whattomine fetch error: {e}");
            fallback_estimates()
        }
    }
}

/// Parse WhatToMine coins.json response into `Vec<ProfitEntry>`.
///
/// WhatToMine returns: `{ "coins": { "1": { "tag": "DCR", "revenue": "0.45", ... } } }`
fn parse_whattomine_for_external_coins(body: &str) -> Vec<ProfitEntry> {
    let parsed: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let Some(json) = parsed else {
        eprintln!("profit_router: whattomine parse error");
        return fallback_estimates();
    };

    let mut entries = Vec::new();
    let fallback = fallback_estimates();

    if let Some(coins) = json.get("coins").and_then(|c| c.as_object()) {
        for (_id, coin_data) in coins {
            let tag = coin_data
                .get("tag")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let revenue = coin_data
                .get("revenue")
                .and_then(|r| r.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);

            // Map WhatToMine coin tags to our ExternalCoin enum.
            if let Some(coin) = tag_to_external_coin(tag) {
                // Use the fallback power cost for this coin.
                let power_cost = fallback
                    .iter()
                    .find(|e| e.coin == coin)
                    .map(|e| e.power_cost_usd)
                    .unwrap_or(0.10);
                entries.push(ProfitEntry {
                    coin,
                    revenue_per_day_usd: revenue.max(0.01),
                    power_cost_usd: power_cost,
                });
            }
        }
    }

    // If we got fewer entries than fallback, merge in any missing coins.
    for fb in &fallback {
        if !entries.iter().any(|e| e.coin == fb.coin) {
            entries.push(fb.clone());
        }
    }

    if entries.is_empty() {
        fallback_estimates()
    } else {
        entries
    }
}

/// Map a WhatToMine coin tag to our `ExternalCoin` enum.
fn tag_to_external_coin(tag: &str) -> Option<ExternalCoin> {
    match tag.to_uppercase().as_str() {
        "DCR" => Some(ExternalCoin::DCR),
        "ALPH" => Some(ExternalCoin::ALPH),
        "KAS" => Some(ExternalCoin::KAS),
        "ERG" => Some(ExternalCoin::ERG),
        "RVN" => Some(ExternalCoin::RVN),
        "ETC" => Some(ExternalCoin::ETC),
        "XMR" => Some(ExternalCoin::XMR),
        "FLUX" => Some(ExternalCoin::FLUX),
        "CLORE" => Some(ExternalCoin::CLORE),
        _ => None, // EVR, MEWC not on WhatToMine
    }
}

/// Fetch a URL with a timeout using a blocking reqwest client.
fn fetch_url_blocking_internal(url: &str, timeout_secs: u64) -> Result<String, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime error: {e}"))?;

    rt.block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .user_agent("ZION-Pool/3.0.4")
            .build()
            .map_err(|e| format!("reqwest client error: {e}"))?;

        let resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("reqwest send error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        resp.text()
            .await
            .map_err(|e| format!("reqwest body error: {e}"))
    })
}

// ── Coin selection ───────────────────────────────────────────────────

/// Pick the most profitable coin from `entries`, applying hysteresis:
/// only switch away from `current` if another coin beats it by ≥ `hysteresis_pct`%.
///
/// Returns `None` if `entries` is empty.
pub fn select_best_coin(
    entries: &[ProfitEntry],
    current: Option<ExternalCoin>,
    hysteresis_pct: f64,
) -> Option<ExternalCoin> {
    if entries.is_empty() {
        return None;
    }

    let mut best = &entries[0];
    for entry in &entries[1..] {
        if entry.profit_per_day_usd() > best.profit_per_day_usd() {
            best = entry;
        }
    }

    if best.profit_per_day_usd() <= 0.0 {
        return None;
    }

    // Apply hysteresis: only switch if the new coin is `hysteresis_pct`% better
    if let Some(cur) = current {
        if cur == best.coin {
            return Some(cur);
        }
        let cur_profit = entries
            .iter()
            .find(|e| e.coin == cur)
            .map(|e| e.profit_per_day_usd())
            .unwrap_or(0.0);

        if cur_profit > 0.0 {
            let improvement_pct = (best.profit_per_day_usd() - cur_profit) / cur_profit * 100.0;
            if improvement_pct < hysteresis_pct {
                return Some(cur);
            }
        }
    }

    Some(best.coin)
}

// ── Helpers ──────────────────────────────────────────────────────────

fn split_host_port(addr: &str) -> (String, u16) {
    if let Some(pos) = addr.rfind(':') {
        let host = addr[..pos].to_string();
        let port = addr[pos + 1..].parse::<u16>().unwrap_or(3333);
        (host, port)
    } else {
        (addr.to_string(), 3333)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dcr_uses_blake3() {
        assert_eq!(ExternalCoin::DCR.algorithm(), "blake3");
        assert!(ExternalCoin::DCR.is_blake3());
        assert_eq!(ExternalCoin::DCR.default_pool(), "pool.woolypooly.com:3152");
    }

    #[test]
    fn alph_uses_blake3() {
        assert_eq!(ExternalCoin::ALPH.algorithm(), "blake3");
        assert!(ExternalCoin::ALPH.is_blake3());
    }

    #[test]
    fn blake3_coins_returns_dcr_and_alph() {
        let blake3 = ExternalCoin::blake3_coins();
        assert_eq!(blake3.len(), 2);
        assert!(blake3.contains(&ExternalCoin::DCR));
        assert!(blake3.contains(&ExternalCoin::ALPH));
    }

    #[test]
    fn from_str_loose_parses_dcr_aliases() {
        assert_eq!(ExternalCoin::from_str_loose("dcr"), Some(ExternalCoin::DCR));
        assert_eq!(
            ExternalCoin::from_str_loose("Decred"),
            Some(ExternalCoin::DCR)
        );
        assert_eq!(
            ExternalCoin::from_str_loose("BLAKE3-DCR"),
            Some(ExternalCoin::DCR)
        );
        assert_eq!(
            ExternalCoin::from_str_loose("blake3dcr"),
            Some(ExternalCoin::DCR)
        );
    }

    #[test]
    fn from_str_loose_parses_others() {
        assert_eq!(
            ExternalCoin::from_str_loose("alph"),
            Some(ExternalCoin::ALPH)
        );
        assert_eq!(ExternalCoin::from_str_loose("KAS"), Some(ExternalCoin::KAS));
        assert_eq!(ExternalCoin::from_str_loose("xmr"), Some(ExternalCoin::XMR));
        assert_eq!(ExternalCoin::from_str_loose("unknown"), None);
    }

    #[test]
    fn coin_profile_default_for_dcr() {
        let profile = CoinProfile::default_for(ExternalCoin::DCR);
        assert_eq!(profile.ticker, "DCR");
        assert_eq!(profile.algorithm, "blake3");
        assert_eq!(profile.pool_host, "pool.woolypooly.com");
        assert_eq!(profile.pool_port, 3152);
        assert_eq!(profile.protocol, StratumProtocol::Stratum);
        assert!(profile.enabled);
    }

    #[test]
    fn select_best_coin_picks_highest_profit() {
        let entries = vec![
            ProfitEntry {
                coin: ExternalCoin::DCR,
                revenue_per_day_usd: 0.45,
                power_cost_usd: 0.08,
            },
            ProfitEntry {
                coin: ExternalCoin::KAS,
                revenue_per_day_usd: 0.85,
                power_cost_usd: 0.10,
            },
            ProfitEntry {
                coin: ExternalCoin::ALPH,
                revenue_per_day_usd: 0.55,
                power_cost_usd: 0.08,
            },
        ];
        let best = select_best_coin(&entries, None, 5.0);
        assert_eq!(best, Some(ExternalCoin::KAS));
    }

    #[test]
    fn select_best_coin_hysteresis_keeps_current() {
        let entries = vec![
            ProfitEntry {
                coin: ExternalCoin::DCR,
                revenue_per_day_usd: 0.45,
                power_cost_usd: 0.08,
            },
            ProfitEntry {
                coin: ExternalCoin::ALPH,
                revenue_per_day_usd: 0.49,
                power_cost_usd: 0.08,
            },
        ];
        // ALPH is ~10.8% better, but hysteresis is 15% → stay on DCR
        let best = select_best_coin(&entries, Some(ExternalCoin::DCR), 15.0);
        assert_eq!(best, Some(ExternalCoin::DCR));
    }

    #[test]
    fn select_best_coin_hysteresis_switches_when_large_gap() {
        let entries = vec![
            ProfitEntry {
                coin: ExternalCoin::DCR,
                revenue_per_day_usd: 0.30,
                power_cost_usd: 0.08,
            },
            ProfitEntry {
                coin: ExternalCoin::KAS,
                revenue_per_day_usd: 0.85,
                power_cost_usd: 0.10,
            },
        ];
        // KAS is ~240% better → switch even with 15% hysteresis
        let best = select_best_coin(&entries, Some(ExternalCoin::DCR), 15.0);
        assert_eq!(best, Some(ExternalCoin::KAS));
    }

    #[test]
    fn fallback_estimates_include_dcr() {
        let estimates = fallback_estimates();
        assert!(estimates.iter().any(|e| e.coin == ExternalCoin::DCR));
        let dcr = estimates
            .iter()
            .find(|e| e.coin == ExternalCoin::DCR)
            .unwrap();
        assert!(dcr.revenue_per_day_usd > 0.0);
        assert!(dcr.profit_per_day_usd() > 0.0);
    }

    #[test]
    fn all_coins_have_distinct_pools() {
        let all = ExternalCoin::all();
        let mut pools: Vec<&str> = all.iter().map(|c| c.default_pool()).collect();
        pools.sort();
        pools.dedup();
        assert_eq!(pools.len(), all.len());
    }

    #[test]
    fn display_shows_ticker() {
        assert_eq!(format!("{}", ExternalCoin::DCR), "DCR");
        assert_eq!(format!("{}", ExternalCoin::ALPH), "ALPH");
    }

    #[test]
    fn nicehash_supported_coin_gets_nh_endpoint() {
        let pool = ExternalCoin::KAS.best_pool(PoolPreference::NiceHash, "eu");
        assert_eq!(pool, "kheavyhash.eu.nicehash.com:9024");
    }

    #[test]
    fn nicehash_blake3_coin_falls_back() {
        let pool = ExternalCoin::DCR.best_pool(PoolPreference::NiceHash, "eu");
        assert_eq!(pool, "pool.woolypooly.com:3152");
    }

    #[test]
    fn profile_for_preference_uses_selected_pool() {
        let profile =
            CoinProfile::for_preference(ExternalCoin::KAS, PoolPreference::NiceHash, "eu");
        assert_eq!(profile.pool_host, "kheavyhash.eu.nicehash.com");
        assert_eq!(profile.pool_port, 9024);
    }
}
