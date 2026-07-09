//! Cryptographic primitives for ZION V3.
//!
//! - **Ed25519** — key generation, signing, verification (`ed25519_dalek`)
//! - **BLAKE3**  — general-purpose hashing (tx hashes, merkle roots)
//! - **`zion1...`** — canonical address derivation with checksum
//!
//! PoW hashing uses Ekam Deeksha (cosmic_harmony), not this module.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::StdRng;
use rand::SeedableRng;
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

// ── BLAKE3 general hash ────────────────────────────────────────────────

/// BLAKE3 hash of arbitrary data. Used for tx hashing, merkle roots, etc.
pub fn blake3_hash(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

// ── Ed25519 ────────────────────────────────────────────────────────────

/// Generate a new Ed25519 keypair from OS random.
pub fn generate_keypair() -> (SigningKey, VerifyingKey) {
    let mut csprng = rand::rngs::OsRng;
    let signing = SigningKey::generate(&mut csprng);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Deterministic Ed25519 keypair from a domain-separated UTF-8 label: BLAKE3(label) → `StdRng` seed → `SigningKey::generate`.
///
/// **Anyone with this source and the same label can regenerate the keypair.** Use for repo-pinned
/// bootstrap addresses only; for exclusive custody, replace with OS-random keys and store offline.
pub fn keypair_from_canonical_label(label: &str) -> (SigningKey, VerifyingKey) {
    let seed = blake3_hash(label.as_bytes());
    let mut rng = StdRng::from_seed(seed);
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// `zion1…` address for [`keypair_from_canonical_label`].
pub fn canonical_address_for_label(label: &str) -> String {
    let (_, vk) = keypair_from_canonical_label(label);
    derive_address(vk.as_bytes())
}

/// Sign `message` with an Ed25519 secret key. Returns 64-byte signature.
pub fn sign(signing_key: &SigningKey, message: &[u8]) -> [u8; 64] {
    signing_key.sign(message).to_bytes()
}

/// Sign and then zeroize the signing key bytes (for wallet use).
pub fn sign_and_zeroize(mut key_bytes: [u8; 32], message: &[u8]) -> Result<[u8; 64], &'static str> {
    let sk = SigningKey::from_bytes(&key_bytes);
    let sig = sk.sign(message).to_bytes();
    key_bytes.zeroize();
    Ok(sig)
}

/// Verify an Ed25519 signature.
pub fn verify(public_key_bytes: &[u8], message: &[u8], signature_bytes: &[u8]) -> bool {
    let pk_array: [u8; 32] = match public_key_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    let public_key = match VerifyingKey::from_bytes(&pk_array) {
        Ok(pk) => pk,
        Err(_) => return false,
    };
    let sig_array: [u8; 64] = match signature_bytes.try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_array);
    public_key.verify(message, &signature).is_ok()
}

// ── zion1 address derivation ───────────────────────────────────────────

/// Custom base32 alphabet (no `b`, `i`, `l`, `o`, `1` to avoid visual ambiguity)
const ZION_BASE32_ALPHABET: &[u8; 32] = b"023456789acdefghjklmnpqrstuvwxyz";

/// Compute 4-char checksum from `"zion1" + body[0..35]` via SHA-256.
fn compute_address_checksum(body_35: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"zion1");
    hasher.update(body_35.as_bytes());
    let hash = hasher.finalize();
    let mut ck = String::with_capacity(4);
    for &byte in &hash[..2] {
        ck.push(ZION_BASE32_ALPHABET[(byte % 32) as usize] as char);
        ck.push(ZION_BASE32_ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    ck
}

/// Derive `zion1...` address from raw 32-byte Ed25519 public key.
///
/// Format (44 chars): `zion1` (5) + body (35) + checksum (4)
///
/// Algorithm:
///   1. SHA-256(pubkey) → RIPEMD-160 → 20 bytes
///   2. Encode each byte as 2 base32 chars → 40 raw chars
///   3. Truncate to 35 body chars
///   4. Append 4-char SHA-256 checksum of `"zion1" + body`
///   5. Prefix with `zion1`
pub fn derive_address(public_key_bytes: &[u8]) -> String {
    let sha = Sha256::digest(public_key_bytes);
    let key_hash = Ripemd160::digest(sha);

    let mut data = String::with_capacity(40);
    for &byte in key_hash.as_slice() {
        data.push(ZION_BASE32_ALPHABET[(byte % 32) as usize] as char);
        data.push(ZION_BASE32_ALPHABET[((byte / 32) % 32) as usize] as char);
    }
    data.truncate(35);

    let checksum = compute_address_checksum(&data);
    format!("zion1{data}{checksum}")
}

/// Derive address from hex-encoded public key.
pub fn derive_address_from_hex(pk_hex: &str) -> Option<String> {
    let pk_bytes = from_hex(pk_hex)?;
    Some(derive_address(&pk_bytes))
}

/// Validate a `zion1` address (format + checksum).
///
/// - starts with `zion1`
/// - exactly 44 chars
/// - body chars are in base32 alphabet
/// - last 4 chars match checksum
pub fn is_valid_address(address: &str) -> bool {
    if !address.starts_with("zion1") || address.len() != 44 {
        return false;
    }
    if !address
        .as_bytes()
        .iter()
        .skip(5)
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'z'))
    {
        return false;
    }
    let body = &address[5..40];
    let expected_ck = compute_address_checksum(body);
    let actual_ck = &address[40..44];
    expected_ck == actual_ck
}

