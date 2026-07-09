//! Migrate atomic-swap escrow funds to a new keypair.
//!
//! Generates a fresh Ed25519 keypair, builds and signs an account-model
//! transaction from the OLD escrow address (using the OLD SK) to the NEW
//! escrow address, and submits it via RPC. Prints the NEW SK + address
//! to stderr so the operator can save it.
//!
//! Usage:
//!   ZION_OLD_ESCROW_SK_HEX=<hex> ZION_RPC_ADDR=127.0.0.1:8443 \
//!     cargo run --release --manifest-path V3/Cargo.toml -p zion-core --bin migrate-escrow

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::json;

fn main() {
    // ── 1. Load old SK ──────────────────────────────────────────────────
    let sk_hex = std::env::var("ZION_OLD_ESCROW_SK_HEX")
        .expect("set ZION_OLD_ESCROW_SK_HEX (old escrow secret key hex)");
    let bytes = zion_core::crypto::from_hex(&sk_hex).expect("invalid hex");
    if bytes.len() != 32 {
        panic!("secret key must be 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let old_sk = SigningKey::from_bytes(&arr);
    let old_address = zion_core::crypto::derive_address(old_sk.verifying_key().as_bytes());

    eprintln!("OLD escrow address: {old_address}");

    // ── 2. Generate new keypair ─────────────────────────────────────────
    let (new_sk, new_vk) = zion_core::crypto::generate_keypair();
    let new_address = zion_core::crypto::derive_address(new_vk.as_bytes());
    let new_sk_hex = zion_core::crypto::to_hex(new_sk.as_bytes());

    eprintln!("NEW escrow address: {new_address}");
    eprintln!("NEW escrow SK hex:  {new_sk_hex}");
    eprintln!();
    eprintln!("⚠️  SAVE THE NEW SK NOW — it will not be shown again!");
    eprintln!();

    // ── 3. Build the migration TX ───────────────────────────────────────
    // Amount: send everything. Balance is 100,002,001,000 flowers.
    // fee_zion=1000 flowers (0.001 ZION). amount=100,002,000,000 flowers (100,002 ZION).
    // 100,002,000,000 + 1,000 = 100,002,001,000 = full balance.
    let amount: u128 = 100_002_000_000;
    let fee: u64 = 1_000;
    let nonce: u64 = 0; // escrow has never sent a TX

    let preimage = format!(
        "account_tx:{}:{}:{}:{}:{}",
        old_address, new_address, amount, fee, nonce
    );
    let tx_id_bytes = zion_core::crypto::blake3_hash(preimage.as_bytes());
    let tx_id = zion_core::crypto::to_hex(&tx_id_bytes);

    let sig = zion_core::crypto::sign(&old_sk, tx_id.as_bytes());
    let signature = zion_core::crypto::to_hex(&sig);
    let public_key = zion_core::crypto::to_hex(old_sk.verifying_key().as_bytes());

    eprintln!("TX details:");
    eprintln!("  from:       {old_address}");
    eprintln!("  to:         {new_address}");
    eprintln!("  amount:     {amount} flowers (100,002 ZION)");
    eprintln!("  fee:        {fee} flowers (0.001 ZION)");
    eprintln!("  nonce:      {nonce}");
    eprintln!("  tx_id:      {tx_id}");
    eprintln!();

    let tx = zion_core::Transaction {
        tx_id,
        from: old_address,
        to: new_address,
        amount_zion: amount,
        fee_zion: fee,
        nonce,
        signature,
        public_key,
        memo: Some("escrow_key_migration".to_string()),
    };

    // ── 4. Submit via RPC ───────────────────────────────────────────────
    let rpc_addr = std::env::var("ZION_RPC_ADDR").unwrap_or_else(|_| "127.0.0.1:8443".into());
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "submitAccountTransaction",
        "params": { "transaction": tx }
    });

    eprintln!("Submitting to RPC at {rpc_addr}...");

    let mut stream = TcpStream::connect(&rpc_addr)
        .unwrap_or_else(|e| panic!("cannot connect to {rpc_addr}: {e}"));
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

    let req = serde_json::to_string(&payload).unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();

    let reader = BufReader::new(&stream);
    for line in reader.lines() {
        let line = line.unwrap();
        if line.trim().is_empty() {
            continue;
        }
        println!("{}", line);
        break;
    }
}
