//! Quest System — Avatar Quest Lines
//!
//! Each of the 201 avatars offers a quest line (typically 5 quests).
//! Completing a quest awards XP and progresses the Golden Egg eligibility.

use crate::db::OasisDb;
use crate::error::{OasisError, OasisResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Avatar metadata (loaded from avatars.json)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvatarDef {
    pub id: u16,
    pub name: String,
    pub subtitle: String,
    pub ray: String,
    pub role: String,
    pub location: String,
    pub quest_line: String,
    pub teaching: String,
    pub ability: String,
    pub consciousness_level_required: u8,
    pub consciousness_level_name: String,
    pub key: String,
    pub rarity: String,
}

impl AvatarDef {
    /// Extract from a JSON avatar value.
    pub fn from_json(val: &serde_json::Value) -> Option<Self> {
        Some(Self {
            id: val.get("id")?.as_u64()? as u16,
            name: val.get("name")?.as_str()?.to_string(),
            subtitle: val
                .get("subtitle")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ray: val
                .get("ray")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            role: val
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            location: val
                .get("location")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            quest_line: val
                .get("quest_line")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            teaching: val
                .get("teaching")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ability: val
                .get("ability")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            consciousness_level_required: val
                .get("consciousness_level_required")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u8,
            consciousness_level_name: val
                .get("consciousness_level_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            key: val
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            rarity: val
                .get("rarity")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
    }
}

/// A single quest definition (loaded from avatars.json)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuestDef {
    pub quest_id: String,
    pub avatar_id: u16,
    pub avatar_name: String,
    pub title: String,
    pub description: String,
    pub xp_reward: u64,
    pub min_consciousness_level: u8,
}

/// Player progress on a single quest
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuestProgress {
    pub player_address: String,
    pub quest_id: String,
    pub completed: bool,
    pub completed_at: Option<u64>,
}

/// In-memory registry of all quest definitions and avatar metadata loaded from JSON.
#[derive(Debug, Clone, Default)]
pub struct QuestRegistry {
    quests: Vec<QuestDef>,
    avatars: Vec<AvatarDef>,
    by_id: HashMap<String, usize>,
    by_avatar: HashMap<u16, Vec<usize>>,
    by_avatar_id: HashMap<u16, usize>,
}

impl QuestRegistry {
    /// Load quests and avatars from `avatars.json` (embedded or file path).
    pub fn from_avatars_json(data: &str) -> OasisResult<Self> {
        let root: serde_json::Value =
            serde_json::from_str(data).map_err(|e| OasisError::Serialization(e.to_string()))?;
        let arr = root
            .get("avatars")
            .and_then(|v| v.as_array())
            .ok_or_else(|| OasisError::Serialization("missing avatars array".into()))?;

        let mut reg = Self::default();
        for (avatar_idx, avatar_val) in arr.iter().enumerate() {
            let avatar_id = avatar_val
                .get("id")
                .and_then(|v| v.as_u64())
                .unwrap_or(avatar_idx as u64) as u16;
            let avatar_name = avatar_val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Register avatar metadata
            if let Some(avatar) = AvatarDef::from_json(avatar_val) {
                reg.by_avatar_id.insert(avatar_id, reg.avatars.len());
                reg.avatars.push(avatar);
            }

            let quests_arr = avatar_val
                .get("quests")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for (q_idx, q_val) in quests_arr.iter().enumerate() {
                let title = q_val
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let description = q_val
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let quest_id = format!("{}_{}", avatar_id, q_idx);
                let def = QuestDef {
                    quest_id: quest_id.clone(),
                    avatar_id,
                    avatar_name: avatar_name.clone(),
                    title,
                    description,
                    xp_reward: 2000 * (q_idx as u64 + 1), // base scaling
                    min_consciousness_level: 1,
                };
                let idx = reg.quests.len();
                reg.by_id.insert(quest_id, idx);
                reg.by_avatar.entry(avatar_id).or_default().push(idx);
                reg.quests.push(def);
            }
        }
        Ok(reg)
    }

    /// Avatar filtering — by Ray, Consciousness Level, or Rarity.
    pub fn filter_avatars(
        &self,
        ray: Option<&str>,
        min_cl: Option<u8>,
        rarity: Option<&str>,
    ) -> Vec<&AvatarDef> {
        self.avatars
            .iter()
            .filter(|a| ray.is_none_or(|r| a.ray.eq_ignore_ascii_case(r)))
            .filter(|a| min_cl.is_none_or(|c| a.consciousness_level_required >= c))
            .filter(|a| rarity.is_none_or(|r| a.rarity.eq_ignore_ascii_case(r)))
            .collect()
    }

    /// Lookup avatar by ID.
    pub fn get_avatar(&self, avatar_id: u16) -> Option<&AvatarDef> {
        self.by_avatar_id.get(&avatar_id).map(|&i| &self.avatars[i])
    }

    /// All avatars.
    pub fn all_avatars(&self) -> &[AvatarDef] {
        &self.avatars
    }

    pub fn get(&self, quest_id: &str) -> Option<&QuestDef> {
        self.by_id.get(quest_id).map(|&i| &self.quests[i])
    }

    pub fn by_avatar(&self, avatar_id: u16) -> Vec<&QuestDef> {
        self.by_avatar
            .get(&avatar_id)
            .map(|indices| indices.iter().map(|&i| &self.quests[i]).collect())
            .unwrap_or_default()
    }

    pub fn all(&self) -> &[QuestDef] {
        &self.quests
    }

    pub fn len(&self) -> usize {
        self.quests.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quests.is_empty()
    }
}

/// Quest manager — wraps registry + database persistence.
pub struct QuestManager {
    pub registry: QuestRegistry,
}

impl QuestManager {
    pub fn new(registry: QuestRegistry) -> Self {
        Self { registry }
    }

    /// Get all quest progress for a player.
    pub fn player_progress(&self, db: &OasisDb, address: &str) -> OasisResult<Vec<QuestProgress>> {
        db.list_quest_progress(address)
    }

    /// Mark a quest as completed for a player. Awards XP if not already done.
    pub fn complete_quest(
        &self,
        db: &OasisDb,
        address: &str,
        quest_id: &str,
    ) -> OasisResult<QuestProgress> {
        let _def = self
            .registry
            .get(quest_id)
            .ok_or_else(|| OasisError::PlayerNotFound(format!("quest {}", quest_id)))?;

        if let Some(existing) = db.get_quest_progress(address, quest_id)? {
            if existing.completed {
                return Ok(existing);
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let progress = QuestProgress {
            player_address: address.to_string(),
            quest_id: quest_id.to_string(),
            completed: true,
            completed_at: Some(now),
        };

        db.save_quest_progress(&progress)?;

        // Award XP via the player XP system (handled by caller / server)
        // We return progress so the server can call xp::award_xp
        Ok(progress)
    }

    /// Total quests completed by a player.
    pub fn completed_count(&self, db: &OasisDb, address: &str) -> OasisResult<u32> {
        db.count_completed_quests(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_AVATARS: &str = r#"{
        "avatars": [
            {
                "id": 1,
                "name": "Rama",
                "quests": [
                    {"title": "Exile", "description": "Choose dharma"},
                    {"title": "Deer", "description": "Illusion"}
                ]
            }
        ]
    }"#;

    #[test]
    fn test_registry_load() {
        let reg = QuestRegistry::from_avatars_json(TEST_AVATARS).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.get("1_0").is_some());
        assert_eq!(reg.get("1_0").unwrap().title, "Exile");
        assert_eq!(reg.get("1_1").unwrap().xp_reward, 4000);
    }

    #[test]
    fn test_by_avatar() {
        let reg = QuestRegistry::from_avatars_json(TEST_AVATARS).unwrap();
        assert_eq!(reg.by_avatar(1).len(), 2);
        assert_eq!(reg.by_avatar(99).len(), 0);
    }

    #[test]
    fn test_avatar_lookup() {
        let reg = QuestRegistry::from_avatars_json(TEST_AVATARS).unwrap();
        let avatar = reg.get_avatar(1).expect("Rama should exist");
        assert_eq!(avatar.name, "Rama");
        assert_eq!(avatar.id, 1);
        assert!(reg.get_avatar(99).is_none());
    }

    #[test]
    fn test_avatar_filter_by_ray() {
        let json = r#"{
            "avatars": [
                {"id": 1, "name": "Rama", "ray": "Blue", "rarity": "Epic", "consciousness_level_required": 4},
                {"id": 2, "name": "Sita", "ray": "Pink", "rarity": "Epic", "consciousness_level_required": 4},
                {"id": 3, "name": "Hanuman", "ray": "Ruby-Gold", "rarity": "Epic", "consciousness_level_required": 5}
            ]
        }"#;
        let reg = QuestRegistry::from_avatars_json(json).unwrap();
        let blue = reg.filter_avatars(Some("Blue"), None, None);
        assert_eq!(blue.len(), 1);
        assert_eq!(blue[0].name, "Rama");

        let epic = reg.filter_avatars(None, None, Some("Epic"));
        assert_eq!(epic.len(), 3);

        let cl5 = reg.filter_avatars(None, Some(5), None);
        assert_eq!(cl5.len(), 1);
        assert_eq!(cl5[0].name, "Hanuman");
    }

    #[test]
    fn test_load_real_avatars_json() {
        let data = include_str!("../data/avatars.json");
        let reg = QuestRegistry::from_avatars_json(data).unwrap();
        assert!(
            reg.all_avatars().len() >= 49,
            "expected at least 49 core avatars"
        );
        // Verify Krishna-Maitreya exists
        let km = reg.get_avatar(0).expect("Krishna-Maitreya should exist");
        assert_eq!(km.name, "Krishna-Maitreya");
        assert_eq!(km.consciousness_level_required, 9);
        // Verify all core avatars have quests
        let core_with_quests: Vec<_> = reg
            .all_avatars()
            .iter()
            .filter(|a| a.id <= 51 && !reg.by_avatar(a.id).is_empty())
            .collect();
        assert!(
            core_with_quests.len() >= 49,
            "expected all 49 core avatars to have quests"
        );
    }
}
