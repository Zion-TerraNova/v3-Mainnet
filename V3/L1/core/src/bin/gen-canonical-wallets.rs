//! Generate 5 fresh OS-random canonical wallet keypairs for ZION V3 mainnet.
//!
//! These are the canonical wallets referenced in genesis.rs:
//!   1. Humanitarian subsidy (5% of coinbase)
//!   2. Issobella subsidy (5% of coinbase)
//!   3. Pool fee subsidy (1% burn — SK not strictly needed but generated for completeness)
//!   4. Default miner (89% fallback)
//!   5. Pool payout signer (PPLNS payout signing key)
//!
//! All keys are OS-random (not label-derived) for the hard genesis reset.
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-canonical-wallets > canonical_wallets.json
//!
//! WARNING: Secret keys are shown ONCE. Encrypt and shred plaintext immediately.

fn main() {
    let wallets = [
        ("humanitarian_subsidy", "Humanitarian tithe recipient (5% of coinbase)"),
        ("issobella_subsidy", "Issobella fund recipient (5% of coinbase)"),
        ("pool_fee_subsidy", "Pool fee subsidy / burn address (1%)"),
        ("default_miner", "Default solo miner coinbase fallback (89%)"),
        ("pool_payout", "Pool PPLNS payout signing key"),
    ];

    println!("{{");
    println!("  \"canonical_wallets\": [");

    for (i, (name, description)) in wallets.iter().enumerate() {
        let (sk, vk) = zion_core::crypto::generate_keypair();
        let pk_hex = zion_core::crypto::to_hex(vk.as_bytes());
        let sk_hex = zion_core::crypto::to_hex(sk.as_bytes());
        let address = zion_core::crypto::derive_address(vk.as_bytes());
        assert!(zion_core::crypto::is_valid_address(&address));

        if i > 0 {
            println!(",");
        }
        println!("    {{");
        println!("      \"name\": \"{name}\",");
        println!("      \"description\": \"{description}\",");
        println!("      \"address\": \"{address}\",");
        println!("      \"public_key_hex\": \"{pk_hex}\",");
        println!("      \"secret_key_hex\": \"{sk_hex}\"");
        print!("    }}");
    }

    println!();
    println!("  ]");
    println!("}}");

    eprintln!();
    eprintln!("WARNING:  Secret keys are shown ONCE.");
    eprintln!("WARNING:  Encrypt output immediately, then shred plaintext.");
    eprintln!("WARNING:  Store encrypted backup on offline media (USB).");
    eprintln!("WARNING:  Never commit secret keys to git.");
}
