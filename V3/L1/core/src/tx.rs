//! UTXO transaction model for ZION V3.
//!
//! Bitcoin-style UTXO model with SegWit-style transaction IDs (signatures
//! excluded from hash preimage to prevent malleability).
//!
//! - `TxInput`  — references a previous output + Ed25519 signature
//! - `TxOutput` — amount in flowers + `zion1...` destination address
//! - `Transaction` — inputs, outputs, fee, timestamp; ID = BLAKE3 hash

use crate::crypto;
use serde::{Deserialize, Serialize};

// ── Structures ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxInput {
    /// Hash of the transaction containing the output being spent.
    pub prev_tx_hash: [u8; 32],
    /// Index of the output within that transaction.
    pub output_index: u32,
    /// Ed25519 signature (64 bytes) over the transaction hash (SegWit-style).
    pub signature: Vec<u8>,
    /// Ed25519 public key (32 bytes) of the spender.
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxOutput {
    /// Amount in flowers (atomic units). u64 max is ~18.4 × 10^18 flowers
    /// (≈ 18,446 ZION). Sufficient for individual transaction outputs.
    /// Genesis premine uses the Account model with u128 `amount_zion` in flowers.
    pub amount: u64,
    /// Destination address (`zion1...` 44-char format).
    pub address: String,
    /// Optional memo / OP_RETURN data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// BLAKE3 hash of the canonical serialization (excluding signatures).
    pub id: [u8; 32],
    /// Transaction format version.
    pub version: u32,
    /// Inputs (UTXOs being spent). Empty for coinbase.
    pub inputs: Vec<TxInput>,
    /// Outputs (new UTXOs created).
    pub outputs: Vec<TxOutput>,
    /// Explicit fee in flowers (must match inputs_sum - outputs_sum).
    pub fee: u64,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
}

/// Transaction format version that activates the length-prefixed,
/// non-malleable preimage scheme (`calculate_hash_v2`). Version 1 (and
/// anything else) keeps the legacy v1 scheme for backward compatibility
/// with txs already mined into the chain. Wallets / RPC must set
/// `version = TX_HASH_V2_VERSION` to opt in once a hard fork that
/// activates the v2 rule has shipped. (Audit Finding §3.2 /
/// `TX_HASH_V2_ACTIVATION_PLAN.md`.)
pub const TX_HASH_V2_VERSION: u32 = 2;

impl Transaction {
    /// Compute the canonical transaction hash (SegWit-style: excludes
    /// signatures).
    ///
    /// Dispatches on `self.version`: versions `<2` use the legacy v1
    /// preimage (raw concatenation, kept for compatibility with txs
    /// already baked into chain state); version `2` uses the
    /// length-prefixed v2 preimage that closes the malleability
    /// findings documented in audit §3.2.
    pub fn calculate_hash(&self) -> [u8; 32] {
        if self.version >= TX_HASH_V2_VERSION {
            self.calculate_hash_v2()
        } else {
            self.calculate_hash_v1()
        }
    }

