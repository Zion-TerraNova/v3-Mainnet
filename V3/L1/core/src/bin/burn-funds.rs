//! Burn account-model funds by sending them to a provably-unspendable address.
//!
//! The burn address is derived from `[0xFF; 32]` — not a valid Ed25519 public
//! key, so nobody can produce a signature for it. Funds sent there are
//! permanently locked.
//!
//! Usage:
//!   ZION_BURN_SK_HEX=<hex> ZION_RPC_ADDR=127.0.0.1:8443 \
//!     cargo run --release --manifest-path V3/Cargo.toml -p zion-core --bin burn-funds

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::json;

fn main() {
    let sk_hex = std::env::var("ZION_BURN_SK_HEX")
        .expect("set ZION_BURN_SK_HEX (secret key hex of the source address)");
    let amount: u128 = std::env::var("ZION_BURN_AMOUNT")
        .expect("set ZION_BURN_AMOUNT (flowers to burn)")
        .parse()
        .expect("invalid amount");

    let bytes = zion_core::crypto::from_hex(&sk_hex).expect("invalid hex");
    if bytes.len() != 32 {
        panic!("secret key must be 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let sk = SigningKey::from_bytes(&arr);
    let from = zion_core::crypto::derive_address(sk.verifying_key().as_bytes());

    // Burn address: derived from [0xFF; 32] — not a valid Ed25519 public key.
    let burn_address = zion_core::crypto::derive_address(&[0xFF; 32]);
    eprintln!("Source address:  {from}");
    eprintln!("Burn address:    {burn_address}");
    eprintln!("Amount to burn:  {amount} flowers");

    let fee: u64 = 1_000;
    // Nonce: 1 (nonce 0 was used for the migration TX)
    let nonce: u64 = std::env::var("ZION_BURN_NONCE")
        .unwrap_or_else(|_| "1".into())
        .parse()
        .expect("invalid nonce");

    let preimage = format!(
        "account_tx:{}:{}:{}:{}:{}",
        from, burn_address, amount, fee, nonce
    );
    let tx_id_bytes = zion_core::crypto::blake3_hash(preimage.as_bytes());
    let tx_id = zion_core::crypto::to_hex(&tx_id_bytes);

    let sig = zion_core::crypto::sign(&sk, tx_id.as_bytes());
    let signature = zion_core::crypto::to_hex(&sig);
    let public_key = zion_core::crypto::to_hex(sk.verifying_key().as_bytes());

    eprintln!("TX: from={from} to={burn_address} amount={amount} fee={fee} nonce={nonce}");
    eprintln!("tx_id: {tx_id}");
    eprintln!();

    let tx = zion_core::Transaction {
        tx_id,
        from,
        to: burn_address,
        amount_zion: amount,
        fee_zion: fee,
        nonce,
        signature,
        public_key,
        memo: Some("burn_inflationary_funds".to_string()),
    };

    let rpc_addr = std::env::var("ZION_RPC_ADDR").unwrap_or_else(|_| "127.0.0.1:8443".into());
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "submitAccountTransaction",
        "params": { "transaction": tx }
    });

    eprintln!("Submitting burn TX to RPC at {rpc_addr}...");

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
