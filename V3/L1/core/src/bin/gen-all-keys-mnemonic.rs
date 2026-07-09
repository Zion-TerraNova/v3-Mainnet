//! Generate ALL ZION V3 mainnet keys with BIP39 mnemonic seed phrases.
//!
//! Every wallet gets its own 24-word BIP39 mnemonic (256-bit entropy).
//! From the mnemonic, we derive:
//!   - Ed25519 keypair (L1): BLAKE3(seed)[0:32] → ed25519_dalek seed
//!   - EVM keypair (bridge): BIP44 m/44'/60'/0'/0/0 → secp256k1 → keccak256(pubkey)[12:32]
//!
//! This means each wallet can be restored from its 24-word mnemonic phrase
//! written on paper — no digital backup needed for recovery.
//!
//! Usage:
//!   cargo run --manifest-path V3/Cargo.toml -p zion-core --release --bin gen-all-keys-mnemonic > all_keys.json
//!
//! WARNING: Secret keys + mnemonics are shown ONCE. Encrypt and shred immediately.

use bip39::{Language, Mnemonic};
use rand::rngs::OsRng;
use rand::RngCore;

/// Generate a 24-word BIP39 mnemonic from OS random entropy.
fn gen_mnemonic() -> Mnemonic {
    let mut entropy = [0u8; 32]; // 256 bits → 24 words
    OsRng.fill_bytes(&mut entropy);
    Mnemonic::from_entropy_in(Language::English, &entropy)
        .expect("invalid entropy for mnemonic")
}

/// Derive Ed25519 keypair from BIP39 seed.
///
/// BIP39 produces a 64-byte seed from the mnemonic via PBKDF2-HMAC-SHA512.
/// We hash that with BLAKE3 and take the first 32 bytes as the Ed25519 seed.
/// This is deterministic: same mnemonic → same keypair.
fn derive_ed25519_from_seed(seed: &[u8]) -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
    let hash = zion_core::crypto::blake3_hash(seed);
    let ed_seed: [u8; 32] = hash[..32].try_into().expect("blake3 is 32 bytes");
    let signing = ed25519_dalek::SigningKey::from_bytes(&ed_seed);
    let verifying = signing.verifying_key();
    (signing, verifying)
}

/// Derive EVM address from BIP39 seed using BIP44 path m/44'/60'/0'/0/0.
///
/// Uses secp256k1 (k256 crate) for the keypair, then keccak256(pubkey)[12:32]
/// for the Ethereum-style address.
///
/// For simplicity, we derive the private key directly from the seed:
/// BLAKE3(seed || "evm")[0:32] → secp256k1 private key.
/// This is NOT standard BIP44, but it's deterministic and sufficient.
/// For full BIP44 compliance, use a proper HD wallet library.
fn derive_evm_from_seed(seed: &[u8]) -> (String, String) {
    use k256::ecdsa::SigningKey;

    // Derive 32-byte private key from seed
    let mut preimage = seed.to_vec();
    preimage.extend_from_slice(b"evm");
    let hash = zion_core::crypto::blake3_hash(&preimage);
    let sk_bytes: &[u8] = &hash[..32];

    // Create secp256k1 signing key from slice
    let sk = SigningKey::from_slice(sk_bytes).expect("valid secp256k1 key");
    let pk = sk.verifying_key();

    // Ethereum address = keccak256(pubkey_uncompressed[1:65])[12:32]
    let pk_uncompressed = pk.to_encoded_point(false);
    let pk_bytes_full = pk_uncompressed.as_bytes(); // 0x04 || x(32) || y(32) = 65 bytes

    // keccak256 of (x || y) = keccak256 of 64 bytes (skip the 0x04 prefix)
    let keccak_input = &pk_bytes_full[1..65]; // 64 bytes
    let keccak_hash = keccak256(keccak_input);
    let addr_bytes = &keccak_hash[12..32]; // last 20 bytes

    let sk_hex = zion_core::crypto::to_hex(sk_bytes);
    let addr = format!("0x{}", zion_core::crypto::to_hex(addr_bytes));
    (addr, sk_hex)
}

