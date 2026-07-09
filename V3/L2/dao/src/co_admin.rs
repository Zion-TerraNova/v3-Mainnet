//! Co-Admin Registry — multi-layer governance participant management.
//!
//! Tracks Co-Admins across L1–L6 with role, reputation, bonding, and term limits.
//! Used for admission proposals, Bodhisattva vow confirmations, cross-layer voting,
//! and expulsion/slashing procedures.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};
use crate::types::{CoAdmin, CoAdminRole, LayerId};

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// In-memory Co-Admin registry with lookups by address and layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoAdminRegistry {
    /// address → CoAdmin
    by_address: HashMap<String, CoAdmin>,
    /// layer → [addresses]
    by_layer: HashMap<LayerId, Vec<String>>,
    /// layer × role → [addresses]
    by_layer_role: HashMap<(LayerId, String), Vec<String>>,
}

impl CoAdminRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new Co-Admin. Fails if address already registered.
    pub fn register(&mut self, admin: CoAdmin) -> DaoResult<()> {
        if self.by_address.contains_key(&admin.address) {
            return Err(DaoError::Internal(format!(
                "CoAdmin {} already registered",
                admin.address
            )));
        }
        self.by_layer
            .entry(admin.layer)
            .or_default()
            .push(admin.address.clone());
        self.by_layer_role
            .entry((admin.layer, admin.role.role_name().to_string()))
            .or_default()
            .push(admin.address.clone());
        self.by_address.insert(admin.address.clone(), admin);
        Ok(())
    }

    /// Get Co-Admin by address.
    pub fn get(&self, address: &str) -> Option<&CoAdmin> {
        self.by_address.get(address)
    }

    /// Get mutable Co-Admin by address.
    pub fn get_mut(&mut self, address: &str) -> Option<&mut CoAdmin> {
        self.by_address.get_mut(address)
    }

    /// List all active Co-Admins in a given layer.
    pub fn layer_admins(&self, layer: LayerId) -> Vec<&CoAdmin> {
        self.by_layer
            .get(&layer)
            .map(|addrs| {
                addrs
                    .iter()
                    .filter_map(|a| self.by_address.get(a))
                    .filter(|admin| admin.is_active)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List all active Co-Admins with a given role in a given layer.
    pub fn role_admins(&self, layer: LayerId, role: CoAdminRole) -> Vec<&CoAdmin> {
        self.by_layer_role
            .get(&(layer, role.role_name().to_string()))
            .map(|addrs| {
                addrs
                    .iter()
                    .filter_map(|a| self.by_address.get(a))
                    .filter(|admin| admin.is_active && admin.role == role)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if address is an active Co-Admin in a layer.
    pub fn is_co_admin(&self, address: &str, layer: LayerId) -> bool {
        self.by_address
            .get(address)
            .map(|a| a.is_active && a.layer == layer)
            .unwrap_or(false)
    }

    /// Deactivate (soft-remove) a Co-Admin.
    pub fn deactivate(&mut self, address: &str) -> DaoResult<()> {
        let admin = self
            .by_address
            .get_mut(address)
            .ok_or_else(|| DaoError::Internal(format!("CoAdmin {} not found", address)))?;
        admin.is_active = false;
        Ok(())
    }

    /// Reactivate a previously deactivated Co-Admin.
    pub fn reactivate(&mut self, address: &str) -> DaoResult<()> {
        let admin = self
            .by_address
            .get_mut(address)
            .ok_or_else(|| DaoError::Internal(format!("CoAdmin {} not found", address)))?;
        admin.is_active = true;
        Ok(())
    }

    /// Slash a Co-Admin: deactivate, zero reputation, optionally burn bond.
    pub fn slash(&mut self, address: &str, burn_bond: bool) -> DaoResult<()> {
        let admin = self
            .by_address
            .get_mut(address)
            .ok_or_else(|| DaoError::Internal(format!("CoAdmin {} not found", address)))?;
        admin.is_active = false;
        admin.reputation = 0;
        if burn_bond {
            admin.bonded = 0;
        }
        Ok(())
    }

    /// Total active Co-Admins in a layer (for quorum / consent calculations).
    pub fn active_count(&self, layer: LayerId) -> usize {
        self.layer_admins(layer).len()
    }

    /// Count active Co-Admins with a specific role in a layer.
    pub fn active_role_count(&self, layer: LayerId, role: CoAdminRole) -> usize {
        self.role_admins(layer, role).len()
    }

    /// Update reputation for a Co-Admin.
    pub fn add_reputation(&mut self, address: &str, delta: i64) -> DaoResult<()> {
        let admin = self
            .by_address
            .get_mut(address)
            .ok_or_else(|| DaoError::Internal(format!("CoAdmin {} not found", address)))?;
        admin.reputation = admin
            .reputation
            .saturating_add(delta.max(0) as u64)
            .saturating_sub(delta.min(0).unsigned_abs());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_admin(name: &str, addr: &str, role: CoAdminRole, layer: LayerId) -> CoAdmin {
        CoAdmin {
            name: name.into(),
            address: addr.into(),
            public_key: format!("pk_{}", addr),
            role,
            layer,
            bonded: 1_000_000,
            reputation: 500,
            is_active: true,
            appointed_at: 100,
            term_end: None,
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut reg = CoAdminRegistry::new();
        let alice = sample_admin("Alice", "zion1alice", CoAdminRole::Treasury, 2);
        reg.register(alice).unwrap();

        assert!(reg.is_co_admin("zion1alice", 2));
        assert!(!reg.is_co_admin("zion1alice", 3));
        assert_eq!(reg.active_count(2), 1);
    }

    #[test]
    fn test_duplicate_registration_fails() {
        let mut reg = CoAdminRegistry::new();
        let a = sample_admin("A", "zion1a", CoAdminRole::Treasury, 2);
        reg.register(a.clone()).unwrap();
        assert!(reg.register(a).is_err());
    }

    #[test]
    fn test_layer_and_role_queries() {
        let mut reg = CoAdminRegistry::new();
        reg.register(sample_admin(
            "Alice",
            "zion1alice",
            CoAdminRole::Treasury,
            2,
        ))
        .unwrap();
        reg.register(sample_admin("Bob", "zion1bob", CoAdminRole::Bridge, 2))
            .unwrap();
        reg.register(sample_admin("Carol", "zion1carol", CoAdminRole::Relayer, 3))
            .unwrap();

        assert_eq!(reg.layer_admins(2).len(), 2);
        assert_eq!(reg.role_admins(2, CoAdminRole::Treasury).len(), 1);
        assert_eq!(reg.role_admins(2, CoAdminRole::Bridge).len(), 1);
        assert_eq!(reg.role_admins(3, CoAdminRole::Relayer).len(), 1);
    }

    #[test]
    fn test_deactivate_reactivate() {
        let mut reg = CoAdminRegistry::new();
        reg.register(sample_admin(
            "Alice",
            "zion1alice",
            CoAdminRole::Treasury,
            2,
        ))
        .unwrap();

        reg.deactivate("zion1alice").unwrap();
        assert_eq!(reg.active_count(2), 0);

        reg.reactivate("zion1alice").unwrap();
        assert_eq!(reg.active_count(2), 1);
    }

    #[test]
    fn test_slash() {
        let mut reg = CoAdminRegistry::new();
        reg.register(sample_admin(
            "Alice",
            "zion1alice",
            CoAdminRole::Treasury,
            2,
        ))
        .unwrap();

        reg.slash("zion1alice", true).unwrap();
        let a = reg.get("zion1alice").unwrap();
        assert!(!a.is_active);
        assert_eq!(a.reputation, 0);
        assert_eq!(a.bonded, 0);
    }

    #[test]
    fn test_reputation_change() {
        let mut reg = CoAdminRegistry::new();
        reg.register(sample_admin(
            "Alice",
            "zion1alice",
            CoAdminRole::Treasury,
            2,
        ))
        .unwrap();

        reg.add_reputation("zion1alice", 100).unwrap();
        assert_eq!(reg.get("zion1alice").unwrap().reputation, 600);

        reg.add_reputation("zion1alice", -200).unwrap();
        assert_eq!(reg.get("zion1alice").unwrap().reputation, 400);
    }
}
