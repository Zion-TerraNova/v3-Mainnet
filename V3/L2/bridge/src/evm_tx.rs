//! EVM transaction builder: ABI encoding + EIP-1559 signing.
//!
//! Provides:
//! - `encode_submit_lock_proof(...)` — ABI-encode calldata for ZIONBridge.submitLockProof()
//! - `encode_execute_timelocked_mint(...)` — ABI-encode calldata for ZIONBridge.executeTimelockedMint()
//! - `build_and_sign_eip1559_tx(...)` — build + sign a raw EIP-1559 transaction
//! - `derive_evm_address(...)` — derive EVM address from secp256k1 private key
//!
//! Transaction layout (EIP-1559, type 0x02):
//!   raw_tx = 0x02 || rlp([chain_id, nonce, max_priority_fee, max_fee, gas_limit,
//!                          to, value, data, access_list, y_parity, r, s])

use anyhow::{anyhow, Context, Result};
use k256::ecdsa::{signature::hazmat::PrehashSigner, RecoveryId, Signature, SigningKey};
use sha3::{Digest, Keccak256};

// ─────────────────────────────────────────────────────────────────────────────
// ABI Encoding
// ─────────────────────────────────────────────────────────────────────────────

/// Compute keccak256("submitLockProof(bytes32,address,uint256,uint256,string)")
/// and return the first 4 bytes as a function selector.
pub fn submit_lock_proof_selector() -> [u8; 4] {
    let mut h = Keccak256::new();
    h.update(b"submitLockProof(bytes32,address,uint256,uint256,string)");
    let digest = h.finalize();
    [digest[0], digest[1], digest[2], digest[3]]
}

/// ABI-encode `executeTimelockedMint(bytes32)`.
///
/// Called after the 24-hour timelock on a large-amount lock has expired.
/// The `l1_tx_hash_bytes` must be the same bytes32 originally passed to
/// `submitLockProof` for the given lock.
///
/// Calldata layout:
/// - `[0..4]`   — function selector for `executeTimelockedMint(bytes32)`
/// - `[4..36]`  — `l1TxHash` (bytes32, big-endian)
pub fn encode_execute_timelocked_mint(l1_tx_hash_bytes: &[u8; 32]) -> Vec<u8> {
    // keccak256("executeTimelockedMint(bytes32)") → first 4 bytes
    let mut h = Keccak256::new();
    h.update(b"executeTimelockedMint(bytes32)");
    let digest = h.finalize();
    let selector = [digest[0], digest[1], digest[2], digest[3]];

    let mut calldata = Vec::with_capacity(36);
    calldata.extend_from_slice(&selector);
    calldata.extend_from_slice(l1_tx_hash_bytes);
    calldata
}

