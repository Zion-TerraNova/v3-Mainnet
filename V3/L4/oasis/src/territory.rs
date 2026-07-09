//! Territory System — mining zones controlled by guilds.
//!
//! Territories are regions of the ZION network that can be claimed
//! and defended by guilds. Controlling a territory grants mining bonuses.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Territory bonus types
pub const TERRITORY_MINING_BONUS: f64 = 0.10; // 10% mining bonus
pub const TERRITORY_XP_BONUS: f64 = 0.05; // 5% XP bonus
pub const TERRITORY_CLAIM_COST: u64 = 10_000; // ZION cost to claim
pub const TERRITORY_DEFENSE_PERIOD: u64 = 24 * 3600; // 24 hours

/// A mining territory in the OASIS world
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Territory {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Region type
    pub region: Region,
    /// Controlling guild ID (None = unclaimed)
    pub controller: Option<String>,
    /// Mining bonus multiplier
    pub mining_bonus: f64,
    /// XP bonus multiplier
    pub xp_bonus: f64,
    /// When the territory was claimed (Unix seconds)
    pub claimed_at: Option<u64>,
    /// Defense power (guild contributions)
    pub defense_power: u64,
    /// Adjacent territory IDs
    pub adjacent: Vec<String>,
    /// Max concurrent miners in this territory
    pub capacity: u32,
    /// Current miners
    pub active_miners: Vec<String>,
    /// When the territory was last contested (Unix seconds). 0 = never.
    #[serde(default)]
    pub last_contested: u64,
}

/// Region types — thematic areas of the OASIS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Region {
    /// Mountains — high difficulty, high reward
    Mountains,
    /// Forest — balanced
    Forest,
    /// Desert — CPU-friendly
    Desert,
    /// Ocean — GPU-optimized
    Ocean,
    /// Volcano — extreme difficulty, extreme reward
    Volcano,
    /// Crystal Caves — NCL compute tasks
    CrystalCaves,
    /// Temple — meditation & consciousness
    Temple,
    /// Nexus — cross-chain portal zone
    Nexus,
}

impl Region {
    pub fn base_difficulty(&self) -> f64 {
        match self {
            Region::Mountains => 1.5,
            Region::Forest => 1.0,
            Region::Desert => 0.8,
            Region::Ocean => 1.2,
            Region::Volcano => 2.0,
            Region::CrystalCaves => 1.3,
            Region::Temple => 0.5,
            Region::Nexus => 1.8,
        }
    }
}

impl Territory {
    pub fn new(id: String, name: String, region: Region) -> Self {
        Self {
            id,
            name,
            description: String::new(),
            mining_bonus: TERRITORY_MINING_BONUS * region.base_difficulty(),
            xp_bonus: TERRITORY_XP_BONUS,
            region,
            controller: None,
            claimed_at: None,
            defense_power: 0,
            adjacent: Vec::new(),
            capacity: 50,
            active_miners: Vec::new(),
            last_contested: 0,
        }
    }

    /// Claim territory for a guild
    pub fn claim(&mut self, guild_id: &str) -> Result<(), TerritoryError> {
        if self.controller.is_some() {
            return Err(TerritoryError::AlreadyClaimed);
        }
        self.controller = Some(guild_id.to_string());
        self.claimed_at = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );
        self.defense_power = 100; // base defense
        Ok(())
    }

    /// Contest this territory (guild war). Returns `Err(CooldownActive)` if
    /// the territory was contested within the last `TERRITORY_DEFENSE_PERIOD`
    /// seconds (24h). On success, updates `last_contested` and resolves the
    /// battle.
    pub fn contest(
        &mut self,
        attacking_guild: &str,
        attack_power: u64,
    ) -> Result<bool, TerritoryError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Enforce 24h cooldown between contests
        if self.last_contested > 0 {
            let elapsed = now.saturating_sub(self.last_contested);
            if elapsed < TERRITORY_DEFENSE_PERIOD {
                return Err(TerritoryError::CooldownActive {
                    remaining_secs: TERRITORY_DEFENSE_PERIOD - elapsed,
                });
            }
        }
        self.last_contested = now;
        if attack_power > self.defense_power {
            self.controller = Some(attacking_guild.to_string());
            self.defense_power = attack_power / 2; // weakened after battle
            self.claimed_at = Some(now);
            Ok(true) // territory captured
        } else {
            self.defense_power = self.defense_power.saturating_sub(attack_power / 3);
            Ok(false) // defense held
        }
    }

    /// Seconds remaining until this territory can be contested again.
    pub fn contest_cooldown_remaining(&self) -> u64 {
        if self.last_contested == 0 {
            return 0;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let elapsed = now.saturating_sub(self.last_contested);
        TERRITORY_DEFENSE_PERIOD.saturating_sub(elapsed)
    }

    /// Is this territory controlled by a specific guild?
    pub fn is_controlled_by(&self, guild_id: &str) -> bool {
        self.controller.as_deref() == Some(guild_id)
    }

    /// Add a miner to the territory
    pub fn enter(&mut self, address: &str) -> Result<(), TerritoryError> {
        if self.active_miners.len() >= self.capacity as usize {
            return Err(TerritoryError::AtCapacity);
        }
        if !self.active_miners.contains(&address.to_string()) {
            self.active_miners.push(address.to_string());
        }
        Ok(())
    }

    /// Remove a miner from the territory
    pub fn leave(&mut self, address: &str) {
        self.active_miners.retain(|m| m != address);
    }
}