    /// Legacy v1 preimage (raw concatenation, no length prefixes).
    ///
    /// **Malleable** — see audit §3.2. Kept verbatim because every tx
    /// currently in the chain was hashed this way; changing it for
    /// `version = 1` txs would retroactively invalidate every historical
    /// UTXO outpoint. New txs should set `version = TX_HASH_V2_VERSION`
    /// once the v2 activation fork ships.
    fn calculate_hash_v1(&self) -> [u8; 32] {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        for input in &self.inputs {
            data.extend_from_slice(&input.prev_tx_hash);
            data.extend_from_slice(&input.output_index.to_le_bytes());
            // Exclude signature — SegWit-style immutable ID
            data.extend_from_slice(&input.public_key);
        }
        for output in &self.outputs {
            data.extend_from_slice(&output.amount.to_le_bytes());
            data.extend_from_slice(output.address.as_bytes());
            if let Some(memo) = &output.memo {
                data.extend_from_slice(memo.as_bytes());
            }
        }
        data.extend_from_slice(&self.fee.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        crypto::blake3_hash(&data)
    }

    /// Length-prefixed v2 preimage, domain-separated per field and per
    /// vector. Fixes the two malleability cases from audit §3.2:
    ///
    /// 1. `tx{inputs:[a,b], outputs:[c]}` vs `tx{inputs:[a], outputs:[b,c]}`
    ///    — counts are explicit.
    /// 2. `address="zion1foo" + memo="bar"` vs
    ///    `address="zion1foobar" + memo=""` — every variable-length
    ///    field is prefixed with its `u32` length, and memo-absent vs
    ///    memo-empty-string are encoded with distinct tags.
    ///
    /// Dormant until a hard fork sets this as the active rule for
    /// `version = TX_HASH_V2_VERSION`. Never called on existing
    /// (`version = 1`) txs, so activation does not rewrite any
    /// historical UTXO IDs.
    fn calculate_hash_v2(&self) -> [u8; 32] {
        // Domain-separation tag — guarantees v1 and v2 preimages are
        // distinct even for identical field content.
        const DOMAIN: &[u8] = b"ZION_TX_V2\x00";

        let mut data = Vec::new();
        data.extend_from_slice(DOMAIN);
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.fee.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());

        // Inputs — length-prefixed vector of length-prefixed fields.
        data.extend_from_slice(&(self.inputs.len() as u32).to_le_bytes());
        for input in &self.inputs {
            data.extend_from_slice(&input.prev_tx_hash);
            data.extend_from_slice(&input.output_index.to_le_bytes());
            data.extend_from_slice(&(input.public_key.len() as u32).to_le_bytes());
            data.extend_from_slice(&input.public_key);
        }

        // Outputs — length-prefixed vector of length-prefixed fields.
        data.extend_from_slice(&(self.outputs.len() as u32).to_le_bytes());
        for output in &self.outputs {
            data.extend_from_slice(&output.amount.to_le_bytes());
            let addr_bytes = output.address.as_bytes();
            data.extend_from_slice(&(addr_bytes.len() as u32).to_le_bytes());
            data.extend_from_slice(addr_bytes);
            match &output.memo {
                None => data.push(0),
                Some(memo) => {
                    data.push(1);
                    let mb = memo.as_bytes();
                    data.extend_from_slice(&(mb.len() as u32).to_le_bytes());
                    data.extend_from_slice(mb);
                }
            }
        }

        crypto::blake3_hash(&data)
    }

    /// Recalculate and set the transaction ID.
    pub fn finalize_id(&mut self) {
        self.id = self.calculate_hash();
    }

    /// True if this is a coinbase transaction (no inputs).
    pub fn is_coinbase(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Verify all input signatures against the transaction hash.
    ///
    /// For each input:
    ///   - The message is the 32-byte transaction hash (ID).
    ///   - The signature must be valid Ed25519 over that hash.
    ///   - The public key must correspond to the address owning the UTXO.
    ///     (UTXO ownership is checked separately during block validation.)
    pub fn verify_signatures(&self) -> bool {
        if self.is_coinbase() {
            return true;
        }

        // Re-derive the hash to verify ID integrity
        let expected_hash = self.calculate_hash();
        if self.id != expected_hash {
            return false;
        }

        for input in &self.inputs {
            if !crypto::verify(&input.public_key, &self.id, &input.signature) {
                return false;
            }
        }
        true
    }

    /// Sum of all output amounts.
    pub fn total_output(&self) -> u64 {
        self.outputs.iter().map(|o| o.amount).sum()
    }
}

// ── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_address, generate_keypair, sign};

    fn make_signed_tx() -> Transaction {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());

