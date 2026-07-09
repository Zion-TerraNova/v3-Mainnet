//! Full block validation pipeline for ZION V3.
//!
//! 10-step validation that every block must pass before acceptance:
//!
//! 1. **Structure** — non-empty, within MAX_BLOCK_SIZE
//! 2. **PoW** — hash meets difficulty target
//! 3. **Difficulty** — matches LWMA output for this height
//! 4. **Timestamp** — within ±MAX_TIMESTAMP_DRIFT of median-time-past
//! 5. **Merkle root** — binary Merkle tree (BLAKE3 hash pairs)
//! 6. **Tx signatures** — Ed25519 per input (SegWit-style)
//! 7. **UTXO double-spend** — no input references already-spent output
//! 8. **Coinbase maturity** — coinbase outputs unspendable for COINBASE_MATURITY blocks
//! 9. **Fee validation** — meets MIN_TX_FEE, fee-rate
//! 10. **Subsidy validation** — coinbase ≤ block reward (no fee in coinbase)

use crate::crypto;
use crate::emission;
use crate::fee;
use crate::genesis;
use crate::tx::Transaction;

// ── Constants ──────────────────────────────────────────────────────────

/// Coinbase outputs are unspendable for this many blocks.
pub const COINBASE_MATURITY: u64 = emission::COINBASE_MATURITY;

/// Maximum block size in bytes (1 MB).
pub const MAX_BLOCK_SIZE: usize = 1_048_576;

/// Maximum timestamp drift from median-time-past (2 hours).
pub const MAX_TIMESTAMP_DRIFT: u64 = 7_200;

// ── Merkle tree ────────────────────────────────────────────────────────

/// Compute binary Merkle root from a list of transaction hashes using BLAKE3.
///
/// If empty, returns all-zeros. If single, returns that hash.
/// Duplicates the last element if odd count (Bitcoin-style).
pub fn merkle_root(tx_hashes: &[[u8; 32]]) -> [u8; 32] {
    if tx_hashes.is_empty() {
        return [0u8; 32];
    }
    if tx_hashes.len() == 1 {
        return tx_hashes[0];
    }

    let mut level: Vec<[u8; 32]> = tx_hashes.to_vec();
    while level.len() > 1 {
        if level.len() % 2 == 1 {
            let last = *level.last().unwrap();
            level.push(last);
        }
        let mut next = Vec::with_capacity(level.len() / 2);
        for pair in level.chunks_exact(2) {
            let mut combined = [0u8; 64];
            combined[..32].copy_from_slice(&pair[0]);
            combined[32..].copy_from_slice(&pair[1]);
            next.push(crypto::blake3_hash(&combined));
        }
        level = next;
    }
    level[0]
}

// ── Validation errors ──────────────────────────────────────────────────

/// Specific reason a block failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyBlock,
    BlockTooLarge {
        size: usize,
        max: usize,
    },
    PowInvalid,
    DifficultyMismatch {
        expected: u64,
        got: u64,
    },
    TimestampTooFarFuture {
        timestamp: u64,
        max: u64,
    },
    TimestampTooOld {
        timestamp: u64,
        min: u64,
    },
    MerkleRootMismatch {
        expected: [u8; 32],
        got: [u8; 32],
    },
    InvalidSignature {
        tx_index: usize,
    },
    DoubleSpend {
        tx_index: usize,
        input_index: usize,
    },
    ImmatureCoinbase {
        tx_index: usize,
        input_index: usize,
        age: u64,
    },
    FeeTooLow {
        tx_index: usize,
    },
    SubsidyExceeded {
        coinbase_output: u64,
        max_reward: u64,
    },
    NoCoinbase,
    CoinbaseHasInputs,
    /// Spending from a time-locked premine address before its unlock height.
    LockedPremine {
        tx_index: usize,
        address: String,
        unlock_height: u64,
    },
    /// A transaction input refers to a UTXO that does not exist (or is
    /// already spent) in the resolver's view. Bridge unlock transactions
    /// are validated separately and excluded from this check.
    InputNotFound {
        tx_index: usize,
        input_index: usize,
        prev_tx_hash: [u8; 32],
        output_index: u32,
    },
    /// A non-coinbase, non-bridge-unlock transaction's outputs (plus fee)
    /// exceed the total value of its referenced inputs
    /// (∑inputs < ∑outputs + fee). This is the core "no money printing"
    /// rule: the consensus must never accept a UTXO transfer that would
    /// create value out of thin air.
    ValueNotConserved {
        tx_index: usize,
        inputs_sum: u64,
        outputs_plus_fee: u64,
    },
    /// Numeric overflow while summing UTXO inputs or outputs. Treated as
    /// a hard rejection to avoid masking malformed data with wrap-around
    /// arithmetic.
    ValueOverflow {
        tx_index: usize,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBlock => write!(f, "block has no transactions"),
            Self::BlockTooLarge { size, max } => write!(f, "block {size} bytes exceeds {max}"),
            Self::PowInvalid => write!(f, "PoW hash does not meet target"),
            Self::DifficultyMismatch { expected, got } => {
                write!(f, "difficulty mismatch: expected {expected}, got {got}")
            }
            Self::TimestampTooFarFuture { timestamp, max } => {
                write!(f, "timestamp {timestamp} too far in future (max {max})")
            }
            Self::TimestampTooOld { timestamp, min } => {
                write!(f, "timestamp {timestamp} too old (min {min})")
            }
            Self::MerkleRootMismatch { .. } => write!(f, "merkle root mismatch"),
            Self::InvalidSignature { tx_index } => write!(f, "invalid signature in tx {tx_index}"),
            Self::DoubleSpend {
                tx_index,
                input_index,
            } => write!(f, "double-spend in tx {tx_index} input {input_index}"),
            Self::ImmatureCoinbase {
                tx_index,
                input_index,
                age,
            } => write!(
                f,
                "immature coinbase in tx {tx_index} input {input_index} (age {age})"
            ),
            Self::FeeTooLow { tx_index } => write!(f, "fee too low in tx {tx_index}"),
            Self::SubsidyExceeded {
                coinbase_output,
                max_reward,
            } => write!(
                f,
                "coinbase output {coinbase_output} exceeds reward {max_reward}"
            ),
            Self::NoCoinbase => write!(f, "block has no coinbase transaction"),
            Self::CoinbaseHasInputs => write!(f, "coinbase transaction has inputs"),
            Self::LockedPremine {
                tx_index,
                address,
                unlock_height,
            } => write!(
                f,
                "tx {tx_index} spends locked premine address {address} (unlock at {unlock_height})"
            ),
            Self::InputNotFound {
                tx_index,
                input_index,
                prev_tx_hash,
                output_index,
            } => write!(
                f,
                "tx {tx_index} input {input_index} references missing/spent UTXO {}:{}",
                crate::hex(prev_tx_hash),
                output_index,
            ),
            Self::ValueNotConserved {
                tx_index,
                inputs_sum,
                outputs_plus_fee,
            } => write!(
                f,
                "tx {tx_index} attempts to mint value: inputs={inputs_sum}, outputs+fee={outputs_plus_fee}"
            ),
            Self::ValueOverflow { tx_index } => write!(
                f,
                "tx {tx_index} arithmetic overflow while summing inputs or outputs"
            ),
        }
    }
}