#[derive(Debug, Clone)]
pub enum TerritoryError {
    AlreadyClaimed,
    NotClaimed,
    NotController,
    AtCapacity,
    InsufficientLevel,
    CooldownActive { remaining_secs: u64 },
}

impl std::fmt::Display for TerritoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyClaimed => write!(f, "Territory already claimed"),
            Self::NotClaimed => write!(f, "Territory not claimed"),
            Self::NotController => write!(f, "Not the territory controller"),
            Self::AtCapacity => write!(f, "Territory at full capacity"),
            Self::InsufficientLevel => write!(f, "Insufficient level to claim"),
            Self::CooldownActive { remaining_secs } => {
                write!(
                    f,
                    "Territory contest cooldown active: {}s remaining",
                    remaining_secs
                )
            }
        }
    }
}

/// Territory world map
#[derive(Debug, Serialize)]
pub struct TerritoryMap {
    territories: HashMap<String, Territory>,
}

impl Default for TerritoryMap {
    fn default() -> Self {
        Self::new()
    }
}

impl TerritoryMap {
    pub fn new() -> Self {
        Self {
            territories: HashMap::new(),
        }
    }

    pub fn add_territory(&mut self, territory: Territory) {
        self.territories.insert(territory.id.clone(), territory);
    }

    pub fn get(&self, id: &str) -> Option<&Territory> {
        self.territories.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Territory> {
        self.territories.get_mut(id)
    }

    /// Get territories controlled by a guild
    pub fn guild_territories(&self, guild_id: &str) -> Vec<&Territory> {
        self.territories
            .values()
            .filter(|t| t.is_controlled_by(guild_id))
            .collect()
    }

    /// Get unclaimed territories
    pub fn unclaimed(&self) -> Vec<&Territory> {
        self.territories
            .values()
            .filter(|t| t.controller.is_none())
            .collect()
    }

    /// Generate the initial OASIS world map
    pub fn genesis_map() -> Self {
        let mut map = Self::new();

        let regions = vec![
            ("mount_zion", "Mount Zion", Region::Mountains),
            ("cedar_forest", "Cedar Forest", Region::Forest),
            ("negev_desert", "Negev Desert", Region::Desert),
            ("galilee_sea", "Sea of Galilee", Region::Ocean),
            ("masada_forge", "Masada Forge", Region::Volcano),
            (
                "crystal_mines",
                "Crystal Mines of Solomon",
                Region::CrystalCaves,
            ),
            ("temple_mount", "Temple of Consciousness", Region::Temple),
            ("babel_nexus", "Babel Nexus", Region::Nexus),
        ];

        for (id, name, region) in regions {
            map.add_territory(Territory::new(id.into(), name.into(), region));
        }

        map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claim_territory() {
        let mut t = Territory::new("t1".into(), "Test".into(), Region::Forest);
        assert!(t.claim("guild1").is_ok());
        assert!(t.is_controlled_by("guild1"));
    }

    #[test]
    fn test_cannot_double_claim() {
        let mut t = Territory::new("t1".into(), "Test".into(), Region::Forest);
        t.claim("guild1").unwrap();
        assert!(t.claim("guild2").is_err());
    }

    #[test]
    fn test_contest_success() {
        let mut t = Territory::new("t1".into(), "Test".into(), Region::Forest);
        t.claim("guild1").unwrap();
        // Attack with more power than defense (100)
        let captured = t.contest("guild2", 200).unwrap();
        assert!(captured);
        assert!(t.is_controlled_by("guild2"));
    }

    #[test]
    fn test_contest_cooldown() {
        let mut t = Territory::new("t1".into(), "Test".into(), Region::Forest);
        t.claim("guild1").unwrap();
        // First contest succeeds
        let result = t.contest("guild2", 50);
        assert!(result.is_ok());
        // Second contest immediately after should fail with cooldown
        let result = t.contest("guild2", 200);
        assert!(result.is_err());
        match result.unwrap_err() {
            TerritoryError::CooldownActive { remaining_secs } => {
                assert!(remaining_secs > 0);
            }
            _ => panic!("expected CooldownActive error"),
        }
    }

    #[test]
    fn test_genesis_map() {
        let map = TerritoryMap::genesis_map();
        assert_eq!(map.unclaimed().len(), 8);
        assert!(map.get("temple_mount").is_some());
    }
}