        let mut tx = Transaction {
            id: [0u8; 32],
            version: 1,
            inputs: vec![TxInput {
                prev_tx_hash: [0xAA; 32],
                output_index: 0,
                signature: vec![],
                public_key: vk.as_bytes().to_vec(),
            }],
            outputs: vec![TxOutput {
                amount: 1_000_000_000_000, // 1 ZION
                address: addr,
                memo: None,
            }],
            fee: 1_000,
            timestamp: 1_700_000_000,
        };
        tx.finalize_id();
        // Sign with the tx hash as message
        let sig = sign(&sk, &tx.id);
        tx.inputs[0].signature = sig.to_vec();
        tx
    }

    #[test]
    fn tx_hash_excludes_signatures() {
        let tx = make_signed_tx();
        // Clear signature and re-hash — should match original ID
        let mut tx2 = tx.clone();
        tx2.inputs[0].signature = vec![0u8; 64]; // different sig
        assert_eq!(tx.calculate_hash(), tx2.calculate_hash());
    }

    #[test]
    fn tx_hash_deterministic() {
        let tx = make_signed_tx();
        assert_eq!(tx.calculate_hash(), tx.calculate_hash());
    }

    #[test]
    fn tx_hash_different_for_different_amounts() {
        let mut tx = make_signed_tx();
        let h1 = tx.calculate_hash();
        tx.outputs[0].amount += 1;
        let h2 = tx.calculate_hash();
        assert_ne!(h1, h2);
    }

    #[test]
    fn verify_signatures_valid() {
        let tx = make_signed_tx();
        assert!(tx.verify_signatures());
    }

    #[test]
    fn verify_signatures_rejects_tampered_amount() {
        let mut tx = make_signed_tx();
        tx.outputs[0].amount += 1;
        // ID no longer matches hash
        assert!(!tx.verify_signatures());
    }

    #[test]
    fn verify_signatures_rejects_wrong_sig() {
        let mut tx = make_signed_tx();
        tx.inputs[0].signature = vec![0u8; 64];
        assert!(!tx.verify_signatures());
    }

    #[test]
    fn coinbase_has_no_inputs() {
        let tx = Transaction {
            id: [0u8; 32],
            version: 1,
            inputs: vec![],
            outputs: vec![TxOutput {
                amount: 5_400_067_000_000_000,
                address: "zion1test".to_string(),
                memo: None,
            }],
            fee: 0,
            timestamp: 1_700_000_000,
        };
        assert!(tx.is_coinbase());
        assert!(tx.verify_signatures()); // coinbase always valid
    }

    #[test]
    fn total_output_sums_correctly() {
        let tx = Transaction {
            id: [0u8; 32],
            version: 1,
            inputs: vec![],
            outputs: vec![
                TxOutput {
                    amount: 100,
                    address: "a".into(),
                    memo: None,
                },
                TxOutput {
                    amount: 200,
                    address: "b".into(),
                    memo: None,
                },
                TxOutput {
                    amount: 300,
                    address: "c".into(),
                    memo: None,
                },
            ],
            fee: 0,
            timestamp: 0,
        };
        assert_eq!(tx.total_output(), 600);
    }

    #[test]
    fn finalize_id_sets_correct_hash() {
        let mut tx = Transaction {
            id: [0u8; 32],
            version: 1,
            inputs: vec![],
            outputs: vec![TxOutput {
                amount: 1000,
                address: "addr".into(),
                memo: Some("hello".into()),
            }],
            fee: 100,
            timestamp: 12345,
        };
        let expected = tx.calculate_hash();
        tx.finalize_id();
        assert_eq!(tx.id, expected);
    }

    #[test]
    fn memo_affects_hash() {
        let base = Transaction {
            id: [0u8; 32],
            version: 1,
            inputs: vec![],
            outputs: vec![TxOutput {
                amount: 1000,
                address: "addr".into(),
                memo: None,
            }],
            fee: 100,
            timestamp: 12345,
        };
        let mut with_memo = base.clone();
        with_memo.outputs[0].memo = Some("memo data".into());
        assert_ne!(base.calculate_hash(), with_memo.calculate_hash());
    }

    // ── audit §3.2: tx hash malleability regression tests ────────────

    fn mk_tx_varlen(version: u32, address: String, memo: Option<String>) -> Transaction {
        Transaction {
            id: [0u8; 32],
            version,
            inputs: vec![],
            outputs: vec![TxOutput {
                amount: 1_000,
                address,
                memo,
            }],
            fee: 0,
            timestamp: 0,
        }
    }

    /// Regression: v1 raw-concat preimage is malleable on adjacent
    /// variable-length fields. `address="A" + memo="B"` hashes identically
    /// to `address="AB" + memo=""` once memo is `Some("")`. This test
    /// *documents* the known v1 weakness from audit §3.2 so anyone who
    /// later tries to "fix" the v1 hash in place is forced to understand
    /// that doing so would invalidate every historical UTXO ID.
    #[test]
    fn tx_hash_v1_is_malleable_across_address_and_memo_boundary() {
        let a = mk_tx_varlen(1, "zion1AB".into(), Some("".into()));
        let b = mk_tx_varlen(1, "zion1A".into(), Some("B".into()));
        assert_eq!(
            a.calculate_hash(),
            b.calculate_hash(),
            "v1 preimage is expected to collide here (audit section 3.2). \
             Do NOT 'fix' v1 in place - it would retroactively invalidate \
             historical UTXO IDs. Bump the tx version field to \
             TX_HASH_V2_VERSION and activate v2 via a coordinated hard \
             fork instead.",
        );
    }

    /// Paired fix: the v2 preimage, because every variable-length field
    /// is prefixed with its length, produces distinct hashes for the
    /// same two inputs above.
    #[test]
    fn tx_hash_v2_rejects_address_memo_boundary_collision() {
        let a = mk_tx_varlen(TX_HASH_V2_VERSION, "zion1AB".into(), Some("".into()));
        let b = mk_tx_varlen(TX_HASH_V2_VERSION, "zion1A".into(), Some("B".into()));
        assert_ne!(
            a.calculate_hash(),
            b.calculate_hash(),
            "v2 length-prefixed preimage must not collide on the boundary \
             shift that v1 does; if this ever starts passing, the length \
             prefixes have been dropped",
        );
    }

    /// The v2 `Option<memo>` encoding distinguishes memo-absent
    /// (`None`) from memo-empty-string (`Some("")`) via a 1-byte tag.
    /// In v1 these two hash identically because `None` is simply skipped
    /// and `Some("")` appends zero bytes.
    #[test]
    fn tx_hash_v2_distinguishes_none_memo_from_empty_string_memo() {
        let none = mk_tx_varlen(TX_HASH_V2_VERSION, "zion1foo".into(), None);
        let empty = mk_tx_varlen(TX_HASH_V2_VERSION, "zion1foo".into(), Some("".into()));
        assert_ne!(
            none.calculate_hash(),
            empty.calculate_hash(),
            "v2 must distinguish Option::None from Option::Some(empty)"
        );
    }

    /// Activation safety: changing the tx format version must change
    /// the computed hash even with identical field content. This is why
    /// the v2 preimage prefixes with a `ZION_TX_V2\0` domain-separation
    /// tag — without it, a v1 tx and a v2 tx with the same fields would
    /// share a preimage and collide across the fork boundary.
    #[test]
    fn tx_hash_v1_and_v2_are_domain_separated() {
        let v1 = mk_tx_varlen(1, "zion1foo".into(), Some("bar".into()));
        let v2 = mk_tx_varlen(TX_HASH_V2_VERSION, "zion1foo".into(), Some("bar".into()));
        assert_ne!(
            v1.calculate_hash(),
            v2.calculate_hash(),
            "v1 and v2 preimages must be domain-separated to prevent \
             cross-fork hash collisions",
        );
    }

    /// Backward-compat: activating v2 in the codebase must not change
    /// the hash of any existing v1 tx. If this test ever fails, every
    /// historical UTXO ID in the chain is about to be invalidated.
    #[test]
    fn tx_hash_v1_activation_is_backward_compatible() {
        let tx = make_signed_tx();
        // `make_signed_tx` sets version = 1.
        assert_eq!(tx.version, 1);
        // Spot-check a hash we can bind to regression by storing its
        // computation directly from the v1 algorithm and confirming
        // `calculate_hash` dispatches there.
        let via_dispatch = tx.calculate_hash();
        let via_v1_direct = tx.calculate_hash_v1();
        assert_eq!(
            via_dispatch, via_v1_direct,
            "version = 1 must dispatch to the legacy v1 preimage"
        );
    }
}
