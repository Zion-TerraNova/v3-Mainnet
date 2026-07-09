//! Consciousness Combat Mechanics
//!
//! Combat in ZION OASIS is not physical violence — it is a test of
//! awareness, will, and spiritual discipline. Higher consciousness levels
//! unlock superior defensive and offensive capabilities.

use crate::consciousness::ConsciousnessLevel;
use serde::{Deserialize, Serialize};

/// A combatant in consciousness combat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Combatant {
    pub address: String,
    pub display_name: String,
    pub level: ConsciousnessLevel,
    pub current_hp: u32,
    pub max_hp: u32,
    pub energy: u32, // used for special abilities
}

/// Consciousness-based damage calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatAction {
    pub action_type: ActionType,
    pub attacker_level: ConsciousnessLevel,
    pub defender_level: ConsciousnessLevel,
    pub base_damage: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionType {
    Strike,      // basic attack
    Meditate,    // heal / energy restore
    SoulShield,  // CL4+ defensive buff
    DharmaBlast, // CL5+ area attack
    CosmicRay,   // CL7+ high damage
    UnityPulse,  // CL8+ team heal
    KeterBeam,   // CL9 ultimate
}

/// Result of a combat action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatResult {
    pub damage_dealt: u32,
    pub healing_done: u32,
    pub energy_cost: u32,
    pub leveled_up: bool,
    pub new_level: Option<ConsciousnessLevel>,
}

/// Consciousness combat engine
pub struct CombatEngine;

impl CombatEngine {
    /// Calculate effective damage based on consciousness delta
    pub fn calculate_damage(action: &CombatAction) -> u32 {
        let attacker_cl = action.attacker_level as u8;
        let defender_cl = action.defender_level as u8;
        let level_delta = (attacker_cl as i16 - defender_cl as i16).max(-5);

        // Multiplier: +20% per level advantage, -10% per level deficit (min 10%)
        let multiplier = 1.0 + (level_delta as f64 * 0.2);
        let base = action.base_damage as f64 * multiplier;
        base.max(1.0) as u32
    }

    /// Process a combat action and return the result
    pub fn resolve(
        action: &CombatAction,
        attacker: &mut Combatant,
        defender: &mut Combatant,
    ) -> CombatResult {
        let mut result = CombatResult {
            damage_dealt: 0,
            healing_done: 0,
            energy_cost: 0,
            leveled_up: false,
            new_level: None,
        };

        match action.action_type {
            ActionType::Strike => {
                let dmg = Self::calculate_damage(action);
                defender.current_hp = defender.current_hp.saturating_sub(dmg);
                result.damage_dealt = dmg;
                result.energy_cost = 10;
            }
            ActionType::Meditate => {
                let heal = 50 + (action.attacker_level as u32 * 20);
                attacker.current_hp = (attacker.current_hp + heal).min(attacker.max_hp);
                attacker.energy = (attacker.energy + 30).min(100);
                result.healing_done = heal;
                result.energy_cost = 0;
            }
            ActionType::SoulShield => {
                if action.attacker_level as u8 >= 4 {
                    attacker.energy = attacker.energy.saturating_sub(20);
                    // Buff logic would go here (reduce next damage)
                    result.energy_cost = 20;
                }
            }
            ActionType::DharmaBlast => {
                if action.attacker_level as u8 >= 5 {
                    let dmg = Self::calculate_damage(action) * 2;
                    defender.current_hp = defender.current_hp.saturating_sub(dmg);
                    result.damage_dealt = dmg;
                    result.energy_cost = 40;
                }
            }
            ActionType::CosmicRay => {
                if action.attacker_level as u8 >= 7 {
                    let dmg = Self::calculate_damage(action) * 3;
                    defender.current_hp = defender.current_hp.saturating_sub(dmg);
                    result.damage_dealt = dmg;
                    result.energy_cost = 60;
                }
            }
            ActionType::UnityPulse => {
                if action.attacker_level as u8 >= 8 {
                    let heal = 200;
                    attacker.current_hp = (attacker.current_hp + heal).min(attacker.max_hp);
                    result.healing_done = heal;
                    result.energy_cost = 50;
                }
            }
            ActionType::KeterBeam => {
                if action.attacker_level as u8 >= 9 {
                    let dmg = Self::calculate_damage(action) * 5;
                    defender.current_hp = defender.current_hp.saturating_sub(dmg);
                    result.damage_dealt = dmg;
                    result.energy_cost = 100;
                }
            }
        }

        attacker.energy = attacker.energy.saturating_sub(result.energy_cost);
        result
    }

    /// Initial HP based on consciousness level
    pub fn base_hp(level: ConsciousnessLevel) -> u32 {
        100 + (level as u32 * 50)
    }

    /// Initial energy pool
    pub fn base_energy(_level: ConsciousnessLevel) -> u32 {
        100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strike_damage() {
        let action = CombatAction {
            action_type: ActionType::Strike,
            attacker_level: ConsciousnessLevel::Spiritual, // 5
            defender_level: ConsciousnessLevel::Physical,  // 1
            base_damage: 100,
        };
        let dmg = CombatEngine::calculate_damage(&action);
        assert!(dmg > 100); // level advantage
    }

    #[test]
    fn test_meditate_heal() {
        let action = CombatAction {
            action_type: ActionType::Meditate,
            attacker_level: ConsciousnessLevel::Spiritual,
            defender_level: ConsciousnessLevel::Physical,
            base_damage: 0,
        };
        let mut attacker = Combatant {
            address: "a".into(),
            display_name: "A".into(),
            level: ConsciousnessLevel::Spiritual,
            current_hp: 50,
            max_hp: 100,
            energy: 50,
        };
        let mut defender = attacker.clone();
        let result = CombatEngine::resolve(&action, &mut attacker, &mut defender);
        assert!(result.healing_done > 0);
        assert!(attacker.current_hp > 50);
    }

    #[test]
    fn test_keter_beam_requires_cl9() {
        let action = CombatAction {
            action_type: ActionType::KeterBeam,
            attacker_level: ConsciousnessLevel::OnTheStar,
            defender_level: ConsciousnessLevel::Physical,
            base_damage: 100,
        };
        let mut attacker = Combatant {
            address: "a".into(),
            display_name: "A".into(),
            level: ConsciousnessLevel::OnTheStar,
            current_hp: 1000,
            max_hp: 1000,
            energy: 100,
        };
        let mut defender = attacker.clone();
        let result = CombatEngine::resolve(&action, &mut attacker, &mut defender);
        assert!(result.damage_dealt > 0);
        assert_eq!(attacker.energy, 0); // costs 100 energy
    }
}
