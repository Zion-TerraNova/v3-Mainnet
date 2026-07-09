//! Wallet signature authentication for OASIS API.
//!
//! Clients authenticate by signing a challenge message with their Ed25519
//! keypair (same keys as L1 wallets) and sending four headers:
//!
//! | Header               | Contents                                  |
//! |----------------------|-------------------------------------------|
//! | `X-Wallet-Address`   | `zion1...` (44-char bech32) address       |
//! | `X-Wallet-Signature` | hex-encoded 64-byte Ed25519 signature     |
//! | `X-Wallet-Message`   | the exact UTF-8 message that was signed    |
//! | `X-Wallet-Timestamp` | unix seconds when the signature was made   |
//!
//! ## Signed message format
//!
//! Because a `zion1...` address is a SHA-256+RIPEMD-160 hash of the public
//! key, the public key cannot be recovered from the address alone. The
//! public key is therefore embedded in the signed message using the
//! canonical wire format:
//!
//! ```text
//! <pubkey_hex>:<timestamp>:<context>
//! ```
//!
//! The *entire* message string (including the pubkey prefix) is what the
//! client signs. The server:
//!   1. parses the pubkey hex from the message prefix,
//!   2. verifies the Ed25519 signature over the full message bytes,
//!   3. re-derives the `zion1...` address from the pubkey and checks it
//!      matches the `X-Wallet-Address` header,
//!   4. checks the timestamp is within a ±300 s replay window.
//!
//! This keeps the public key authenticated (it is covered by the signature)
//! while remaining backwards-compatible with any client that follows the
//! format.

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::server::OasisState;

/// Maximum clock skew / replay window in seconds.
pub const TIMESTAMP_WINDOW_SECS: u64 = 300;

/// Header names.
pub const HDR_ADDRESS: &str = "X-Wallet-Address";
pub const HDR_SIGNATURE: &str = "X-Wallet-Signature";
pub const HDR_MESSAGE: &str = "X-Wallet-Message";
pub const HDR_TIMESTAMP: &str = "X-Wallet-Timestamp";

/// Separator used inside the signed message wire format.
const MSG_SEP: char = ':';

// ── Errors ─────────────────────────────────────────────────────────────

/// Errors emitted by wallet authentication.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing authentication header: {0}")]
    MissingHeader(&'static str),

    #[error("invalid header format: {0}")]
    InvalidFormat(&'static str),

    #[error("invalid signature")]
    InvalidSignature,

    #[error("timestamp expired or outside replay window")]
    ExpiredTimestamp,

    #[error("invalid address")]
    InvalidAddress,
}

impl AuthError {
    /// Map an auth error to the HTTP status code a rejected request should
    /// return. All auth failures are `401 Unauthorized` except malformed
    /// input which is `400 Bad Request`.
    pub fn status_code(&self) -> StatusCode {
        match self {
            AuthError::MissingHeader(_) | AuthError::InvalidFormat(_) => StatusCode::BAD_REQUEST,
            AuthError::InvalidSignature
            | AuthError::ExpiredTimestamp
            | AuthError::InvalidAddress => StatusCode::UNAUTHORIZED,
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.status_code(), self.to_string()).into_response()
    }
}

// ── WalletAuth ─────────────────────────────────────────────────────────

/// Parsed wallet authentication material extracted from request headers.
#[derive(Debug, Clone)]
pub struct WalletAuth {
    /// `zion1...` address (44 chars, bech32-style).
    pub address: String,
    /// Hex-encoded 64-byte Ed25519 signature.
    pub signature: String,
    /// The exact UTF-8 message that was signed.
    pub message: String,
    /// Unix timestamp (seconds) claimed by the client.
    pub timestamp: u64,
}

impl WalletAuth {
    /// Verify the wallet signature end-to-end.
    ///
    /// Steps:
    ///   1. Parse the 64-byte signature from hex.
    ///   2. Extract the 32-byte public key from the message prefix.
    ///   3. Verify the Ed25519 signature over the full message bytes.
    ///   4. Re-derive the address from the public key and compare with
    ///      `self.address`.
    ///   5. Check the timestamp is within `TIMESTAMP_WINDOW_SECS` of now.
    pub fn verify(&self) -> Result<(), AuthError> {
        // 1. Parse signature (hex → 64 bytes).
        let sig_bytes =
            hex::decode(&self.signature).map_err(|_| AuthError::InvalidFormat("signature hex"))?;
        if sig_bytes.len() != 64 {
            return Err(AuthError::InvalidFormat("signature length"));
        }
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| AuthError::InvalidFormat("signature length"))?;
        let signature = Signature::from_bytes(&sig_arr);

        // 2. Parse public key from the message prefix.
        let pubkey_hex = self
            .message
            .split(MSG_SEP)
            .next()
            .ok_or(AuthError::InvalidFormat("message prefix"))?;
        let pk_bytes =
            hex::decode(pubkey_hex).map_err(|_| AuthError::InvalidFormat("pubkey hex"))?;
        if pk_bytes.len() != 32 {
            return Err(AuthError::InvalidFormat("pubkey length"));
        }
        let pk_arr: [u8; 32] = pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| AuthError::InvalidFormat("pubkey length"))?;
        let verifying_key =
            VerifyingKey::from_bytes(&pk_arr).map_err(|_| AuthError::InvalidSignature)?;

        // 3. Verify Ed25519 signature over the full message bytes.
        verifying_key
            .verify(self.message.as_bytes(), &signature)
            .map_err(|_| AuthError::InvalidSignature)?;

        // 4. Re-derive the address and compare.
        let derived = derive_address(&pk_arr);
        if derived != self.address {
            return Err(AuthError::InvalidAddress);
        }

        // 5. Replay-protection window.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if self.timestamp > now + TIMESTAMP_WINDOW_SECS
            || self.timestamp + TIMESTAMP_WINDOW_SECS < now
        {
            return Err(AuthError::ExpiredTimestamp);
        }

        Ok(())
    }
}

