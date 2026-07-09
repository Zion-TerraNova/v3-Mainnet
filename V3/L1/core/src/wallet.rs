//! ZION Wallet — UTXO coin selection, transaction building, and signing.
//!
//! Provides the core logic for constructing and signing transactions:
//! - Largest-first UTXO coin selection
//! - Explicit change output generation
//! - Ed25519 signing with post-sign `zeroize`
//! - Multi-recipient batch payouts (pool PPLNS)
//! - Account-model transaction building for hybrid balance sends

use crate::crypto;
use crate::fee;
use crate::tx::{Transaction, TxInput, TxOutput, TX_HASH_V2_VERSION};
use ed25519_dalek::SigningKey;
use std::time::{SystemTime, UNIX_EPOCH};
use zion_cosmic_harmony::tx_hash_v2_active;

/// Choose `Transaction.version` for UTXO txs broadcast while chain tip is `chain_tip_height`.
///
/// Transactions are validated for inclusion in block `chain_tip_height + 1`, matching
/// mempool admission (`pending_height = chain_tip_height + 1`).
/// mempool admission logic.
#[inline]
pub fn pending_utxo_tx_version(chain_tip_height: u64) -> u32 {
    let pending_height = chain_tip_height.saturating_add(1);
    if tx_hash_v2_active(pending_height) {
        TX_HASH_V2_VERSION
    } else {
        1
    }
}

// ── Constants ──────────────────────────────────────────────────────────

/// Maximum recipients in a single batch payout transaction.
pub const MAX_BATCH_RECIPIENTS: usize = 200;

/// Minimum payout amount: 10 ZION in flowers (post-3.0.3: 6-decimal).
/// Pre-3.0.3: was 10_000_000_000_000 (10 ZION × 1e12).
pub const MIN_PAYOUT_AMOUNT: u64 = 10_000_000;

// ── Types ──────────────────────────────────────────────────────────────

/// A spendable UTXO known to the wallet.
#[derive(Debug, Clone)]
pub struct SpendableUtxo {
    pub tx_hash: [u8; 32],
    pub output_index: u32,
    pub amount: u64,
    pub address: String,
}

/// Parameters for a single send operation.
#[derive(Debug, Clone)]
pub struct SendParams {
    pub to_address: String,
    pub amount: u64,
    pub fee: u64,
    /// Optional memo attached to the primary output (e.g. `BRIDGE:base:0x...`).
    /// Change outputs never carry a memo.
    pub memo: Option<String>,
}

/// Result of building a transaction.
#[derive(Debug, Clone)]
pub struct BuildResult {
    pub transaction: Transaction,
    /// The change UTXO returned to the sender (if any).
    pub change_amount: u64,
}

/// Wallet errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalletError {
    InsufficientFunds { available: u64, needed: u64 },
    NoUtxos,
    InvalidAddress(String),
    InvalidMemo(String),
    FeeTooLow { fee: u64, minimum: u64 },
    AmountTooSmall(u64),
    TooManyRecipients(usize),
    SigningFailed,
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientFunds { available, needed } => {
                write!(f, "insufficient funds: have {available}, need {needed}")
            }
            Self::NoUtxos => write!(f, "no spendable UTXOs"),
            Self::InvalidAddress(a) => write!(f, "invalid address: {a}"),
            Self::InvalidMemo(m) => write!(f, "invalid memo: {m}"),
            Self::FeeTooLow { fee, minimum } => write!(f, "fee {fee} below minimum {minimum}"),
            Self::AmountTooSmall(a) => write!(f, "amount {a} too small"),
            Self::TooManyRecipients(n) => {
                write!(f, "too many recipients: {n} (max {MAX_BATCH_RECIPIENTS})")
            }
            Self::SigningFailed => write!(f, "signing failed"),
        }
    }
}

// ── Coin selection ─────────────────────────────────────────────────────

/// Select UTXOs using largest-first strategy.
///
/// Returns selected UTXOs and total selected amount, or error if insufficient.
fn select_utxos(
    available: &[SpendableUtxo],
    target: u64,
) -> Result<(Vec<&SpendableUtxo>, u64), WalletError> {
    if available.is_empty() {
        return Err(WalletError::NoUtxos);
    }

    let mut sorted: Vec<&SpendableUtxo> = available.iter().collect();
    sorted.sort_by(|a, b| b.amount.cmp(&a.amount));

    let mut selected = Vec::new();
    let mut total: u64 = 0;
    for utxo in sorted {
        selected.push(utxo);
        total = total.saturating_add(utxo.amount);
        if total >= target {
            return Ok((selected, total));
        }
    }
    Err(WalletError::InsufficientFunds {
        available: total,
        needed: target,
    })
}