// ── Validation context ─────────────────────────────────────────────────

/// Information about a UTXO needed for validation.
#[derive(Debug, Clone)]
pub struct UtxoInfo {
    pub amount: u64,
    pub address: String,
    /// Height at which this UTXO was created.
    pub created_height: u64,
    /// Whether this UTXO was created by a coinbase transaction.
    pub is_coinbase: bool,
}

/// Context needed to validate a block.
pub struct ValidationContext {
    /// Block height being validated.
    pub height: u64,
    /// Expected difficulty for this block.
    pub expected_difficulty: u64,
    /// Median time of the past 11 blocks.
    pub median_time_past: u64,
    /// Current time (for future timestamp check).
    pub current_time: u64,
}

// ── Step functions ─────────────────────────────────────────────────────

/// Step 1: Block structure — must have transactions, within size limit.
pub fn validate_structure(
    transactions: &[Transaction],
    block_size_bytes: usize,
) -> Result<(), ValidationError> {
    if transactions.is_empty() {
        return Err(ValidationError::EmptyBlock);
    }
    if block_size_bytes > MAX_BLOCK_SIZE {
        return Err(ValidationError::BlockTooLarge {
            size: block_size_bytes,
            max: MAX_BLOCK_SIZE,
        });
    }
    Ok(())
}

/// Step 4: Timestamp validation — within ±MAX_TIMESTAMP_DRIFT of median-time-past.
pub fn validate_timestamp(
    block_timestamp: u64,
    median_time_past: u64,
    current_time: u64,
) -> Result<(), ValidationError> {
    let max_allowed = current_time + MAX_TIMESTAMP_DRIFT;
    if block_timestamp > max_allowed {
        return Err(ValidationError::TimestampTooFarFuture {
            timestamp: block_timestamp,
            max: max_allowed,
        });
    }
    // Block must be newer than median-time-past
    if block_timestamp <= median_time_past.saturating_sub(MAX_TIMESTAMP_DRIFT) {
        return Err(ValidationError::TimestampTooOld {
            timestamp: block_timestamp,
            min: median_time_past.saturating_sub(MAX_TIMESTAMP_DRIFT),
        });
    }
    Ok(())
}

/// Step 5: Validate merkle root matches computed root from transaction hashes.
pub fn validate_merkle_root(
    expected: &[u8; 32],
    transactions: &[Transaction],
) -> Result<(), ValidationError> {
    let tx_hashes: Vec<[u8; 32]> = transactions.iter().map(|tx| tx.id).collect();
    let computed = merkle_root(&tx_hashes);
    if &computed != expected {
        return Err(ValidationError::MerkleRootMismatch {
            expected: *expected,
            got: computed,
        });
    }
    Ok(())
}

/// Step 6: Validate all transaction signatures (skip coinbase at index 0).
pub fn validate_signatures(transactions: &[Transaction]) -> Result<(), ValidationError> {
    for (i, tx) in transactions.iter().enumerate().skip(1) {
        if !tx.verify_signatures() {
            return Err(ValidationError::InvalidSignature { tx_index: i });
        }
    }
    Ok(())
}

/// Step 7: Check for double-spends within the block.
pub fn validate_no_double_spend(transactions: &[Transaction]) -> Result<(), ValidationError> {
    let mut spent: std::collections::HashSet<([u8; 32], u32)> = std::collections::HashSet::new();
    for (tx_i, tx) in transactions.iter().enumerate().skip(1) {
        for (inp_i, input) in tx.inputs.iter().enumerate() {
            let outpoint = (input.prev_tx_hash, input.output_index);
            if !spent.insert(outpoint) {
                return Err(ValidationError::DoubleSpend {
                    tx_index: tx_i,
                    input_index: inp_i,
                });
            }
        }
    }
    Ok(())
}

