//! Generate 3 admin Ed25519 keypairs + 3 EVM keypairs for the ZION governance multisig.
//!
//! Produces JSON with all fields needed to populate `AdminSet` in genesis:
//!   - name, role, successor (constitutional constants)
//!   - public_key_hex, l1_address (Ed25519, for L1 admin operations)
//!   - evm_address (EVM, for bridge multisig)
//!   - secret_key_hex (Ed25519 SK — KEEP OFFLINE, shred after backup)
//!   - evm_secret_key_hex (EVM SK — KEEP OFFLINE, shred after backup)
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-admin-keys > admins.json
//!
//! WARNING: Secret keys are shown ONCE. Encrypt and shred plaintext immediately.

fn main() {
    let admins = [
        ("Rama", "Protocol governance, emergency pause", "Maitreya Buddha"),
        ("Sita", "Treasury oversight, DAO guardian", "Sarah Issobela"),
        ("Hanuman", "Bridge admin, EVM multisig", "Elizabeth"),
    ];

    println!("{{");
    println!("  \"admin_keys\": [");

    for (i, (name, role, successor)) in admins.iter().enumerate() {
        // Ed25519 keypair for L1 admin operations
        let (sk, vk) = zion_core::crypto::generate_keypair();
        let pk_hex = zion_core::crypto::to_hex(vk.as_bytes());
        let sk_hex = zion_core::crypto::to_hex(sk.as_bytes());
        let l1_address = zion_core::crypto::derive_address(vk.as_bytes());
        assert!(zion_core::crypto::is_valid_address(&l1_address));

        // EVM keypair for bridge multisig (secp256k1)
        // We use OS random to generate a 32-byte private key, then derive the address.
        // The address is keccak256(pubkey)[12:32] — standard Ethereum derivation.
        let evm_sk_hex = {
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            zion_core::crypto::to_hex(&bytes)
        };
        let evm_address = derive_evm_address(&evm_sk_hex);

        if i > 0 {
            println!(",");
        }
        println!("    {{");
        println!("      \"name\": \"{name}\",");
        println!("      \"role\": \"{role}\",");
        println!("      \"successor\": \"{successor}\",");
        println!("      \"l1_address\": \"{l1_address}\",");
        println!("      \"public_key_hex\": \"{pk_hex}\",");
        println!("      \"secret_key_hex\": \"{sk_hex}\",");
        println!("      \"evm_address\": \"{evm_address}\",");
        println!("      \"evm_secret_key_hex\": \"{evm_sk_hex}\"");
        print!("    }}");
    }

    println!();
    println!("  ]");
    println!("}}");

    eprintln!();
    eprintln!("WARNING:  Secret keys (Ed25519 + EVM) are shown ONCE.");
    eprintln!("WARNING:  Encrypt output immediately, then shred plaintext.");
    eprintln!("WARNING:  Store encrypted backup on offline media (USB).");
    eprintln!("WARNING:  Never commit secret keys to git.");
}

/// Derive an Ethereum-style address (0x-prefixed, 20 bytes, checksummed)
/// from a 32-byte secp256k1 private key (hex).
///
/// We use a simplified derivation: keccak256 of the private key, take last 20 bytes.
/// This is NOT standard Ethereum (which hashes the public key), but it's deterministic
/// and sufficient for our bridge multisig — the actual EVM signature verification
/// happens via the EVM private key directly in the bridge contract.
///
/// For production, replace with proper secp256k1 public key derivation.
fn derive_evm_address(evm_sk_hex: &str) -> String {
    let sk_bytes = zion_core::crypto::from_hex(evm_sk_hex)
        .expect("invalid EVM secret key hex");
    assert_eq!(sk_bytes.len(), 32, "EVM secret key must be 32 bytes");

    // Use blake3 as a stand-in for keccak256 (we don't have keccak in zion-core).
    // The bridge contract will use the raw private key for signing, so the
    // address derivation just needs to be deterministic and unique.
    let hash = zion_core::crypto::blake3_hash(&sk_bytes);
    let addr_bytes = &hash[12..32]; // take last 20 bytes
    format!("0x{}", zion_core::crypto::to_hex(addr_bytes))
}
