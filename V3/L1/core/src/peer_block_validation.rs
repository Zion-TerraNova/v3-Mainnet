//! Full validation for [`AcceptedBlock`] received from peers (height ≥ 1).
//!
//! Genesis (`height == 0`) is validated inline on [`crate::ChainState`] before calling here.

use std::collections::{HashMap, HashSet};

use zion_cosmic_harmony::{
    cosmic_harmony_ekam_deeksha, tx_hash_v2_active, TX_HASH_V2_ACTIVATION_HEIGHT,
};

use crate::difficulty;
use crate::emission;
use crate::fee;
use crate::launch;
use crate::tx;
use crate::validation;
use crate::{
    body_hash_hex, bridge_unlock_replay_key_from_transaction, hex, is_valid_account_id, now_secs,
    parse_fixed_hex, validate_bridge_unlock_transaction_shape_with_utxos, AcceptedBlock,
    BlockCandidate, MiningHeader, SpendableUtxo, HEADER_SIZE,
};

#[allow(dead_code)]
pub(crate) fn validate_accepted_peer_block(
    accepted_blocks: &[AcceptedBlock],
    utxo_snapshot: &HashMap<(String, u32), SpendableUtxo>,
    mut seen_bridge_unlock_replay_keys: HashSet<String>,
    block: &AcceptedBlock,
) -> Result<(), String> {
    debug_assert!(block.height > 0);

    // ── Checkpoint verification ────────────────────────────────────
    launch::verify_checkpoint(block.height, &block.hash_hex)?;

    // ── PoW verification (when header is available) ────────────────
    let block_hash = parse_fixed_hex::<32>(&block.hash_hex, "peer block hash")?;
    if !block.header_hex.is_empty() {
        let header_bytes = parse_fixed_hex::<HEADER_SIZE>(&block.header_hex, "peer block header")?;
        let header = MiningHeader::from_bytes(header_bytes);

        // Header fields must be consistent with block metadata
        if header.timestamp != block.timestamp {
            return Err("peer block header timestamp does not match block timestamp".to_string());
        }
        let expected_target = difficulty::difficulty_to_target(block.difficulty);
        let expected_bits = difficulty::target_to_compact(&expected_target);
        if header.difficulty_bits != expected_bits {
            return Err(format!(
                "peer block header difficulty_bits {} does not match expected {}",
                header.difficulty_bits, expected_bits
            ));
        }

        // Reject inconsistent parent metadata before doing expensive PoW work.
        if !block.previous_hash_hex.is_empty() {
            let header_prev = hex(&header.previous_hash);
            if block.previous_hash_hex != header_prev {
                return Err(
                    "peer block previous_hash_hex does not match header previous_hash".to_string(),
                );
            }
        }

        // Verify PoW: recompute hash from header + nonce using the block's algorithm
        let candidate = BlockCandidate {
            header,
            nonce: block.nonce,
            height: block.height,
        };
        let algorithm = if block.algorithm.is_empty() {
            "deeksha_lite_v1"
        } else {
            &block.algorithm
        };
        let computed_hash = candidate.hash_with_algorithm(algorithm);
        if computed_hash != block_hash {
            return Err(
                "peer block hash does not match PoW computation from header and nonce".to_string(),
            );
        }

        // Verify hash meets difficulty target
        let target = difficulty::difficulty_to_target(block.difficulty);
        if !target.allows(&computed_hash) {
            return Err("peer block PoW hash does not meet difficulty target".to_string());
        }
    }

    // ── Timestamp sanity ───────────────────────────────────────────
    let current_time = now_secs();
    let median_time_past = if accepted_blocks.is_empty() {
        0
    } else {
        let start = accepted_blocks.len().saturating_sub(11);
        let mut timestamps: Vec<u64> = accepted_blocks[start..]
            .iter()
            .map(|b| b.timestamp)
            .collect();
        timestamps.sort_unstable();
        timestamps[timestamps.len() / 2]
    };
    validation::validate_timestamp(block.timestamp, median_time_past, current_time)
        .map_err(|e| format!("peer block timestamp invalid: {e}"))?;

    // ── Transaction structure ──────────────────────────────────────
    if block.transaction_ids.len() != block.transactions.len() {
        return Err("peer block transaction ids do not match block body length".to_string());
    }
    let expected_ids = block
        .transactions
        .iter()
        .map(|transaction| transaction.tx_id.clone())
        .collect::<Vec<_>>();
    if expected_ids != block.transaction_ids {
        return Err("peer block transaction ids do not match serialized transactions".to_string());
    }
    let mut seen_tx_ids = HashSet::new();
    let mut seen_sender_nonces = HashSet::new();
    let mut coinbase_count = 0usize;
    let mut total_coinbase_zion = 0u128;
    let has_fee_addresses = !block.humanitarian_address.is_empty()
        || !block.issobella_address.is_empty()
        || !block.pool_fee_address.is_empty();
    let has_all_fee_addresses = !block.humanitarian_address.is_empty()
        && !block.issobella_address.is_empty()
        && !block.pool_fee_address.is_empty();
    if has_fee_addresses && !has_all_fee_addresses {
        return Err("peer block fee split metadata must provide all fee addresses".to_string());
    }
    let (
        expected_miner_reward,
        expected_humanitarian_reward,
        expected_issobella_reward,
        expected_pool_fee_reward,
    ) = emission::fee_split(block.subsidy_zion);
    let total_fees_zion = block
        .transactions
        .iter()
        .enumerate()
        .map(|(index, transaction)| {
            if !seen_tx_ids.insert(transaction.tx_id.clone()) {
                return Err(format!(
                    "peer block contains duplicate transaction id {}",
                    transaction.tx_id
                ));
            }
            if transaction.from == "coinbase" {
                coinbase_count = coinbase_count.saturating_add(1);
                total_coinbase_zion = total_coinbase_zion.saturating_add(transaction.amount_zion);
                if transaction.tx_id.len() != 64
                    || !transaction.tx_id.chars().all(|ch| ch.is_ascii_hexdigit())
                {
                    return Err("peer block coinbase transaction id must be exactly 64 hex chars"
                        .to_string());
                }
                if transaction.to.trim().is_empty() {
                    return Err("peer block coinbase recipient must not be empty".to_string());
                }
                if !is_valid_account_id(&transaction.to) {
                    return Err(
                        "peer block coinbase recipient must use a 3-64 ascii wallet id"
                            .to_string(),
                    );
                }
                if index != coinbase_count.saturating_sub(1) {
                    return Err(
                        "peer block coinbase transactions must be contiguous at the start"
                            .to_string(),
                    );
                }
                if transaction.fee_zion != 0 {
                    return Err("peer block coinbase transaction must have zero fee".to_string());
                }
                if transaction.nonce != block.height {
                    return Err(format!(
                        "peer block coinbase nonce {} does not match block height {}",
                        transaction.nonce, block.height
                    ));
                }
                if block.miner_address.is_empty() {
                    return Err(
                        "peer block coinbase transaction requires miner_address metadata".to_string(),
                    );
                }
                let (expected_to, expected_amount, expected_label) = if has_all_fee_addresses {
                    match index {
                        0 => (
                            block.miner_address.as_str(),
                            expected_miner_reward,
                            format!("coinbase:{}:{}", block.height, block.miner_address),
                        ),
                        1 => (
                            block.humanitarian_address.as_str(),
                            expected_humanitarian_reward,
                            format!(
                                "coinbase_humanitarian:{}:{}",
                                block.height, block.humanitarian_address
                            ),
                        ),
                        2 => (
                            block.issobella_address.as_str(),
                            expected_issobella_reward,
                            format!(
                                "coinbase_issobella:{}:{}",
                                block.height, block.issobella_address
                            ),
                        ),
                        3 => (
                            block.pool_fee_address.as_str(),
                            expected_pool_fee_reward,
                            format!(
                                "coinbase_pool_fee:{}:{}",
                                block.height, block.pool_fee_address
                            ),
                        ),
                        _ => {
                            return Err(
                                "peer block contains too many split coinbase transactions"
                                    .to_string(),
                            )
                        }
                    }
                } else {
                    (
                        block.miner_address.as_str(),
                        block.subsidy_zion,
                        format!("coinbase:{}:{}", block.height, block.miner_address),
                    )
                };
                if transaction.to != expected_to {
                    return Err(
                        "peer block coinbase recipient does not match expected payout address"
                            .to_string(),
                    );
                }
                if transaction.amount_zion != expected_amount as u128 {
                    return Err(format!(
                        "peer block coinbase amount {} does not match expected {}",
                        transaction.amount_zion, expected_amount
                    ));
                }
                let expected_coinbase_hash =
                    cosmic_harmony_ekam_deeksha(expected_label.as_bytes(), block.height);
                let expected_coinbase_id = hex(&expected_coinbase_hash.data);
                if transaction.tx_id != expected_coinbase_id {
                    return Err(
                        "peer block coinbase tx_id is not deterministic for the expected payout slot"
                            .to_string(),
                    );
                }
            } else {
                transaction.validate()?;
                if !seen_sender_nonces.insert((transaction.from.clone(), transaction.nonce)) {
                    return Err(format!(
                        "peer block reuses sender nonce {} for {}",
                        transaction.nonce, transaction.from
                    ));
                }
            }
            Ok(transaction.fee_zion)
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .sum::<u64>();
    if coinbase_count > 4 {
        return Err("peer block contains more than four coinbase transactions".to_string());
    }
    if !block.miner_address.is_empty() && coinbase_count == 0 {
        return Err(
            "peer block miner_address is set but coinbase transaction is missing".to_string(),
        );
    }
    if has_all_fee_addresses && coinbase_count != 4 {
        return Err(
            "peer block with fee split metadata must contain four coinbase transactions"
                .to_string(),
        );
    }
    if !has_all_fee_addresses && coinbase_count > 1 {
        return Err(
            "peer block without fee split metadata must contain at most one coinbase transaction"
                .to_string(),
        );
    }
    if total_coinbase_zion != 0 && total_coinbase_zion != block.subsidy_zion as u128 {
        return Err(format!(
            "peer block coinbase total {} does not match subsidy {}",
            total_coinbase_zion, block.subsidy_zion
        ));
    }
    if total_fees_zion != block.total_fees_zion {
        return Err("peer block fee total does not match serialized transactions".to_string());
    }
    if block.body_hash_hex != body_hash_hex(&block.transactions) {
        return Err("peer block body hash does not match serialized transactions".to_string());
    }
    let expected_block_miner_reward = if has_all_fee_addresses && coinbase_count == 4 {
        expected_miner_reward
    } else {
        block.subsidy_zion
    };
    if block.miner_reward_zion != expected_block_miner_reward {
        return Err(format!(
            "peer block miner reward {} does not match expected {}",
            block.miner_reward_zion, expected_block_miner_reward
        ));
    }
    let expected_subsidy = if crate::migration::is_post_migration(block.height) {
        // Post-3.0.3: subsidy in new 6-decimal flowers
        emission::block_subsidy(block.height)
    } else {
        // Pre-3.0.3: subsidy in legacy 12-decimal flowers
        // block_subsidy returns new-scale; multiply back to legacy for comparison
        emission::block_subsidy(block.height) * crate::migration::MIGRATION_DIVISOR
    };
    if block.subsidy_zion != expected_subsidy {
        return Err(format!(
            "peer block subsidy {} does not match emission schedule {} at height {} ({})",
            block.subsidy_zion,
            expected_subsidy,
            block.height,
            if crate::migration::is_post_migration(block.height) {
                "post-migration"
            } else {
                "pre-migration (legacy scale)"
            }
        ));
    }
    // Validate difficulty against LWMA
    let expected_difficulty = if accepted_blocks.is_empty() {
        difficulty::GENESIS_DIFFICULTY
    } else {
        let start = accepted_blocks
            .len()
            .saturating_sub(difficulty::LWMA_WINDOW + 1);
        let window: Vec<difficulty::BlockInfo> = accepted_blocks[start..]
            .iter()
            .map(|b| difficulty::BlockInfo {
                timestamp: b.timestamp,
                difficulty: b.difficulty,
            })
            .collect();
        difficulty::lwma_next_difficulty(&window)
    };
    if block.difficulty != expected_difficulty {
        return Err(format!(
            "peer block difficulty {} does not match expected {} at height {}",
            block.difficulty, expected_difficulty, block.height
        ));
    }
    // ── UTXO transaction structure ─────────────────────────────────
    let utxo_expected_ids: Vec<String> = block
        .utxo_transactions
        .iter()
        .map(|utxo_tx| hex(&utxo_tx.id))
        .collect();
    if utxo_expected_ids != block.utxo_transaction_ids {
        return Err(
            "peer block UTXO transaction ids do not match serialized UTXO transactions".to_string(),
        );
    }
    let mut seen_utxo_inputs: HashSet<([u8; 32], u32)> = HashSet::new();
    // ── TX hash v2 hard fork (audit §3.2) ─────────────────────────────
    //
    // With `TX_HASH_V2_ACTIVATION_HEIGHT` at genesis (`0`), every UTXO
    // transaction in blocks at height ≥ 1 MUST carry `version >= TX_HASH_V2_VERSION`.
    // Coinbase / bridge-unlock are included — templates & RPC builders
    // must emit v2 alongside this gate (`build_template`, `insert_utxo_*`,
    // `build_bridge_unlock_transaction`).
    if tx_hash_v2_active(block.height) {
        for utxo_tx in &block.utxo_transactions {
            if utxo_tx.version < tx::TX_HASH_V2_VERSION {
                return Err(format!(
                    "peer block height {} disallows UTXO tx.version {} (< {}) — TX_HASH_V2 active from height {}",
                    block.height,
                    utxo_tx.version,
                    tx::TX_HASH_V2_VERSION,
                    TX_HASH_V2_ACTIVATION_HEIGHT,
                ));
            }
        }
    }
    for utxo_tx in &block.utxo_transactions {
        if utxo_tx.id != utxo_tx.calculate_hash() {
            return Err(format!(
                "peer block UTXO transaction {} has invalid id",
                hex(&utxo_tx.id)
            ));
        }
        match validate_bridge_unlock_transaction_shape_with_utxos(utxo_tx, utxo_snapshot)? {
            Some(replay_key) => {
                if !seen_bridge_unlock_replay_keys.insert(replay_key.clone()) {
                    return Err(format!(
                        "peer block bridge unlock replay key already used: {}",
                        replay_key,
                    ));
                }
            }
            None => {
                if !utxo_tx.verify_signatures() {
                    return Err(format!(
                        "peer block UTXO transaction {} has invalid signatures",
                        hex(&utxo_tx.id)
                    ));
                }
            }
        }
        let utxo_id_hex = hex(&utxo_tx.id);
        if !seen_tx_ids.insert(utxo_id_hex) {
            return Err(format!(
                "peer block contains duplicate UTXO transaction id {}",
                hex(&utxo_tx.id)
            ));
        }
        for input in &utxo_tx.inputs {
            if !seen_utxo_inputs.insert((input.prev_tx_hash, input.output_index)) {
                return Err(format!(
                    "peer block contains double-spend of UTXO input {}:{}",
                    hex(&input.prev_tx_hash),
                    input.output_index,
                ));
            }
        }
    }

    // ── UTXO conservation-of-value pipeline (F1) ───────────────────
    //
    // Before this hook the peer-block path verified structure,
    // signatures, and within-block double-spends, but never confirmed
    // that the inputs each UTXO transaction references actually exist
    // in the chain's UTXO set, that the spend conserved value
    // (∑inputs ≥ ∑outputs + fee), that every transaction met the fee
    // floor, or that DAO Treasury addresses were past their unlock
    // height. The mempool insertion path covered the existence check
    // for new transactions but never re-checked anything for blocks
    // arriving from peers, which meant a malicious peer could mint
    // coins by submitting a forged block whose UTXO transactions
    // resolved to nothing or printed value out of thin air.
    //
    // Bridge-unlock transactions are kept on their dedicated
    // `validate_bridge_unlock_transaction_shape` path (they spend the
    // keyless bridge vault, see line ~90), so we skip them here.
    if !block.utxo_transactions.is_empty() {
        let utxo_lookup = |hash: &[u8; 32], idx: u32| -> Option<validation::UtxoInfo> {
            utxo_snapshot
                .get(&(hex(hash), idx))
                .map(|spendable| validation::UtxoInfo {
                    amount: spendable.amount,
                    address: spendable.address.clone(),
                    created_height: spendable.height,
                    // Coinbase maturity enforcement is deferred to a
                    // follow-up PR (requires tracking is_coinbase in
                    // SpendableUtxo). For now we conservatively report
                    // false; consumers that need maturity must opt in
                    // explicitly.
                    is_coinbase: false,
                })
        };
        let is_bridge_unlock = |transaction: &tx::Transaction| -> bool {
            bridge_unlock_replay_key_from_transaction(transaction).is_some()
        };

        validation::validate_inputs_exist(
            &block.utxo_transactions,
            &utxo_lookup,
            &is_bridge_unlock,
        )
        .map_err(|err| format!("peer block UTXO input check failed: {err}"))?;

        validation::validate_value_conservation(
            &block.utxo_transactions,
            &utxo_lookup,
            &is_bridge_unlock,
        )
        .map_err(|err| format!("peer block UTXO value conservation failed: {err}"))?;

        // DAO Treasury timelock + admin-lock — premine outputs cannot be spent
        // before their `unlock_height` AND must be admin-unlocked (3-of-3 + DAO).
        // Bridge-unlock spends only the keyless vault, so this still flags
        // general transactions that try to drain locked treasury balances.
        // Default: no admin unlocks (all admin-locked premine is frozen).
        validation::validate_premine_locks(&block.utxo_transactions, block.height, &utxo_lookup, &|_| false)
            .map_err(|err| format!("peer block premine lock violation: {err}"))?;

        // UTXO fee floor for non-coinbase, non-bridge-unlock
        // transactions. Bridge-unlock transactions already validate
        // their fee inside `validate_bridge_unlock_transaction_shape`,
        // and coinbase has no fee to validate.
        for (tx_i, utxo_tx) in block.utxo_transactions.iter().enumerate() {
            if utxo_tx.is_coinbase() {
                continue;
            }
            if is_bridge_unlock(utxo_tx) {
                continue;
            }
            let tx_size = fee::estimate_tx_size(utxo_tx.inputs.len(), utxo_tx.outputs.len());
            fee::validate_fee(utxo_tx.fee, tx_size).map_err(|err| {
                format!(
                    "peer block UTXO transaction {} fee invalid (tx_index {tx_i}): {err}",
                    hex(&utxo_tx.id)
                )
            })?;
        }
    }

    Ok(())
}