/// Step 8: Verify coinbase maturity — any input spending a coinbase UTXO
/// must have been confirmed at least COINBASE_MATURITY blocks ago.
pub fn validate_coinbase_maturity(
    transactions: &[Transaction],
    current_height: u64,
    utxo_lookup: &dyn Fn(&[u8; 32], u32) -> Option<UtxoInfo>,
) -> Result<(), ValidationError> {
    for (tx_i, tx) in transactions.iter().enumerate().skip(1) {
        for (inp_i, input) in tx.inputs.iter().enumerate() {
            if let Some(utxo) = utxo_lookup(&input.prev_tx_hash, input.output_index) {
                if utxo.is_coinbase {
                    let age = current_height.saturating_sub(utxo.created_height);
                    if age < COINBASE_MATURITY {
                        return Err(ValidationError::ImmatureCoinbase {
                            tx_index: tx_i,
                            input_index: inp_i,
                            age,
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

/// Step 9: Validate fees for all non-coinbase transactions.
pub fn validate_fees(
    transactions: &[Transaction],
    estimated_sizes: &[usize],
    block_height: u64,
) -> Result<(), ValidationError> {
    for (i, (tx, &size)) in transactions
        .iter()
        .zip(estimated_sizes.iter())
        .enumerate()
        .skip(1)
    {
        // Height-conditional: pre-migration txs have fees in legacy 12-decimal
        // flowers (min 1000), post-migration in new 6-decimal flowers (min 1).
        let valid = if crate::migration::is_post_migration(block_height) {
            fee::validate_fee(tx.fee, size).is_ok()
        } else {
            // Pre-migration: scale the fee check to legacy units
            // MIN_TX_FEE=1 in new scale → 1000 in legacy scale (× MIGRATION_DIVISOR)
            // MIN_FEE_RATE=1 in new scale → 1000000 in legacy scale
            let legacy_min_fee = fee::MIN_TX_FEE * crate::migration::MIGRATION_DIVISOR;
            let legacy_min_rate = fee::MIN_FEE_RATE * crate::migration::MIGRATION_DIVISOR;
            let rate_based = size as u64 * legacy_min_rate;
            let min_for_size = rate_based.max(legacy_min_fee);
            tx.fee >= min_for_size
        };
        if !valid {
            return Err(ValidationError::FeeTooLow { tx_index: i });
        }
    }
    Ok(())
}

/// Step 10: Validate coinbase subsidy — coinbase output ≤ block reward.
pub fn validate_subsidy(coinbase: &Transaction, block_height: u64) -> Result<(), ValidationError> {
    if !coinbase.is_coinbase() {
        return Err(ValidationError::CoinbaseHasInputs);
    }
    let total_output = coinbase.total_output();
    // Height-conditional: pre-migration blocks use legacy 12-decimal flowers,
    // post-migration blocks use new 6-decimal flowers.
    let max_reward = if crate::migration::is_post_migration(block_height) {
        fee::max_coinbase_output(block_height)
    } else {
        // Pre-migration: scale up to legacy flowers for comparison
        fee::max_coinbase_output(block_height) * crate::migration::MIGRATION_DIVISOR
    };
    if total_output > max_reward {
        return Err(ValidationError::SubsidyExceeded {
            coinbase_output: total_output,
            max_reward,
        });
    }
    Ok(())
}

/// Step 11: Validate premine lock — inputs spending from DAO Treasury addresses
/// must have the current height >= unlock_height.
///
/// Coinbase transactions are skipped via `tx.is_coinbase()` (they have no
/// inputs to validate) rather than positional `.skip(1)`. This matches the
/// pattern used by `validate_inputs_exist` and `validate_value_conservation`,
/// and is required for callers that pass the runtime's
/// `block.utxo_transactions` slice — that slice does not always have a
/// coinbase at index 0, so a positional skip would silently bypass the
/// timelock check on the very first transaction.
pub fn validate_premine_locks(
    transactions: &[Transaction],
    current_height: u64,
    utxo_lookup: &dyn Fn(&[u8; 32], u32) -> Option<UtxoInfo>,
    admin_unlocked: &dyn Fn(&str) -> bool,
) -> Result<(), ValidationError> {
    for (tx_i, tx) in transactions.iter().enumerate() {
        if tx.is_coinbase() {
            continue;
        }
        for input in &tx.inputs {
            if let Some(utxo) = utxo_lookup(&input.prev_tx_hash, input.output_index) {
                if genesis::is_premine_transfer_allowed(&utxo.address, current_height, admin_unlocked).is_err() {
                    // Find the unlock height for the error message
                    let unlock_height = genesis::PREMINE_OUTPUTS
                        .iter()
                        .find(|o| o.address == utxo.address)
                        .and_then(|o| o.unlock_height)
                        .unwrap_or(0);
                    return Err(ValidationError::LockedPremine {
                        tx_index: tx_i,
                        address: utxo.address.clone(),
                        unlock_height,
                    });
                }
            }
        }
    }
    Ok(())
}

/// Step 7b: Verify every non-coinbase, non-bridge-unlock transaction's inputs
/// resolve to UTXOs that currently exist (and are not already spent) in the
/// caller-provided view of the UTXO set.
///
/// Coinbase transactions (`tx.is_coinbase()`) are skipped because they have no
/// inputs. Bridge-unlock transactions are validated separately by the runtime
/// because they spend the keyless bridge vault, so they are skipped here when
/// the caller's predicate marks them.
pub fn validate_inputs_exist(
    transactions: &[Transaction],
    utxo_lookup: &dyn Fn(&[u8; 32], u32) -> Option<UtxoInfo>,
    is_bridge_unlock: &dyn Fn(&Transaction) -> bool,
) -> Result<(), ValidationError> {
    for (tx_i, tx) in transactions.iter().enumerate() {
        if tx.is_coinbase() {
            continue;
        }
        if is_bridge_unlock(tx) {
            continue;
        }
        for (inp_i, input) in tx.inputs.iter().enumerate() {
            if utxo_lookup(&input.prev_tx_hash, input.output_index).is_none() {
                return Err(ValidationError::InputNotFound {
                    tx_index: tx_i,
                    input_index: inp_i,
                    prev_tx_hash: input.prev_tx_hash,
                    output_index: input.output_index,
                });
            }
        }
    }
    Ok(())
}

/// Step 12: Conservation of value — for every non-coinbase, non-bridge-unlock
/// transaction, the sum of input UTXO amounts must be ≥ outputs + fee.
///
/// This is the core "no money printing" rule. Bridge-unlock transactions are
/// validated separately by the runtime (they spend the keyless bridge vault and
/// have their own conservation check that requires `inputs == outputs + fee`).
///
/// Returns `Ok(())` when every relevant transaction conserves value. Errors out
/// of the loop on the first offender to mirror the style of the other steps.
pub fn validate_value_conservation(
    transactions: &[Transaction],
    utxo_lookup: &dyn Fn(&[u8; 32], u32) -> Option<UtxoInfo>,
    is_bridge_unlock: &dyn Fn(&Transaction) -> bool,
) -> Result<(), ValidationError> {
    for (tx_i, tx) in transactions.iter().enumerate() {
        if tx.is_coinbase() {
            continue;
        }
        if is_bridge_unlock(tx) {
            continue;
        }

        let mut inputs_sum: u64 = 0;
        for (inp_i, input) in tx.inputs.iter().enumerate() {
            // Missing inputs are surfaced by `validate_inputs_exist`. Here we
            // tolerate the absence and short-circuit so the error type stays
            // descriptive and stable for callers that run the steps in order.
            let Some(utxo) = utxo_lookup(&input.prev_tx_hash, input.output_index) else {
                return Err(ValidationError::InputNotFound {
                    tx_index: tx_i,
                    input_index: inp_i,
                    prev_tx_hash: input.prev_tx_hash,
                    output_index: input.output_index,
                });
            };
            inputs_sum = inputs_sum
                .checked_add(utxo.amount)
                .ok_or(ValidationError::ValueOverflow { tx_index: tx_i })?;
        }

        // Sum outputs with `checked_add` rather than `tx.total_output()`,
        // which uses `Iterator::sum()` and silently wraps in release builds
        // (the V3 workspace does not enable `overflow-checks` in release).
        // Wrapping the output sum would let an attacker craft outputs that
        // wrap to a value ≤ inputs and bypass conservation entirely.
        let mut outputs_sum: u64 = 0;
        for output in &tx.outputs {
            outputs_sum = outputs_sum
                .checked_add(output.amount)
                .ok_or(ValidationError::ValueOverflow { tx_index: tx_i })?;
        }
        let outputs_plus_fee = outputs_sum
            .checked_add(tx.fee)
            .ok_or(ValidationError::ValueOverflow { tx_index: tx_i })?;

        if inputs_sum < outputs_plus_fee {
            return Err(ValidationError::ValueNotConserved {
                tx_index: tx_i,
                inputs_sum,
                outputs_plus_fee,
            });
        }
    }
    Ok(())
}

/// Run the full 11-step validation pipeline.
///
/// Steps 2 (PoW) and 3 (difficulty) are expected to be done by the caller
/// before invoking this, since they depend on mining header data not available
/// in the transaction list.
pub fn validate_block(
    transactions: &[Transaction],
    block_timestamp: u64,
    block_size_bytes: usize,
    merkle_root_expected: &[u8; 32],
    ctx: &ValidationContext,
    estimated_tx_sizes: &[usize],
    utxo_lookup: &dyn Fn(&[u8; 32], u32) -> Option<UtxoInfo>,
) -> Result<(), ValidationError> {
    // Step 1: Structure
    validate_structure(transactions, block_size_bytes)?;

    // Steps 2-3: PoW + difficulty — done externally

    // Step 4: Timestamp
    validate_timestamp(block_timestamp, ctx.median_time_past, ctx.current_time)?;

    // Step 5: Merkle root
    validate_merkle_root(merkle_root_expected, transactions)?;

    // Step 6: Signatures
    validate_signatures(transactions)?;

    // Step 7: Double-spend
    validate_no_double_spend(transactions)?;

    // Step 8: Coinbase maturity
    validate_coinbase_maturity(transactions, ctx.height, utxo_lookup)?;

    // Step 9: Fees
    validate_fees(transactions, estimated_tx_sizes, ctx.height)?;

    // Step 10: Subsidy
    if let Some(coinbase) = transactions.first() {
        validate_subsidy(coinbase, ctx.height)?;
    } else {
        return Err(ValidationError::NoCoinbase);
    }

    // Step 11: Premine lock enforcement (DAO Treasury + admin-lock)
    // Default: no admin unlocks (all admin-locked premine is frozen).
    // Chain state should override with a real unlock registry when available.
    validate_premine_locks(transactions, ctx.height, utxo_lookup, &|_| false)?;

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{derive_address, generate_keypair, sign};
    use crate::tx::{TxInput, TxOutput, TX_HASH_V2_VERSION};

    fn make_coinbase(height: u64) -> Transaction {
        let reward = emission::block_subsidy(height);
        let mut tx = Transaction {
            id: [0u8; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![],
            outputs: vec![TxOutput {
                amount: reward,
                address: "zion1miner000000000000000000000000000000test".to_string(),
                memo: None,
            }],
            fee: 0,
            timestamp: 1_700_000_000,
        };
        tx.finalize_id();
        tx
    }

    fn make_signed_tx(prev_hash: [u8; 32]) -> Transaction {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let mut tx = Transaction {
            id: [0u8; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: prev_hash,
                output_index: 0,
                signature: vec![],
                public_key: vk.as_bytes().to_vec(),
            }],
            outputs: vec![TxOutput {
                amount: 1_000_000,
                address: addr,
                memo: None,
            }],
            fee: 2_000,
            timestamp: 1_700_000_000,
        };
        tx.finalize_id();
        let sig = sign(&sk, &tx.id);
        tx.inputs[0].signature = sig.to_vec();
        tx
    }

    // ── Merkle tree ────────────────────────────────────────────────

    #[test]
    fn merkle_root_empty() {
        assert_eq!(merkle_root(&[]), [0u8; 32]);
    }

    #[test]
    fn merkle_root_single() {
        let h = [42u8; 32];
        assert_eq!(merkle_root(&[h]), h);
    }

    #[test]
    fn merkle_root_two_deterministic() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let r1 = merkle_root(&[a, b]);
        let r2 = merkle_root(&[a, b]);
        assert_eq!(r1, r2);
    }

    #[test]
    fn merkle_root_order_matters() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_ne!(merkle_root(&[a, b]), merkle_root(&[b, a]));
    }

    #[test]
    fn merkle_root_odd_count_handles_duplication() {
        let hashes = vec![[1u8; 32], [2u8; 32], [3u8; 32]];
        let root = merkle_root(&hashes);
        assert_ne!(root, [0u8; 32]);
    }

    // ── Step 1: Structure ──────────────────────────────────────────

    #[test]
    fn validate_structure_empty_rejected() {
        let err = validate_structure(&[], 100).unwrap_err();
        assert_eq!(err, ValidationError::EmptyBlock);
    }

    #[test]
    fn validate_structure_too_large_rejected() {
        let tx = make_coinbase(1);
        let err = validate_structure(&[tx], MAX_BLOCK_SIZE + 1).unwrap_err();
        assert!(matches!(err, ValidationError::BlockTooLarge { .. }));
    }

    #[test]
    fn validate_structure_ok() {
        let tx = make_coinbase(1);
        assert!(validate_structure(&[tx], 500).is_ok());
    }

    // ── Step 4: Timestamp ──────────────────────────────────────────

    #[test]
    fn validate_timestamp_ok() {
        assert!(validate_timestamp(1000, 900, 1000).is_ok());
    }

    #[test]
    fn validate_timestamp_too_far_future() {
        let err = validate_timestamp(20000, 900, 1000).unwrap_err();
        assert!(matches!(err, ValidationError::TimestampTooFarFuture { .. }));
    }

    // ── Step 5: Merkle root ────────────────────────────────────────

    #[test]
    fn validate_merkle_root_correct() {
        let cb = make_coinbase(1);
        let root = merkle_root(&[cb.id]);
        assert!(validate_merkle_root(&root, &[cb]).is_ok());
    }

    #[test]
    fn validate_merkle_root_mismatch() {
        let cb = make_coinbase(1);
        let bad_root = [0xFF; 32];
        let err = validate_merkle_root(&bad_root, &[cb]).unwrap_err();
        assert!(matches!(err, ValidationError::MerkleRootMismatch { .. }));
    }

    // ── Step 6: Signatures ─────────────────────────────────────────

    #[test]
    fn validate_signatures_ok() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xBB; 32]);
        assert!(validate_signatures(&[cb, tx]).is_ok());
    }

    #[test]
    fn validate_signatures_bad_sig() {
        let cb = make_coinbase(1);
        let mut tx = make_signed_tx([0xBB; 32]);
        tx.inputs[0].signature = vec![0u8; 64]; // bad sig
        let err = validate_signatures(&[cb, tx]).unwrap_err();
        assert_eq!(err, ValidationError::InvalidSignature { tx_index: 1 });
    }

    // ── Step 7: Double-spend ───────────────────────────────────────

    #[test]
    fn validate_no_double_spend_ok() {
        let cb = make_coinbase(1);
        let tx1 = make_signed_tx([0xAA; 32]);
        let tx2 = make_signed_tx([0xBB; 32]);
        assert!(validate_no_double_spend(&[cb, tx1, tx2]).is_ok());
    }

    #[test]
    fn validate_double_spend_detected() {
        let cb = make_coinbase(1);
        let tx1 = make_signed_tx([0xAA; 32]);
        let mut tx2 = make_signed_tx([0xAA; 32]);
        // Same outpoint as tx1
        tx2.inputs[0].prev_tx_hash = tx1.inputs[0].prev_tx_hash;
        tx2.inputs[0].output_index = tx1.inputs[0].output_index;
        let err = validate_no_double_spend(&[cb, tx1, tx2]).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::DoubleSpend { tx_index: 2, .. }
        ));
    }

    // ── Step 8: Coinbase maturity ──────────────────────────────────

    #[test]
    fn validate_coinbase_maturity_mature_ok() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xCC; 32]);
        let lookup = |_hash: &[u8; 32], _idx: u32| -> Option<UtxoInfo> {
            Some(UtxoInfo {
                amount: 5_000_000,
                address: "test".into(),
                created_height: 0,
                is_coinbase: true,
            })
        };
        // Current height 200, coinbase at 0 → age 200 >= 100, OK
        assert!(validate_coinbase_maturity(&[cb, tx], 200, &lookup).is_ok());
    }

    #[test]
    fn validate_coinbase_maturity_immature_rejected() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xCC; 32]);
        let lookup = |_hash: &[u8; 32], _idx: u32| -> Option<UtxoInfo> {
            Some(UtxoInfo {
                amount: 5_000_000,
                address: "test".into(),
                created_height: 50,
                is_coinbase: true,
            })
        };
        // Current height 100, coinbase at 50 → age 50 < 100
        let err = validate_coinbase_maturity(&[cb, tx], 100, &lookup).unwrap_err();
        assert!(matches!(
            err,
            ValidationError::ImmatureCoinbase { age: 50, .. }
        ));
    }

    // ── Step 10: Subsidy ───────────────────────────────────────────

    #[test]
    fn validate_subsidy_ok() {
        let cb = make_coinbase(1);
        assert!(validate_subsidy(&cb, 1).is_ok());
    }

    #[test]
    fn validate_subsidy_exceeded() {
        let mut cb = make_coinbase(1);
        cb.outputs[0].amount += 1; // over-reward
        let err = validate_subsidy(&cb, 1).unwrap_err();
        assert!(matches!(err, ValidationError::SubsidyExceeded { .. }));
    }

    #[test]
    fn validate_subsidy_coinbase_with_inputs_rejected() {
        let (_, vk) = generate_keypair();
        let tx = Transaction {
            id: [0u8; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: [0xFF; 32],
                output_index: 0,
                signature: vec![0u8; 64],
                public_key: vk.as_bytes().to_vec(),
            }],
            outputs: vec![TxOutput {
                amount: 1,
                address: "test".into(),
                memo: None,
            }],
            fee: 0,
            timestamp: 0,
        };
        let err = validate_subsidy(&tx, 1).unwrap_err();
        assert_eq!(err, ValidationError::CoinbaseHasInputs);
    }

    // ── Full pipeline ──────────────────────────────────────────────

    #[test]
    fn full_validation_valid_block() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xDD; 32]);
        let txs = vec![cb, tx];
        let tx_hashes: Vec<[u8; 32]> = txs.iter().map(|t| t.id).collect();
        let root = merkle_root(&tx_hashes);

        let ctx = ValidationContext {
            height: 1,
            expected_difficulty: 1_000,
            median_time_past: 1_699_999_900,
            current_time: 1_700_000_000,
        };

        let sizes = vec![200, 300]; // estimated sizes
        let lookup = |_: &[u8; 32], _: u32| -> Option<UtxoInfo> { None };

        assert!(validate_block(&txs, 1_700_000_000, 1000, &root, &ctx, &sizes, &lookup).is_ok());
    }

    #[test]
    fn full_validation_rejects_future_timestamp() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xDE; 32]);
        let txs = vec![cb, tx];
        let tx_hashes: Vec<[u8; 32]> = txs.iter().map(|t| t.id).collect();
        let root = merkle_root(&tx_hashes);

        let ctx = ValidationContext {
            height: 1,
            expected_difficulty: 1_000,
            median_time_past: 1_699_999_900,
            current_time: 1_700_000_000,
        };

        let sizes = vec![200, 300];
        let lookup = |_: &[u8; 32], _: u32| -> Option<UtxoInfo> { None };

        let result = validate_block(
            &txs,
            1_700_000_000 + MAX_TIMESTAMP_DRIFT + 1,
            1000,
            &root,
            &ctx,
            &sizes,
            &lookup,
        );
        assert!(matches!(
            result,
            Err(ValidationError::TimestampTooFarFuture { .. })
        ));
    }

    #[test]
    fn validate_premine_lock_rejects_early_spend() {
        let dao_addr = genesis::PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "dao_treasury")
            .unwrap()
            .address;

        let (_, pub_key) = generate_keypair();
        let tx = Transaction {
            id: [0xEE; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: [0xAA; 32],
                output_index: 0,
                signature: vec![0u8; 64],
                public_key: pub_key.as_bytes().to_vec(),
            }],
            outputs: vec![TxOutput {
                amount: 1000,
                address: derive_address(pub_key.as_bytes()),
                memo: None,
            }],
            fee: 1_000,
            timestamp: 1_700_000_000,
        };

        // UTXO belongs to locked DAO treasury address
        let lookup = |_: &[u8; 32], _: u32| -> Option<UtxoInfo> {
            Some(UtxoInfo {
                amount: 10_000,
                address: dao_addr.to_string(),
                created_height: 0,
                is_coinbase: false,
            })
        };

        // height 100 < 144_000 — should be rejected (admin unlocked =>
        // isolates the time-lock behaviour being tested here).
        let result = validate_premine_locks(&[make_coinbase(100), tx.clone()], 100, &lookup, &|_| true);
        assert!(matches!(result, Err(ValidationError::LockedPremine { .. })));

        // height 144_000 — should be allowed (time-lock satisfied AND
        // admin-unlocked for the test).
        let result2 =
            validate_premine_locks(&[make_coinbase(144_000), tx.clone()], 144_000, &lookup, &|_| true);
        assert!(result2.is_ok());

        // Regression for PR #20 review: when the caller passes a slice
        // whose index 0 is **not** a coinbase (as `block.utxo_transactions`
        // can be), the old `.skip(1)` would silently bypass the check.
        // After the fix, `is_coinbase()` is the only skip predicate, so a
        // locked-premine spend at index 0 must still be flagged.
        let result_no_coinbase = validate_premine_locks(&[tx], 100, &lookup, &|_| true);
        assert!(matches!(
            result_no_coinbase,
            Err(ValidationError::LockedPremine { tx_index: 0, .. })
        ));
    }

    #[test]
    fn validate_premine_lock_allows_non_dao_addresses() {
        let infra_addr = genesis::PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "infrastructure")
            .unwrap()
            .address;

        let (_, pub_key) = generate_keypair();
        let tx = Transaction {
            id: [0xFF; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: [0xBB; 32],
                output_index: 0,
                signature: vec![0u8; 64],
                public_key: pub_key.as_bytes().to_vec(),
            }],
            outputs: vec![TxOutput {
                amount: 1000,
                address: derive_address(pub_key.as_bytes()),
                memo: None,
            }],
            fee: 1_000,
            timestamp: 1_700_000_000,
        };

        // UTXO belongs to infrastructure (unlocked immediately)
        let lookup = |_: &[u8; 32], _: u32| -> Option<UtxoInfo> {
            Some(UtxoInfo {
                amount: 10_000,
                address: infra_addr.to_string(),
                created_height: 0,
                is_coinbase: false,
            })
        };

        // height 0 — should be allowed for non-DAO (unlock_height = 0).
        // Admin-unlocked for the test to isolate the time-lock bypass.
        let result = validate_premine_locks(&[make_coinbase(0), tx], 0, &lookup, &|_| true);
        assert!(result.is_ok());
    }

    // ── Step 7b / 12: Inputs exist + Conservation of value (F1) ─────

    fn no_bridge_unlocks(_tx: &Transaction) -> bool {
        false
    }

    fn lookup_with_amount(amount: u64) -> impl Fn(&[u8; 32], u32) -> Option<UtxoInfo> {
        move |_h: &[u8; 32], _i: u32| {
            Some(UtxoInfo {
                amount,
                address: "zion1producer".to_string(),
                created_height: 0,
                is_coinbase: false,
            })
        }
    }

    fn empty_lookup(_h: &[u8; 32], _i: u32) -> Option<UtxoInfo> {
        None
    }

    /// A non-coinbase, non-bridge-unlock UTXO transaction whose outputs+fee
    /// exceed the value of its referenced inputs MUST be rejected — this is
    /// the "no money printing" rule. Reproduces the dead-validation gap that
    /// allowed a peer to inflate supply through a forged block.
    #[test]
    fn validate_value_conservation_rejects_inflation() {
        let cb = make_coinbase(1);
        // make_signed_tx outputs 1_000_000 + fee 2_000 = needs 1_002_000 inputs.
        // Lookup returns 500_000 → must fail.
        let tx = make_signed_tx([0xAA; 32]);
        let txs = vec![cb, tx];
        let lookup = lookup_with_amount(500_000);

        let err = validate_value_conservation(&txs, &lookup, &no_bridge_unlocks).unwrap_err();
        assert!(
            matches!(err, ValidationError::ValueNotConserved { tx_index: 1, .. }),
            "expected ValueNotConserved, got {err:?}",
        );
    }

    /// A non-coinbase, non-bridge-unlock UTXO transaction whose inputs cover
    /// exactly the outputs+fee MUST pass conservation.
    #[test]
    fn validate_value_conservation_accepts_balanced_spend() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xBB; 32]); // outputs 1_000_000 + fee 2_000
        let txs = vec![cb, tx];
        // Inputs sum to exactly 1_002_000 → ok.
        let lookup = lookup_with_amount(1_002_000);

        validate_value_conservation(&txs, &lookup, &no_bridge_unlocks)
            .expect("balanced UTXO transfer must validate");
    }

    /// Excess input value (e.g. when miner takes the change as fee) is allowed
    /// because conservation only forbids minting (∑inputs < ∑outputs + fee),
    /// not over-payment. This mirrors how Bitcoin handles "burned" change.
    #[test]
    fn validate_value_conservation_accepts_overpayment() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xCC; 32]);
        let txs = vec![cb, tx];
        // Lookup returns 5_000_000 (>> outputs+fee = 1_002_000) → ok.
        let lookup = lookup_with_amount(5_000_000);

        validate_value_conservation(&txs, &lookup, &no_bridge_unlocks)
            .expect("overpaying inputs must validate");
    }

    /// Coinbase transactions have no inputs and must be skipped by the
    /// conservation check; otherwise empty inputs would always trigger the
    /// minting error and prevent any block from validating.
    #[test]
    fn validate_value_conservation_skips_coinbase() {
        let cb = make_coinbase(1);
        let lookup = empty_lookup;

        validate_value_conservation(&[cb], &lookup, &no_bridge_unlocks)
            .expect("coinbase must be skipped by value conservation");
    }

    /// Bridge-unlock transactions have their conservation enforced by
    /// `validate_bridge_unlock_transaction_shape` in lib.rs (which asserts
    /// `total_input == outputs + fee`). The general validator must skip them
    /// when the predicate marks them, otherwise the keyless inputs would fail
    /// the input-existence step.
    #[test]
    fn validate_value_conservation_skips_bridge_unlocks() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xDD; 32]);
        let txs = vec![cb, tx];
        let lookup = empty_lookup; // would fail without skip
        let always_bridge = |_tx: &Transaction| true;

        validate_value_conservation(&txs, &lookup, &always_bridge)
            .expect("bridge-unlock predicate must skip the conservation check");
    }

    /// A transaction whose input refers to a UTXO that does not exist (or has
    /// already been spent) MUST be rejected with `InputNotFound`. Without this
    /// check a peer could submit a block whose UTXO transactions resolved to
    /// nothing and still get accepted.
    #[test]
    fn validate_inputs_exist_rejects_missing_utxo() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xEE; 32]);
        let txs = vec![cb, tx];

        let err = validate_inputs_exist(&txs, &empty_lookup, &no_bridge_unlocks).unwrap_err();
        assert!(
            matches!(err, ValidationError::InputNotFound { tx_index: 1, .. }),
            "expected InputNotFound, got {err:?}",
        );
    }

    /// All-resolved inputs must validate.
    #[test]
    fn validate_inputs_exist_accepts_resolved_utxo() {
        let cb = make_coinbase(1);
        let tx = make_signed_tx([0xFF; 32]);
        let txs = vec![cb, tx];
        let lookup = lookup_with_amount(1_500_000);

        validate_inputs_exist(&txs, &lookup, &no_bridge_unlocks)
            .expect("resolvable inputs must validate");
    }

    /// Sentinel: u64 overflow on input sums must be reported as
    /// `ValueOverflow`, not silently wrapped. Defends against malformed peer
    /// blocks crafted to bypass conservation via wrap-around arithmetic.
    #[test]
    fn validate_value_conservation_detects_overflow() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());
        let mut tx = Transaction {
            id: [0u8; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![
                TxInput {
                    prev_tx_hash: [0xAA; 32],
                    output_index: 0,
                    signature: vec![],
                    public_key: vk.as_bytes().to_vec(),
                },
                TxInput {
                    prev_tx_hash: [0xBB; 32],
                    output_index: 0,
                    signature: vec![],
                    public_key: vk.as_bytes().to_vec(),
                },
            ],
            outputs: vec![TxOutput {
                amount: 1,
                address: addr,
                memo: None,
            }],
            fee: 0,
            timestamp: 1_700_000_000,
        };
        tx.finalize_id();
        let sig = sign(&sk, &tx.id);
        tx.inputs[0].signature = sig.to_vec();
        tx.inputs[1].signature = sig.to_vec();

        let cb = make_coinbase(1);
        let txs = vec![cb, tx];
        // Each input claims max u64 → sum overflows.
        let lookup = lookup_with_amount(u64::MAX);

        let err = validate_value_conservation(&txs, &lookup, &no_bridge_unlocks).unwrap_err();
        assert_eq!(err, ValidationError::ValueOverflow { tx_index: 1 });
    }

    /// Exploit regression test for the `total_output()` wrap-around bypass
    /// flagged in PR #20 review. With release-mode `Iterator::sum()` an
    /// attacker could craft outputs whose unchecked sum wrapped to a value
    /// less than or equal to inputs, passing the conservation check while
    /// printing roughly 2^64 flowers. We must reject this with
    /// `ValueOverflow`, not `ValueNotConserved` (and certainly not Ok).
    #[test]
    fn validate_value_conservation_rejects_output_wraparound_attack() {
        let (sk, vk) = generate_keypair();
        let addr = derive_address(vk.as_bytes());

        // Two outputs, each ≈ 2^63, whose wrapping sum is ~ 2 * (2^63) = 0
        // mod 2^64. With unchecked Iterator::sum() the wrapped sum would
        // satisfy `outputs_sum ≤ inputs_sum`, bypassing conservation.
        let half_max = (u64::MAX / 2) + 1; // 0x8000_0000_0000_0000
        let mut tx = Transaction {
            id: [0u8; 32],
            version: TX_HASH_V2_VERSION,
            inputs: vec![TxInput {
                prev_tx_hash: [0xAA; 32],
                output_index: 0,
                signature: vec![],
                public_key: vk.as_bytes().to_vec(),
            }],
            outputs: vec![
                TxOutput {
                    amount: half_max,
                    address: addr.clone(),
                    memo: None,
                },
                TxOutput {
                    amount: half_max,
                    address: addr,
                    memo: None,
                },
            ],
            fee: 0,
            timestamp: 1_700_000_000,
        };
        tx.finalize_id();
        tx.inputs[0].signature = sign(&sk, &tx.id).to_vec();

        let cb = make_coinbase(1);
        let txs = vec![cb, tx];
        // Inputs are tiny (1 ZION); without overflow checks the wrapped
        // outputs_sum would equal 0, falsely passing conservation.
        let lookup = lookup_with_amount(1_000_000);

        let err = validate_value_conservation(&txs, &lookup, &no_bridge_unlocks).unwrap_err();
        assert_eq!(err, ValidationError::ValueOverflow { tx_index: 1 });
    }
}
