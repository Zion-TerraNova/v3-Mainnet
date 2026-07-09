//! Generate a complete set of fresh keys for a local test mainnet.
//!
//! Outputs all wallet addresses and secret keys in `.env` format.
//! Save the output to a file and keep it SECURE — secret keys are shown only once.
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-keys > .env.local

fn main() {
    let labels = [
        ("ZION_MINER_ADDRESS", "Miner payout address"),
        ("ZION_HUMANITARIAN_WALLET", "Humanitarian tithe recipient"),
        ("ZION_ISSOBELLA_WALLET", "Issobella fund recipient"),
        ("ZION_POOL_FEE_WALLET", "Pool operator fee recipient"),
    ];

    println!("# ZION V3 — Local Test Mainnet Keys");
    println!("# Generated: {}", chrono_now());
    println!("# WARNING: These are FRESH keys for local testing ONLY.");
    println!("# For production mainnet, use canonical addresses from genesis.");
    println!();

    for (env_name, description) in &labels {
        let (_sk, vk) = zion_core::crypto::generate_keypair();
        let address = zion_core::crypto::derive_address(vk.as_bytes());
        assert!(
            zion_core::crypto::is_valid_address(&address),
            "Generated address failed validation: {address}"
        );
        println!("# {description}");
        println!("{env_name}={address}");
        println!();
    }

    // Pool wallet + payout keypair — one keypair, address + secret
    let (sk, vk) = zion_core::crypto::generate_keypair();
    let pool_wallet = zion_core::crypto::derive_address(vk.as_bytes());
    let secret_hex = zion_core::crypto::to_hex(sk.as_bytes());
    assert!(zion_core::crypto::is_valid_address(&pool_wallet));

    println!("# Pool operational wallet (payouts are signed with the SK below)");
    println!("ZION_POOL_WALLET={pool_wallet}");
    println!();
    println!("# CRITICAL: Pool payout signing key — keep SECRET!");
    println!("ZION_POOL_PAYOUT_SK_HEX={secret_hex}");
    println!();

    eprintln!("⚠️  SAVE THIS OUTPUT SECURELY — secret keys are shown only once!");
    eprintln!("⚠️  Copy the block above into your .env file for local testing.");
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dt = chrono::DateTime::from_timestamp(secs as i64, 0).unwrap();
    dt.to_rfc3339()
}