// ── Build & sign ───────────────────────────────────────────────────────

/// Build and sign a single-recipient transaction.
///
/// - Selects UTXOs (largest-first) to cover `params.amount + params.fee`
/// - Creates change output if excess > 0
/// - Signs all inputs with Ed25519
/// - Zeroizes signing key bytes after use
pub fn build_and_sign(
    signing_key: &SigningKey,
    change_address: &str,
    params: &SendParams,
    available_utxos: &[SpendableUtxo],
    chain_tip_height: u64,
) -> Result<BuildResult, WalletError> {
    // Validate
    if !crypto::is_valid_address(&params.to_address) {
        return Err(WalletError::InvalidAddress(params.to_address.clone()));
    }

    let num_inputs_est = available_utxos.len().min(10); // rough estimate
    let num_outputs_est = 2; // send + change
    let estimated_size = fee::estimate_tx_size(num_inputs_est, num_outputs_est);
    let min_fee = fee::minimum_fee_for_size(estimated_size);
    if params.fee < min_fee {
        return Err(WalletError::FeeTooLow {
            fee: params.fee,
            minimum: min_fee,
        });
    }

    let target = params
        .amount
        .checked_add(params.fee)
        .ok_or(WalletError::InsufficientFunds {
            available: 0,
            needed: u64::MAX,
        })?;

    let (selected, total) = select_utxos(available_utxos, target)?;

    let change = total - target;

    // Build outputs
    let mut outputs = vec![TxOutput {
        amount: params.amount,
        address: params.to_address.clone(),
        memo: params.memo.clone(),
    }];
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            address: change_address.to_string(),
            memo: None,
        });
    }

    // Build inputs (unsigned)
    let vk = signing_key.verifying_key();
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|utxo| TxInput {
            prev_tx_hash: utxo.tx_hash,
            output_index: utxo.output_index,
            signature: vec![],
            public_key: vk.as_bytes().to_vec(),
        })
        .collect();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut tx = Transaction {
        id: [0u8; 32],
        version: pending_utxo_tx_version(chain_tip_height),
        inputs,
        outputs,
        fee: params.fee,
        timestamp: now,
    };

    // Finalize hash (before signing)
    tx.finalize_id();

    // Sign each input
    let mut key_bytes = signing_key.to_bytes();
    for input in &mut tx.inputs {
        let sig =
            crypto::sign_and_zeroize(key_bytes, &tx.id).map_err(|_| WalletError::SigningFailed)?;
        input.signature = sig.to_vec();
        // Re-derive key for next input (zeroize happens inside sign_and_zeroize)
        key_bytes = signing_key.to_bytes();
    }
    // Final zeroize
    use zeroize::Zeroize;
    key_bytes.zeroize();

    Ok(BuildResult {
        change_amount: change,
        transaction: tx,
    })
}

// ── Batch payouts ──────────────────────────────────────────────────────

/// Recipient for a batch payout.
#[derive(Debug, Clone)]
pub struct BatchRecipient {
    pub address: String,
    pub amount: u64,
}

/// Build and sign a multi-recipient batch payout transaction (for pool PPLNS).
pub fn build_batch_payout(
    signing_key: &SigningKey,
    change_address: &str,
    recipients: &[BatchRecipient],
    fee: u64,
    available_utxos: &[SpendableUtxo],
    chain_tip_height: u64,
) -> Result<BuildResult, WalletError> {
    if recipients.len() > MAX_BATCH_RECIPIENTS {
        return Err(WalletError::TooManyRecipients(recipients.len()));
    }

    let total_payout: u64 = recipients.iter().map(|r| r.amount).sum();
    let target = total_payout
        .checked_add(fee)
        .ok_or(WalletError::InsufficientFunds {
            available: 0,
            needed: u64::MAX,
        })?;

    let (selected, total) = select_utxos(available_utxos, target)?;
    let change = total - target;

    // Build outputs
    let mut outputs: Vec<TxOutput> = recipients
        .iter()
        .map(|r| TxOutput {
            amount: r.amount,
            address: r.address.clone(),
            memo: None,
        })
        .collect();
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            address: change_address.to_string(),
            memo: None,
        });
    }

    let vk = signing_key.verifying_key();
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|utxo| TxInput {
            prev_tx_hash: utxo.tx_hash,
            output_index: utxo.output_index,
            signature: vec![],
            public_key: vk.as_bytes().to_vec(),
        })
        .collect();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut tx = Transaction {
        id: [0u8; 32],
        version: pending_utxo_tx_version(chain_tip_height),
        inputs,
        outputs,
        fee,
        timestamp: now,
    };
    tx.finalize_id();

    let mut key_bytes = signing_key.to_bytes();
    for input in &mut tx.inputs {
        let sig =
            crypto::sign_and_zeroize(key_bytes, &tx.id).map_err(|_| WalletError::SigningFailed)?;
        input.signature = sig.to_vec();
        key_bytes = signing_key.to_bytes();
    }
    use zeroize::Zeroize;
    key_bytes.zeroize();

    Ok(BuildResult {
        change_amount: change,
        transaction: tx,
    })
}