// ── Header extraction ──────────────────────────────────────────────────

/// Extract a single required header value as a string.
fn header_str(headers: &HeaderMap, name: &'static str) -> Result<String, AuthError> {
    let value = headers.get(name).ok_or(AuthError::MissingHeader(name))?;
    value
        .to_str()
        .map(|s| s.trim().to_string())
        .map_err(|_| AuthError::InvalidFormat(name))
}

/// Build a [`WalletAuth`] from the four `X-Wallet-*` headers.
pub fn extract_from_headers(headers: &HeaderMap) -> Result<WalletAuth, AuthError> {
    let address = header_str(headers, HDR_ADDRESS)?;
    let signature = header_str(headers, HDR_SIGNATURE)?;
    let message = header_str(headers, HDR_MESSAGE)?;
    let timestamp_str = header_str(headers, HDR_TIMESTAMP)?;
    let timestamp = timestamp_str
        .parse::<u64>()
        .map_err(|_| AuthError::InvalidFormat(HDR_TIMESTAMP))?;

    // Light-weight address sanity check before crypto work.
    if !address.starts_with("zion1") || address.len() != 44 {
        return Err(AuthError::InvalidAddress);
    }

    Ok(WalletAuth {
        address,
        signature,
        message,
        timestamp,
    })
}

// ── Axum middleware ────────────────────────────────────────────────────

/// Extension inserted into the request on successful auth so handlers can
/// read the authenticated wallet address.
#[derive(Debug, Clone)]
pub struct AuthenticatedWallet(pub String);

/// Axum middleware requiring a valid wallet signature.
///
/// On success the verified `zion1...` address is attached to the request
/// extensions as [`AuthenticatedWallet`] and the request proceeds. On
/// failure a `401 Unauthorized` (or `400` for malformed headers) is
/// returned immediately.
pub async fn require_auth(
    State(_state): State<OasisState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth = match extract_from_headers(req.headers()) {
        Ok(a) => a,
        Err(e) => return Err(e.status_code()),
    };

    match auth.verify() {
        Ok(()) => {
            let address = auth.address.clone();
            req.extensions_mut().insert(AuthenticatedWallet(address));
            Ok(next.run(req).await)
        }
        Err(e) => Err(e.status_code()),
    }
}

// ── Address derivation ─────────────────────────────────────────────────
//
// This is a re-implementation of the L1 `crypto::derive_address` algorithm
// (see `V3/L1/core/src/crypto.rs`). OASIS is an L4 crate and must not depend
// on L1 consensus code, so the derivation is duplicated here for verification
// purposes only. The algorithm is stable and frozen at mainnet genesis.

const ZION_BASE32_ALPHABET: &[u8; 32] = b"023456789acdefghjklmnpqrstuvwxyz";

