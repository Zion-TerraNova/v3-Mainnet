//! Send an account-model transaction from a wallet (e.g. Bridge Seed Fund) to a
//! destination address.  **WARNING:** the bridge vault (`zion1w0r0...`)
//! only accepts UTXO deposits — an account-model tx to the vault will not
//! create a bridge lock event and is effectively unspendable.  Use this tool
//! only for account-model destinations (validators, relayers, etc.).
//!
//! Usage:
//!   ZION_WALLET_SK_HEX=<hex> ZION_RPC_ADDR=127.0.0.1:8443 cargo run --release --manifest-path V3/Cargo.toml -p zion-core --bin fund-bridge-vault

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::json;

fn main() {
    let sk_hex = std::env::var("ZION_WALLET_SK_HEX").expect("set ZION_WALLET_SK_HEX");
    let bytes = zion_core::crypto::from_hex(&sk_hex).expect("invalid hex");
    if bytes.len() != 32 {
        panic!("secret key must be 32 bytes, got {}", bytes.len());
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    let sk = SigningKey::from_bytes(&arr);

    let from = zion_core::crypto::derive_address(sk.verifying_key().as_bytes());
    let to = zion_core::fee::BRIDGE_VAULT_ADDRESS;
    let amount: u128 = 100_000_000_000_000_000_000; // 100M ZION in flowers
    let fee: u64 = 1_000;
    let nonce: u64 = 0;

    // Build a deterministic tx_id from the canonical pre-image.
    // The node accepts any 64-hex tx_id as long as the signature verifies.
    let preimage = format!("account_tx:{}:{}:{}:{}:{}", from, to, amount, fee, nonce);
    let tx_id_bytes = zion_core::crypto::blake3_hash(preimage.as_bytes());
    let tx_id = zion_core::crypto::to_hex(&tx_id_bytes);

    let sig = zion_core::crypto::sign(&sk, tx_id.as_bytes());
    let signature = zion_core::crypto::to_hex(&sig);
    let public_key = zion_core::crypto::to_hex(sk.verifying_key().as_bytes());

    let tx = zion_core::Transaction {
        tx_id,
        from,
        to: to.to_string(),
        amount_zion: amount,
        fee_zion: fee,
        nonce,
        signature,
        public_key,
        memo: None,
    };

    // Submit via RPC
    let rpc_addr = std::env::var("ZION_RPC_ADDR").unwrap_or_else(|_| "127.0.0.1:8443".into());
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "submitAccountTransaction",
        "params": { "transaction": tx }
    });

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
