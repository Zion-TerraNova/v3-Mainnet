//! Bridge Unlock Validation — L1-enforced multisig verification for cross-chain
//! wZION ↔ Base Mainnet bridge unlock transactions.
//!
//! Every bridge unlock transaction must carry at least
//! [`BRIDGE_MIN_VALIDATOR_PROOFS`] distinct, allow-listed secp256k1 ECDSA
//! signatures over a canonical operation message reconstructed from the
//! transaction itself.

use k256::ecdsa::{signature::Verifier as _, Signature as EcdsaSignature, VerifyingKey};
use std::collections::HashSet;

const BRIDGE_UNLOCK_MEMO_PREFIX: &str = "BRIDGE_UNLOCK:";
/// Separator between the replay-key body and the multisig proofs payload in a
/// bridge unlock memo.
const BRIDGE_PROOFS_DELIMITER: char = '|';
/// Prefix introducing the multisig proofs payload after the replay-key body.
const BRIDGE_PROOFS_PREFIX: &str = "PROOFS=";
/// Hard ceiling on the number of validator proofs accepted in a single
/// bridge unlock memo. Prevents pathological tx sizes / quadratic
/// verification cost.
pub(crate) const BRIDGE_MAX_VALIDATOR_PROOFS: usize = 16;
/// Hard ceiling on bridge unlock memo size (bytes). The replay-key body plus
/// up to [`BRIDGE_MAX_VALIDATOR_PROOFS`] proofs comfortably fit well under
/// this limit; anything larger is rejected as malformed.
pub(crate) const BRIDGE_MAX_MEMO_LEN: usize = 8192;

/// Required minimum number of distinct verified validator proofs that a
/// bridge unlock transaction must carry. This is the protocol-level floor
/// enforced by every node (peer-block import, mempool ingress, RPC entry).
/// The runtime threshold may be raised via
/// [`required_bridge_validator_threshold`] but never lowered below this.
pub const BRIDGE_MIN_VALIDATOR_PROOFS: usize = 3;

/// A single validator multisig proof carried by a bridge unlock transaction.
///
/// Constructed via [`BridgeValidatorProof::new`] (which enforces the canonical
/// hex / ID shape) and consumed by the protocol-level verifier
/// [`verify_bridge_proofs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeValidatorProof {
    /// Free-form identifier (alphanumeric, underscore, dash). Used only for
    /// diagnostics and de-duplication of proofs that share an identity.
    pub(crate) validator_id: String,
    /// secp256k1 SEC1-encoded compressed public key, lowercase hex (66 chars).
    pub(crate) pubkey_hex: String,
    /// 64-byte ECDSA signature over `bridge_operation_message`, lowercase hex
    /// (128 chars).
    pub(crate) signature_hex: String,
}

impl BridgeValidatorProof {
    /// Construct a proof from raw fields, normalising hex casing and
    /// rejecting malformed inputs early. Validation here mirrors the
    /// stricter parser used during peer-block import so the RPC surface
    /// rejects bad shapes before they ever hit the chain.
    pub fn new(
        validator_id: impl Into<String>,
        pubkey_hex: impl Into<String>,
        signature_hex: impl Into<String>,
    ) -> Result<Self, String> {
        let validator_id = validator_id.into();
        let pubkey_hex = pubkey_hex
            .into()
            .trim_start_matches("0x")
            .to_ascii_lowercase();
        let signature_hex = signature_hex
            .into()
            .trim_start_matches("0x")
            .to_ascii_lowercase();
        if validator_id.is_empty() {
            return Err("bridge validator proof has empty validator_id".to_string());
        }
        if !validator_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || validator_id.len() > 64
        {
            return Err(
                "bridge validator proof validator_id has illegal characters or is too long"
                    .to_string(),
            );
        }
        if pubkey_hex.len() != 66 || !pubkey_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(
                "bridge validator proof pubkey must be 66 hex chars (compressed SEC1)".to_string(),
            );
        }
        if signature_hex.len() != 128 || !signature_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("bridge validator proof signature must be 128 hex chars".to_string());
        }
        Ok(Self {
            validator_id,
            pubkey_hex,
            signature_hex,
        })
    }
}

