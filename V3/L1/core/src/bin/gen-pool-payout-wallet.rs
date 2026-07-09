fn main() {
    let (signing_key, verifying_key) = zion_core::crypto::generate_keypair();
    let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
    let secret_hex = zion_core::crypto::to_hex(signing_key.as_bytes());
    let public_hex = zion_core::crypto::to_hex(verifying_key.as_bytes());
    assert!(zion_core::crypto::is_valid_address(&address));
    println!(
        r#"{{
  "name": "pool_payout",
  "description": "Pool Payout Wallet — signs miner payouts",
  "address": "{address}",
  "public_key_hex": "{public_hex}",
  "secret_key_hex": "{secret_hex}"
}}"#
    );
    eprintln!("⚠️  SAVE THIS OUTPUT SECURELY — secret key shown only once!");
}