// ── Account-model build & sign ─────────────────────────────────────────

/// Generate a deterministic 64-hex-char tx_id for an account transaction.
/// If a memo is provided and the account-model memo v1 gate is active at the
/// given chain height, it is mixed into the preimage so the memo is covered by
/// the signature.
pub fn generate_account_tx_id(
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
    memo: Option<&str>,
    chain_height: u64,
) -> String {
    let mut bytes = [0u8; 32];
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    bytes[..16].copy_from_slice(&ts.to_le_bytes());
    bytes[16..24].copy_from_slice(&amount.to_le_bytes());
    bytes[24..32].copy_from_slice(&nonce.to_le_bytes());
    // XOR-in sender and recipient for uniqueness
    for (i, b) in from.bytes().chain(to.bytes()).enumerate() {
        bytes[i % 32] ^= b;
    }
    // XOR-in memo bytes if the memo v1 gate is active, so the tx_id commits to
    // the memo and the signature covers it.
    if let Some(m) = memo {
        if zion_cosmic_harmony::account_tx_memo_v1_active(chain_height) {
            for (i, b) in m.bytes().enumerate() {
                bytes[i % 32] ^= b;
            }
        }
    }
    hex::encode(bytes)
}

/// Build and sign an account-model transaction.
///
/// Creates a [`crate::Transaction`] (account model) signed with Ed25519.
/// The `nonce` must be unique per sender to prevent replay attacks.
/// `chain_tip` is used to decide whether the account-model memo v1 hard fork
/// is active when computing the signed tx_id.
#[allow(clippy::too_many_arguments)]
pub fn build_and_sign_account(
    signing_key: &SigningKey,
    from_address: &str,
    to_address: &str,
    amount_zion: u128,
    fee_zion: u64,
    nonce: u64,
    memo: Option<String>,
    chain_tip: u64,
) -> Result<crate::Transaction, WalletError> {
    if !crypto::is_valid_address(to_address) {
        return Err(WalletError::InvalidAddress(to_address.to_string()));
    }

    if let Some(ref m) = memo {
        if m.len() > 256 {
            return Err(WalletError::InvalidMemo(
                "memo exceeds 256 bytes".to_string(),
            ));
        }
        if !m.is_ascii() {
            return Err(WalletError::InvalidMemo("memo must be ASCII".to_string()));
        }
    }

    let tx_id = generate_account_tx_id(
        from_address,
        to_address,
        amount_zion as u64,
        nonce,
        memo.as_deref(),
        chain_tip,
    );
    let pk_hex = crypto::to_hex(signing_key.verifying_key().as_bytes());

    let key_bytes = signing_key.to_bytes();
    let sig = crypto::sign_and_zeroize(key_bytes, tx_id.as_bytes())
        .map_err(|_| WalletError::SigningFailed)?;

    Ok(crate::Transaction {
        tx_id,
        from: from_address.to_string(),
        to: to_address.to_string(),
        amount_zion,
        fee_zion,
        nonce,
        signature: crypto::to_hex(&sig),
        public_key: pk_hex,
        memo,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_address, generate_keypair};

    fn make_utxos(amounts: &[u64], address: &str) -> Vec<SpendableUtxo> {
        amounts
            .iter()
            .enumerate()
            .map(|(i, &amount)| SpendableUtxo {
                tx_hash: [i as u8; 32],
                output_index: 0,
                amount,
                address: address.to_string(),
            })
            .collect()
    }

    #[cfg(not(feature = "testnet_fork_rehearsal"))]
    #[test]
    fn pending_utxo_tx_version_is_v2_from_genesis() {
        assert_eq!(pending_utxo_tx_version(0), TX_HASH_V2_VERSION);
        assert_eq!(pending_utxo_tx_version(1), TX_HASH_V2_VERSION);
        assert_eq!(pending_utxo_tx_version(u64::MAX / 2), TX_HASH_V2_VERSION);
    }

    #[cfg(feature = "testnet_fork_rehearsal")]
    #[test]
    fn pending_utxo_tx_version_respects_rehearsal_gate() {
        assert_eq!(pending_utxo_tx_version(0), 1);
        assert_eq!(pending_utxo_tx_version(8), 1);
        assert_eq!(pending_utxo_tx_version(9), TX_HASH_V2_VERSION);
        assert_eq!(pending_utxo_tx_version(10), TX_HASH_V2_VERSION);
        assert_eq!(pending_utxo_tx_version(u64::MAX / 2), TX_HASH_V2_VERSION);
    }

    #[test]
    fn build_and_sign_single_send() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[5_000_000_000_000], &addr); // 5 ZION

        let params = SendParams {
            to_address: derive_address(&[99u8; 32]),
            amount: 2_000_000_000_000, // 2 ZION
            fee: 1_000,
            memo: None,
        };

        let result = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap();
        assert!(result.transaction.verify_signatures());
        assert_eq!(result.transaction.outputs[0].amount, 2_000_000_000_000);
        assert_eq!(
            result.change_amount,
            5_000_000_000_000 - 2_000_000_000_000 - 1_000
        );
    }

    #[test]
    fn build_and_sign_exact_amount_no_change() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[1_001_000], &addr);

        let params = SendParams {
            to_address: derive_address(&[99u8; 32]),
            amount: 1_000_000,
            fee: 1_000,
            memo: None,
        };

        let result = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap();
        assert_eq!(result.change_amount, 0);
        assert_eq!(result.transaction.outputs.len(), 1); // no change output
    }

    #[test]
    fn insufficient_funds_error() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[500], &addr);

        let params = SendParams {
            to_address: derive_address(&[99u8; 32]),
            amount: 1_000_000,
            fee: 1_000,
            memo: None,
        };

        let err = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap_err();
        assert!(matches!(err, WalletError::InsufficientFunds { .. }));
    }

    #[test]
    fn no_utxos_error() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());

        let params = SendParams {
            to_address: derive_address(&[99u8; 32]),
            amount: 1_000,
            fee: 1_000,
            memo: None,
        };

        let err = build_and_sign(&sk, &addr, &params, &[], 0).unwrap_err();
        assert_eq!(err, WalletError::NoUtxos);
    }

    #[test]
    fn invalid_address_rejected() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[1_000_000], &addr);

        let params = SendParams {
            to_address: "invalid_address".to_string(),
            amount: 1_000,
            fee: 1_000,
            memo: None,
        };

        let err = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap_err();
        assert!(matches!(err, WalletError::InvalidAddress(_)));
    }

    #[test]
    fn largest_first_coin_selection() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        // 3 UTXOs: 100, 500, 200 → should select 500 first
        let utxos = make_utxos(&[100_000, 500_000, 200_000], &addr);

        let params = SendParams {
            to_address: derive_address(&[99u8; 32]),
            amount: 400_000,
            fee: 1_000,
            memo: None,
        };

        let result = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap();
        // Should use the 500K UTXO only
        assert_eq!(result.transaction.inputs.len(), 1);
        assert_eq!(result.change_amount, 500_000 - 400_000 - 1_000);
    }

    #[test]
    fn batch_payout_basic() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[100_000_000_000_000], &addr); // 100 ZION

        let recipients: Vec<BatchRecipient> = (0..5)
            .map(|i| BatchRecipient {
                address: derive_address(&[i as u8; 32]),
                amount: MIN_PAYOUT_AMOUNT,
            })
            .collect();

        let result = build_batch_payout(&sk, &addr, &recipients, 5_000, &utxos, 0).unwrap();
        assert!(result.transaction.verify_signatures());
        assert_eq!(result.transaction.outputs.len(), 6); // 5 recipients + change
        let total_out: u64 = result.transaction.outputs.iter().map(|o| o.amount).sum();
        assert_eq!(total_out + result.transaction.fee, 100_000_000_000_000);
    }

    #[test]
    fn batch_too_many_recipients() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[u64::MAX / 2], &addr);

        let recipients: Vec<BatchRecipient> = (0..201)
            .map(|i| BatchRecipient {
                address: derive_address(&[i as u8; 32]),
                amount: 1_000,
            })
            .collect();

        let err = build_batch_payout(&sk, &addr, &recipients, 1_000, &utxos, 0).unwrap_err();
        assert!(matches!(err, WalletError::TooManyRecipients(201)));
    }

    #[test]
    fn signatures_verify_after_build() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let utxos = make_utxos(&[1_000_000, 2_000_000], &addr);

        let params = SendParams {
            to_address: derive_address(&[42u8; 32]),
            amount: 2_500_000,
            fee: 1_000,
            memo: None,
        };

        let result = build_and_sign(&sk, &addr, &params, &utxos, 0).unwrap();
        assert!(result.transaction.verify_signatures());
        // Should use both UTXOs (2M + 1M = 3M, need 2.501M)
        assert_eq!(result.transaction.inputs.len(), 2);
    }

    #[test]
    fn build_and_sign_account_basic() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let dest = derive_address(&[99u8; 32]);

        let tx = build_and_sign_account(&sk, &addr, &dest, 1_000_000, 1_000, 42, None, 1).unwrap();
        assert_eq!(tx.from, addr);
        assert_eq!(tx.to, dest);
        assert_eq!(tx.amount_zion, 1_000_000);
        assert_eq!(tx.fee_zion, 1_000);
        assert_eq!(tx.nonce, 42);
        assert!(tx.memo.is_none());
        assert!(!tx.signature.is_empty());
        assert!(!tx.public_key.is_empty());
        assert!(!tx.tx_id.is_empty());
    }

    #[test]
    fn build_and_sign_account_with_memo() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let dest = derive_address(&[99u8; 32]);

        let tx_no_memo =
            build_and_sign_account(&sk, &addr, &dest, 1_000_000, 1_000, 42, None, 1).unwrap();
        let tx_with_memo = build_and_sign_account(
            &sk,
            &addr,
            &dest,
            1_000_000,
            1_000,
            42,
            Some("BRIDGE:base:0x1234".to_string()),
            1,
        )
        .unwrap();
        assert!(tx_with_memo.memo.is_some());
        assert_ne!(
            tx_no_memo.tx_id, tx_with_memo.tx_id,
            "memo must change tx_id"
        );
        assert!(tx_with_memo.verify_signature());
    }

    #[test]
    fn build_and_sign_account_rejects_invalid_address() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let err =
            build_and_sign_account(&sk, &addr, "invalid", 1_000, 1_000, 1, None, 1).unwrap_err();
        assert!(matches!(err, WalletError::InvalidAddress(_)));
    }

    #[test]
    fn build_and_sign_account_rejects_non_ascii_memo() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let dest = derive_address(&[99u8; 32]);
        let err = build_and_sign_account(
            &sk,
            &addr,
            &dest,
            1_000,
            1_000,
            1,
            Some("žluťoučký".to_string()),
            1,
        )
        .unwrap_err();
        assert!(matches!(err, WalletError::InvalidMemo(_)));
    }

    /// Regression test for CRITICAL 3.0.4 Finding 1: an account transaction
    /// whose `public_key` does not derive to the `from` address must be
    /// rejected, even if the Ed25519 signature is otherwise valid. Without the
    /// from-address derivation check, any funded account could be spent by
    /// signing with an unrelated key.
    #[test]
    fn verify_signature_rejects_public_key_not_matching_sender() {
        // Victim owns a funded account.
        let (victim_sk, victim_vk) = generate_keypair();
        let victim_addr = derive_address(victim_vk.as_bytes());
        let dest = derive_address(&[99u8; 32]);

        // A genuine victim-signed transaction must verify.
        let legit = build_and_sign_account(
            &victim_sk,
            &victim_addr,
            &dest,
            1_000_000,
            1_000,
            1,
            None,
            1,
        )
        .unwrap();
        assert!(legit.verify_signature(), "legit victim tx must verify");

        // Attacker forges a transaction that spends from the victim address but
        // signs it with their own unrelated key and presents their own pubkey.
        let (attacker_sk, attacker_vk) = generate_keypair();
        let mut forged = legit.clone();
        let sig = crypto::sign(&attacker_sk, forged.tx_id.as_bytes());
        forged.signature = crypto::to_hex(&sig);
        forged.public_key = crypto::to_hex(attacker_vk.as_bytes());

        // Signature is valid for (attacker_pk, tx_id) but the key does not
        // derive to `from` (victim) → must be rejected.
        assert!(
            !forged.verify_signature(),
            "forged tx with a key not matching the sender address must be rejected"
        );
    }
}