pub(crate) fn bridge_unlock_replay_key(
    source_chain: &str,
    burn_id: &str,
    evm_tx_hash: &str,
) -> String {
    format!("{source_chain}:{burn_id}:{evm_tx_hash}")
}

pub(crate) fn bridge_unlock_memo(source_chain: &str, burn_id: &str, evm_tx_hash: &str) -> String {
    format!(
        "{BRIDGE_UNLOCK_MEMO_PREFIX}{}",
        bridge_unlock_replay_key(source_chain, burn_id, evm_tx_hash)
    )
}

/// Build the canonical bridge unlock memo carrying multisig proofs.
///
/// The format is:
/// `BRIDGE_UNLOCK:<source>:<burn_id>:<evm_tx>|PROOFS=<id1>:<pk1>:<sig1>,...`
///
/// The replay-key body before the `|` is stable and matches
/// [`bridge_unlock_memo`] exactly, so callers tracking replay protection by
/// the body-only memo continue to work unchanged.
pub(crate) fn bridge_unlock_memo_with_proofs(
    source_chain: &str,
    burn_id: &str,
    evm_tx_hash: &str,
    proofs: &[BridgeValidatorProof],
) -> String {
    let body = bridge_unlock_memo(source_chain, burn_id, evm_tx_hash);
    if proofs.is_empty() {
        return body;
    }
    let payload = proofs
        .iter()
        .map(|p| format!("{}:{}:{}", p.validator_id, p.pubkey_hex, p.signature_hex))
        .collect::<Vec<_>>()
        .join(",");
    format!("{body}{BRIDGE_PROOFS_DELIMITER}{BRIDGE_PROOFS_PREFIX}{payload}")
}

/// Parse a bridge unlock memo into its replay-key components plus, when
/// present, the raw proofs slice.
///
/// Returns `Some((source_chain, burn_id, evm_tx_hash, Some(proofs_raw)))`
/// when the memo carries a `|PROOFS=...` suffix, or
/// `Some((source_chain, burn_id, evm_tx_hash, None))` when only the body is
/// present. Returns `None` on a malformed memo (missing prefix, empty
/// fields, oversized).
pub(crate) fn parse_bridge_unlock_memo(memo: &str) -> Option<(&str, &str, &str, Option<&str>)> {
    if memo.len() > BRIDGE_MAX_MEMO_LEN {
        return None;
    }
    let rest = memo.strip_prefix(BRIDGE_UNLOCK_MEMO_PREFIX)?;
    let (body, proofs_raw) = match rest.split_once(BRIDGE_PROOFS_DELIMITER) {
        Some((body, suffix)) => {
            let proofs_raw = suffix.strip_prefix(BRIDGE_PROOFS_PREFIX)?;
            (body, Some(proofs_raw))
        }
        None => (rest, None),
    };
    let mut parts = body.splitn(3, ':');
    let source_chain = parts.next()?;
    let burn_id = parts.next()?;
    let evm_tx_hash = parts.next()?;
    if source_chain.is_empty() || burn_id.is_empty() || evm_tx_hash.is_empty() {
        return None;
    }
    if source_chain.contains(BRIDGE_PROOFS_DELIMITER)
        || burn_id.contains(BRIDGE_PROOFS_DELIMITER)
        || evm_tx_hash.contains(BRIDGE_PROOFS_DELIMITER)
    {
        return None;
    }
    Some((source_chain, burn_id, evm_tx_hash, proofs_raw))
}