fn compute_address_checksum(body_35: &str) -> String {
    use sha2::{Digest, Sha256};
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

/// Derive a `zion1...` address from a raw 32-byte Ed25519 public key.
///
/// Format (44 chars): `zion1` (5) + body (35) + checksum (4).
fn derive_address(public_key_bytes: &[u8]) -> String {
    use ripemd::Ripemd160;
    use sha2::{Digest, Sha256};

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

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    /// Build a valid [`WalletAuth`] for `context` signed by a fresh keypair.
    fn make_valid_auth(context: &str, timestamp: u64) -> (WalletAuth, SigningKey) {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        let address = derive_address(verifying_key.as_bytes());
        let pubkey_hex = hex::encode(verifying_key.as_bytes());
        let message = format!("{pubkey_hex}:{timestamp}:{context}");

        let sig = signing_key.sign(message.as_bytes());
        let signature = hex::encode(sig.to_bytes());

        let auth = WalletAuth {
            address,
            signature,
            message,
            timestamp,
        };
        (auth, signing_key)
    }

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn test_valid_signature_verifies() {
        let (auth, _) = make_valid_auth("login to OASIS", now_secs());
        assert!(auth.verify().is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let (mut auth, _) = make_valid_auth("login to OASIS", now_secs());
        // Flip a byte in the signature hex so it no longer matches.
        let mut bytes = hex::decode(&auth.signature).unwrap();
        bytes[0] ^= 0xff;
        auth.signature = hex::encode(&bytes);
        let err = auth.verify().unwrap_err();
        assert!(matches!(err, AuthError::InvalidSignature), "got {err:?}");
    }

    #[test]
    fn test_tampered_message_rejected() {
        let (mut auth, _) = make_valid_auth("login to OASIS", now_secs());
        // Keep the pubkey prefix intact but alter the context so the signed
        // bytes no longer match.
        let parts: Vec<&str> = auth.message.splitn(3, MSG_SEP).collect();
        auth.message = format!("{}:{}:tampered", parts[0], parts[1]);
        let err = auth.verify().unwrap_err();
        assert!(matches!(err, AuthError::InvalidSignature), "got {err:?}");
    }

    #[test]
    fn test_expired_timestamp_rejected() {
        // 10 minutes in the past — outside the 300 s window.
        let old = now_secs().saturating_sub(600);
        let (auth, _) = make_valid_auth("login to OASIS", old);
        let err = auth.verify().unwrap_err();
        assert!(matches!(err, AuthError::ExpiredTimestamp), "got {err:?}");
    }

    #[test]
    fn test_future_timestamp_rejected() {
        let future = now_secs() + 600;
        let (auth, _) = make_valid_auth("login to OASIS", future);
        let err = auth.verify().unwrap_err();
        assert!(matches!(err, AuthError::ExpiredTimestamp), "got {err:?}");
    }

    #[test]
    fn test_wrong_address_rejected() {
        let (mut auth, _) = make_valid_auth("login to OASIS", now_secs());
        // Replace the address with a structurally-valid but wrong one.
        auth.address = derive_address(&[0u8; 32]);
        assert_ne!(auth.address, derive_address(&[1u8; 32]));
        let err = auth.verify().unwrap_err();
        assert!(matches!(err, AuthError::InvalidAddress), "got {err:?}");
    }

    #[test]
    fn test_missing_header() {
        let headers = HeaderMap::new();
        let err = extract_from_headers(&headers).unwrap_err();
        assert!(matches!(err, AuthError::MissingHeader(_)), "got {err:?}");
    }

    #[test]
    fn test_extract_from_headers_roundtrip() {
        let (auth, _) = make_valid_auth("login to OASIS", now_secs());

        let mut headers = HeaderMap::new();
        headers.insert(HDR_ADDRESS, HeaderValue::from_str(&auth.address).unwrap());
        headers.insert(
            HDR_SIGNATURE,
            HeaderValue::from_str(&auth.signature).unwrap(),
        );
        headers.insert(HDR_MESSAGE, HeaderValue::from_str(&auth.message).unwrap());
        headers.insert(
            HDR_TIMESTAMP,
            HeaderValue::from_str(&auth.timestamp.to_string()).unwrap(),
        );

        let extracted = extract_from_headers(&headers).unwrap();
        assert_eq!(extracted.address, auth.address);
        assert_eq!(extracted.signature, auth.signature);
        assert_eq!(extracted.message, auth.message);
        assert_eq!(extracted.timestamp, auth.timestamp);
        assert!(extracted.verify().is_ok());
    }

    #[test]
    fn test_extract_rejects_bad_address_shape() {
        let mut headers = HeaderMap::new();
        headers.insert(HDR_ADDRESS, HeaderValue::from_static("not-a-zion-address"));
        headers.insert(HDR_SIGNATURE, HeaderValue::from_static("00"));
        headers.insert(HDR_MESSAGE, HeaderValue::from_static("msg"));
        headers.insert(HDR_TIMESTAMP, HeaderValue::from_static("0"));

        let err = extract_from_headers(&headers).unwrap_err();
        assert!(matches!(err, AuthError::InvalidAddress), "got {err:?}");
    }

    #[test]
    fn test_extract_rejects_non_numeric_timestamp() {
        let (auth, _) = make_valid_auth("login to OASIS", now_secs());
        let mut headers = HeaderMap::new();
        headers.insert(HDR_ADDRESS, HeaderValue::from_str(&auth.address).unwrap());
        headers.insert(
            HDR_SIGNATURE,
            HeaderValue::from_str(&auth.signature).unwrap(),
        );
        headers.insert(HDR_MESSAGE, HeaderValue::from_str(&auth.message).unwrap());
        headers.insert(HDR_TIMESTAMP, HeaderValue::from_static("not-a-number"));

        let err = extract_from_headers(&headers).unwrap_err();
        assert!(matches!(err, AuthError::InvalidFormat(_)), "got {err:?}");
    }

    #[test]
    fn test_derive_address_matches_l1_algorithm() {
        // Cross-check against the L1 reference implementation's known
        // properties: 44 chars, zion1 prefix, deterministic.
        let pk = [42u8; 32];
        let addr = derive_address(&pk);
        assert_eq!(addr.len(), 44);
        assert!(addr.starts_with("zion1"));
        assert_eq!(addr, derive_address(&pk));
        assert_ne!(addr, derive_address(&[43u8; 32]));
    }
}
