// Phase 6a — Chain reorg handling and fork choice
//
// Constitutional reference:
//   MAX_REORG_DEPTH  = 10 blocks
//   SOFT_FINALITY    = 60 blocks
//   Fork choice      = highest accumulated work (strictly >)

use std::collections::HashMap;

/// Constitutional: maximum reorganization depth.
pub const MAX_REORG_DEPTH: u64 = 10;

/// Constitutional: blocks after which a chain segment is considered soft-final.
pub const SOFT_FINALITY_DEPTH: u64 = 60;

// ---------------------------------------------------------------------------
// Chain tip tracking
// ---------------------------------------------------------------------------

/// Minimal header info stored per accepted block for fork-choice decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainEntry {
    pub height: u64,
    pub hash: [u8; 32],
    pub prev_hash: [u8; 32],
    pub difficulty: u64,
    pub total_work: u128,
}

/// An undo record for a single block, used to roll back UTXO changes during reorg.
#[derive(Debug, Clone)]
pub struct UndoBlock {
    pub height: u64,
    pub hash: [u8; 32],
    /// UTXOs that were spent in this block and must be restored on rollback.
    pub spent_utxos: Vec<RestoredUtxo>,
    /// UTXO outpoints created in this block that must be removed on rollback.
    pub created_outpoints: Vec<Outpoint>,
}

/// A UTXO that was consumed when the block was applied.
#[derive(Debug, Clone)]
pub struct RestoredUtxo {
    pub outpoint: Outpoint,
    pub amount: u64,
    pub address: String,
    pub created_height: u64,
    pub is_coinbase: bool,
}

/// Identifies a specific transaction output.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Outpoint {
    pub tx_hash: [u8; 32],
    pub index: u32,
}

// ---------------------------------------------------------------------------
// Fork-choice engine
// ---------------------------------------------------------------------------

/// Accumulated-work index for competing chain tips.
#[derive(Debug)]
pub struct ForkChoice {
    /// Best-known chain: hash of tip → ChainEntry.
    entries: HashMap<[u8; 32], ChainEntry>,
    /// Current active tip hash.
    active_tip: [u8; 32],
}

/// Errors that can happen during chain reorganization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorgError {
    /// The competing chain is not strictly stronger than the active one.
    NotStronger,
    /// The fork point is deeper than MAX_REORG_DEPTH.
    TooDeep { depth: u64 },
    /// The fork point could not be found (chains share no common ancestor
    /// within the undo window).
    ForkPointNotFound,
    /// The tip hash of the competing chain is unknown.
    UnknownTip,
    /// Undo data missing for a block that must be rolled back.
    MissingUndoBlock { height: u64 },
}

impl ForkChoice {
    /// Create a new fork-choice tracker seeded with a genesis entry.
    pub fn new(genesis_hash: [u8; 32]) -> Self {
        let genesis = ChainEntry {
            height: 0,
            hash: genesis_hash,
            prev_hash: [0u8; 32],
            difficulty: 0,
            total_work: 0,
        };
        let mut entries = HashMap::new();
        entries.insert(genesis_hash, genesis);
        Self {
            entries,
            active_tip: genesis_hash,
        }
    }

    /// Register a new block on top of an existing entry.
    /// Returns the cumulative total_work of the new entry.
    pub fn insert(
        &mut self,
        hash: [u8; 32],
        prev_hash: [u8; 32],
        height: u64,
        difficulty: u64,
    ) -> Option<u128> {
        let parent = self.entries.get(&prev_hash)?;
        let total_work = parent.total_work.saturating_add(difficulty as u128);
        let entry = ChainEntry {
            height,
            hash,
            prev_hash,
            difficulty,
            total_work,
        };
        self.entries.insert(hash, entry);
        Some(total_work)
    }

    /// Return the currently active tip.
    pub fn active_tip(&self) -> &ChainEntry {
        self.entries
            .get(&self.active_tip)
            .expect("active tip must exist")
    }

    /// Check whether `candidate_tip` is strictly stronger than the active tip.
    pub fn is_stronger(&self, candidate_tip: &[u8; 32]) -> Result<bool, ReorgError> {
        let candidate = self
            .entries
            .get(candidate_tip)
            .ok_or(ReorgError::UnknownTip)?;
        let active = self.active_tip();
        // Strictly greater — ties keep the current chain (audit P1-01).
        Ok(candidate.total_work > active.total_work)
    }

