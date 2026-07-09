//! ZION L1 Wallet CLI — key management, balance queries, send, and bridge lock.
//!
//! Usage:
//!   wallet info                             — show address from signing key
//!   wallet balance [address]                — query balance (default: own address)
//!   wallet utxos   [address]                — list spendable UTXOs
//!   wallet send    <to> <amount_zion>       — send ZION
//!   wallet bridge-lock <evm_recipient> <amount_zion> [--chain base]
//!                                           — lock ZION to bridge vault
//!
//! Configuration (env vars):
//!   ZION_WALLET_SK_HEX  — Ed25519 secret key hex (64 hex chars)
//!   ZION_WALLET_KEY_FILE — path to file containing secret key hex
//!   ZION_RPC_ADDR       — node RPC address (default: 127.0.0.1:8443)

use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
    }

    match args[1].as_str() {
        "info" => cmd_info(),
        "balance" => cmd_balance(args.get(2).map(|s| s.as_str())),
        "utxos" => cmd_utxos(args.get(2).map(|s| s.as_str())),
        "send" => {
            if args.len() < 4 {
                eprintln!("Usage: wallet send <to_address> <amount_zion> [--fee <fee_flowers>] [--memo <memo>]");
                process::exit(1);
            }
            let fee = parse_flag(&args, "--fee")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1_000);
            let memo = parse_flag(&args, "--memo");
            cmd_send(&args[2], &args[3], fee, memo);
        }
        "bridge-lock" => {
            if args.len() < 4 {
                eprintln!(
                    "Usage: wallet bridge-lock <evm_recipient> <amount_zion> [--chain <chain>]"
                );
                process::exit(1);
            }
            let chain = parse_flag(&args, "--chain").unwrap_or_else(|| "base".to_string());
            cmd_bridge_lock(&args[2], &args[3], &chain);
        }
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("ZION L1 Wallet CLI");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  info                                  Show wallet address");
    eprintln!("  balance [address]                     Query balance");
    eprintln!("  utxos   [address]                     List spendable UTXOs");
    eprintln!("  send <to> <amount_zion> [--fee N]     Send ZION");
    eprintln!("  bridge-lock <evm_addr> <amount_zion> [--chain C]  Lock to bridge vault");
    eprintln!();
    eprintln!("Environment:");
    eprintln!("  ZION_WALLET_SK_HEX   Secret key (hex)");
    eprintln!("  ZION_WALLET_KEY_FILE File containing secret key hex");
    eprintln!("  ZION_RPC_ADDR        Node RPC address (default: 127.0.0.1:8443)");
    process::exit(1);
}

fn parse_flag(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

// ── Key loading ────────────────────────────────────────────────────────

fn load_signing_key() -> SigningKey {
    let sk_hex = if let Ok(hex) = env::var("ZION_WALLET_SK_HEX") {
        hex.trim().to_string()
    } else if let Ok(path) = env::var("ZION_WALLET_KEY_FILE") {
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| die(&format!("cannot read key file {path}: {e}")))
            .trim()
            .to_string()
    } else {
        die("set ZION_WALLET_SK_HEX or ZION_WALLET_KEY_FILE");
    };

    let bytes =
        zion_core::crypto::from_hex(&sk_hex).unwrap_or_else(|| die("invalid hex in secret key"));
    if bytes.len() != 32 {
        die(&format!("secret key must be 32 bytes, got {}", bytes.len()));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&bytes);
    SigningKey::from_bytes(&arr)
}

fn own_address(sk: &SigningKey) -> String {
    zion_core::crypto::derive_address(sk.verifying_key().as_bytes())
}

// ── RPC ────────────────────────────────────────────────────────────────

fn rpc_addr() -> String {
    env::var("ZION_RPC_ADDR").unwrap_or_else(|_| "127.0.0.1:8443".into())
}

fn rpc_call(method: &str, params: Value) -> Value {
    let addr = rpc_addr();
    let mut stream = TcpStream::connect(&addr)
        .unwrap_or_else(|e| die(&format!("cannot connect to {addr}: {e}")));
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();

    let request = json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": 1,
    });
    let mut line = serde_json::to_string(&request).expect("json serialize");
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .unwrap_or_else(|e| die(&format!("write to {addr}: {e}")));
    stream.flush().ok();

    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .unwrap_or_else(|e| die(&format!("read from {addr}: {e}")));

    let resp: Value = serde_json::from_str(&resp_line)
        .unwrap_or_else(|e| die(&format!("invalid JSON response: {e}")));

    if let Some(err) = resp.get("error") {
        die(&format!("RPC error: {err}"));
    }
    resp["result"].clone()
}

fn rpc_chain_tip_height() -> u64 {
    let info = rpc_call("getChainInfo", json!(null));
    info["chain_height"].as_u64().unwrap_or(0)
}

// ── Helpers ────────────────────────────────────────────────────────────

const FLOWERS_PER_ZION: u64 = zion_core::emission::FLOWERS_PER_ZION;

