// Phase 9a — Checkpoint registry with multi-signature verification
//
// Ported from TREE_NODES/sync/checkpoint_manager.py into V3 Rust.
//
// Provides:
//   - `SignedCheckpoint`: height + block_hash + state_root + timestamp + signatures
//   - `CheckpointRegistry`: loads hardcoded + JSON file checkpoints, verifies
//     ed25519 signatures against a set of trusted signers, supports IBD fast-sync
//     by returning the best verified checkpoint below a given height.
//   - Signature verification: SHA-256 digest of "height:block_hash:state_root:timestamp"
//     signed by ed25519 keys. Requires MIN_SIGNATURES valid signatures to accept.
//
// This module is a pure state machine — no I/O. The node runtime is responsible
// for loading checkpoint files (JSON) and passing them in.

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

// ── Constants ──────────────────────────────────────────────────────────

/// Checkpoints are created every N blocks on the network.
pub const CHECKPOINT_INTERVAL: u64 = 10_000;

/// Minimum number of valid signatures required to trust a checkpoint.
pub const MIN_SIGNATURES: usize = 2;

/// Maximum age (in seconds) a checkpoint timestamp can be in the future
/// relative to local time before we reject it.
pub const MAX_FUTURE_SECS: u64 = 7200;

// ── Types ──────────────────────────────────────────────────────────────

/// A checkpoint with cryptographic attestation from trusted signers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedCheckpoint {
    /// Block height of this checkpoint.
    pub height: u64,
    /// Hex-encoded block hash at this height.
    pub block_hash: String,
    /// Hex-encoded prev-block hash.
    pub prev_hash: String,
    /// Hex-encoded state root (UTXO commitment or similar).
    pub state_root: String,
    /// UNIX timestamp of the block.
    pub timestamp: u64,
    /// Cumulative proof-of-work (encoded as u128 string for JSON compat).
    pub cumulative_work: String,
    /// List of (signer_pubkey_hex, signature_hex) attestations.
    pub signatures: Vec<(String, String)>,
}

/// Result of verifying a checkpoint's signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckpointVerification {
    /// Checkpoint is valid with N signatures.
    Valid { valid_sigs: usize },
    /// Not enough valid signatures.
    InsufficientSignatures { valid: usize, required: usize },
    /// Checkpoint references a non-interval height.
    InvalidHeight,
}

// ── Checkpoint Registry ────────────────────────────────────────────────

/// Registry of verified checkpoints, keyed by height.
///
/// Combines hardcoded (compile-time) checkpoints with dynamically loaded
/// signed checkpoints from JSON files or peer sync.
pub struct CheckpointRegistry {
    /// Verified checkpoints keyed by height (BTreeMap for ordered iteration).
    checkpoints: BTreeMap<u64, SignedCheckpoint>,
    /// Trusted signer public keys (hex-encoded ed25519 verifying keys).
    trusted_signers: Vec<String>,
}

impl CheckpointRegistry {
    /// Create a new registry with the given set of trusted signer pubkeys.
    pub fn new(trusted_signers: Vec<String>) -> Self {
        Self {
            checkpoints: BTreeMap::new(),
            trusted_signers,
        }
    }

    /// Number of verified checkpoints in the registry.
    pub fn len(&self) -> usize {
        self.checkpoints.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.checkpoints.is_empty()
    }

    /// Highest checkpoint height in the registry, or 0 if empty.
    pub fn highest_height(&self) -> u64 {
        self.checkpoints.keys().next_back().copied().unwrap_or(0)
    }

    /// Get the checkpoint at a specific height, if it exists.
    pub fn get(&self, height: u64) -> Option<&SignedCheckpoint> {
        self.checkpoints.get(&height)
    }

    /// Get the best (highest) verified checkpoint at or below `height`.
    /// Used by IBD to determine the fast-sync boundary.
    pub fn best_checkpoint_at_or_below(&self, height: u64) -> Option<&SignedCheckpoint> {
        self.checkpoints
            .range(..=height)
            .next_back()
            .map(|(_, cp)| cp)
    }