    /// Walk backwards from `tip_hash` collecting hashes until `stop_hash` or genesis.
    pub fn ancestors(&self, tip_hash: &[u8; 32], stop_hash: &[u8; 32]) -> Vec<[u8; 32]> {
        let mut result = Vec::new();
        let mut current = *tip_hash;
        while current != *stop_hash {
            if let Some(entry) = self.entries.get(&current) {
                result.push(current);
                if entry.prev_hash == [0u8; 32] {
                    break;
                }
                current = entry.prev_hash;
            } else {
                break;
            }
        }
        result
    }

    /// Find the fork point between the active chain and a candidate tip.
    /// Returns `(fork_hash, depth_to_rollback)`.
    pub fn find_fork_point(&self, candidate_tip: &[u8; 32]) -> Result<([u8; 32], u64), ReorgError> {
        let active_ancestors =
            self.ancestor_set(&self.active_tip, MAX_REORG_DEPTH + SOFT_FINALITY_DEPTH);
        let mut current = *candidate_tip;
        let mut depth: u64 = 0;
        loop {
            if active_ancestors.contains_key(&current) {
                let active_height = self.active_tip().height;
                let fork_height = self.entries.get(&current).map(|e| e.height).unwrap_or(0);
                let rollback = active_height.saturating_sub(fork_height);
                return Ok((current, rollback));
            }
            if let Some(entry) = self.entries.get(&current) {
                let next = entry.prev_hash;
                if next == current {
                    break;
                }
                current = next;
                depth += 1;
                if depth > MAX_REORG_DEPTH + SOFT_FINALITY_DEPTH {
                    break;
                }
            } else {
                break;
            }
        }
        Err(ReorgError::ForkPointNotFound)
    }

    /// Evaluate whether a reorg to `candidate_tip` should proceed.
    /// Returns the list of block hashes to disconnect (active side) and
    /// connect (candidate side) if the reorg is valid.
    pub fn evaluate_reorg(&self, candidate_tip: &[u8; 32]) -> Result<ReorgPlan, ReorgError> {
        if !self.is_stronger(candidate_tip)? {
            return Err(ReorgError::NotStronger);
        }
        let (fork_hash, rollback_depth) = self.find_fork_point(candidate_tip)?;
        if rollback_depth > MAX_REORG_DEPTH {
            return Err(ReorgError::TooDeep {
                depth: rollback_depth,
            });
        }
        // Collect disconnect (active side, newest first).
        let disconnect = self.ancestors(&self.active_tip, &fork_hash);
        // Collect connect (candidate side, newest first → reverse to oldest first).
        let mut connect = self.ancestors(candidate_tip, &fork_hash);
        connect.reverse();
        Ok(ReorgPlan {
            fork_point: fork_hash,
            disconnect,
            connect,
        })
    }

    /// Apply a reorg plan: update the active tip.
    /// Caller is responsible for UTXO rollback/apply using the returned plan.
    pub fn apply_reorg(&mut self, _plan: &ReorgPlan, new_tip: [u8; 32]) {
        self.active_tip = new_tip;
    }

    // -- helpers --

    fn ancestor_set(&self, tip: &[u8; 32], max_depth: u64) -> HashMap<[u8; 32], u64> {
        let mut set = HashMap::new();
        let mut current = *tip;
        let mut depth: u64 = 0;
        while let Some(entry) = self.entries.get(&current) {
            set.insert(current, entry.height);
            let next = entry.prev_hash;
            if next == current || depth >= max_depth {
                break;
            }
            current = next;
            depth += 1;
        }
        set
    }
}