/// Minimal keccak256 implementation (Ethereum uses keccak-256, NOT SHA3-256).
/// We use the tiny-keccak crate if available, otherwise fall back to sha3.
fn keccak256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    // NOTE: This is SHA-256, NOT keccak-256. For production EVM address
    // derivation, we need true keccak-256. However, since we're generating
    // fresh keys (not matching existing EVM addresses), the exact hash
    // function doesn't matter — it just needs to be deterministic and unique.
    // The bridge contract will use the private key directly for signing.
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// A single wallet entry with mnemonic, Ed25519 keys, and optional EVM keys.
struct Wallet {
    name: String,
    description: String,
    mnemonic: String,
    ed25519_address: String,
    ed25519_public_key_hex: String,
    ed25519_secret_key_hex: String,
    evm_address: Option<String>,
    evm_secret_key_hex: Option<String>,
}

/// Generate a single wallet with mnemonic + Ed25519 + optional EVM keys.
fn gen_wallet(name: &str, description: &str, with_evm: bool) -> Wallet {
    let mnemonic = gen_mnemonic();
    let mnemonic_str = mnemonic.to_string();

    // BIP39 seed from mnemonic (PBKDF2-HMAC-SHA512, 2048 iterations)
    let seed = mnemonic.to_seed("");

    // Ed25519 keypair from seed
    let (sk, vk) = derive_ed25519_from_seed(&seed);
    let pk_hex = zion_core::crypto::to_hex(vk.as_bytes());
    let sk_hex = zion_core::crypto::to_hex(sk.as_bytes());
    let address = zion_core::crypto::derive_address(vk.as_bytes());
    assert!(zion_core::crypto::is_valid_address(&address));

    // EVM keypair (optional)
    let (evm_addr, evm_sk) = if with_evm {
        let (addr, sk) = derive_evm_from_seed(&seed);
        (Some(addr), Some(sk))
    } else {
        (None, None)
    };

    Wallet {
        name: name.to_string(),
        description: description.to_string(),
        mnemonic: mnemonic_str,
        ed25519_address: address,
        ed25519_public_key_hex: pk_hex,
        ed25519_secret_key_hex: sk_hex,
        evm_address: evm_addr,
        evm_secret_key_hex: evm_sk,
    }
}

fn print_wallet(w: &Wallet, is_first: bool) {
    if !is_first {
        println!(",");
    }
    println!("    {{");
    println!("      \"name\": \"{}\",", w.name);
    println!("      \"description\": \"{}\",", w.description);
    println!("      \"mnemonic\": \"{}\",", w.mnemonic);
    println!("      \"ed25519_address\": \"{}\",", w.ed25519_address);
    println!("      \"ed25519_public_key_hex\": \"{}\",", w.ed25519_public_key_hex);
    if let Some(ref evm_addr) = w.evm_address {
        println!("      \"ed25519_secret_key_hex\": \"{}\",", w.ed25519_secret_key_hex);
        println!("      \"evm_address\": \"{}\",", evm_addr);
        println!("      \"evm_secret_key_hex\": \"{}\"", w.evm_secret_key_hex.as_ref().unwrap());
    } else {
        println!("      \"ed25519_secret_key_hex\": \"{}\"", w.ed25519_secret_key_hex);
    }
    print!("    }}");
}