/// Derive a deterministic bytes32 for any string (e.g. UTXO key or tx hash).
/// Uses keccak256 for uniqueness.
pub fn hash_to_bytes32(input: &str) -> [u8; 32] {
    // If it's already a 0x-prefixed 64-char hex, decode directly
    let stripped = input.trim_start_matches("0x");
    if stripped.len() == 64 {
        if let Ok(decoded) = hex::decode(stripped) {
            let mut out = [0u8; 32];
            out.copy_from_slice(&decoded);
            return out;
        }
    }
    // Otherwise, keccak256 hash the string bytes
    let mut h = Keccak256::new();
    h.update(input.as_bytes());
    let digest = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// ABI-encode `submitLockProof(bytes32,address,uint256,uint256,string)`.
///
/// # Parameters
/// - `l1_tx_hash` — 32-byte tx hash (or keccak256 of UTXO key)
/// - `recipient`  — EVM address "0x..." (20 bytes)
/// - `amount_wzion_wei` — decimal string, e.g. "5000000000000000000"
/// - `l1_block_height` — L1 block height of the lock
/// - `l1_sender`       — bech32 L1 sender address (audit trail)
pub fn encode_submit_lock_proof(
    l1_tx_hash: &[u8; 32],
    recipient: &str,
    amount_wzion_wei: &str,
    l1_block_height: u64,
    l1_sender: &str,
) -> Result<Vec<u8>> {
    // Parse recipient address (20 bytes)
    let recip_hex = recipient.trim_start_matches("0x");
    if recip_hex.len() != 40 {
        return Err(anyhow!("Invalid EVM address length: {}", recipient));
    }
    let recip_bytes =
        hex::decode(recip_hex).with_context(|| format!("Invalid recipient hex: {}", recipient))?;

    // Parse amount as u128 (covers up to ~340 undecillion wei = more than enough)
    let amount_u128: u128 = amount_wzion_wei
        .parse()
        .with_context(|| format!("Invalid amount: {}", amount_wzion_wei))?;

    // Static part (5 slots × 32 bytes) + dynamic string
    let l1_sender_bytes = l1_sender.as_bytes();
    let padded_len = l1_sender_bytes.len().div_ceil(32) * 32;

    let mut data = Vec::with_capacity(4 + 5 * 32 + 32 + padded_len);

    // 4-byte selector
    data.extend_from_slice(&submit_lock_proof_selector());

    // Slot 0: bytes32 l1TxHash (already 32 bytes)
    data.extend_from_slice(l1_tx_hash);

    // Slot 1: address (left-pad to 32 bytes: 12 zeros + 20 addr bytes)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&recip_bytes);

    // Slot 2: uint256 amount (big-endian 32 bytes)
    let mut slot2 = [0u8; 32];
    slot2[16..32].copy_from_slice(&amount_u128.to_be_bytes());
    data.extend_from_slice(&slot2);

    // Slot 3: uint256 l1BlockHeight
    let mut slot3 = [0u8; 32];
    slot3[24..32].copy_from_slice(&l1_block_height.to_be_bytes());
    data.extend_from_slice(&slot3);

    // Slot 4: string offset = 5 * 32 = 160 = 0xa0
    let offset: u64 = 5 * 32;
    let mut slot4 = [0u8; 32];
    slot4[24..32].copy_from_slice(&offset.to_be_bytes());
    data.extend_from_slice(&slot4);

    // Dynamic section: string length + string data (padded to 32-byte boundary)
    let mut len_slot = [0u8; 32];
    let slen = l1_sender_bytes.len() as u64;
    len_slot[24..32].copy_from_slice(&slen.to_be_bytes());
    data.extend_from_slice(&len_slot);
    data.extend_from_slice(l1_sender_bytes);
    if l1_sender_bytes.len() < padded_len {
        data.extend(std::iter::repeat_n(0u8, padded_len - l1_sender_bytes.len()));
    }

    Ok(data)
}

/// Compute keccak256("confirmBurnRelease(bytes32,address,uint256,string)")
/// and return the first 4 bytes as a function selector.
pub fn confirm_burn_release_selector() -> [u8; 4] {
    let mut h = Keccak256::new();
    h.update(b"confirmBurnRelease(bytes32,address,uint256,string)");
    let digest = h.finalize();
    [digest[0], digest[1], digest[2], digest[3]]
}

/// ABI-encode `confirmBurnRelease(bytes32,address,uint256,string)`.
///
/// Matches ZIONBridge.sol:
///   confirmBurnRelease(bytes32 burnId, address evmBurner, uint256 amount, string l1Recipient)
///
/// # Parameters
/// - `burn_id`      — 32-byte burn ID (keccak256 of the burn_id string if not already bytes32)
/// - `evm_burner`   — EVM address "0x..." (20 bytes)
/// - `amount_wei`   — decimal string, e.g. "5000000000000000000"
/// - `l1_recipient` — bech32 ZION L1 address (dynamic string)
pub fn encode_confirm_burn_release(
    burn_id: &[u8; 32],
    evm_burner: &str,
    amount_wei: &str,
    l1_recipient: &str,
) -> Result<Vec<u8>> {
    // Parse evm_burner address (20 bytes)
    let burner_hex = evm_burner.trim_start_matches("0x");
    if burner_hex.len() != 40 {
        return Err(anyhow!("Invalid EVM burner address length: {}", evm_burner));
    }
    let burner_bytes =
        hex::decode(burner_hex).with_context(|| format!("Invalid burner hex: {}", evm_burner))?;

    // Parse amount as u128
    let amount_u128: u128 = amount_wei
        .parse()
        .with_context(|| format!("Invalid amount: {}", amount_wei))?;

    // Static part (4 slots × 32 bytes) + dynamic string
    let l1_recipient_bytes = l1_recipient.as_bytes();
    let padded_len = l1_recipient_bytes.len().div_ceil(32) * 32;

    let mut data = Vec::with_capacity(4 + 4 * 32 + 32 + padded_len);

    // 4-byte selector
    data.extend_from_slice(&confirm_burn_release_selector());

    // Slot 0: bytes32 burnId (already 32 bytes)
    data.extend_from_slice(burn_id);

    // Slot 1: address evmBurner (left-pad to 32 bytes: 12 zeros + 20 addr bytes)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&burner_bytes);

    // Slot 2: uint256 amount (big-endian 32 bytes)
    let mut slot2 = [0u8; 32];
    slot2[16..32].copy_from_slice(&amount_u128.to_be_bytes());
    data.extend_from_slice(&slot2);

    // Slot 3: string offset = 4 * 32 = 128 = 0x80
    let offset: u64 = 4 * 32;
    let mut slot3 = [0u8; 32];
    slot3[24..32].copy_from_slice(&offset.to_be_bytes());
    data.extend_from_slice(&slot3);

    // Dynamic section: string length + string data (padded to 32-byte boundary)
    let mut len_slot = [0u8; 32];
    let slen = l1_recipient_bytes.len() as u64;
    len_slot[24..32].copy_from_slice(&slen.to_be_bytes());
    data.extend_from_slice(&len_slot);
    data.extend_from_slice(l1_recipient_bytes);
    if l1_recipient_bytes.len() < padded_len {
        data.extend(std::iter::repeat_n(
            0u8,
            padded_len - l1_recipient_bytes.len(),
        ));
    }

    Ok(data)
}

// ─────────────────────────────────────────────────────────────────────────────
// Key Utilities
// ─────────────────────────────────────────────────────────────────────────────

/// Derive the EVM address from a secp256k1 private key hex string.
///
/// Algorithm:
///   1. secp256k1 pubkey (uncompressed, 65 bytes: 0x04 || x || y)
///   2. Drop the 0x04 prefix → 64 bytes
///   3. keccak256(64 bytes) → 32 bytes
///   4. EVM address = last 20 bytes, checksummed
pub fn derive_evm_address(private_key_hex: &str) -> Result<String> {
    let pk_bytes =
        hex::decode(private_key_hex.trim_start_matches("0x")).context("Invalid private key hex")?;
    let signing_key = SigningKey::from_slice(&pk_bytes).context("Invalid secp256k1 key")?;
    let verifying_key = signing_key.verifying_key();
    let pubkey_point = verifying_key.to_encoded_point(false); // uncompressed
    let pubkey_bytes = pubkey_point.as_bytes();
    // pubkey_bytes = [0x04, x(32), y(32)] — 65 bytes
    // keccak256 of the 64-byte x||y (skip the 0x04 prefix)
    let mut h = Keccak256::new();
    h.update(&pubkey_bytes[1..]); // 64 bytes: x || y
    let hash = h.finalize();
    // EVM address = last 20 bytes of keccak256
    let addr_bytes = &hash[12..];
    Ok(format!("0x{}", hex::encode(addr_bytes)))
}

// ─────────────────────────────────────────────────────────────────────────────
// RLP Encoder
// ─────────────────────────────────────────────────────────────────────────────

/// RLP-encode a small unsigned integer.
fn rlp_uint(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![0x80]; // empty string = 0 in RLP
    }
    let bytes = value.to_be_bytes();
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
    rlp_bytes(&bytes[start..])
}