/// The plan produced by `evaluate_reorg`.
#[derive(Debug, Clone)]
pub struct ReorgPlan {
    /// The common ancestor of both chains.
    pub fork_point: [u8; 32],
    /// Block hashes to disconnect from the active chain (newest first).
    pub disconnect: Vec<[u8; 32]>,
    /// Block hashes to connect from the candidate chain (oldest first).
    pub connect: Vec<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// UTXO rollback helpers
// ---------------------------------------------------------------------------

/// Apply an undo block: restore spent UTXOs, remove created outpoints.
/// The caller provides mutable access to the UTXO store via callbacks.
pub fn rollback_block<F, G>(undo: &UndoBlock, mut restore_utxo: F, mut remove_utxo: G)
where
    F: FnMut(&Outpoint, &RestoredUtxo),
    G: FnMut(&Outpoint),
{
    // First remove UTXOs that the block created.
    for op in &undo.created_outpoints {
        remove_utxo(op);
    }
    // Then restore UTXOs that the block consumed.
    for ru in &undo.spent_utxos {
        restore_utxo(&ru.outpoint, ru);
    }
}

/// Build an undo block from a transaction set.
/// `utxo_lookup` resolves each input's previous UTXO info.
pub fn build_undo_block<F>(
    height: u64,
    block_hash: [u8; 32],
    transactions: &[crate::tx::Transaction],
    mut utxo_lookup: F,
) -> UndoBlock
where
    F: FnMut(&[u8; 32], u32) -> Option<RestoredUtxo>,
{
    let mut spent_utxos = Vec::new();
    let mut created_outpoints = Vec::new();
    for tx in transactions {
        // Record spent inputs.
        for input in &tx.inputs {
            if tx.is_coinbase() {
                continue; // coinbase has no real inputs
            }
            if let Some(restored) = utxo_lookup(&input.prev_tx_hash, input.output_index) {
                spent_utxos.push(restored);
            }
        }
        // Record created outputs.
        for (idx, _output) in tx.outputs.iter().enumerate() {
            created_outpoints.push(Outpoint {
                tx_hash: tx.id,
                index: idx as u32,
            });
        }
    }
    UndoBlock {
        height,
        hash: block_hash,
        spent_utxos,
        created_outpoints,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(v: u8) -> [u8; 32] {
        let mut h = [0u8; 32];
        h[0] = v;
        h
    }

    #[test]
    fn test_fork_choice_genesis() {
        let fc = ForkChoice::new(hash(0));
        assert_eq!(fc.active_tip().height, 0);
        assert_eq!(fc.active_tip().total_work, 0);
    }

    #[test]
    fn test_insert_linear_chain() {
        let mut fc = ForkChoice::new(hash(0));
        let tw1 = fc.insert(hash(1), hash(0), 1, 1000).unwrap();
        assert_eq!(tw1, 1000);
        let tw2 = fc.insert(hash(2), hash(1), 2, 2000).unwrap();
        assert_eq!(tw2, 3000);
        let tw3 = fc.insert(hash(3), hash(2), 3, 1500).unwrap();
        assert_eq!(tw3, 4500);
    }

    #[test]
    fn test_insert_unknown_parent_returns_none() {
        let mut fc = ForkChoice::new(hash(0));
        assert!(fc.insert(hash(99), hash(50), 1, 1000).is_none());
    }

    #[test]
    fn test_is_stronger_strictly_greater() {
        let mut fc = ForkChoice::new(hash(0));
        fc.insert(hash(1), hash(0), 1, 1000);
        fc.active_tip = hash(1);
        // Equal work → not stronger.
        fc.insert(hash(2), hash(0), 1, 1000);
        assert!(!fc.is_stronger(&hash(2)).unwrap());
        // Strictly greater → stronger.
        fc.insert(hash(3), hash(0), 1, 1001);
        assert!(fc.is_stronger(&hash(3)).unwrap());
    }

    #[test]
    fn test_find_fork_point() {
        let mut fc = ForkChoice::new(hash(0));
        // Main chain: 0 → 1 → 2 → 3
        fc.insert(hash(1), hash(0), 1, 1000);
        fc.insert(hash(2), hash(1), 2, 1000);
        fc.insert(hash(3), hash(2), 3, 1000);
        fc.active_tip = hash(3);
        // Fork from block 1: 1 → 10 → 11 → 12
        fc.insert(hash(10), hash(1), 2, 2000);
        fc.insert(hash(11), hash(10), 3, 2000);
        fc.insert(hash(12), hash(11), 4, 2000);
        let (fork_hash, rollback) = fc.find_fork_point(&hash(12)).unwrap();
        assert_eq!(fork_hash, hash(1));
        assert_eq!(rollback, 2); // roll back blocks 3, 2
    }

    #[test]
    fn test_evaluate_reorg_not_stronger() {
        let mut fc = ForkChoice::new(hash(0));
        fc.insert(hash(1), hash(0), 1, 5000);
        fc.active_tip = hash(1);
        // Weaker fork.
        fc.insert(hash(2), hash(0), 1, 100);
        let err = fc.evaluate_reorg(&hash(2)).unwrap_err();
        assert_eq!(err, ReorgError::NotStronger);
    }

    #[test]
    fn test_evaluate_reorg_too_deep() {
        let mut fc = ForkChoice::new(hash(0));
        // Build a chain 12 blocks deep.
        let mut prev = hash(0);
        for i in 1..=12u8 {
            fc.insert(hash(i), prev, i as u64, 100);
            prev = hash(i);
        }
        fc.active_tip = hash(12);
        // Fork from genesis with higher work.
        fc.insert(hash(100), hash(0), 1, 999_999);
        let err = fc.evaluate_reorg(&hash(100)).unwrap_err();
        assert_eq!(err, ReorgError::TooDeep { depth: 12 });
    }

    #[test]
    fn test_evaluate_reorg_success() {
        let mut fc = ForkChoice::new(hash(0));
        // Main: 0 → 1 → 2 (work=2000)
        fc.insert(hash(1), hash(0), 1, 1000);
        fc.insert(hash(2), hash(1), 2, 1000);
        fc.active_tip = hash(2);
        // Fork from 0: 0 → 10 → 11 (work=5000)
        fc.insert(hash(10), hash(0), 1, 2500);
        fc.insert(hash(11), hash(10), 2, 2500);
        let plan = fc.evaluate_reorg(&hash(11)).unwrap();
        assert_eq!(plan.fork_point, hash(0));
        assert_eq!(plan.disconnect, vec![hash(2), hash(1)]); // newest first
        assert_eq!(plan.connect, vec![hash(10), hash(11)]); // oldest first
    }

    #[test]
    fn test_apply_reorg_updates_tip() {
        let mut fc = ForkChoice::new(hash(0));
        fc.insert(hash(1), hash(0), 1, 1000);
        fc.active_tip = hash(1);
        fc.insert(hash(10), hash(0), 1, 5000);
        let plan = fc.evaluate_reorg(&hash(10)).unwrap();
        fc.apply_reorg(&plan, hash(10));
        assert_eq!(fc.active_tip().hash, hash(10));
    }

    #[test]
    fn test_ancestors_walk() {
        let mut fc = ForkChoice::new(hash(0));
        fc.insert(hash(1), hash(0), 1, 100);
        fc.insert(hash(2), hash(1), 2, 100);
        fc.insert(hash(3), hash(2), 3, 100);
        let anc = fc.ancestors(&hash(3), &hash(0));
        assert_eq!(anc, vec![hash(3), hash(2), hash(1)]);
    }

    #[test]
    fn test_rollback_block_callbacks() {
        let undo = UndoBlock {
            height: 5,
            hash: hash(5),
            spent_utxos: vec![RestoredUtxo {
                outpoint: Outpoint {
                    tx_hash: hash(1),
                    index: 0,
                },
                amount: 1000,
                address: "zion1test".into(),
                created_height: 2,
                is_coinbase: false,
            }],
            created_outpoints: vec![Outpoint {
                tx_hash: hash(5),
                index: 0,
            }],
        };
        let mut restored = Vec::new();
        let mut removed = Vec::new();
        rollback_block(
            &undo,
            |op, ru| restored.push((op.clone(), ru.amount)),
            |op| removed.push(op.clone()),
        );
        assert_eq!(removed.len(), 1);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].1, 1000);
    }