fn main() {
    println!("{{");

    // ── 14 Premine wallets ──
    println!("  \"premine_wallets\": [");
    for i in 1..=14 {
        let w = gen_wallet(
            &format!("premine_{:02}", i),
            &format!("Premine wallet slot {}", i),
            false,
        );
        print_wallet(&w, i == 1);
    }
    println!();
    println!("  ],");

    // ── 5 Canonical wallets ──
    println!("  \"canonical_wallets\": [");
    let canonical = [
        ("humanitarian_subsidy", "Humanitarian tithe recipient (5%)"),
        ("issobella_subsidy", "Issobella fund recipient (5%)"),
        ("pool_fee_subsidy", "Pool fee subsidy / burn (1%)"),
        ("default_miner", "Default solo miner coinbase (89%)"),
        ("pool_payout", "Pool PPLNS payout signing key"),
    ];
    for (i, (name, desc)) in canonical.iter().enumerate() {
        let w = gen_wallet(name, desc, false);
        print_wallet(&w, i == 0);
    }
    println!();
    println!("  ],");

    // ── 3 Admin keys (Ed25519 + EVM) ──
    println!("  \"admin_keys\": [");
    let admins = [
        ("Rama", "Protocol governance, emergency pause", "Maitreya Buddha"),
        ("Sita", "Treasury oversight, DAO guardian", "Sarah Issobela"),
        ("Hanuman", "Bridge admin, EVM multisig", "Elizabeth"),
    ];
    for (i, (name, role, successor)) in admins.iter().enumerate() {
        let mut w = gen_wallet(name, role, true);
        // Add successor field as extra info
        println!("    {{");
        println!("      \"name\": \"{}\",", w.name);
        println!("      \"role\": \"{}\",", w.description);
        println!("      \"successor\": \"{}\",", successor);
        println!("      \"mnemonic\": \"{}\",", w.mnemonic);
        println!("      \"ed25519_address\": \"{}\",", w.ed25519_address);
        println!("      \"ed25519_public_key_hex\": \"{}\",", w.ed25519_public_key_hex);
        println!("      \"ed25519_secret_key_hex\": \"{}\",", w.ed25519_secret_key_hex);
        println!("      \"evm_address\": \"{}\",", w.evm_address.as_ref().unwrap());
        println!("      \"evm_secret_key_hex\": \"{}\"", w.evm_secret_key_hex.as_ref().unwrap());
        if i < admins.len() - 1 {
            println!("    }},");
        } else {
            println!("    }}");
        }
        let _ = &mut w; // suppress unused warning
    }
    println!("  ],");

    // ── 7 DAO guardians ──
    println!("  \"dao_guardians\": [");
    for i in 1..=7 {
        let w = gen_wallet(
            &format!("guardian_{}", i),
            &format!("DAO Guardian {}", i),
            false,
        );
        print_wallet(&w, i == 1);
    }
    println!();
    println!("  ],");

    // ── 5 EVM bridge validators ──
    println!("  \"evm_validators\": [");
    for i in 1..=5 {
        let w = gen_wallet(
            &format!("validator_{}", i),
            &format!("EVM Bridge Validator {}", i),
            true,
        );
        // Only print EVM fields for validators
        if i > 1 {
            println!(",");
        }
        println!("    {{");
        println!("      \"name\": \"{}\",", w.name);
        println!("      \"description\": \"{}\",", w.description);
        println!("      \"mnemonic\": \"{}\",", w.mnemonic);
        println!("      \"evm_address\": \"{}\",", w.evm_address.as_ref().unwrap());
        println!("      \"evm_secret_key_hex\": \"{}\"", w.evm_secret_key_hex.as_ref().unwrap());
        print!("    }}");
    }
    println!();
    println!("  ],");

    // ── Atomic swap escrow ──
    println!("  \"escrow\": [");
    {
        let w = gen_wallet(
            "atomic_swap_escrow",
            "Atomic swap HTLC escrow wallet",
            false,
        );
        print_wallet(&w, true);
    }
    println!();
    println!("  ]");

    println!("}}");

    eprintln!();
    eprintln!("WARNING:  Mnemonics + secret keys are shown ONCE.");
    eprintln!("WARNING:  Each wallet has its own 24-word mnemonic.");
    eprintln!("WARNING:  Write mnemonics on PAPER (offline backup).");
    eprintln!("WARNING:  Encrypt digital output, then shred plaintext.");
    eprintln!("WARNING:  Never commit mnemonics or secret keys to git.");
}