    /// Check if a block hash at a given height matches the checkpoint.
    /// Returns `Ok(())` if no checkpoint exists at that height, or if the hash matches.
    /// Returns `Err` if the hash disagrees with a verified checkpoint.
    pub fn verify_block_hash(&self, height: u64, hash: &str) -> Result<(), String> {
        if let Some(cp) = self.checkpoints.get(&height) {
            if cp.block_hash != hash {
                return Err(format!(
                    "checkpoint violation at height {}: expected {}, got {}",
                    height, cp.block_hash, hash
                ));
            }
        }
        Ok(())
    }

    /// Whether a given height is below the highest checkpoint — meaning
    /// full PoW re-validation can be skipped during IBD (the block hash
    /// will still be checked against the checkpoint if one exists).
    pub fn is_below_checkpoint(&self, height: u64) -> bool {
        height < self.highest_height()
    }

    /// Add a signed checkpoint after verifying its signatures.
    /// Returns the verification result.
    pub fn add_checkpoint(&mut self, cp: SignedCheckpoint) -> CheckpointVerification {
        // Validate height is on checkpoint interval (except genesis)
        if cp.height != 0 && !cp.height.is_multiple_of(CHECKPOINT_INTERVAL) {
            return CheckpointVerification::InvalidHeight;
        }

        let valid_sigs = self.count_valid_signatures(&cp);

        if valid_sigs < MIN_SIGNATURES {
            return CheckpointVerification::InsufficientSignatures {
                valid: valid_sigs,
                required: MIN_SIGNATURES,
            };
        }

        self.checkpoints.insert(cp.height, cp);

        CheckpointVerification::Valid { valid_sigs }
    }

    /// Add a checkpoint without signature verification (for hardcoded/genesis).
    /// Use only for checkpoints baked into the binary.
    pub fn add_hardcoded(&mut self, cp: SignedCheckpoint) {
        self.checkpoints.insert(cp.height, cp);
    }

    /// Load checkpoints from a JSON array (e.g., from a file or peer).
    /// Returns (accepted_count, rejected_count).
    pub fn load_from_json(&mut self, json: &str) -> Result<(usize, usize), String> {
        let cps: Vec<SignedCheckpoint> =
            serde_json::from_str(json).map_err(|e| format!("invalid checkpoint JSON: {e}"))?;

        let mut accepted = 0;
        let mut rejected = 0;

        for cp in cps {
            match self.add_checkpoint(cp) {
                CheckpointVerification::Valid { .. } => accepted += 1,
                _ => rejected += 1,
            }
        }

        Ok((accepted, rejected))
    }

    /// Export all checkpoints as JSON (for distribution to peers).
    pub fn export_json(&self) -> String {
        let cps: Vec<&SignedCheckpoint> = self.checkpoints.values().collect();
        serde_json::to_string_pretty(&cps).unwrap_or_else(|_| "[]".to_string())
    }

    /// Compute the canonical digest for a checkpoint.
    /// Format: SHA-256("height:block_hash:state_root:timestamp")
    pub fn compute_digest(cp: &SignedCheckpoint) -> [u8; 32] {
        let msg = format!(
            "{}:{}:{}:{}",
            cp.height, cp.block_hash, cp.state_root, cp.timestamp
        );
        let mut hasher = Sha256::new();
        hasher.update(msg.as_bytes());
        let result = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&result);
        out
    }

    /// Count how many valid trusted signatures a checkpoint has.
    fn count_valid_signatures(&self, cp: &SignedCheckpoint) -> usize {
        let digest = Self::compute_digest(cp);
        let mut valid = 0;

        for (pubkey_hex, sig_hex) in &cp.signatures {
            // Must be a trusted signer
            if !self.trusted_signers.contains(pubkey_hex) {
                continue;
            }

            // Parse pubkey
            let pubkey_bytes = match hex_decode(pubkey_hex) {
                Some(b) if b.len() == 32 => b,
                _ => continue,
            };
            let pubkey_arr: [u8; 32] = match pubkey_bytes.try_into() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let verifying_key = match VerifyingKey::from_bytes(&pubkey_arr) {
                Ok(k) => k,
                Err(_) => continue,
            };

            // Parse signature
            let sig_bytes = match hex_decode(sig_hex) {
                Some(b) if b.len() == 64 => b,
                _ => continue,
            };
            let sig_arr: [u8; 64] = match sig_bytes.try_into() {
                Ok(a) => a,
                Err(_) => continue,
            };
            let signature = Signature::from_bytes(&sig_arr);

            // Verify
            if verifying_key.verify(&digest, &signature).is_ok() {
                valid += 1;
            }
        }

        valid
    }
}