/// Parse the raw `PROOFS=...` payload into structured proofs.
pub(crate) fn parse_bridge_proofs(raw: &str) -> Result<Vec<BridgeValidatorProof>, String> {
    if raw.is_empty() {
        return Err("bridge unlock proofs payload is empty".to_string());
    }
    let chunks: Vec<&str> = raw.split(',').collect();
    if chunks.len() > BRIDGE_MAX_VALIDATOR_PROOFS {
        return Err(format!(
            "bridge unlock memo carries {} proofs, exceeding limit {}",
            chunks.len(),
            BRIDGE_MAX_VALIDATOR_PROOFS,
        ));
    }
    let mut proofs = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let mut parts = chunk.splitn(3, ':');
        let validator_id = parts
            .next()
            .ok_or_else(|| format!("bridge unlock proof {i} is missing validator_id"))?;
        let pubkey_hex = parts
            .next()
            .ok_or_else(|| format!("bridge unlock proof {i} is missing pubkey"))?;
        let signature_hex = parts
            .next()
            .ok_or_else(|| format!("bridge unlock proof {i} is missing signature"))?;
        if validator_id.is_empty() {
            return Err(format!("bridge unlock proof {i} has empty validator_id"));
        }
        if !validator_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || validator_id.len() > 64
        {
            return Err(format!(
                "bridge unlock proof {i} validator_id has illegal characters or is too long"
            ));
        }
        if pubkey_hex.len() != 66 || !pubkey_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "bridge unlock proof {i} pubkey must be 66 lowercase hex chars (compressed SEC1)"
            ));
        }
        if signature_hex.len() != 128 || !signature_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "bridge unlock proof {i} signature must be 128 lowercase hex chars"
            ));
        }
        proofs.push(BridgeValidatorProof {
            validator_id: validator_id.to_string(),
            pubkey_hex: pubkey_hex.to_ascii_lowercase(),
            signature_hex: signature_hex.to_ascii_lowercase(),
        });
    }
    Ok(proofs)
}

/// Verify multisig proofs over `operation_message`.
///
/// - Each proof's secp256k1 signature must verify against its embedded pubkey.
/// - Each proof's pubkey must appear in `allowed_pubkeys`.
/// - Pubkeys must be unique (no double-counting one signer to satisfy threshold).
/// - At least `threshold` proofs must verify.
pub(crate) fn verify_bridge_proofs(
    proofs: &[BridgeValidatorProof],
    operation_message: &str,
    allowed_pubkeys: &HashSet<String>,
    threshold: usize,
) -> Result<(), String> {
    if allowed_pubkeys.is_empty() {
        return Err(
            "bridge validator allowlist is empty (set ZION_BRIDGE_VALIDATOR_PUBKEYS)".to_string(),
        );
    }
    if threshold < BRIDGE_MIN_VALIDATOR_PROOFS {
        return Err(format!(
            "bridge validator threshold {threshold} is below protocol minimum {BRIDGE_MIN_VALIDATOR_PROOFS}"
        ));
    }
    if proofs.len() < threshold {
        return Err(format!(
            "bridge unlock has {} proofs, need at least {}",
            proofs.len(),
            threshold,
        ));
    }
    let mut verified = HashSet::new();
    for (i, proof) in proofs.iter().enumerate() {
        if !allowed_pubkeys.contains(&proof.pubkey_hex) {
            return Err(format!(
                "bridge unlock proof {i} ({}) pubkey is not in core allowlist",
                proof.validator_id,
            ));
        }
        let pubkey_bytes = hex::decode(&proof.pubkey_hex).map_err(|_| {
            format!(
                "bridge unlock proof {i} ({}) has invalid pubkey hex",
                proof.validator_id,
            )
        })?;
        let verifying_key = VerifyingKey::from_sec1_bytes(&pubkey_bytes).map_err(|_| {
            format!(
                "bridge unlock proof {i} ({}) pubkey is not a valid secp256k1 point",
                proof.validator_id,
            )
        })?;
        let sig_bytes = hex::decode(&proof.signature_hex).map_err(|_| {
            format!(
                "bridge unlock proof {i} ({}) has invalid signature hex",
                proof.validator_id,
            )
        })?;
        let signature = EcdsaSignature::from_slice(&sig_bytes).map_err(|_| {
            format!(
                "bridge unlock proof {i} ({}) signature is not canonical ECDSA",
                proof.validator_id,
            )
        })?;
        verifying_key
            .verify(operation_message.as_bytes(), &signature)
            .map_err(|_| {
                format!(
                    "bridge unlock proof {i} ({}) failed secp256k1 signature verification",
                    proof.validator_id,
                )
            })?;
        if !verified.insert(proof.pubkey_hex.clone()) {
            return Err(format!(
                "bridge unlock has duplicate pubkey for proof {i} ({}); each signer must be unique",
                proof.validator_id,
            ));
        }
    }
    if verified.len() < threshold {
        return Err(format!(
            "bridge unlock verified only {} distinct signers, need {}",
            verified.len(),
            threshold,
        ));
    }
    Ok(())
}

