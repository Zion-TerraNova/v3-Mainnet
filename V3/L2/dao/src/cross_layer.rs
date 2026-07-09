//! Cross-Layer Governance — proposals affecting multiple layers with veto support.
//!
//! Some DAO proposals span layers (e.g. adding a bridge chain affects L2 + L3).
//! This module tracks which layers must consent, records vetoes, and enforces
//! the cross-layer consent threshold.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{DaoError, DaoResult};
use crate::types::LayerId;

// ---------------------------------------------------------------------------
// Cross-Layer Proposal State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerConsent {
    Pending,   // Layer has not yet reviewed
    Consented, // Layer has consented (no objection)
    Vetoed,    // Layer has vetoed (blocks execution)
    Waived,    // Layer explicitly waives its veto right
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossLayerState {
    pub proposal_id: u64,
    /// Layers that must consent for this proposal to execute
    pub required_layers: Vec<LayerId>,
    /// Current status per layer
    pub layer_status: HashMap<LayerId, LayerConsent>,
    /// Veto reason hashes (only for Vetoed layers)
    pub veto_reasons: HashMap<LayerId, String>,
}

impl CrossLayerState {
    pub fn new(proposal_id: u64, required_layers: Vec<LayerId>) -> Self {
        let mut layer_status = HashMap::new();
        for layer in &required_layers {
            layer_status.insert(*layer, LayerConsent::Pending);
        }
        Self {
            proposal_id,
            required_layers,
            layer_status,
            veto_reasons: HashMap::new(),
        }
    }

    /// Record consent from a layer.
    pub fn consent(&mut self, layer: LayerId) -> DaoResult<()> {
        self.ensure_layer(layer)?;
        if self.layer_status[&layer] == LayerConsent::Vetoed {
            return Err(DaoError::Internal(format!(
                "Layer {} already vetoed proposal {}; cannot consent after veto",
                layer, self.proposal_id
            )));
        }
        self.layer_status.insert(layer, LayerConsent::Consented);
        Ok(())
    }

    /// Record veto from a layer.
    pub fn veto(&mut self, layer: LayerId, reason_hash: String) -> DaoResult<()> {
        self.ensure_layer(layer)?;
        self.layer_status.insert(layer, LayerConsent::Vetoed);
        self.veto_reasons.insert(layer, reason_hash);
        Ok(())
    }

    /// Waive a layer's right to veto (rare, used for auto-consent layers).
    pub fn waive(&mut self, layer: LayerId) -> DaoResult<()> {
        self.ensure_layer(layer)?;
        self.layer_status.insert(layer, LayerConsent::Waived);
        Ok(())
    }

    /// Check if enough layers have consented for the proposal to proceed.
    pub fn is_ready(&self, consent_threshold: u8) -> bool {
        if self.has_veto() {
            return false;
        }
        let consented = self
            .layer_status
            .values()
            .filter(|s| matches!(s, LayerConsent::Consented | LayerConsent::Waived))
            .count();
        consented >= consent_threshold as usize
    }

    /// Check if any layer has vetoed.
    pub fn has_veto(&self) -> bool {
        self.layer_status
            .values()
            .any(|s| matches!(s, LayerConsent::Vetoed))
    }

    /// Which layers have vetoed? (sorted for determinism)
    pub fn vetoed_layers(&self) -> Vec<LayerId> {
        let mut v: Vec<_> = self
            .layer_status
            .iter()
            .filter(|(_, s)| matches!(s, LayerConsent::Vetoed))
            .map(|(l, _)| *l)
            .collect();
        v.sort_unstable();
        v
    }

    /// Which layers are still pending? (sorted for determinism)
    pub fn pending_layers(&self) -> Vec<LayerId> {
        let mut v: Vec<_> = self
            .layer_status
            .iter()
            .filter(|(_, s)| matches!(s, LayerConsent::Pending))
            .map(|(l, _)| *l)
            .collect();
        v.sort_unstable();
        v
    }