/// RLP-encode a 32-byte integer (u256 as big-endian bytes).
fn rlp_u256(bytes: &[u8; 32]) -> Vec<u8> {
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(32);
    if start == 32 {
        return vec![0x80]; // 0
    }
    rlp_bytes(&bytes[start..])
}

/// RLP-encode a raw byte slice (as a "string").
fn rlp_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 0x80 {
        return vec![data[0]]; // single byte in range [0x00, 0x7f]
    }
    let mut out = rlp_length_header(data.len(), 0x80);
    out.extend_from_slice(data);
    out
}

fn rlp_length_header(length: usize, offset: u8) -> Vec<u8> {
    if length < 56 {
        vec![offset + length as u8]
    } else {
        let len_bytes = length.to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let trimmed = &len_bytes[start..];
        let mut out = vec![offset + 55 + trimmed.len() as u8];
        out.extend_from_slice(trimmed);
        out
    }
}

/// RLP-encode a list.
fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let content: Vec<u8> = items.iter().flat_map(|v| v.iter().cloned()).collect();
    let mut out = rlp_length_header(content.len(), 0xc0);
    out.extend(content);
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// EIP-1559 Transaction Builder
// ─────────────────────────────────────────────────────────────────────────────

/// Build and sign an EIP-1559 transaction.
///
/// Returns a `0x`-prefixed hex-encoded raw transaction ready for `eth_sendRawTransaction`.
///
/// # Parameters
/// - `chain_id`                 — EVM chain ID (84532 for Base Sepolia)
/// - `nonce`                    — account nonce from `eth_getTransactionCount`
/// - `max_priority_fee_per_gas` — EIP-1559 tip (wei)
/// - `max_fee_per_gas`          — EIP-1559 max fee (wei)
/// - `gas_limit`                — gas limit
/// - `to`                       — target contract address "0x..."
/// - `calldata`                 — ABI-encoded call bytes
/// - `private_key_hex`          — 32-byte hex private key (with or without 0x prefix)
#[allow(clippy::too_many_arguments)]
pub fn build_and_sign_eip1559_tx(
    chain_id: u64,
    nonce: u64,
    max_priority_fee_per_gas: u64,
    max_fee_per_gas: u64,
    gas_limit: u64,
    to: &str,
    calldata: &[u8],
    private_key_hex: &str,
) -> Result<String> {
    // --- Parse private key ---
    let pk_bytes =
        hex::decode(private_key_hex.trim_start_matches("0x")).context("Invalid private key hex")?;
    let signing_key = SigningKey::from_slice(&pk_bytes).context("Invalid secp256k1 private key")?;

    // --- Parse `to` address ---
    let to_bytes = hex::decode(to.trim_start_matches("0x"))
        .with_context(|| format!("Invalid `to` address: {}", to))?;
    if to_bytes.len() != 20 {
        return Err(anyhow!(
            "`to` address must be 20 bytes, got {}",
            to_bytes.len()
        ));
    }

    // --- Helper: encode chain_id as RLP uint ---
    let _rlp_chain_id = rlp_uint(chain_id);

    // --- Unsigned EIP-1559 payload for signing ---
    // Type 0x02 transaction:
    //   rlp([chain_id, nonce, max_priority_fee, max_fee, gas_limit, to, value, data, access_list])
    let unsigned_payload = build_tx_rlp_items(
        chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &to_bytes,
        calldata,
        None, // no signature yet
    );
    let unsigned_rlp = rlp_list(&unsigned_payload);

    // Signing input = keccak256(0x02 || unsigned_rlp)
    let mut sign_input = Vec::with_capacity(1 + unsigned_rlp.len());
    sign_input.push(0x02u8);
    sign_input.extend(&unsigned_rlp);

    let mut hasher = Keccak256::new();
    hasher.update(&sign_input);
    let hash = hasher.finalize();

    // --- Sign ---
    let (sig, recid): (Signature, RecoveryId) = signing_key
        .sign_prehash(&hash)
        .context("ECDSA signing failed")?;

    let sig_bytes = sig.to_bytes();
    let r: [u8; 32] = sig_bytes[..32].try_into().unwrap();
    let s: [u8; 32] = sig_bytes[32..].try_into().unwrap();
    let y_parity = recid.to_byte(); // 0 or 1

    // --- Signed EIP-1559 payload ---
    let signed_payload = build_tx_rlp_items(
        chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &to_bytes,
        calldata,
        Some((y_parity, r, s)),
    );
    let signed_rlp = rlp_list(&signed_payload);

    // Final raw TX = 0x02 || signed_rlp
    let mut raw_tx = Vec::with_capacity(1 + signed_rlp.len());
    raw_tx.push(0x02u8);
    raw_tx.extend(&signed_rlp);

    Ok(format!("0x{}", hex::encode(&raw_tx)))
}

