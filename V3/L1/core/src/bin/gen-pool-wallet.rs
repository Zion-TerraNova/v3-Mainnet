//! One-shot generator for the pool payout wallet keypair.
//!
//! Outputs:
//!   - zion1... address  → ZION_POOL_WALLET
//!   - secret key hex    → ZION_POOL_PAYOUT_SK_HEX
//!   - public key hex    → for audit records
//!
//! ⚠️  Secret key is printed ONCE. Save it securely.

fn main() {
    let (signing_key, verifying_key) = zion_core::crypto::generate_keypair();
    let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
    let secret_hex = zion_core::crypto::to_hex(signing_key.as_bytes());
    let public_hex = zion_core::crypto::to_hex(verifying_key.as_bytes());

    assert!(
        zion_core::crypto::is_valid_address(&address),
        "Generated address failed validation: {address}"
    );

    println!("ZION_POOL_WALLET={address}");
    println!("ZION_POOL_PAYOUT_SK_HEX={secret_hex}");
    println!("ZION_POOL_PAYOUT_PK_HEX={public_hex}");

    eprintln!("⚠️  SAVE THESE VALUES SECURELY — secret key shown only once!");
}