    fn ensure_layer(&self, layer: LayerId) -> DaoResult<()> {
        if !self.required_layers.contains(&layer) {
            return Err(DaoError::Internal(format!(
                "Layer {} is not a required layer for proposal {}",
                layer, self.proposal_id
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Cross-Layer Registry
// ---------------------------------------------------------------------------

/// Tracks cross-layer states for all proposals.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrossLayerRegistry {
    states: HashMap<u64, CrossLayerState>,
}

impl CrossLayerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a proposal as cross-layer.
    pub fn register(&mut self, proposal_id: u64, required_layers: Vec<LayerId>) {
        self.states.insert(
            proposal_id,
            CrossLayerState::new(proposal_id, required_layers),
        );
    }

    /// Get mutable state for a proposal.
    pub fn get_mut(&mut self, proposal_id: u64) -> Option<&mut CrossLayerState> {
        self.states.get_mut(&proposal_id)
    }

    /// Get state for a proposal.
    pub fn get(&self, proposal_id: u64) -> Option<&CrossLayerState> {
        self.states.get(&proposal_id)
    }

    /// Consent from a layer.
    pub fn layer_consent(&mut self, proposal_id: u64, layer: LayerId) -> DaoResult<()> {
        self.states
            .get_mut(&proposal_id)
            .ok_or_else(|| {
                DaoError::Internal(format!("No cross-layer state for proposal {}", proposal_id))
            })?
            .consent(layer)
    }

    /// Veto from a layer.
    pub fn layer_veto(
        &mut self,
        proposal_id: u64,
        layer: LayerId,
        reason_hash: String,
    ) -> DaoResult<()> {
        self.states
            .get_mut(&proposal_id)
            .ok_or_else(|| {
                DaoError::Internal(format!("No cross-layer state for proposal {}", proposal_id))
            })?
            .veto(layer, reason_hash)
    }

    /// Check if a proposal is ready (all required layers consented, no veto).
    pub fn is_ready(&self, proposal_id: u64, threshold: u8) -> bool {
        self.states
            .get(&proposal_id)
            .map(|s| s.is_ready(threshold))
            .unwrap_or(false)
    }

    /// Check if any layer vetoed.
    pub fn has_veto(&self, proposal_id: u64) -> bool {
        self.states
            .get(&proposal_id)
            .map(|s| s.has_veto())
            .unwrap_or(false)
    }

    /// Remove a proposal from tracking.
    pub fn remove(&mut self, proposal_id: u64) {
        self.states.remove(&proposal_id);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_layer_consent_flow() {
        let mut reg = CrossLayerRegistry::new();
        reg.register(1, vec![2, 3]); // L2 + L3 must consent

        assert!(!reg.is_ready(1, 2));

        reg.layer_consent(1, 2).unwrap();
        assert!(!reg.is_ready(1, 2)); // still need L3

        reg.layer_consent(1, 3).unwrap();
        assert!(reg.is_ready(1, 2));
    }

    #[test]
    fn test_cross_layer_veto_blocks() {
        let mut reg = CrossLayerRegistry::new();
        reg.register(1, vec![2, 3, 4]);

        reg.layer_consent(1, 2).unwrap();
        reg.layer_veto(1, 3, "reason_hash".into()).unwrap();

        assert!(reg.has_veto(1));
        assert!(!reg.is_ready(1, 2));
    }

    #[test]
    fn test_consent_after_veto_fails() {
        let mut reg = CrossLayerRegistry::new();
        reg.register(1, vec![2, 3]);

        reg.layer_veto(1, 2, "r".into()).unwrap();
        assert!(reg.layer_consent(1, 2).is_err());
    }

    #[test]
    fn test_invalid_layer_rejected() {
        let mut reg = CrossLayerRegistry::new();
        reg.register(1, vec![2, 3]);
        assert!(reg.layer_consent(1, 5).is_err());
    }

    #[test]
    fn test_vetoed_layers_list() {
        let mut reg = CrossLayerRegistry::new();
        reg.register(1, vec![2, 3, 4, 5]);
        reg.layer_veto(1, 3, "r".into()).unwrap();

        let state = reg.get(1).unwrap();
        assert_eq!(state.vetoed_layers(), vec![3]);
        assert_eq!(state.pending_layers(), vec![2, 4, 5]);
    }
}
