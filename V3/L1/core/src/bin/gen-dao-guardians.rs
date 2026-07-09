//! Generate 7 DAO guardian Ed25519 keypairs for ZION V3 governance.
//!
//! DAO guardians vote on proposals (admin rotation, treasury spend, hard fork).
//! The guardian set is 7 addresses with a 5-of-7 threshold for DAO proposals.
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-dao-guardians > dao_guardians.json
//!
//! WARNING: Secret keys are shown ONCE. Encrypt and shred plaintext immediately.

fn main() {
    let guardians = [
        "Guardian-1",
        "Guardian-2",
        "Guardian-3",
        "Guardian-4",
        "Guardian-5",
        "Guardian-6",
        "Guardian-7",
    ];

    println!("{{");
    println!("  \"dao_guardians\": [");

    for (i, name) in guardians.iter().enumerate() {
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
}