fn parse_zion_amount(s: &str) -> u64 {
    if let Some((whole, frac)) = s.split_once('.') {
        let whole_flowers: u64 = whole
            .parse::<u64>()
            .unwrap_or_else(|_| die("invalid amount"))
            * FLOWERS_PER_ZION;
        let padded = format!("{:0<6}", frac);
        if padded.len() > 6 {
            die("amount has too many decimal places (max 6)");
        }
        let frac_flowers: u64 = padded[..6]
            .parse()
            .unwrap_or_else(|_| die("invalid fractional amount"));
        whole_flowers + frac_flowers
    } else {
        s.parse::<u64>().unwrap_or_else(|_| die("invalid amount")) * FLOWERS_PER_ZION
    }
}

fn format_zion(flowers: u64) -> String {
    let whole = flowers / FLOWERS_PER_ZION;
    let frac = flowers % FLOWERS_PER_ZION;
    if frac == 0 {
        format!("{whole}")
    } else {
        let s = format!("{whole}.{frac:06}");
        s.trim_end_matches('0').to_string()
    }
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1);
}

// ── Commands ───────────────────────────────────────────────────────────

fn cmd_info() {
    let sk = load_signing_key();
    let address = own_address(&sk);
    let pk_hex = zion_core::crypto::to_hex(sk.verifying_key().as_bytes());
    println!("address:    {address}");
    println!("public_key: {pk_hex}");
}

fn cmd_balance(address: Option<&str>) {
    let addr = match address {
        Some(a) => a.to_string(),
        None => {
            let sk = load_signing_key();
            own_address(&sk)
        }
    };
    let result = rpc_call("getBalance", json!({ "address": addr }));
    let flowers = result["balance_flowers"].as_u64().unwrap_or(0);
    let utxo = result["utxo_balance_flowers"].as_u64().unwrap_or(0);
    let total = flowers + utxo;
    println!("address:     {addr}");
    println!("account:     {} ZION", format_zion(flowers));
    println!("utxo:        {} ZION", format_zion(utxo));
    println!("total:       {} ZION", format_zion(total));
}

fn cmd_utxos(address: Option<&str>) {
    let addr = match address {
        Some(a) => a.to_string(),
        None => {
            let sk = load_signing_key();
            own_address(&sk)
        }
    };
    let result = rpc_call("getUtxos", json!({ "address": addr }));
    let utxos = result["utxos"].as_array().cloned().unwrap_or_default();
    let total = result["total_amount"].as_u64().unwrap_or(0);
    println!("address: {addr}");
    println!("utxos:   {}", utxos.len());
    println!("total:   {} ZION", format_zion(total));
    for u in &utxos {
        println!(
            "  {}:{} — {} ZION (h={})",
            u["tx_hash"].as_str().unwrap_or("?"),
            u["output_index"],
            format_zion(u["amount"].as_u64().unwrap_or(0)),
            u["height"],
        );
    }
}

fn cmd_send(to: &str, amount_str: &str, fee: u64, memo: Option<String>) {
    let sk = load_signing_key();
    let address = own_address(&sk);
    let amount_flowers = parse_zion_amount(amount_str);

    println!("from:   {address}");
    println!("to:     {to}");
    println!(
        "amount: {} ZION ({amount_flowers} flowers)",
        format_zion(amount_flowers)
    );
    println!("fee:    {fee} flowers");
    if let Some(ref m) = memo {
        println!("memo:   {m}");
    }

    // Fetch UTXOs
    let utxo_result = rpc_call("getUtxos", json!({ "address": address }));
    let utxo_list = utxo_result["utxos"].as_array().cloned().unwrap_or_default();
    if utxo_list.is_empty() {
        die("no spendable UTXOs for this address");
    }

    let available: Vec<zion_core::wallet::SpendableUtxo> = utxo_list
        .iter()
        .map(|u| {
            let hash_hex = u["tx_hash"].as_str().unwrap_or("");
            let hash_bytes = zion_core::crypto::from_hex(hash_hex).unwrap_or_else(|| vec![0u8; 32]);
            let mut arr = [0u8; 32];
            let len = hash_bytes.len().min(32);
            arr[..len].copy_from_slice(&hash_bytes[..len]);
            zion_core::wallet::SpendableUtxo {
                tx_hash: arr,
                output_index: u["output_index"].as_u64().unwrap_or(0) as u32,
                amount: u["amount"].as_u64().unwrap_or(0),
                address: address.clone(),
            }
        })
        .collect();

    let chain_tip = rpc_chain_tip_height();

    let params = zion_core::wallet::SendParams {
        to_address: to.to_string(),
        amount: amount_flowers,
        fee,
        memo,
    };

    let result = zion_core::wallet::build_and_sign(&sk, &address, &params, &available, chain_tip)
        .unwrap_or_else(|e| die(&format!("build failed: {e}")));

    // Submit
    let tx_json = serde_json::to_value(&result.transaction).expect("serialize tx");
    let submit = rpc_call("sendRawTransaction", json!({ "transaction": tx_json }));

    let tx_id = zion_core::crypto::to_hex(&result.transaction.id);
    if submit["accepted"].as_bool() == Some(true) {
        println!("submitted: {tx_id}");
        if result.change_amount > 0 {
            println!("change:    {} ZION", format_zion(result.change_amount));
        }
    } else {
        die(&format!("rejected: {}", submit));
    }
}

