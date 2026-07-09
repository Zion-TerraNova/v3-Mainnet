//! Generate 5 EVM validator keypairs for the ZION bridge multisig.
//!
//! Bridge validators sign bridge unlock transactions on the EVM side.
//! The validator set is 5 EVM addresses with a 3-of-5 threshold.
//!
//! Additionally generates 1 escrow keypair for the atomic swap escrow.
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-evm-validators > evm_validators.json
//!
//! WARNING: Secret keys are shown ONCE. Encrypt and shred plaintext immediately.

fn main() {
    use rand::RngCore;

    println!("{{");

    // 5 EVM bridge validators
    println!("  \"evm_validators\": [");
    for i in 0..5 {
        let mut sk_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut sk_bytes);
        let sk_hex = zion_core::crypto::to_hex(&sk_bytes);
        let hash = zion_core::crypto::blake3_hash(&sk_bytes);
        let addr = format!("0x{}", zion_core::crypto::to_hex(&hash[12..32]));

        if i > 0 {
            println!(",");
        }
        println!("    {{");
        println!("      \"name\": \"Validator-{}\",", i + 1);
        println!("      \"evm_address\": \"{addr}\",");
        println!("      \"evm_secret_key_hex\": \"{sk_hex}\"");
        print!("    }}");
    }
    println!();
    println!("  ],");

    // 1 atomic swap escrow keypair (Ed25519 for L1 HTLC)
    let (esk, evk) = zion_core::crypto::generate_keypair();
    let epk_hex = zion_core::crypto::to_hex(evk.as_bytes());
    let esk_hex = zion_core::crypto::to_hex(esk.as_bytes());
    let eaddr = zion_core::crypto::derive_address(evk.as_bytes());
    assert!(zion_core::crypto::is_valid_address(&eaddr));

    println!("  \"escrow\": {{");
    println!("    \"name\": \"atomic_swap_escrow\",");
    println!("    \"description\": \"Atomic swap HTLC escrow wallet\",");
    println!("    \"address\": \"{eaddr}\",");
    println!("    \"public_key_hex\": \"{epk_hex}\",");
    println!("    \"secret_key_hex\": \"{esk_hex}\"");
    println!("  }}");

    println!("}}");

    eprintln!();
    eprintln!("WARNING:  Secret keys (EVM + Ed25519) are shown ONCE.");
    eprintln!("WARNING:  Encrypt output immediately, then shred plaintext.");
}