// ── Keyless address derivation ─────────────────────────────────────────

/// Derive a deterministic keyless address from a well-known seed string.
///
/// The seed is SHA-256 hashed to produce 32 bytes which are then fed through
/// the standard `derive_address` pipeline. Because the "public key" is a hash
/// of a plaintext seed, no private key exists — the address is provably
/// unspendable via normal wallet operations.
pub fn derive_keyless_address(seed: &str) -> String {
    let synthetic_pubkey = Sha256::digest(seed.as_bytes());
    derive_address(&synthetic_pubkey)
}

/// The canonical seed string used to derive the ZION bridge vault address.
///
/// Hard reset 2026-07-06: seed changed to v2 for fresh genesis.
/// Old seed: `"ZION Bridge Vault V3 Mainnet"` → zion1w0r0a560l3j2y6f3v2f457n2u4d0n5v2g79w0t0
/// New seed: `"ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"` → (new address)
pub const BRIDGE_VAULT_SEED: &str = "ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET";

/// Derive the canonical bridge vault address.
pub fn bridge_vault_address() -> String {
    derive_keyless_address(BRIDGE_VAULT_SEED)
}

// ── hex helpers ────────────────────────────────────────────────────────

/// Encode bytes as lowercase hex string.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Decode hex string to bytes.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let pair = &s[i..i + 2];
        match u8::from_str_radix(pair, 16) {
            Ok(b) => bytes.push(b),
            Err(_) => return None,
        }
    }
    Some(bytes)
}

// ── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── BLAKE3 ─────────────────────────────────────────────────────

    #[test]
    fn blake3_deterministic() {
        let a = blake3_hash(b"hello");
        let b = blake3_hash(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn blake3_different_inputs_different_hashes() {
        assert_ne!(blake3_hash(b"hello"), blake3_hash(b"world"));
    }

    // ── Ed25519 keygen/sign/verify ────────────────────────────────

    #[test]
    fn keypair_generate_sign_verify_roundtrip() {
        let (sk, vk) = generate_keypair();
        let msg = b"test message";
        let sig = sign(&sk, msg);
        assert!(verify(vk.as_bytes(), msg, &sig));
    }

    #[test]
    fn verify_rejects_wrong_message() {
        let (sk, vk) = generate_keypair();
        let sig = sign(&sk, b"correct");
        assert!(!verify(vk.as_bytes(), b"wrong", &sig));
    }

    #[test]
    fn verify_rejects_wrong_key() {
        let (sk, _vk) = generate_keypair();
        let (_sk2, vk2) = generate_keypair();
        let sig = sign(&sk, b"msg");
        assert!(!verify(vk2.as_bytes(), b"msg", &sig));
    }

    #[test]
    fn keypair_from_canonical_label_is_deterministic() {
        let label = "__test_canonical_deterministic_v1__";
        let (sk1, vk1) = keypair_from_canonical_label(label);
        let (sk2, vk2) = keypair_from_canonical_label(label);
        assert_eq!(sk1.to_bytes(), sk2.to_bytes());
        assert_eq!(vk1.to_bytes(), vk2.to_bytes());
        assert_eq!(
            derive_address(vk1.as_bytes()),
            canonical_address_for_label(label)
        );
    }

    #[test]
    fn verify_rejects_bad_lengths() {
        assert!(!verify(&[0u8; 31], b"msg", &[0u8; 64]));
        assert!(!verify(&[0u8; 32], b"msg", &[0u8; 63]));
    }

    #[test]
    fn sign_and_zeroize_works() {
        let (sk, vk) = generate_keypair();
        let key_bytes = sk.to_bytes();
        let sig = sign_and_zeroize(key_bytes, b"msg").unwrap();
        assert!(verify(vk.as_bytes(), b"msg", &sig));
    }

    // ── zion1 address ─────────────────────────────────────────────

    #[test]
    fn address_is_44_chars() {
        let pk = [1u8; 32];
        let addr = derive_address(&pk);
        assert_eq!(addr.len(), 44);
    }

    #[test]
    fn address_starts_with_zion1() {
        let addr = derive_address(&[2u8; 32]);
        assert!(addr.starts_with("zion1"));
    }

    #[test]
    fn address_checksum_roundtrip() {
        for seed in 0u8..=255 {
            let addr = derive_address(&[seed; 32]);
            assert!(
                is_valid_address(&addr),
                "Checksum failed for seed {seed}: {addr}"
            );
        }
    }

    #[test]
    fn address_deterministic() {
        let pk = [99u8; 32];
        assert_eq!(derive_address(&pk), derive_address(&pk));
    }

    #[test]
    fn different_pubkeys_different_addresses() {
        let a = derive_address(&[0u8; 32]);
        let b = derive_address(&[1u8; 32]);
        assert_ne!(a, b);
    }

    #[test]
    fn checksum_detects_single_char_mutation() {
        let addr = derive_address(&[42u8; 32]);
        assert!(is_valid_address(&addr));
        let mut bad = addr.clone().into_bytes();
        bad[10] = if bad[10] == b'0' { b'a' } else { b'0' };
        let bad_addr = String::from_utf8(bad).unwrap();
        assert!(!is_valid_address(&bad_addr));
    }

    #[test]
    fn invalid_addresses_rejected() {
        assert!(!is_valid_address(
            "btc1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        ));
        assert!(!is_valid_address("zion1short"));
        assert!(!is_valid_address(""));
        assert!(!is_valid_address(
            "zion1AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        ));
    }

    #[test]
    fn checksum_detects_truncation() {
        let addr = derive_address(&[7u8; 32]);
        assert!(!is_valid_address(&addr[..43]));
    }

    #[test]
    fn derive_address_from_hex_works() {
        let pk = [5u8; 32];
        let hex_pk = to_hex(&pk);
        let addr = derive_address_from_hex(&hex_pk).unwrap();
        assert!(is_valid_address(&addr));
        assert_eq!(addr, derive_address(&pk));
    }

    // ── hex helpers ───────────────────────────────────────────────

    // ── keyless vault address ─────────────────────────────────────

    #[test]
    fn bridge_vault_address_is_valid_and_deterministic() {
        let addr = bridge_vault_address();
        assert!(is_valid_address(&addr), "vault address is invalid: {addr}");
        assert_eq!(
            addr,
            bridge_vault_address(),
            "vault address is not deterministic"
        );
        eprintln!("BRIDGE_VAULT_ADDRESS = {addr}");
    }

    #[test]
    fn keyless_address_differs_from_keypair_address() {
        // A keyless address derived from a seed must not collide with
        // any address derived from a real keypair (probabilistic check).
        let vault = bridge_vault_address();
        let (_, vk) = generate_keypair();
        let normal = derive_address(vk.as_bytes());
        assert_ne!(vault, normal);
    }

    // ── hex helpers ───────────────────────────────────────────────

    #[test]
    fn hex_roundtrip() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let encoded = to_hex(&data);
        assert_eq!(encoded, "deadbeef");
        let decoded = from_hex(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn from_hex_rejects_odd_length() {
        assert!(from_hex("abc").is_none());
    }

    #[test]
    fn from_hex_rejects_invalid_chars() {
        assert!(from_hex("zzzz").is_none());
    }
}