/// Read the L1-enforced bridge validator allowlist from
/// `ZION_BRIDGE_VALIDATOR_PUBKEYS` (comma-separated, hex-encoded compressed
/// SEC1 secp256k1 pubkeys).
pub(crate) fn load_bridge_validator_pubkey_allowlist() -> HashSet<String> {
    std::env::var("ZION_BRIDGE_VALIDATOR_PUBKEYS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.trim_start_matches("0x").to_ascii_lowercase())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

/// Read the L1-enforced bridge unlock multisig threshold from
/// `ZION_BRIDGE_VALIDATOR_THRESHOLD`. Defaults to (and is floored at)
/// [`BRIDGE_MIN_VALIDATOR_PROOFS`].
pub(crate) fn required_bridge_validator_threshold() -> usize {
    std::env::var("ZION_BRIDGE_VALIDATOR_THRESHOLD")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= BRIDGE_MIN_VALIDATOR_PROOFS)
        .unwrap_or(BRIDGE_MIN_VALIDATOR_PROOFS)
}

/// Canonical operation message that all validator proofs must sign.
///
/// This is the byte-for-byte preimage used by both the JSON-RPC entrypoint
/// (`submitBridgeUnlock`) and the peer-block / mempool validation path. Any
/// deviation between the producer's signed message and what this function
/// reconstructs from the on-chain transaction will fail signature
/// verification.
pub fn bridge_operation_message(
    recipient: &str,
    amount_flowers: u64,
    source_chain: &str,
    burn_id: &str,
    evm_tx_hash: &str,
) -> String {
    format!(
        "unlock|recipient={}|amount={}|chain={}|burn_id={}|evm_tx={}",
        recipient, amount_flowers, source_chain, burn_id, evm_tx_hash
    )
}

pub(crate) fn bridge_unlock_replay_key_from_transaction(
    transaction: &crate::tx::Transaction,
) -> Option<String> {
    transaction
        .outputs
        .iter()
        .filter_map(|output| output.memo.as_deref())
        .find_map(|memo| {
            let (source_chain, burn_id, evm_tx_hash, _proofs) = parse_bridge_unlock_memo(memo)?;
            Some(bridge_unlock_replay_key(source_chain, burn_id, evm_tx_hash))
        })
}

#[derive(Debug, Clone)]
pub struct BridgeUnlockRequest {
    pub recipient: String,
    pub amount_flowers: u64,
    pub source_chain: String,
    pub burn_id: String,
    pub evm_tx_hash: String,
}

pub(crate) fn validate_bridge_unlock_transaction_shape_with_utxos(
    transaction: &crate::tx::Transaction,
    utxos: &std::collections::HashMap<(String, u32), crate::SpendableUtxo>,
) -> Result<Option<String>, String> {
    let Some(replay_key) = bridge_unlock_replay_key_from_transaction(transaction) else {
        return Ok(None);
    };

    if transaction.inputs.is_empty() {
        return Err("bridge unlock transaction must spend bridge vault UTXOs".to_string());
    }

    let memo_outputs: Vec<&crate::tx::TxOutput> = transaction
        .outputs
        .iter()
        .filter(|output| {
            output
                .memo
                .as_deref()
                .is_some_and(|memo| memo.starts_with(BRIDGE_UNLOCK_MEMO_PREFIX))
        })
        .collect();
    if memo_outputs.len() != 1 {
        return Err(
            "bridge unlock transaction must contain exactly one unlock memo output".to_string(),
        );
    }

    let recipient_output = memo_outputs[0];
    if recipient_output.address == crate::fee::BRIDGE_VAULT_ADDRESS {
        return Err(
            "bridge unlock recipient output must not point back to the bridge vault".to_string(),
        );
    }

    // Step 1: parse the unlock memo into its replay-key body and (mandatory)
    // multisig proofs. The proofs payload is required — every node enforces
    // ≥ BRIDGE_MIN_VALIDATOR_PROOFS distinct, allow-listed secp256k1
    // signatures over the canonical operation message reconstructed from
    // the transaction itself.
    let memo_str = recipient_output
        .memo
        .as_deref()
        .expect("memo presence guaranteed by filter above");
    let (source_chain, burn_id, evm_tx_hash, proofs_raw) = parse_bridge_unlock_memo(memo_str)
        .ok_or_else(|| "bridge unlock memo is malformed".to_string())?;
    let raw = proofs_raw.ok_or_else(|| {
        "bridge unlock memo is missing required validator proofs (PROOFS=...)".to_string()
    })?;
    let proofs = parse_bridge_proofs(raw)?;

    // Step 2: reconstruct the canonical operation message from the
    // transaction itself (recipient, amount, source/burn/evm metadata).
    // Validators must have signed exactly this message; mutating any of
    // these fields after signing breaks verification.
    let operation_message = bridge_operation_message(
        &recipient_output.address,
        recipient_output.amount,
        source_chain,
        burn_id,
        evm_tx_hash,
    );
    let allowed_pubkeys = load_bridge_validator_pubkey_allowlist();
    let threshold = required_bridge_validator_threshold();
    verify_bridge_proofs(&proofs, &operation_message, &allowed_pubkeys, threshold)?;

    let outputs_for_validation: Vec<(u64, &str)> = transaction
        .outputs
        .iter()
        .map(|output| (output.amount, output.address.as_str()))
        .collect();
    crate::fee::validate_outputs(&outputs_for_validation)?;
    for output in &transaction.outputs {
        if output.address != crate::fee::BRIDGE_VAULT_ADDRESS
            && !crate::crypto::is_valid_address(&output.address)
        {
            return Err(format!(
                "bridge unlock output address is invalid: {}",
                output.address
            ));
        }
        if output.memo.is_some() && output.address == crate::fee::BRIDGE_VAULT_ADDRESS {
            return Err("bridge unlock change output must not carry a memo".to_string());
        }
    }

    let tx_size = crate::fee::estimate_tx_size(transaction.inputs.len(), transaction.outputs.len());
    crate::fee::validate_fee(transaction.fee, tx_size)?;

    let mut seen_inputs = HashSet::new();
    let mut total_input = 0u64;
    for input in &transaction.inputs {
        if !input.signature.is_empty() || !input.public_key.is_empty() {
            return Err(
                "bridge unlock inputs must use the dedicated keyless bridge authorization path"
                    .to_string(),
            );
        }
        if !seen_inputs.insert((input.prev_tx_hash, input.output_index)) {
            return Err("bridge unlock transaction contains duplicate inputs".to_string());
        }
        let Some(utxo) = utxos.get(&(crate::hex(&input.prev_tx_hash), input.output_index)) else {
            return Err(format!(
                "bridge unlock input {}:{} does not exist or is already spent",
                crate::hex(&input.prev_tx_hash),
                input.output_index,
            ));
        };
        if utxo.address != crate::fee::BRIDGE_VAULT_ADDRESS {
            return Err("bridge unlock may only spend UTXOs owned by the bridge vault".to_string());
        }
        total_input = total_input
            .checked_add(utxo.amount)
            .ok_or_else(|| "bridge unlock input sum overflowed".to_string())?;
    }

    let total_output = transaction.total_output();
    let required_input = total_output
        .checked_add(transaction.fee)
        .ok_or_else(|| "bridge unlock outputs plus fee overflowed".to_string())?;
    if total_input != required_input {
        return Err(format!(
            "bridge unlock input total {} does not match outputs {} plus fee {}",
            total_input, total_output, transaction.fee,
        ));
    }

    Ok(Some(replay_key))
}