fn cmd_bridge_lock(evm_recipient: &str, amount_str: &str, chain: &str) {
    let sk = load_signing_key();
    let address = own_address(&sk);
    let amount_flowers = parse_zion_amount(amount_str);
    let fee: u64 = 1_000; // MIN_TX_FEE
    let vault = zion_core::fee::BRIDGE_VAULT_ADDRESS;
    let memo = format!("BRIDGE:{chain}:{evm_recipient}");

    println!("from:      {address}");
    println!("vault:     {vault}");
    println!(
        "amount:    {} ZION ({amount_flowers} flowers)",
        format_zion(amount_flowers)
    );
    println!("chain:     {chain}");
    println!("recipient: {evm_recipient}");
    println!("memo:      {memo}");

    // Fetch UTXOs
    let utxo_result = rpc_call("getUtxos", json!({ "address": address }));
    let utxo_list = utxo_result["utxos"].as_array().cloned().unwrap_or_default();
    if utxo_list.is_empty() {
        die("no spendable UTXOs for this address");
    }

    let available: Vec<zion_core::wallet::SpendableUtxo> = utxo_list
        .iter()
        .map(|u| {
            let hash_hex = u["tx_hash"].as_str().unwrap_or("");
            let hash_bytes = zion_core::crypto::from_hex(hash_hex).unwrap_or_else(|| vec![0u8; 32]);
            let mut arr = [0u8; 32];
            let len = hash_bytes.len().min(32);
            arr[..len].copy_from_slice(&hash_bytes[..len]);
            zion_core::wallet::SpendableUtxo {
                tx_hash: arr,
                output_index: u["output_index"].as_u64().unwrap_or(0) as u32,
                amount: u["amount"].as_u64().unwrap_or(0),
                address: address.clone(),
            }
        })
        .collect();

    // Build a transaction sending to vault with BRIDGE memo
    // We need to build manually since build_and_sign doesn't support memos
    let target = amount_flowers
        .checked_add(fee)
        .unwrap_or_else(|| die("amount overflow"));
    let (selected, total) = select_utxos_largest_first(&available, target);
    let change = total - target;

    let tip = rpc_chain_tip_height();

    let vk = sk.verifying_key();
    let mut outputs = vec![zion_core::tx::TxOutput {
        amount: amount_flowers,
        address: vault.to_string(),
        memo: Some(memo),
    }];
    if change > 0 {
        outputs.push(zion_core::tx::TxOutput {
            amount: change,
            address: address.clone(),
            memo: None,
        });
    }

    let inputs: Vec<zion_core::tx::TxInput> = selected
        .iter()
        .map(|utxo| zion_core::tx::TxInput {
            prev_tx_hash: utxo.tx_hash,
            output_index: utxo.output_index,
            signature: vec![],
            public_key: vk.as_bytes().to_vec(),
        })
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut tx = zion_core::tx::Transaction {
        id: [0u8; 32],
        version: zion_core::wallet::pending_utxo_tx_version(tip),
        inputs,
        outputs,
        fee,
        timestamp: now,
    };
    tx.finalize_id();

    // Sign inputs
    for input in &mut tx.inputs {
        let sig = zion_core::crypto::sign_and_zeroize(sk.to_bytes(), &tx.id)
            .unwrap_or_else(|_| die("signing failed"));
        input.signature = sig.to_vec();
    }

    // Submit
    let tx_json = serde_json::to_value(&tx).expect("serialize tx");
    let submit = rpc_call("sendRawTransaction", json!({ "transaction": tx_json }));

    let tx_id = zion_core::crypto::to_hex(&tx.id);
    if submit["accepted"].as_bool() == Some(true) {
        println!("submitted: {tx_id}");
        if change > 0 {
            println!("change:    {} ZION", format_zion(change));
        }
    } else {
        die(&format!("rejected: {}", submit));
    }
}

fn select_utxos_largest_first(
    available: &[zion_core::wallet::SpendableUtxo],
    target: u64,
) -> (Vec<&zion_core::wallet::SpendableUtxo>, u64) {
    let mut sorted: Vec<&zion_core::wallet::SpendableUtxo> = available.iter().collect();
    sorted.sort_by(|a, b| b.amount.cmp(&a.amount));
    let mut selected = Vec::new();
    let mut total: u64 = 0;
    for utxo in sorted {
        selected.push(utxo);
        total = total.saturating_add(utxo.amount);
        if total >= target {
            return (selected, total);
        }
    }
    die(&format!("insufficient funds: have {total}, need {target}"));
}