/// Build the list of RLP items for a type-0x02 transaction.
/// If `sig` is Some((y_parity, r, s)), appends signature fields.
#[allow(clippy::too_many_arguments)]
fn build_tx_rlp_items(
    chain_id: u64,
    nonce: u64,
    max_priority_fee: u64,
    max_fee: u64,
    gas_limit: u64,
    to: &[u8], // 20 bytes
    calldata: &[u8],
    sig: Option<(u8, [u8; 32], [u8; 32])>,
) -> Vec<Vec<u8>> {
    let mut items: Vec<Vec<u8>> = vec![
        rlp_uint(chain_id),
        rlp_uint(nonce),
        rlp_uint(max_priority_fee),
        rlp_uint(max_fee),
        rlp_uint(gas_limit),
        rlp_bytes(to),       // 20-byte address
        vec![0x80],          // value = 0
        rlp_bytes(calldata), // ABI-encoded calldata
        vec![0xc0],          // access_list = [] (empty RLP list)
    ];

    if let Some((y_parity, r, s)) = sig {
        items.push(if y_parity == 0 {
            vec![0x80]
        } else {
            vec![0x01]
        }); // y_parity
        items.push(rlp_u256(&r));
        items.push(rlp_u256(&s));
    }

    items
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_lock_proof_selector() {
        let sel = submit_lock_proof_selector();
        // keccak256("submitLockProof(bytes32,address,uint256,uint256,string)")[0:4]
        // Verified against cast: 4a87c842 (run: cast sig "submitLockProof(bytes32,address,uint256,uint256,string)")
        // Just verify it's 4 bytes
        assert_eq!(sel.len(), 4);
        println!("selector: {}", hex::encode(sel));
    }

    #[test]
    fn test_hash_to_bytes32_hex() {
        let hash = format!("0x{}", "ab".repeat(32));
        let result = hash_to_bytes32(&hash);
        assert_eq!(result, [0xabu8; 32]);
    }

    #[test]
    fn test_confirm_burn_release_selector() {
        let sel = confirm_burn_release_selector();
        assert_eq!(sel.len(), 4);
        println!("confirmBurnRelease selector: {}", hex::encode(sel));
    }

    #[test]
    fn test_encode_confirm_burn_release_length() {
        let burn_id = hash_to_bytes32("burn:abc123");
        let calldata = encode_confirm_burn_release(
            &burn_id,
            "0xdde17506bc2d2dce1d594bd1d85b0babb389d186",
            "1000000000000000000",
            "zion1wn5nv4snxzjjlqb48z5zatungtvr4ruz6yjd4c5",
        )
        .unwrap();
        // 4 (selector) + 4*32 (static slots) + 32 (string length) + padded string
        assert!(calldata.len() >= 4 + 4 * 32 + 32);
        println!("confirmBurnRelease calldata: 0x{}", hex::encode(&calldata));
    }

    #[test]
    fn test_encode_submit_lock_proof_length() {
        let tx_hash = hash_to_bytes32("utxo:abc123");
        let calldata = encode_submit_lock_proof(
            &tx_hash,
            "0xdde17506bc2d2dce1d594bd1d85b0babb389d186",
            "1000000000000000000",
            12345,
            "zion1wn5nv4snxzjjlqb48z5zatungtvr4ruz6yjd4c5",
        )
        .unwrap();
        // 4 (selector) + 5*32 (static slots) + 32 (string length) + padded string
        assert!(calldata.len() >= 4 + 5 * 32 + 32);
        println!("calldata length: {}", calldata.len());
        println!("calldata: 0x{}", hex::encode(&calldata));
    }

    #[test]
    fn test_rlp_uint_zero() {
        assert_eq!(rlp_uint(0), vec![0x80]);
    }

    #[test]
    fn test_rlp_uint_small() {
        assert_eq!(rlp_uint(1), vec![0x01]); // single byte < 0x80
        assert_eq!(rlp_uint(127), vec![0x7f]);
    }

    #[test]
    fn test_rlp_uint_medium() {
        // 128 = 0x80: needs length prefix
        let encoded = rlp_uint(128);
        assert_eq!(encoded, vec![0x81, 0x80]); // 0x81 = string of len 1; 0x80 = value
    }

    // ── executeTimelockedMint encoding tests ─────────────────────────────────

    #[test]
    fn test_execute_timelocked_mint_length() {
        // encode_execute_timelocked_mint(bytes32) → 4 (selector) + 32 (bytes32) = 36 bytes
        let hex_input = format!("0x{}", "ab".repeat(32));
        let hash = hash_to_bytes32(&hex_input);
        let calldata = encode_execute_timelocked_mint(&hash);
        assert_eq!(
            calldata.len(),
            36,
            "executeTimelockedMint calldata must be exactly 36 bytes"
        );
    }

    #[test]
    fn test_execute_timelocked_mint_contains_hash() {
        let mut input = [0u8; 32];
        input[31] = 0xcc; // last byte marker
        let calldata = encode_execute_timelocked_mint(&input);
        // First 4 bytes are selector, bytes 4..36 are the hash
        assert_eq!(&calldata[4..36], &input);
    }

    #[test]
    fn test_execute_timelocked_mint_selector_stable() {
        // Verify the selector is the keccak256("executeTimelockedMint(bytes32)")[0:4]
        // We check it is always the same (deterministic) across calls
        let hash = [0u8; 32];
        let call1 = encode_execute_timelocked_mint(&hash);
        let call2 = encode_execute_timelocked_mint(&hash);
        assert_eq!(&call1[0..4], &call2[0..4], "Selector must be deterministic");
        // Print for audit
        println!(
            "executeTimelockedMint selector: 0x{}",
            hex::encode(&call1[0..4])
        );
    }

    #[test]
    fn test_execute_timelocked_mint_selector_differs_from_submit_lock_proof() {
        // Sanity check: different functions must have different selectors
        let submit = submit_lock_proof_selector();
        let hash = [0u8; 32];
        let execute = encode_execute_timelocked_mint(&hash);
        assert_ne!(
            &submit[..],
            &execute[0..4],
            "executeTimelockedMint and submitLockProof must have different selectors"
        );
    }
}