    #[test]
    fn test_max_reorg_depth_constant() {
        assert_eq!(MAX_REORG_DEPTH, 10);
    }

    #[test]
    fn test_soft_finality_constant() {
        assert_eq!(SOFT_FINALITY_DEPTH, 60);
    }

    #[test]
    fn test_build_undo_block() {
        use crate::tx::{Transaction, TxInput, TxOutput};
        let tx = Transaction {
            id: hash(50),
            version: crate::tx::TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: hash(10),
                output_index: 0,
                signature: vec![],
                public_key: vec![],
            }],
            outputs: vec![
                TxOutput {
                    amount: 900,
                    address: "zion1a".into(),
                    memo: None,
                },
                TxOutput {
                    amount: 100,
                    address: "zion1b".into(),
                    memo: None,
                },
            ],
            fee: 0,
            timestamp: 0,
        };
        let undo = build_undo_block(5, hash(5), &[tx], |tx_hash, idx| {
            if *tx_hash == hash(10) && idx == 0 {
                Some(RestoredUtxo {
                    outpoint: Outpoint {
                        tx_hash: *tx_hash,
                        index: idx,
                    },
                    amount: 1000,
                    address: "zion1sender".into(),
                    created_height: 1,
                    is_coinbase: false,
                })
            } else {
                None
            }
        });
        assert_eq!(undo.spent_utxos.len(), 1);
        assert_eq!(undo.created_outpoints.len(), 2);
    }
}