// ── Hex helpers ────────────────────────────────────────────────────────

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_signer() -> (SigningKey, String) {
        let sk = SigningKey::generate(&mut OsRng);
        let pk_hex = hex_encode(sk.verifying_key().as_bytes());
        (sk, pk_hex)
    }

    fn sign_checkpoint(sk: &SigningKey, cp: &SignedCheckpoint) -> String {
        let digest = CheckpointRegistry::compute_digest(cp);
        let sig = sk.sign(&digest);
        hex_encode(&sig.to_bytes())
    }

    fn make_checkpoint(height: u64, signers: &[(SigningKey, String)]) -> SignedCheckpoint {
        let mut cp = SignedCheckpoint {
            height,
            block_hash: format!("{:064x}", height),
            prev_hash: format!("{:064x}", height.saturating_sub(1)),
            state_root: format!("{:064x}", height * 1000),
            timestamp: 1700000000 + height * 60,
            cumulative_work: (height * 1000).to_string(),
            signatures: Vec::new(),
        };
        for (sk, pk_hex) in signers {
            let sig_hex = sign_checkpoint(sk, &cp);
            cp.signatures.push((pk_hex.clone(), sig_hex));
        }
        cp
    }

    #[test]
    fn empty_registry() {
        let reg = CheckpointRegistry::new(vec![]);
        assert!(reg.is_empty());
        assert_eq!(reg.highest_height(), 0);
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn add_hardcoded_checkpoint() {
        let mut reg = CheckpointRegistry::new(vec![]);
        let cp = SignedCheckpoint {
            height: 0,
            block_hash: "genesis_hash".into(),
            prev_hash: "0".repeat(64),
            state_root: "0".repeat(64),
            timestamp: 0,
            cumulative_work: "0".into(),
            signatures: vec![],
        };
        reg.add_hardcoded(cp);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.highest_height(), 0);
        assert!(reg.get(0).is_some());
    }

    #[test]
    fn verify_block_hash_match() {
        let mut reg = CheckpointRegistry::new(vec![]);
        let cp = SignedCheckpoint {
            height: 0,
            block_hash: "abc123".into(),
            prev_hash: "0".repeat(64),
            state_root: "0".repeat(64),
            timestamp: 0,
            cumulative_work: "0".into(),
            signatures: vec![],
        };
        reg.add_hardcoded(cp);

        assert!(reg.verify_block_hash(0, "abc123").is_ok());
        assert!(reg.verify_block_hash(0, "wrong").is_err());
        // No checkpoint at height 1 — should pass
        assert!(reg.verify_block_hash(1, "anything").is_ok());
    }

    #[test]
    fn signed_checkpoint_with_valid_signatures() {
        let (sk1, pk1) = make_signer();
        let (sk2, pk2) = make_signer();
        let (_sk3, pk3) = make_signer();

        let trusted = vec![pk1.clone(), pk2.clone(), pk3.clone()];
        let mut reg = CheckpointRegistry::new(trusted);

        // Checkpoint at height 10000 signed by 2 of 3 — should pass
        let cp = make_checkpoint(
            10_000,
            &[(sk1.clone(), pk1.clone()), (sk2.clone(), pk2.clone())],
        );
        let result = reg.add_checkpoint(cp);
        assert_eq!(result, CheckpointVerification::Valid { valid_sigs: 2 });
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn reject_insufficient_signatures() {
        let (sk1, pk1) = make_signer();
        let (_, pk2) = make_signer();

        let trusted = vec![pk1.clone(), pk2.clone()];
        let mut reg = CheckpointRegistry::new(trusted);

        // Only 1 signature — needs 2
        let cp = make_checkpoint(10_000, &[(sk1, pk1)]);
        let result = reg.add_checkpoint(cp);
        assert_eq!(
            result,
            CheckpointVerification::InsufficientSignatures {
                valid: 1,
                required: 2
            }
        );
        assert!(reg.is_empty());
    }

    #[test]
    fn reject_non_interval_height() {
        let (sk1, pk1) = make_signer();
        let (sk2, pk2) = make_signer();

        let trusted = vec![pk1.clone(), pk2.clone()];
        let mut reg = CheckpointRegistry::new(trusted);

        let cp = make_checkpoint(5_000, &[(sk1, pk1), (sk2, pk2)]);
        let result = reg.add_checkpoint(cp);
        assert_eq!(result, CheckpointVerification::InvalidHeight);
    }

    #[test]
    fn best_checkpoint_at_or_below() {
        let (sk1, pk1) = make_signer();
        let (sk2, pk2) = make_signer();

        let trusted = vec![pk1.clone(), pk2.clone()];
        let mut reg = CheckpointRegistry::new(trusted);

        let cp1 = make_checkpoint(
            10_000,
            &[(sk1.clone(), pk1.clone()), (sk2.clone(), pk2.clone())],
        );
        let cp2 = make_checkpoint(
            20_000,
            &[(sk1.clone(), pk1.clone()), (sk2.clone(), pk2.clone())],
        );

        reg.add_checkpoint(cp1);
        reg.add_checkpoint(cp2);

        // At height 25000: best checkpoint is 20000
        let best = reg.best_checkpoint_at_or_below(25_000).unwrap();
        assert_eq!(best.height, 20_000);

        // At height 15000: best checkpoint is 10000
        let best = reg.best_checkpoint_at_or_below(15_000).unwrap();
        assert_eq!(best.height, 10_000);

        // At height 5000: no checkpoint
        assert!(reg.best_checkpoint_at_or_below(5_000).is_none());
    }

    #[test]
    fn is_below_checkpoint() {
        let mut reg = CheckpointRegistry::new(vec![]);
        let cp = SignedCheckpoint {
            height: 10_000,
            block_hash: "test".into(),
            prev_hash: "0".repeat(64),
            state_root: "0".repeat(64),
            timestamp: 0,
            cumulative_work: "0".into(),
            signatures: vec![],
        };
        reg.add_hardcoded(cp);

        assert!(reg.is_below_checkpoint(5_000));
        assert!(!reg.is_below_checkpoint(10_000));
        assert!(!reg.is_below_checkpoint(15_000));
    }

    #[test]
    fn json_roundtrip() {
        let (sk1, pk1) = make_signer();
        let (sk2, pk2) = make_signer();

        let trusted = vec![pk1.clone(), pk2.clone()];
        let mut reg = CheckpointRegistry::new(trusted.clone());

        let cp = make_checkpoint(10_000, &[(sk1, pk1), (sk2, pk2)]);
        reg.add_checkpoint(cp);

        let json = reg.export_json();

        let mut reg2 = CheckpointRegistry::new(trusted);
        let (accepted, rejected) = reg2.load_from_json(&json).unwrap();
        assert_eq!(accepted, 1);
        assert_eq!(rejected, 0);
        assert_eq!(reg2.len(), 1);
    }

    #[test]
    fn untrusted_signer_ignored() {
        let (sk1, pk1) = make_signer();
        let (_sk2, pk2) = make_signer();
        let (sk3, pk3) = make_signer(); // not trusted

        let trusted = vec![pk1.clone(), pk2.clone()]; // pk3 NOT trusted
        let mut reg = CheckpointRegistry::new(trusted);

        // Signed by sk1 (trusted) and sk3 (untrusted) — only 1 valid sig
        let cp = make_checkpoint(10_000, &[(sk1, pk1), (sk3, pk3)]);
        let result = reg.add_checkpoint(cp);
        assert_eq!(
            result,
            CheckpointVerification::InsufficientSignatures {
                valid: 1,
                required: 2
            }
        );
    }

    #[test]
    fn digest_is_deterministic() {
        let cp = SignedCheckpoint {
            height: 100,
            block_hash: "abc".into(),
            prev_hash: "def".into(),
            state_root: "ghi".into(),
            timestamp: 12345,
            cumulative_work: "0".into(),
            signatures: vec![],
        };
        let d1 = CheckpointRegistry::compute_digest(&cp);
        let d2 = CheckpointRegistry::compute_digest(&cp);
        assert_eq!(d1, d2);
    }

    #[test]
    fn hex_roundtrip() {
        let data = vec![0u8, 1, 255, 128, 64];
        let encoded = hex_encode(&data);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(data, decoded);
    }
}
