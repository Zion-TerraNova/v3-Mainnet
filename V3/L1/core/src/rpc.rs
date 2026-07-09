// Phase 7c — JSON-RPC 2.0 protocol handler
//
// Audit reference: #12
//
// Pure protocol layer: parses JSON-RPC 2.0 requests, routes to handler
// functions, and builds spec-compliant responses. Transport-agnostic —
// the node can plug this into HTTP (Axum), TCP, or any byte stream.
//
// Spec: https://www.jsonrpc.org/specification

use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::crypto;
use crate::emission;
use crate::fee;
use crate::migration;
use crate::{
    bridge_operation_message, BridgeValidatorProof, NodeRuntime, BRIDGE_MIN_VALIDATOR_PROOFS,
};

// ── Constants ──────────────────────────────────────────────────────────

pub const JSONRPC_VERSION: &str = "2.0";

// Standard error codes (JSON-RPC 2.0 spec)
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

// Application-specific error codes (-32000 to -32099)
pub const BLOCK_NOT_FOUND: i64 = -32001;
pub const TX_NOT_FOUND: i64 = -32002;
pub const INVALID_ADDRESS: i64 = -32003;
pub const TX_REJECTED: i64 = -32004;
pub const NOT_SYNCED: i64 = -32005;

const ACTIVE_TRANSACTION_MODEL: &str = "hybrid";

/// Convert a transaction amount from a given block height to post-migration
/// flowers (6-decimal scale). Pre-migration blocks store amounts in legacy
/// 12-decimal flowers; this divides by MIGRATION_DIVISOR to normalize.
/// If migration_height is 0 (not set), returns the amount unchanged (fresh
/// nodes that start post-fork have all amounts in new scale already).
#[inline]
fn scaled_amount(amount: u128, block_height: u64) -> u128 {
    if migration::is_post_migration(block_height) {
        amount
    } else {
        amount / migration::MIGRATION_DIVISOR as u128
    }
}

/// Compute the scaled UTXO balance for an address by iterating accepted blocks
/// and applying scaled_amount() per block height. This ensures pre-migration
/// UTXO outputs (stored in 1e12 flowers) are normalised to 1e6 flowers before
/// being summed — unlike rt.utxo_balance() which sums raw amounts.
fn scaled_utxo_balance(rt: &NodeRuntime, address: &str) -> u128 {
    // Replay the UTXO set block-by-block so we know each output's block height.
    let mut utxo_map: std::collections::HashMap<(String, u32), (u64, u64)> =
        std::collections::HashMap::new(); // key → (amount_raw, block_height)
    for block in rt.accepted_blocks() {
        for utxo_tx in &block.utxo_transactions {
            for input in &utxo_tx.inputs {
                utxo_map.remove(&(crypto::to_hex(&input.prev_tx_hash), input.output_index));
            }
        }
        for utxo_tx in &block.utxo_transactions {
            let tx_hash = crypto::to_hex(&utxo_tx.id);
            for (idx, output) in utxo_tx.outputs.iter().enumerate() {
                if output.address == address {
                    utxo_map.insert((tx_hash.clone(), idx as u32), (output.amount, block.height));
                }
            }
        }
    }
    utxo_map
        .values()
        .map(|&(amount, height)| scaled_amount(amount as u128, height))
        .sum()
}

/// RPC-side mirror of the L1 protocol allow-list (env-var driven). Kept
/// here so the JSON-RPC entry-point can reject obviously bad submissions
/// fast, before they ever reach the runtime.
fn load_bridge_validator_pubkey_allowlist() -> HashSet<String> {
    std::env::var("ZION_BRIDGE_VALIDATOR_PUBKEYS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.trim_start_matches("0x").to_ascii_lowercase())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

/// RPC-side mirror of [`crate::required_bridge_validator_threshold`]; the
/// floor enforced by the protocol is [`BRIDGE_MIN_VALIDATOR_PROOFS`].
fn required_bridge_validator_threshold() -> usize {
    std::env::var("ZION_BRIDGE_VALIDATOR_THRESHOLD")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= BRIDGE_MIN_VALIDATOR_PROOFS)
        .unwrap_or(BRIDGE_MIN_VALIDATOR_PROOFS)
}

// ── Request / Response types ───────────────────────────────────────────

/// A parsed JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub id: Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Value,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcResponse {
    /// Build a success response.
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Build an error response.
    pub fn error(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }

    /// Build an error response with extra data.
    pub fn error_with_data(id: Value, code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
            id,
        }
    }
}

// ── Handler trait ──────────────────────────────────────────────────────

/// Result type returned by RPC method handlers.
pub type HandlerResult = Result<Value, (i64, String)>;

/// A handler function signature: takes params, returns result or error.
pub type HandlerFn = Box<dyn Fn(&Value) -> HandlerResult + Send + Sync>;

// ── Router ─────────────────────────────────────────────────────────────

/// JSON-RPC 2.0 router. Maps method names to handler functions.
pub struct RpcRouter {
    handlers: HashMap<String, HandlerFn>,
}

impl RpcRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a method handler.
    pub fn register(&mut self, method: &str, handler: HandlerFn) {
        self.handlers.insert(method.to_string(), handler);
    }

    /// How many methods are registered.
    pub fn method_count(&self) -> usize {
        self.handlers.len()
    }

    /// Check if a method is registered.
    pub fn has_method(&self, method: &str) -> bool {
        self.handlers.contains_key(method)
    }

    /// List all registered method names.
    pub fn methods(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }

    /// Parse raw JSON bytes into a request, route to handler, return response bytes.
    pub fn handle_raw(&self, input: &[u8]) -> Vec<u8> {
        let response = match serde_json::from_slice::<Value>(input) {
            Err(_) => RpcResponse::error(Value::Null, PARSE_ERROR, "Parse error"),
            Ok(val) => {
                // Check for batch request
                if let Some(arr) = val.as_array() {
                    if arr.is_empty() {
                        RpcResponse::error(Value::Null, INVALID_REQUEST, "Empty batch")
                    } else {
                        // Batch: process each, return array
                        let responses: Vec<RpcResponse> =
                            arr.iter().map(|v| self.handle_value(v)).collect();
                        // Serialize as array
                        return serde_json::to_vec(&responses).unwrap_or_default();
                    }
                } else {
                    self.handle_value(&val)
                }
            }
        };
        serde_json::to_vec(&response).unwrap_or_default()
    }

    /// Handle a parsed JSON value as an RPC request.
    pub fn handle_value(&self, val: &Value) -> RpcResponse {
        let req: RpcRequest = match serde_json::from_value(val.clone()) {
            Ok(r) => r,
            Err(_) => return RpcResponse::error(Value::Null, INVALID_REQUEST, "Invalid Request"),
        };
        self.handle_request(&req)
    }

    /// Handle a parsed RPC request.
    pub fn handle_request(&self, req: &RpcRequest) -> RpcResponse {
        if req.jsonrpc != JSONRPC_VERSION {
            return RpcResponse::error(req.id.clone(), INVALID_REQUEST, "Invalid jsonrpc version");
        }

        match self.handlers.get(&req.method) {
            None => RpcResponse::error(
                req.id.clone(),
                METHOD_NOT_FOUND,
                format!("Method not found: {}", req.method),
            ),
            Some(handler) => match handler(&req.params) {
                Ok(result) => RpcResponse::success(req.id.clone(), result),
                Err((code, msg)) => RpcResponse::error(req.id.clone(), code, msg),
            },
        }
    }
}

impl Default for RpcRouter {
    fn default() -> Self {
        Self::new()
    }
}

fn looks_like_utxo_address(value: &str) -> bool {
    value.starts_with("zion1")
}

fn format_flowers_as_zion(amount: u128) -> String {
    format!(
        "{}.{:06}",
        amount / emission::FLOWERS_PER_ZION as u128,
        amount % emission::FLOWERS_PER_ZION as u128
    )
}

fn parse_bridge_memo(memo: &str) -> Option<(&str, &str)> {
    let rest = memo.strip_prefix("BRIDGE:")?;
    let (chain, recipient) = rest.split_once(':')?;
    if chain.is_empty() || recipient.is_empty() {
        return None;
    }
    Some((chain, recipient))
}

fn utxo_balance_at_height(rt: &NodeRuntime, address: &str, height: u64) -> u64 {
    let mut utxos: HashMap<(String, u32), u64> = HashMap::new();
    for block in rt
        .accepted_blocks()
        .iter()
        .filter(|block| block.height <= height)
    {
        for utxo_tx in &block.utxo_transactions {
            for input in &utxo_tx.inputs {
                utxos.remove(&(crypto::to_hex(&input.prev_tx_hash), input.output_index));
            }
        }
        for utxo_tx in &block.utxo_transactions {
            let tx_hash = crypto::to_hex(&utxo_tx.id);
            for (index, output) in utxo_tx.outputs.iter().enumerate() {
                utxos.insert((tx_hash.clone(), index as u32), output.amount);
            }
        }
    }

    let mut balance = 0u64;
    for block in rt
        .accepted_blocks()
        .iter()
        .filter(|block| block.height <= height)
    {
        for utxo_tx in &block.utxo_transactions {
            let tx_hash = crypto::to_hex(&utxo_tx.id);
            for (index, output) in utxo_tx.outputs.iter().enumerate() {
                if output.address == address && utxos.contains_key(&(tx_hash.clone(), index as u32))
                {
                    balance = balance.saturating_add(output.amount);
                }
            }
        }
    }
    balance
}

// ── Helper: build a router with standard node methods ──────────────────

/// Create a stub router with all method names registered but no live state.
/// Used by node_builder and other modules that don't have a NodeRuntime.
pub fn build_stub_router() -> RpcRouter {
    let mut router = RpcRouter::new();
    let stub = |method_name: &'static str| -> HandlerFn {
        Box::new(move |_params: &Value| {
            Err((
                INTERNAL_ERROR,
                format!("{method_name}: not yet bound to node state"),
            ))
        })
    };
    for method in [
        "getBalance",
        "getAccountBalance",
        "getBlock",
        "getBlockByHeight",
        "getTransaction",
        "getAccountTransaction",
        "sendRawTransaction",
        "submitTransaction",
        "submitAccountTransaction",
        "getBlockTemplate",
        "getMempoolInfo",
        "getPeerInfo",
        "getChainInfo",
        "getNodeInfo",
        "submitBlock",
        "getUtxos",
        "getSupplyInfo",
        "getBalanceAtHeight",
        "getBridgeLocks",
        "getBridgeVaultBalance",
        "submitBridgeUnlock",
        "getTransactionHistory",
    ] {
        router.register(method, stub(method));
    }
    router
}

/// Create a router pre-seeded with the priority RPC methods.
/// Each handler captures an `Arc<Mutex<NodeRuntime>>` for live state access.
pub fn build_node_router(runtime: Arc<Mutex<NodeRuntime>>) -> RpcRouter {
    let mut router = RpcRouter::new();

    // ── getChainInfo ───────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getChainInfo",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let status = rt.status();
                Ok(json!({
                    "network": status.network,
                    "consensus_profile": status.consensus_profile,
                    "chain_height": status.chain_height,
                    "tip_hash": status.tip_hash_hex,
                    "accepted_blocks": status.accepted_blocks,
                    "mempool_transactions": status.mempool_transactions,
                    "protocol_version": status.protocol_version,
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                    "utxo_validation_available": true,
                }))
            }),
        );
    }

    // ── getNodeInfo ────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getNodeInfo",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let status = rt.status();
                Ok(json!({
                    "node_id": status.node_id,
                    "protocol_version": status.protocol_version,
                    "protocol_version_numeric": crate::PROTOCOL_VERSION,
                    "flowers_per_zion": emission::FLOWERS_PER_ZION,
                    "network": status.network,
                    "chain_height": status.chain_height,
                    "p2p_bind": status.p2p_bind.address(),
                    "rpc_bind": status.rpc_bind.address(),
                    "pool_bind": status.pool_bind.address(),
                    "known_peers": status.known_peers.len(),
                    "accepted_blocks": status.accepted_blocks,
                    "mempool_transactions": status.mempool_transactions,
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                    "balance_lookup": "account_id_or_zion1_address",
                }))
            }),
        );
    }

    // ── getBlockByHeight ───────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBlockByHeight",
            Box::new(move |params: &Value| {
                let height = params
                    .get("height")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'height' param".into()))?;
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                match rt.accepted_block_by_height(height) {
                    Some(block) => Ok(serde_json::to_value(block).unwrap_or(Value::Null)),
                    None => Err((BLOCK_NOT_FOUND, format!("no block at height {height}"))),
                }
            }),
        );
    }

    // ── getBlock (by hash) ─────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBlock",
            Box::new(move |params: &Value| {
                let hash = params
                    .get("hash")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'hash' param".into()))?;
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                for block in rt.accepted_blocks() {
                    if block.hash_hex == hash {
                        return Ok(serde_json::to_value(block).unwrap_or(Value::Null));
                    }
                }
                Err((BLOCK_NOT_FOUND, format!("no block with hash {hash}")))
            }),
        );
    }

    // ── getTransaction / getAccountTransaction ───────────────────────
    let register_get_transaction = |router: &mut RpcRouter, method_name: &'static str| {
        let rt = Arc::clone(&runtime);
        router.register(
            method_name,
            Box::new(move |params: &Value| {
                let txid = params
                    .get("txid")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'txid' param".into()))?;
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                for block in rt.accepted_blocks() {
                    for tx in &block.transactions {
                        if tx.tx_id == txid {
                            return Ok(json!({
                                "transaction_model": ACTIVE_TRANSACTION_MODEL,
                                "transaction": tx,
                                "block_height": block.height,
                                "block_hash": block.hash_hex,
                                "confirmed": true,
                                "source": "confirmed",
                            }));
                        }
                    }
                }
                Err((TX_NOT_FOUND, format!("transaction {txid} not found")))
            }),
        );
    };
    register_get_transaction(&mut router, "getTransaction");
    register_get_transaction(&mut router, "getAccountTransaction");

    // ── getTransactionHistory ─────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getTransactionHistory",
            Box::new(move |params: &Value| {
                let address = params
                    .get("address")
                    .or_else(|| params.get("account"))
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'address' param".into()))?;

                let offset = params
                    .get("offset")
                    .or_else(|| params.get(1))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);

                let limit = params
                    .get("limit")
                    .or_else(|| params.get(2))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .min(1000); // Cap at 1000 to prevent abuse

                if address.is_empty() {
                    return Err((INVALID_ADDRESS, "empty address".into()));
                }

                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                let all_blocks = rt.accepted_blocks();
                let mut transactions = Vec::new();

                // Use the in-memory address index for O(1) block lookup
                // instead of scanning all accepted blocks.
                let matching_indices = rt.block_indices_for_address(address);
                for &idx in &matching_indices {
                    let block = match all_blocks.get(idx) {
                        Some(b) => b,
                        None => continue,
                    };
                    // 1. Account-model transactions (from/to fields)
                    for tx in &block.transactions {
                        // Check if address is involved (from or to)
                        if tx.from == address || tx.to == address {
                            transactions.push(json!({
                                "transaction": tx,
                                "tx_model": "account",
                                "block_height": block.height,
                                "block_hash": block.hash_hex,
                                "timestamp": block.timestamp,
                                "confirmed": true,
                            }));
                        }
                    }

                    // 2. UTXO transactions (inputs/outputs)
                    // Match by output address (recipient) or by derived input address (sender)
                    for utxo_tx in &block.utxo_transactions {
                        let is_recipient = utxo_tx.outputs.iter().any(|o| o.address == address);
                        let is_sender = utxo_tx.inputs.iter().any(|input| {
                            crate::crypto::derive_address(&input.public_key) == address
                        });

                        if is_recipient || is_sender {
                            // Calculate total amount sent to this address
                            let received: u64 = utxo_tx
                                .outputs
                                .iter()
                                .filter(|o| o.address == address)
                                .map(|o| o.amount)
                                .sum();

                            transactions.push(json!({
                                "transaction": utxo_tx,
                                "tx_model": "utxo",
                                "tx_hash": hex::encode(utxo_tx.id),
                                "block_height": block.height,
                                "block_hash": block.hash_hex,
                                "timestamp": block.timestamp,
                                "confirmed": true,
                                "is_sender": is_sender,
                                "is_recipient": is_recipient,
                                "received_amount_flowers": received,
                            }));
                        }
                    }

                    // 3. Coinbase / miner reward — check if address is the miner
                    if !block.miner_address.is_empty() && block.miner_address == address {
                        transactions.push(json!({
                            "transaction": {
                                "type": "coinbase",
                                "miner_address": block.miner_address,
                                "subsidy_zion": block.subsidy_zion,
                                "miner_reward_zion": block.miner_reward_zion,
                                "humanitarian_address": block.humanitarian_address,
                                "issobella_address": block.issobella_address,
                                "pool_fee_address": block.pool_fee_address,
                            },
                            "tx_model": "coinbase",
                            "block_height": block.height,
                            "block_hash": block.hash_hex,
                            "timestamp": block.timestamp,
                            "confirmed": true,
                        }));
                    }
                }

                // Sort by height descending (newest first)
                transactions.sort_by(|a, b| {
                    let height_a = a["block_height"].as_u64().unwrap_or(0);
                    let height_b = b["block_height"].as_u64().unwrap_or(0);
                    height_b.cmp(&height_a)
                });

                // Apply pagination
                let total = transactions.len();
                let start = offset as usize;
                let end = (start + limit as usize).min(total);

                let page_transactions = if start < total {
                    transactions[start..end].to_vec()
                } else {
                    Vec::new()
                };

                Ok(json!({
                    "address": address,
                    "transactions": page_transactions,
                    "total": total,
                    "offset": offset,
                    "limit": limit,
                    "has_more": end < total,
                }))
            }),
        );
    }

    // ── getAddressInfo ──────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getAddressInfo",
            Box::new(move |params: &Value| {
                let address = params
                    .get("address")
                    .or_else(|| params.get("account"))
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'address' param".into()))?;

                if address.is_empty() {
                    return Err((INVALID_ADDRESS, "empty address".into()));
                }

                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                // Get balance: UTXO + account-model for all addresses.
                // Use scaled_utxo_balance() to normalise pre-migration (1e12)
                // UTXO outputs to post-migration (1e6) flowers before summing.
                let utxo_balance = scaled_utxo_balance(&rt, address);
                let mut account_balance: i128 = 0;
                for block in rt.accepted_blocks() {
                    for tx in &block.transactions {
                        let amt = scaled_amount(tx.amount_zion, block.height);
                        let fee = scaled_amount(tx.fee_zion as u128, block.height);
                        if tx.to == address {
                            account_balance += amt as i128;
                        }
                        if tx.from == address {
                            account_balance -= (amt + fee) as i128;
                        }
                    }
                }
                let account_balance = account_balance.max(0) as u128;
                let balance_flowers = utxo_balance + account_balance;

                // Count transactions and find first/last seen
                let mut tx_count = 0;
                let mut first_seen_height: Option<u64> = None;
                let mut last_seen_height: Option<u64> = None;

                for block in rt.accepted_blocks() {
                    for tx in &block.transactions {
                        if tx.from == address || tx.to == address {
                            tx_count += 1;
                            first_seen_height = Some(
                                first_seen_height.map_or(block.height, |h| h.min(block.height)),
                            );
                            last_seen_height = Some(
                                last_seen_height.map_or(block.height, |h| h.max(block.height)),
                            );
                        }
                    }
                }

                // Get UTXO count if applicable
                let utxo_count = if looks_like_utxo_address(address) {
                    rt.spendable_utxos(address).len() as u64
                } else {
                    0
                };

                Ok(json!({
                    "address": address,
                    "balance_flowers": balance_flowers.to_string(),
                    "balance_zion": format_flowers_as_zion(balance_flowers),
                    "transaction_count": tx_count,
                    "utxo_count": utxo_count,
                    "first_seen_height": first_seen_height,
                    "last_seen_height": last_seen_height,
                    "chain_height": rt.chain_height(),
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                }))
            }),
        );
    }

    // ── getBalance / getAccountBalance ────────────────────────────────
    let register_get_balance = |router: &mut RpcRouter, method_name: &'static str| {
        let rt = Arc::clone(&runtime);
        router.register(
            method_name,
            Box::new(move |params: &Value| {
                let account_id = params
                    .get("account")
                    .or_else(|| params.get("address"))
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'account' param".into()))?;
                if account_id.is_empty() {
                    return Err((INVALID_ADDRESS, "empty account id".into()));
                }
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                if looks_like_utxo_address(account_id) {
                    // Use scaled_utxo_balance() to normalise pre-migration (1e12)
                    // UTXO outputs to post-migration (1e6) flowers.
                    let utxo_balance = scaled_utxo_balance(&rt, account_id);
                    // Also scan account-model transactions (coinbase/premine credits)
                    let mut account_balance: i128 = 0;
                    for block in rt.accepted_blocks() {
                        for tx in &block.transactions {
                            let amt = scaled_amount(tx.amount_zion, block.height);
                            let fee = scaled_amount(tx.fee_zion as u128, block.height);
                            if tx.to == account_id {
                                account_balance += amt as i128;
                            }
                            if tx.from == account_id {
                                account_balance -= (amt + fee) as i128;
                            }
                        }
                    }
                    let account_balance = account_balance.max(0) as u128;
                    let utxo_count = rt.spendable_utxos(account_id).len() as u64;
                    let total = utxo_balance + account_balance;
                    return Ok(json!({
                        "address": account_id,
                        "balance_flowers": total.to_string(),
                        "utxo_balance_flowers": utxo_balance.to_string(),
                        "account_balance_flowers": account_balance.to_string(),
                        "utxo_count": utxo_count,
                        "chain_height": rt.chain_height(),
                        "transaction_model": ACTIVE_TRANSACTION_MODEL,
                        "balance_scope": "confirmed_chain_only",
                    }));
                }
                let mut balance: i128 = 0;
                for block in rt.accepted_blocks() {
                    for tx in &block.transactions {
                        let amt = scaled_amount(tx.amount_zion, block.height);
                        let fee = scaled_amount(tx.fee_zion as u128, block.height);
                        if tx.to == account_id {
                            balance += amt as i128;
                        }
                        if tx.from == account_id {
                            balance -= (amt + fee) as i128;
                        }
                    }
                }
                Ok(json!({
                    "account_id": account_id,
                    "balance_zion": balance.max(0).to_string(),
                    "chain_height": rt.chain_height(),
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                    "balance_scope": "confirmed_chain_only",
                }))
            }),
        );
    };
    register_get_balance(&mut router, "getBalance");
    register_get_balance(&mut router, "getAccountBalance");

    // ── getBalanceAtHeight ────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBalanceAtHeight",
            Box::new(move |params: &Value| {
                let account_id = params
                    .get("account")
                    .or_else(|| params.get("address"))
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'account' param".into()))?;
                let height = params
                    .get("height")
                    .or_else(|| params.get(1))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'height' param".into()))?;
                if account_id.is_empty() {
                    return Err((INVALID_ADDRESS, "empty account id".into()));
                }
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let effective_height = height.min(rt.chain_height());
                if looks_like_utxo_address(account_id) {
                    let utxo_balance = utxo_balance_at_height(&rt, account_id, effective_height);
                    let mut account_balance: i128 = 0;
                    for block in rt
                        .accepted_blocks()
                        .iter()
                        .filter(|block| block.height <= effective_height)
                    {
                        for tx in &block.transactions {
                            let amt = scaled_amount(tx.amount_zion, block.height);
                            let fee = scaled_amount(tx.fee_zion as u128, block.height);
                            if tx.to == account_id {
                                account_balance += amt as i128;
                            }
                            if tx.from == account_id {
                                account_balance -= (amt + fee) as i128;
                            }
                        }
                    }
                    let account_balance = account_balance.max(0) as u128;
                    let total = utxo_balance as u128 + account_balance;
                    return Ok(json!({
                        "address": account_id,
                        "height": effective_height,
                        "balance_flowers": total.to_string(),
                        "utxo_balance_flowers": utxo_balance.to_string(),
                        "account_balance_flowers": account_balance.to_string(),
                        "balance_zion": format_flowers_as_zion(total),
                        "transaction_model": ACTIVE_TRANSACTION_MODEL,
                        "balance_scope": "confirmed_chain_only",
                    }));
                }
                let mut balance: i128 = 0;
                for block in rt
                    .accepted_blocks()
                    .iter()
                    .filter(|block| block.height <= effective_height)
                {
                    for tx in &block.transactions {
                        let amt = scaled_amount(tx.amount_zion, block.height);
                        let fee = scaled_amount(tx.fee_zion as u128, block.height);
                        if tx.to == account_id {
                            balance += amt as i128;
                        }
                        if tx.from == account_id {
                            balance -= (amt + fee) as i128;
                        }
                    }
                }
                Ok(json!({
                    "account_id": account_id,
                    "height": effective_height,
                    "balance_zion": balance.max(0).to_string(),
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                    "balance_scope": "confirmed_chain_only",
                }))
            }),
        );
    }

    // ── getUtxos ───────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getUtxos",
            Box::new(move |params: &Value| {
                let address = params
                    .get("address")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'address' param".into()))?;
                if address.is_empty() {
                    return Err((INVALID_ADDRESS, "empty address".into()));
                }
                if !looks_like_utxo_address(address) {
                    return Err((
                        INVALID_ADDRESS,
                        "getUtxos requires a zion1 UTXO address".into(),
                    ));
                }
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let utxos = rt.spendable_utxos(address);
                // Scale UTXO amounts: pre-migration blocks (height <= MIGRATION_HEIGHT)
                // store amounts in 1e12 flowers; normalise to 1e6 for all callers.
                let utxo_list: Vec<Value> = utxos
                    .iter()
                    .map(|u| {
                        let amount = scaled_amount(u.amount as u128, u.height);
                        json!({
                            "tx_hash": u.tx_hash,
                            "output_index": u.output_index,
                            "amount": amount,
                            "address": u.address,
                            "height": u.height,
                        })
                    })
                    .collect();
                let total_scaled: u128 = utxos
                    .iter()
                    .map(|u| scaled_amount(u.amount as u128, u.height))
                    .sum();
                Ok(json!({
                    "address": address,
                    "utxos": utxo_list,
                    "count": utxo_list.len(),
                    "total_amount": total_scaled,
                    "chain_height": rt.chain_height(),
                }))
            }),
        );
    }

    // ── getBridgeLocks ────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBridgeLocks",
            Box::new(move |params: &Value| {
                let from_height = params
                    .get("from_height")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        (
                            INVALID_PARAMS,
                            "missing or invalid 'from_height' param".into(),
                        )
                    })?;
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let to_height = params
                    .get("to_height")
                    .or_else(|| params.get(1))
                    .and_then(|v| v.as_u64())
                    .unwrap_or_else(|| rt.chain_height())
                    .min(rt.chain_height());
                if from_height > to_height {
                    return Err((
                        INVALID_PARAMS,
                        "from_height cannot be greater than to_height".into(),
                    ));
                }

                let mut locks = Vec::new();
                for block in rt
                    .accepted_blocks()
                    .iter()
                    .filter(|block| block.height >= from_height && block.height <= to_height)
                {
                    for utxo_tx in &block.utxo_transactions {
                        let sender = utxo_tx
                            .inputs
                            .first()
                            .map(|input| crypto::derive_address(&input.public_key))
                            .unwrap_or_default();
                        let txid = crypto::to_hex(&utxo_tx.id);
                        for output in &utxo_tx.outputs {
                            if output.address != fee::BRIDGE_VAULT_ADDRESS {
                                continue;
                            }
                            let Some(memo) = output.memo.as_deref() else {
                                continue;
                            };
                            let Some((recipient_chain, recipient)) = parse_bridge_memo(memo) else {
                                continue;
                            };
                            locks.push(json!({
                                "txid": txid,
                                "block_height": block.height,
                                "sender": sender,
                                "recipient_chain": recipient_chain,
                                "recipient": recipient,
                                "amount_flowers": output.amount,
                                "amount_zion": format_flowers_as_zion(output.amount as u128),
                                "memo": memo,
                                "confirmed": true,
                            }));
                        }
                    }
                }

                Ok(json!({
                    "from_height": from_height,
                    "to_height": to_height,
                    "locks": locks,
                    "count": locks.len(),
                }))
            }),
        );
    }

    // ── getBridgeVaultBalance ─────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBridgeVaultBalance",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let balance = rt.utxo_balance(fee::BRIDGE_VAULT_ADDRESS);
                Ok(json!({
                    "address": fee::BRIDGE_VAULT_ADDRESS,
                    "balance_flowers": balance.to_string(),
                    "balance_zion": format_flowers_as_zion(balance),
                    "chain_height": rt.chain_height(),
                }))
            }),
        );
    }

    // ── submitBridgeUnlock ────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register("submitBridgeUnlock", Box::new(move |params: &Value| {
            let recipient = params.get("recipient")
                .or_else(|| params.get("l1_recipient"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'recipient' param".into()))?;
            let amount_flowers = params.get("amount_flowers")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'amount_flowers' param".into()))?;
            let burn_id = params.get("burn_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'burn_id' param".into()))?;
            let source_chain = params.get("evm_chain")
                .or_else(|| params.get("source_chain"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'evm_chain' param".into()))?;
            let evm_tx_hash = params.get("evm_tx_hash")
                .and_then(|v| v.as_str())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'evm_tx_hash' param".into()))?;
            let validator_proofs = params.get("validator_proofs")
                .or_else(|| params.get("validators"))
                .and_then(|v| v.as_array())
                .ok_or_else(|| (INVALID_PARAMS, "missing or invalid 'validator_proofs' param".into()))?;

            if !crypto::is_valid_address(recipient) {
                return Err((INVALID_ADDRESS, "recipient must be a valid zion1 address".into()));
            }
            if amount_flowers == 0 {
                return Err((INVALID_PARAMS, "amount_flowers must be > 0".into()));
            }
            if validator_proofs.len() < BRIDGE_MIN_VALIDATOR_PROOFS {
                return Err((
                    INVALID_PARAMS,
                    format!(
                        "submitBridgeUnlock requires at least {} validator proofs",
                        BRIDGE_MIN_VALIDATOR_PROOFS,
                    ),
                ));
            }
            if burn_id.trim().is_empty() || source_chain.trim().is_empty() || evm_tx_hash.trim().is_empty() {
                return Err((INVALID_PARAMS, "bridge unlock metadata must not be empty".into()));
            }

            let operation_message = bridge_operation_message(
                recipient,
                amount_flowers,
                source_chain,
                burn_id,
                evm_tx_hash,
            );
            let allowed_pubkeys = load_bridge_validator_pubkey_allowlist();
            let required_threshold = required_bridge_validator_threshold();

            if allowed_pubkeys.is_empty() {
                return Err((
                    INVALID_PARAMS,
                    "core bridge validator allowlist is empty (set ZION_BRIDGE_VALIDATOR_PUBKEYS)".into(),
                ));
            }

            // Parse + cryptographically verify every proof. We deliberately
            // *do not* honour any client-supplied "synthetic" flag — the
            // previous version skipped signature verification for synthetic
            // proofs, which let the relayer fill threshold with placeholder
            // entries and bypass multisig in practice (audit finding F4).
            // The same checks are re-run by the runtime in
            // `build_bridge_unlock_transaction` and again at peer-block
            // import, so even a buggy or compromised RPC entrypoint cannot
            // smuggle a bridge unlock onto the chain without ≥
            // BRIDGE_MIN_VALIDATOR_PROOFS valid secp256k1 signatures.
            let mut validator_ids: HashSet<String> = HashSet::new();
            let mut verified_pubkeys: HashSet<String> = HashSet::new();
            let mut accepted_proofs: Vec<BridgeValidatorProof> = Vec::with_capacity(validator_proofs.len());

            for proof in validator_proofs {
                let validator_id = proof.get("validator_id")
                    .or_else(|| proof.get("id"))
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "each validator proof requires a string validator_id".into()))?;
                if !validator_ids.insert(validator_id.to_string()) {
                    return Err((INVALID_PARAMS, format!("duplicate validator_id in validator_proofs: {validator_id}")));
                }

                if proof.get("synthetic").and_then(Value::as_bool).unwrap_or(false) {
                    return Err((
                        INVALID_PARAMS,
                        format!(
                            "validator proof {validator_id} is marked synthetic; placeholder proofs are no longer accepted",
                        ),
                    ));
                }

                let signature = proof
                    .get("signature")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, format!("validator proof {validator_id} is missing string signature")))?;
                let public_key = proof
                    .get("validator_public_key")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| (
                        INVALID_PARAMS,
                        format!(
                            "validator proof {validator_id} requires validator_public_key for cryptographic verification"
                        ),
                    ))?;

                let pubkey_hex = public_key.trim_start_matches("0x").to_ascii_lowercase();
                if !allowed_pubkeys.contains(&pubkey_hex) {
                    return Err((
                        INVALID_PARAMS,
                        format!("validator proof {validator_id} pubkey is not in core allowlist"),
                    ));
                }

                let pubkey_bytes = hex::decode(&pubkey_hex).map_err(|_| (
                    INVALID_PARAMS,
                    format!("validator proof {validator_id} has invalid validator_public_key hex"),
                ))?;
                let verifying_key = VerifyingKey::from_sec1_bytes(&pubkey_bytes).map_err(|_| (
                    INVALID_PARAMS,
                    format!("validator proof {validator_id} has invalid secp256k1 public key bytes"),
                ))?;

                let sig_hex = signature.trim_start_matches("0x").to_ascii_lowercase();
                let sig_bytes = hex::decode(&sig_hex).map_err(|_| (
                    INVALID_PARAMS,
                    format!("validator proof {validator_id} has invalid signature hex"),
                ))?;
                if sig_bytes.len() != 64 {
                    return Err((
                        INVALID_PARAMS,
                        format!("validator proof {validator_id} signature must be 64 bytes"),
                    ));
                }
                let parsed_signature = Signature::from_slice(&sig_bytes).map_err(|_| (
                    INVALID_PARAMS,
                    format!("validator proof {validator_id} signature is not canonical ECDSA"),
                ))?;

                let proof_message = proof
                    .get("operation_message")
                    .and_then(|value| value.as_str())
                    .unwrap_or(&operation_message);
                if proof_message != operation_message {
                    return Err((
                        INVALID_PARAMS,
                        format!("validator proof {validator_id} operation_message mismatch"),
                    ));
                }

                verifying_key
                    .verify(operation_message.as_bytes(), &parsed_signature)
                    .map_err(|_| (
                        INVALID_PARAMS,
                        format!("validator proof {validator_id} failed secp256k1 signature verification"),
                    ))?;

                if !verified_pubkeys.insert(pubkey_hex.clone()) {
                    return Err((
                        INVALID_PARAMS,
                        format!("validator proof {validator_id} reuses a pubkey already counted; each signer must be unique"),
                    ));
                }

                let typed = BridgeValidatorProof::new(validator_id, pubkey_hex, sig_hex)
                    .map_err(|reason| (INVALID_PARAMS, reason))?;
                accepted_proofs.push(typed);
            }

            if verified_pubkeys.len() < required_threshold {
                return Err((
                    INVALID_PARAMS,
                    format!(
                        "submitBridgeUnlock requires at least {} cryptographically verified validator proofs",
                        required_threshold
                    ),
                ));
            }

            let mut rt = rt.lock().map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
            let response = rt.submit_bridge_unlock(
                crate::BridgeUnlockRequest {
                    recipient: recipient.to_string(),
                    amount_flowers,
                    source_chain: source_chain.to_string(),
                    burn_id: burn_id.to_string(),
                    evm_tx_hash: evm_tx_hash.to_string(),
                },
                accepted_proofs,
            );
            match response {
                crate::RpcResponse::TransactionResult { accepted, tx_id, reason } => {
                    if accepted {
                        Ok(json!({ "accepted": true, "tx_id": tx_id }))
                    } else {
                        Err((TX_REJECTED, reason.unwrap_or_else(|| "bridge unlock rejected".into())))
                    }
                }
                _ => Err((INTERNAL_ERROR, "unexpected response".into())),
            }
        }));
    }

    // ── getBlockTemplate ───────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBlockTemplate",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let template = rt.active_template();
                Ok(serde_json::to_value(&template).unwrap_or(Value::Null))
            }),
        );
    }

    // ── getMempoolInfo ─────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getMempoolInfo",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let status = rt.status();
                Ok(json!({
                    "size": status.mempool_transactions,
                    "template_transactions": status.active_template_transactions,
                    "template_total_fees_zion": status.active_template_total_fees_zion,
                    "transaction_model": ACTIVE_TRANSACTION_MODEL,
                }))
            }),
        );
    }

    // ── getPeerInfo ────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getPeerInfo",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let peers: Vec<Value> = rt.known_peers().iter().map(|peer| {
                json!({ "host": peer.host, "port": peer.port, "address": peer.address() })
            }).collect();
                Ok(json!({ "peers": peers, "count": peers.len() }))
            }),
        );
    }

    // ── estimateFee ────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "estimateFee",
            Box::new(move |params: &Value| {
                let amount_zion = params
                    .get("amount")
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);

                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                // ZION uses 100% fee burn policy with minimum fee
                // Calculate based on amount and current network conditions
                let base_fee = 1_000_000u64; // Minimum 0.001 ZION fee
                let amount_fee = (amount_zion / 10000).max(base_fee); // 0.01% of amount or base_fee

                // Get current mempool congestion info
                let mempool_size = rt.status().mempool_transactions;
                let congestion_multiplier = if mempool_size > 1000 {
                    2.0
                } else if mempool_size > 500 {
                    1.5
                } else {
                    1.0
                };

                let estimated_fee = (amount_fee as f64 * congestion_multiplier) as u64;

                Ok(json!({
                    "estimated_fee_flowers": estimated_fee.to_string(),
                    "estimated_fee_zion": format_flowers_as_zion(estimated_fee as u128),
                    "amount_zion": amount_zion,
                    "mempool_size": mempool_size,
                    "congestion_multiplier": congestion_multiplier,
                    "min_fee_flowers": base_fee.to_string(),
                    "min_fee_zion": format_flowers_as_zion(base_fee as u128),
                }))
            }),
        );
    }

    // ── getBlockRange ────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getBlockRange",
            Box::new(move |params: &Value| {
                let start_height = params
                    .get("start_height")
                    .or_else(|| params.get("from"))
                    .or_else(|| params.get(0))
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        (
                            INVALID_PARAMS,
                            "missing or invalid 'start_height' param".into(),
                        )
                    })?;

                let end_height = params
                    .get("end_height")
                    .or_else(|| params.get("to"))
                    .or_else(|| params.get(1))
                    .and_then(|v| v.as_u64())
                    .unwrap_or({
                        // Default to current chain height if not specified
                        // We'll get this after locking runtime
                        start_height
                    });

                let limit = params
                    .get("limit")
                    .or_else(|| params.get(2))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .min(500); // Cap at 500 blocks to prevent abuse

                if start_height > end_height {
                    return Err((
                        INVALID_PARAMS,
                        "start_height cannot be greater than end_height".into(),
                    ));
                }

                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                let actual_end_height = end_height.min(rt.chain_height());
                let requested_count = (actual_end_height - start_height + 1).min(limit);

                let mut blocks = Vec::new();
                for height in
                    start_height..=(start_height + requested_count - 1).min(actual_end_height)
                {
                    if let Some(block) = rt.accepted_block_by_height(height) {
                        blocks.push(block);
                    }
                }

                Ok(json!({
                    "blocks": blocks,
                    "count": blocks.len(),
                    "start_height": start_height,
                    "end_height": actual_end_height,
                    "chain_height": rt.chain_height(),
                    "has_more": actual_end_height > start_height + requested_count - 1,
                }))
            }),
        );
    }

    // ── getNetworkStats ─────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getNetworkStats",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                let blocks = rt.accepted_blocks();
                let chain_height = rt.chain_height();

                if blocks.len() < 2 {
                    return Ok(json!({
                        "error": "insufficient blocks for statistics",
                        "min_blocks_required": 2,
                    }));
                }

                // Calculate average block time over last 100 blocks
                let sample_size = 100.min(blocks.len());
                let recent_blocks = &blocks[blocks.len() - sample_size..];

                let mut total_block_time = 0u64;
                let mut total_difficulty = 0u64;
                let mut block_times = Vec::new();
                let mut difficulties = Vec::new();

                for (i, block) in recent_blocks.iter().enumerate() {
                    if i > 0 {
                        let prev_block = &recent_blocks[i - 1];
                        let block_time = block.timestamp.saturating_sub(prev_block.timestamp);
                        total_block_time += block_time;
                        block_times.push(block_time);
                    }
                    total_difficulty += block.difficulty;
                    difficulties.push(block.difficulty);
                }

                let avg_block_time = if !block_times.is_empty() {
                    total_block_time / block_times.len() as u64
                } else {
                    60 // Default target
                };

                let avg_difficulty = if !difficulties.is_empty() {
                    total_difficulty / difficulties.len() as u64
                } else {
                    0
                };

                // Calculate estimated hashrate (hashes per second)
                // hashrate = difficulty * 2^32 / block_time (for standard Bitcoin-like PoW)
                // For Cosmic Harmony, this is an approximation
                let estimated_hashrate = if avg_block_time > 0 {
                    (avg_difficulty as f64 * 4_294_967_296.0) / avg_block_time as f64
                } else {
                    0.0
                };

                // Format hashrate for display
                let hashrate_hps = if estimated_hashrate >= 1e18 {
                    format!("{:.2} EH/s", estimated_hashrate / 1e18)
                } else if estimated_hashrate >= 1e15 {
                    format!("{:.2} PH/s", estimated_hashrate / 1e15)
                } else if estimated_hashrate >= 1e12 {
                    format!("{:.2} TH/s", estimated_hashrate / 1e12)
                } else if estimated_hashrate >= 1e9 {
                    format!("{:.2} GH/s", estimated_hashrate / 1e9)
                } else if estimated_hashrate >= 1e6 {
                    format!("{:.2} MH/s", estimated_hashrate / 1e6)
                } else {
                    format!("{:.2} H/s", estimated_hashrate)
                };

                Ok(json!({
                    "chain_height": chain_height,
                    "average_block_time": avg_block_time,
                    "target_block_time": 60,
                    "average_difficulty": avg_difficulty,
                    "current_difficulty": blocks.last().map(|b| b.difficulty).unwrap_or(0),
                    "estimated_hashrate_hps": estimated_hashrate,
                    "estimated_hashrate_formatted": hashrate_hps,
                    "sample_size": sample_size,
                    "peer_count": rt.known_peers().len(),
                    "mempool_size": rt.status().mempool_transactions,
                    "network_hashrate": hashrate_hps,
                }))
            }),
        );
    }

    // ── getTokenInfo ─────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "getTokenInfo",
            Box::new(move |_params: &Value| {
                let rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;

                let blocks = rt.accepted_blocks();

                // Get bridge vault balance (wZION locked in L1)
                let vault_balance = rt.utxo_balance(fee::BRIDGE_VAULT_ADDRESS);

                // Get supply info for circulating supply
                let height = rt.chain_height();

                // Calculate total minted wZION (this would typically come from bridge stats)
                // For now, we'll estimate based on vault balance assuming 1:1 peg
                let total_locked_flowers = vault_balance;
                let total_locked_zion = format_flowers_as_zion(vault_balance);

                // Bridge contract address on Base (from deployment)
                let bridge_contract = "0xa5a09b2C09A7182BBA9623A2D2cd46cD7D041721"; // ZIONBridge
                let wzion_contract = "0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6"; // wZION

                Ok(json!({
                    "token_name": "Wrapped ZION",
                    "token_symbol": "wZION",
                    "bridge_contract": bridge_contract,
                    "wzion_contract": wzion_contract,
                    "bridge_network": "Base Mainnet",
                    "total_locked_flowers": total_locked_flowers.to_string(),
                    "total_locked_zion": total_locked_zion,
                    "total_minted_wzion": total_locked_zion, // Assuming 1:1 peg
                    "bridge_vault_address": fee::BRIDGE_VAULT_ADDRESS,
                    "bridge_vault_balance_flowers": vault_balance.to_string(),
                    "chain_height": height,
                    "last_updated_timestamp": blocks.last().map(|b| b.timestamp).unwrap_or(0), // Use last block timestamp
                    "peg_status": "1:1 maintained",
                    "bridge_status": "operational",
                }))
            }),
        );
    }

    // ── sendRawTransaction / submitTransaction ────────────────────────
    let register_submit_transaction = |router: &mut RpcRouter, method_name: &'static str| {
        let rt = Arc::clone(&runtime);
        router.register(
            method_name,
            Box::new(move |params: &Value| {
                let tx_value = params
                    .get("transaction")
                    .cloned()
                    .unwrap_or_else(|| params.clone());
                let submitted = match crate::SubmittedTransaction::parse_value(tx_value) {
                    Ok(transaction) => transaction,
                    Err(message) => return Err((INVALID_PARAMS, message)),
                };
                let mut rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let resp = rt.submit_submitted_transaction(submitted);
                match resp {
                    crate::RpcResponse::TransactionResult {
                        accepted,
                        tx_id,
                        reason,
                    } => {
                        if accepted {
                            Ok(json!({ "accepted": true, "tx_id": tx_id }))
                        } else {
                            Err((TX_REJECTED, reason.unwrap_or_else(|| "rejected".into())))
                        }
                    }
                    _ => Err((INTERNAL_ERROR, "unexpected response".into())),
                }
            }),
        );
    };
    register_submit_transaction(&mut router, "sendRawTransaction");
    register_submit_transaction(&mut router, "submitTransaction");
    register_submit_transaction(&mut router, "submitAccountTransaction");

    // ── getSupplyInfo ───────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register("getSupplyInfo", Box::new(move |_params: &Value| {
            let rt = rt.lock().map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
            let height = rt.chain_height();
            let block_reward = emission::block_subsidy(height.max(1));

            // Cumulative mined supply in flowers (walk decade boundaries)
            let mined_flowers: u128 = {
                let mut sum: u128 = 0;
                let mut h: u64 = 1;
                while h <= height {
                    let decade_end =
                        ((h - 1) / emission::BLOCKS_PER_DECADE + 1) * emission::BLOCKS_PER_DECADE;
                    let blocks_in_range = decade_end.min(height) - h + 1;
                    sum += emission::block_subsidy(h) as u128 * blocks_in_range as u128;
                    h = decade_end + 1;
                }
                sum
            };

            let circulating_flowers = emission::GENESIS_PREMINE + mined_flowers;
            let remaining_flowers = emission::TOTAL_SUPPLY.saturating_sub(circulating_flowers);

            let supply_mined_pct = if emission::MINING_EMISSION > 0 {
                (mined_flowers as f64 / emission::MINING_EMISSION as f64) * 100.0
            } else {
                0.0
            };

            Ok(json!({
                // Canonical (post-3.0.3): _flowers suffix = raw atomic units
                "total_supply_flowers": emission::TOTAL_SUPPLY.to_string(),
                "premine_flowers": emission::GENESIS_PREMINE.to_string(),
                "mining_emission_flowers": emission::MINING_EMISSION.to_string(),
                "mined_so_far_flowers": mined_flowers.to_string(),
                "circulating_supply_flowers": circulating_flowers.to_string(),
                "remaining_supply_flowers": remaining_flowers.to_string(),
                "block_reward_flowers": block_reward,
                // Human-readable ZION (decimal)
                "total_supply_zion": (emission::TOTAL_SUPPLY / emission::FLOWERS_PER_ZION as u128) as u64,
                "premine_zion": (emission::GENESIS_PREMINE / emission::FLOWERS_PER_ZION as u128) as u64,
                "mining_emission_zion": (emission::MINING_EMISSION / emission::FLOWERS_PER_ZION as u128) as u64,
                "mined_so_far_zion": (mined_flowers / emission::FLOWERS_PER_ZION as u128) as u64,
                "circulating_supply_zion": (circulating_flowers / emission::FLOWERS_PER_ZION as u128) as u64,
                "remaining_supply_zion": (remaining_flowers / emission::FLOWERS_PER_ZION as u128) as u64,
                "block_reward_zion": block_reward as f64 / emission::FLOWERS_PER_ZION as f64,
                // Legacy aliases (deprecated 3.0.3, drop in 3.0.4)
                "total_supply_atomic": emission::TOTAL_SUPPLY.to_string(),
                "premine_atomic": emission::GENESIS_PREMINE.to_string(),
                "mining_emission_atomic": emission::MINING_EMISSION.to_string(),
                "mined_so_far_atomic": mined_flowers.to_string(),
                "circulating_supply_atomic": circulating_flowers.to_string(),
                "remaining_supply_atomic": remaining_flowers.to_string(),
                "block_reward_atomic": block_reward,
                "supply_mined_percent": format!("{:.6}", supply_mined_pct),
                "height": height,
                "protocol_version": crate::PROTOCOL_VERSION,
            }))
        }));
    }

    // ── submitBlock ────────────────────────────────────────────────────
    {
        let rt = Arc::clone(&runtime);
        router.register(
            "submitBlock",
            Box::new(move |params: &Value| {
                let template_id = params
                    .get("template_id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| (INVALID_PARAMS, "missing 'template_id'".into()))?;
                let header_hex = params
                    .get("header_hex")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing 'header_hex'".into()))?
                    .to_string();
                let nonce = params
                    .get("nonce")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| (INVALID_PARAMS, "missing 'nonce'".into()))?;
                let target_hex = params
                    .get("target_hex")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| (INVALID_PARAMS, "missing 'target_hex'".into()))?
                    .to_string();
                let algorithm = params
                    .get("algorithm")
                    .and_then(|v| v.as_str())
                    .unwrap_or("deeksha_lite_v1")
                    .to_string();
                let mut rt = rt
                    .lock()
                    .map_err(|_| (INTERNAL_ERROR, "runtime lock poisoned".into()))?;
                let resp = rt.handle_rpc_request(crate::RpcRequest::SubmitCandidate {
                    template_id,
                    header_hex,
                    nonce,
                    target_hex,
                    algorithm,
                });
                match resp {
                    crate::RpcResponse::SubmitResult {
                        accepted,
                        template_id,
                        block_height,
                        hash_hex,
                        reason,
                    } => Ok(json!({
                        "accepted": accepted,
                        "template_id": template_id,
                        "block_height": block_height,
                        "hash_hex": hash_hex,
                        "reason": reason,
                    })),
                    _ => Err((INTERNAL_ERROR, "unexpected response".into())),
                }
            }),
        );
    }

    router
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::{signature::Signer, Signature, SigningKey};
    use serde_json::json;

    /// Deterministic test keypair for signing account transactions in RPC tests.
    fn test_keypair() -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
        crypto::keypair_from_canonical_label("__test_dummy_signer_v1__")
    }

    /// Generate a valid Ed25519 signature + public key hex for a given tx_id,
    /// plus the derived sender address that matches the public key.
    fn dummy_sig_for_tx_id(tx_id: &str) -> (String, String, String) {
        let (sk, vk) = test_keypair();
        let sig = crypto::sign(&sk, tx_id.as_bytes());
        let from = crypto::derive_address(vk.as_bytes());
        (hex::encode(sig), hex::encode(vk.as_bytes()), from)
    }

    static BRIDGE_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn test_router() -> RpcRouter {
        let mut router = RpcRouter::new();
        router.register("echo", Box::new(|params: &Value| Ok(params.clone())));
        router.register(
            "add",
            Box::new(|params: &Value| {
                let a = params.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                let b = params.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                Ok(json!(a + b))
            }),
        );
        router.register(
            "fail",
            Box::new(|_params: &Value| Err((TX_REJECTED, "transaction rejected".to_string()))),
        );
        router
    }

    #[test]
    fn success_response() {
        let router = test_router();
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "echo".to_string(),
            params: json!({"hello": "world"}),
            id: json!(1),
        };
        let resp = router.handle_request(&req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), json!({"hello": "world"}));
        assert_eq!(resp.id, json!(1));
    }

    #[test]
    fn error_response() {
        let router = test_router();
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "fail".to_string(),
            params: json!(null),
            id: json!(2),
        };
        let resp = router.handle_request(&req);
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, TX_REJECTED);
    }

    #[test]
    fn method_not_found() {
        let router = test_router();
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "nonexistent".to_string(),
            params: json!(null),
            id: json!(3),
        };
        let resp = router.handle_request(&req);
        let err = resp.error.unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[test]
    fn parse_error_on_garbage() {
        let router = test_router();
        let resp_bytes = router.handle_raw(b"this is not json");
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, PARSE_ERROR);
    }

    #[test]
    fn invalid_request_on_bad_structure() {
        let router = test_router();
        let input = serde_json::to_vec(&json!({"foo": "bar"})).unwrap();
        let resp_bytes = router.handle_raw(&input);
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn invalid_jsonrpc_version() {
        let router = test_router();
        let req = RpcRequest {
            jsonrpc: "1.0".to_string(),
            method: "echo".to_string(),
            params: json!(null),
            id: json!(4),
        };
        let resp = router.handle_request(&req);
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn handle_raw_roundtrip() {
        let router = test_router();
        let input = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "method": "add",
            "params": {"a": 3, "b": 4},
            "id": 5
        }))
        .unwrap();

        let resp_bytes = router.handle_raw(&input);
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        assert_eq!(resp.result.unwrap(), json!(7));
        assert_eq!(resp.id, json!(5));
    }

    #[test]
    fn batch_request() {
        let router = test_router();
        let input = serde_json::to_vec(&json!([
            {"jsonrpc": "2.0", "method": "echo", "params": "a", "id": 1},
            {"jsonrpc": "2.0", "method": "echo", "params": "b", "id": 2}
        ]))
        .unwrap();

        let resp_bytes = router.handle_raw(&input);
        let responses: Vec<RpcResponse> = serde_json::from_slice(&resp_bytes).unwrap();
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].result.as_ref().unwrap(), &json!("a"));
        assert_eq!(responses[1].result.as_ref().unwrap(), &json!("b"));
    }

    #[test]
    fn empty_batch_error() {
        let router = test_router();
        let input = serde_json::to_vec(&json!([])).unwrap();
        let resp_bytes = router.handle_raw(&input);
        let resp: RpcResponse = serde_json::from_slice(&resp_bytes).unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_REQUEST);
    }

    /// Create a stub router for tests that don't need real node state.
    fn stub_node_router() -> RpcRouter {
        let mut router = RpcRouter::new();
        let stub = |method_name: &'static str| -> HandlerFn {
            Box::new(move |_params: &Value| Err((INTERNAL_ERROR, format!("{method_name}: stub"))))
        };
        for method in [
            "getBalance",
            "getAccountBalance",
            "getBlock",
            "getBlockByHeight",
            "getTransaction",
            "getAccountTransaction",
            "sendRawTransaction",
            "submitTransaction",
            "submitAccountTransaction",
            "getBlockTemplate",
            "getMempoolInfo",
            "getPeerInfo",
            "getChainInfo",
            "getNodeInfo",
            "submitBlock",
            "getUtxos",
            "getSupplyInfo",
            "getBalanceAtHeight",
            "getBridgeLocks",
            "getBridgeVaultBalance",
            "submitBridgeUnlock",
            "getTransactionHistory",
        ] {
            router.register(method, stub(method));
        }
        router
    }

    #[test]
    fn node_router_has_priority_methods() {
        let router = stub_node_router();
        assert!(router.has_method("getBalance"));
        assert!(router.has_method("getAccountBalance"));
        assert!(router.has_method("getBlock"));
        assert!(router.has_method("getBlockByHeight"));
        assert!(router.has_method("getTransaction"));
        assert!(router.has_method("getAccountTransaction"));
        assert!(router.has_method("sendRawTransaction"));
        assert!(router.has_method("submitTransaction"));
        assert!(router.has_method("submitAccountTransaction"));
        assert!(router.has_method("getBlockTemplate"));
        assert!(router.has_method("getMempoolInfo"));
        assert!(router.has_method("getPeerInfo"));
        assert!(router.has_method("getChainInfo"));
        assert!(router.has_method("getNodeInfo"));
        assert!(router.has_method("submitBlock"));
        assert!(router.has_method("getUtxos"));
        assert!(router.has_method("getSupplyInfo"));
        assert!(router.has_method("getBalanceAtHeight"));
        assert!(router.has_method("getBridgeLocks"));
        assert!(router.has_method("getBridgeVaultBalance"));
        assert!(router.has_method("submitBridgeUnlock"));
        assert!(router.has_method("getTransactionHistory"));
        assert_eq!(router.method_count(), 22);
    }

    #[test]
    fn node_router_stubs_return_error() {
        let router = stub_node_router();
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "getBalance".to_string(),
            params: json!({"address": "zion1test"}),
            id: json!(1),
        };
        let resp = router.handle_request(&req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INTERNAL_ERROR);
    }

    #[test]
    fn method_listing() {
        let router = test_router();
        let methods = router.methods();
        assert!(methods.contains(&"echo"));
        assert!(methods.contains(&"add"));
        assert!(methods.contains(&"fail"));
    }

    #[test]
    fn error_with_data() {
        let resp = RpcResponse::error_with_data(
            json!(1),
            TX_REJECTED,
            "rejected",
            json!({"reason": "double-spend"}),
        );
        let err = resp.error.unwrap();
        assert_eq!(err.data.unwrap(), json!({"reason": "double-spend"}));
    }

    // ── Live router integration tests ──────────────────────────────────

    fn live_router() -> RpcRouter {
        use std::sync::{Arc, Mutex};
        let runtime = Arc::new(Mutex::new(crate::NodeRuntime::new(
            "rpc-test",
            crate::NodeConfig::mainnet(),
        )));
        build_node_router(runtime)
    }

    fn bridge_unlock_ready_router(amount: u64) -> RpcRouter {
        use std::sync::{Arc, Mutex};

        let mut runtime = crate::NodeRuntime::new("rpc-test", crate::NodeConfig::mainnet());
        let funding = {
            let mut transaction = crate::tx::Transaction {
                id: [0u8; 32],
                version: crate::tx::TX_HASH_V2_VERSION,
                inputs: vec![],
                outputs: vec![crate::tx::TxOutput {
                    amount,
                    address: fee::BRIDGE_VAULT_ADDRESS.to_string(),
                    memo: None,
                }],
                fee: 0,
                timestamp: 1_700_000_000,
            };
            transaction.finalize_id();
            transaction
        };

        let funding_block = crate::AcceptedBlock {
            template_id: 1,
            height: 1,
            timestamp: crate::now_secs(),
            difficulty: crate::difficulty::GENESIS_DIFFICULTY,
            nonce: 0,
            hash_hex: crate::hex(&[0x11; 32]),
            header_hex: String::new(),
            previous_hash_hex: runtime.accepted_blocks()[0].hash_hex.clone(),
            algorithm: "deeksha_lite_v1".to_string(),
            transaction_ids: vec![],
            transactions: vec![],
            total_fees_zion: 0,
            body_hash_hex: crate::body_hash_hex(&[]),
            subsidy_zion: crate::emission::block_subsidy(1),
            miner_reward_zion: crate::emission::block_subsidy(1),
            miner_address: String::new(),
            humanitarian_address: String::new(),
            issobella_address: String::new(),
            pool_fee_address: String::new(),
            utxo_transaction_ids: vec![crate::hex(&funding.id)],
            utxo_transactions: vec![funding],
        };

        let imported = runtime.import_peer_blocks(vec![funding_block]);
        assert!(matches!(imported, Ok(1)));

        build_node_router(Arc::new(Mutex::new(runtime)))
    }

    fn rpc_call(router: &RpcRouter, method: &str, params: Value) -> RpcResponse {
        router.handle_request(&RpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
            id: json!(1),
        })
    }

    fn make_bridge_validator_proofs(
        recipient: &str,
        amount_flowers: u64,
        source_chain: &str,
        burn_id: &str,
        evm_tx_hash: &str,
    ) -> (Vec<Value>, String) {
        let operation_message = bridge_operation_message(
            recipient,
            amount_flowers,
            source_chain,
            burn_id,
            evm_tx_hash,
        );

        let mut proofs = Vec::new();
        let mut pubkeys = Vec::new();
        for index in 0..3u8 {
            let key_bytes = [index + 1; 32];
            let signing_key = SigningKey::from_slice(&key_bytes).expect("test signing key");
            let signature: Signature = signing_key.sign(operation_message.as_bytes());
            let sec1 = signing_key.verifying_key().to_encoded_point(true);
            let pubkey_hex = format!("0x{}", hex::encode(sec1.as_bytes()));
            pubkeys.push(pubkey_hex.clone());

            proofs.push(json!({
                "validator_id": format!("v{}", usize::from(index) + 1),
                "validator_public_key": pubkey_hex,
                "signature": format!("0x{}", hex::encode(signature.to_bytes())),
                "signature_scheme": "secp256k1-ecdsa",
                "operation_message": operation_message,
                "synthetic": false,
            }));
        }

        (proofs, pubkeys.join(","))
    }

    #[test]
    fn live_get_chain_info() {
        let router = live_router();
        let resp = rpc_call(&router, "getChainInfo", json!(null));
        assert!(
            resp.error.is_none(),
            "getChainInfo failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["chain_height"], 0);
        assert!(result["network"].is_string());
        assert!(result["consensus_profile"].is_string());
        assert_eq!(result["transaction_model"], ACTIVE_TRANSACTION_MODEL);
    }

    #[test]
    fn live_get_node_info() {
        let router = live_router();
        let resp = rpc_call(&router, "getNodeInfo", json!(null));
        assert!(resp.error.is_none(), "getNodeInfo failed: {:?}", resp.error);
        let result = resp.result.unwrap();
        assert_eq!(result["node_id"], "rpc-test");
        assert!(result["protocol_version"].is_string());
        assert!(result["known_peers"].is_number());
        assert_eq!(result["transaction_model"], ACTIVE_TRANSACTION_MODEL);
    }

    #[test]
    fn live_get_block_by_height_genesis() {
        let router = live_router();
        let resp = rpc_call(&router, "getBlockByHeight", json!({"height": 0}));
        assert!(
            resp.error.is_none(),
            "getBlockByHeight(0) failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["height"], 0);
        assert!(result["hash_hex"].is_string());
    }

    #[test]
    fn live_get_block_by_height_not_found() {
        let router = live_router();
        let resp = rpc_call(&router, "getBlockByHeight", json!({"height": 9999}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, BLOCK_NOT_FOUND);
    }

    #[test]
    fn live_get_block_by_hash() {
        let router = live_router();
        // First get genesis block hash
        let resp = rpc_call(&router, "getBlockByHeight", json!({"height": 0}));
        let genesis_hash = resp.result.unwrap()["hash_hex"]
            .as_str()
            .unwrap()
            .to_string();
        // Now fetch by hash
        let resp = rpc_call(&router, "getBlock", json!({"hash": genesis_hash}));
        assert!(
            resp.error.is_none(),
            "getBlock by hash failed: {:?}",
            resp.error
        );
        assert_eq!(resp.result.unwrap()["height"], 0);
    }

    #[test]
    fn live_get_balance_empty() {
        let router = live_router();
        let resp = rpc_call(&router, "getBalance", json!({"account": "wallet.alpha"}));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["balance_zion"], "0");
        assert_eq!(result["transaction_model"], ACTIVE_TRANSACTION_MODEL);
    }

    #[test]
    fn live_get_balance_returns_zero_for_unknown_utxo_address() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getBalance",
            json!({"address": "zion1nobody000000000000000000000000000000000"}),
        );
        assert!(
            resp.error.is_none(),
            "getBalance for zion1 failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["balance_flowers"], "0");
        // After Phase 18 fix, zion1 addresses report combined account+UTXO balance
        assert_eq!(result["transaction_model"], ACTIVE_TRANSACTION_MODEL);
    }

    #[test]
    fn live_get_account_balance_alias_works() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getAccountBalance",
            json!({"account": "wallet.alpha"}),
        );
        assert!(
            resp.error.is_none(),
            "getAccountBalance failed: {:?}",
            resp.error
        );
        assert_eq!(resp.result.unwrap()["account_id"], "wallet.alpha");
    }

    #[test]
    fn live_get_block_template() {
        let router = live_router();
        let resp = rpc_call(&router, "getBlockTemplate", json!(null));
        assert!(
            resp.error.is_none(),
            "getBlockTemplate failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert!(result["template_id"].is_number());
        assert!(result["height"].is_number());
        assert!(result["header_hex"].is_string());
    }

    #[test]
    fn live_get_mempool_info() {
        let router = live_router();
        let resp = rpc_call(&router, "getMempoolInfo", json!(null));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["size"], 0);
    }

    #[test]
    fn live_get_peer_info() {
        let router = live_router();
        let resp = rpc_call(&router, "getPeerInfo", json!(null));
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert!(result["count"].is_number());
        assert!(result["peers"].is_array());
    }

    #[test]
    fn live_get_transaction_not_found() {
        let router = live_router();
        let resp = rpc_call(&router, "getTransaction", json!({"txid": "nonexistent"}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, TX_NOT_FOUND);
    }

    #[test]
    fn live_get_account_transaction_alias_not_found() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getAccountTransaction",
            json!({"txid": "nonexistent"}),
        );
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, TX_NOT_FOUND);
    }

    #[test]
    fn live_send_raw_transaction_invalid() {
        let router = live_router();
        let resp = rpc_call(&router, "sendRawTransaction", json!({"bad": true}));
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[test]
    fn live_send_raw_transaction_rejects_hex_string_payload() {
        let router = live_router();
        let resp = rpc_call(&router, "sendRawTransaction", json!("deadbeef"));
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("transaction object"));
    }

    #[test]
    fn live_submit_transaction_rejects_utxo_payload() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "submitTransaction",
            json!({
                "id": vec![0u8; 32],
                "version": 1,
                "inputs": [{
                    "prev_tx_hash": vec![1u8; 32],
                    "output_index": 0,
                    "signature": vec![2u8; 64],
                    "public_key": vec![3u8; 32]
                }],
                "outputs": [{
                    "amount": 1000,
                    "address": "zion1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq"
                }],
                "fee": 100,
                "timestamp": 1700000000
            }),
        );
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, TX_REJECTED);
        // Production: v2 from genesis → `version = 1` rejected at mempool gate.
        // Rehearsal: below coordinated height, v1 is still valid — bogus payload
        // may fail id/hash validation before any version gate.
        #[cfg(not(feature = "testnet_fork_rehearsal"))]
        assert!(
            err.message.contains("tx.version")
                || err.message.contains("requires tx.version")
                || err.message.contains("TX_HASH_V2"),
            "unexpected rejection message: {}",
            err.message
        );
        #[cfg(feature = "testnet_fork_rehearsal")]
        assert!(
            err.message.contains("tx.version")
                || err.message.contains("requires tx.version")
                || err.message.contains("TX_HASH_V2")
                || err.message.contains("does not match calculated hash"),
            "unexpected rejection message: {}",
            err.message
        );
    }

    #[test]
    fn live_submit_transaction_alias_accepts_object_payload() {
        let router = live_router();
        let tx_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let (sig, pk, from) = dummy_sig_for_tx_id(tx_id);
        let resp = rpc_call(
            &router,
            "submitTransaction",
            json!({
                "tx_id": tx_id,
                "from": from,
                "to": "wallet.beta",
                "amount_zion": 25,
                "fee_zion": 5,
                "nonce": 1,
                "signature": sig,
                "public_key": pk,
            }),
        );
        assert!(
            resp.error.is_none(),
            "submitTransaction failed: {:?}",
            resp.error
        );
        assert_eq!(resp.result.unwrap()["accepted"], true);
    }

    #[test]
    fn live_submit_account_transaction_alias_accepts_object_payload() {
        let router = live_router();
        let tx_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let (sig, pk, from) = dummy_sig_for_tx_id(tx_id);
        let resp = rpc_call(
            &router,
            "submitAccountTransaction",
            json!({
                "tx_id": tx_id,
                "from": from,
                "to": "wallet.beta",
                "amount_zion": 30,
                "fee_zion": 5,
                "nonce": 1,
                "signature": sig,
                "public_key": pk,
            }),
        );
        assert!(
            resp.error.is_none(),
            "submitAccountTransaction failed: {:?}",
            resp.error
        );
        assert_eq!(resp.result.unwrap()["accepted"], true);
    }

    #[test]
    fn live_router_method_count() {
        let router = live_router();
        assert_eq!(router.method_count(), 27);
    }

    #[test]
    fn live_get_supply_info() {
        let router = live_router();
        let resp = rpc_call(&router, "getSupplyInfo", json!(null));
        assert!(
            resp.error.is_none(),
            "getSupplyInfo failed: {:?}",
            resp.error
        );
        let r = resp.result.unwrap();
        assert_eq!(r["total_supply_zion"], 144_000_000_000u64);
        assert_eq!(r["premine_zion"], 16_780_000_000u64);
        assert_eq!(r["mining_emission_zion"], 127_220_000_000u64);
        assert_eq!(r["height"], 0);
        assert!(r["block_reward_atomic"].as_u64().unwrap() > 0);
        assert!(r["total_supply_atomic"].is_string());
        assert!(r["circulating_supply_atomic"].is_string());
    }

    #[test]
    fn live_supply_emission_invariant() {
        let router = live_router();
        let resp = rpc_call(&router, "getSupplyInfo", json!(null));
        let r = resp.result.unwrap();
        let total: u128 = r["total_supply_atomic"].as_str().unwrap().parse().unwrap();
        let premine: u128 = r["premine_atomic"].as_str().unwrap().parse().unwrap();
        let emission: u128 = r["mining_emission_atomic"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(emission, total - premine);
    }

    #[test]
    fn live_get_utxos_returns_empty_for_unknown_address() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getUtxos",
            json!({"address": "zion1nobody000000000000000000000000000000000"}),
        );
        assert!(resp.error.is_none(), "getUtxos failed: {:?}", resp.error);
        let result = resp.result.unwrap();
        assert_eq!(result["count"], 0);
        assert_eq!(result["total_amount"], 0);
        assert!(result["utxos"].as_array().unwrap().is_empty());
    }

    #[test]
    fn live_get_transaction_history_returns_empty_for_unknown_address() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getTransactionHistory",
            json!({"address": "zion1nobody000000000000000000000000000000000"}),
        );
        assert!(
            resp.error.is_none(),
            "getTransactionHistory failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["total"], 0);
        assert!(result["transactions"].as_array().unwrap().is_empty());
        assert_eq!(result["has_more"], false);
    }

    #[test]
    fn live_get_transaction_history_includes_genesis_premine() {
        let router = live_router();
        // Genesis block has account-model premine transactions
        // Use the first premine address from genesis.rs (hard reset 2026-07-06)
        let resp = rpc_call(
            &router,
            "getTransactionHistory",
            json!({"address": "zion1n3t6v6w3m8g4v6q8g7h7j4j6f7s8q2m7g7un8u0", "limit": 100}),
        );
        assert!(
            resp.error.is_none(),
            "getTransactionHistory failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        let txs = result["transactions"].as_array().unwrap();
        // Genesis premine should appear in history
        assert!(!txs.is_empty(), "expected genesis premine txs in history");
        // All should be from block 0 (genesis)
        for tx in txs {
            assert_eq!(tx["block_height"], 0);
            assert_eq!(tx["confirmed"], true);
        }
    }

    #[test]
    fn live_get_transaction_history_pagination_works() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getTransactionHistory",
            json!({"address": "zion153e378e4x0g6s380h2h8z4t506g5s323f5se8g5", "limit": 1, "offset": 0}),
        );
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let txs = result["transactions"].as_array().unwrap();
        assert!(txs.len() <= 1);
        assert_eq!(result["limit"], 1);
    }

    #[test]
    fn live_get_utxos_rejects_account_address() {
        let router = live_router();
        let resp = rpc_call(&router, "getUtxos", json!({"address": "wallet.alpha"}));
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_ADDRESS);
    }

    #[test]
    fn live_get_balance_at_height_genesis() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getBalanceAtHeight",
            json!({
                "account": "wallet.alpha",
                "height": 0
            }),
        );
        assert!(
            resp.error.is_none(),
            "getBalanceAtHeight failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["height"], 0);
        assert_eq!(result["balance_zion"], "0");
    }

    #[test]
    fn live_get_bridge_locks_empty_at_genesis() {
        let router = live_router();
        let resp = rpc_call(
            &router,
            "getBridgeLocks",
            json!({
                "from_height": 0,
                "to_height": 0
            }),
        );
        assert!(
            resp.error.is_none(),
            "getBridgeLocks failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["count"], 0);
        assert!(result["locks"].as_array().unwrap().is_empty());
    }

    #[test]
    fn live_get_bridge_vault_balance_has_genesis_seed() {
        let router = live_router();
        let resp = rpc_call(&router, "getBridgeVaultBalance", json!(null));
        assert!(
            resp.error.is_none(),
            "getBridgeVaultBalance failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        // Hard reset 2026-07-06: bridge vault UTXO seed (100M ZION) is now
        // on the same keyless address as fee::BRIDGE_VAULT_ADDRESS.
        assert_eq!(result["address"], fee::BRIDGE_VAULT_ADDRESS);
        // Genesis seeds 100M ZION = 100_000_000_000_000_000_000 flowers
        assert_eq!(result["balance_flowers"], "100000000000000000000");
    }

    #[test]
    #[ignore = "hard reset 2026-07-06: genesis now seeds 100M ZION into bridge vault, so vault is never empty in live_router or bridge_unlock_ready_router"]
    fn live_submit_bridge_unlock_rejects_when_vault_is_empty() {
        let _guard = BRIDGE_ENV_MUTEX.lock().expect("bridge env mutex lock");
        let router = live_router();
        let recipient = crate::crypto::derive_address(&[7u8; 32]);
        let (validator_proofs, allowlist) = make_bridge_validator_proofs(
            &recipient,
            1_000_000u64,
            "base-sepolia",
            "burn-empty",
            "0xempty",
        );
        std::env::set_var("ZION_BRIDGE_VALIDATOR_PUBKEYS", allowlist);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_THRESHOLD", "3");
        let resp = rpc_call(
            &router,
            "submitBridgeUnlock",
            json!({
                "recipient": recipient,
                "amount_flowers": 1_000_000u64,
                "burn_id": "burn-empty",
                "evm_chain": "base-sepolia",
                "evm_tx_hash": "0xempty",
                "validator_proofs": validator_proofs
            }),
        );
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_PUBKEYS");
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_THRESHOLD");
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, TX_REJECTED);
        assert!(err.message.contains("insufficient"));
    }

    #[test]
    fn live_submit_bridge_unlock_accepts_funded_vault_request() {
        let _guard = BRIDGE_ENV_MUTEX.lock().expect("bridge env mutex lock");
        let router = bridge_unlock_ready_router(2_000_000_000_000);
        let recipient = crate::crypto::derive_address(&[9u8; 32]);
        let (validator_proofs, allowlist) = make_bridge_validator_proofs(
            &recipient,
            1_000_000u64,
            "base-sepolia",
            "burn-1",
            "0xabc123",
        );
        std::env::set_var("ZION_BRIDGE_VALIDATOR_PUBKEYS", allowlist);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_THRESHOLD", "3");
        let resp = rpc_call(
            &router,
            "submitBridgeUnlock",
            json!({
                "recipient": recipient,
                "amount_flowers": 1_000_000u64,
                "burn_id": "burn-1",
                "evm_chain": "base-sepolia",
                "evm_tx_hash": "0xabc123",
                "validator_proofs": validator_proofs
            }),
        );
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_PUBKEYS");
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_THRESHOLD");
        assert!(
            resp.error.is_none(),
            "submitBridgeUnlock failed: {:?}",
            resp.error
        );
        let result = resp.result.unwrap();
        assert_eq!(result["accepted"], true);
        assert!(result["tx_id"].as_str().is_some());
    }

    #[test]
    fn live_submit_bridge_unlock_rejects_replay_key_reuse() {
        let _guard = BRIDGE_ENV_MUTEX.lock().expect("bridge env mutex lock");
        let router = bridge_unlock_ready_router(3_000_000_000_000);
        let recipient = crate::crypto::derive_address(&[11u8; 32]);
        let (validator_proofs, allowlist) = make_bridge_validator_proofs(
            &recipient,
            1_000_000u64,
            "base-sepolia",
            "burn-replay",
            "0xreplay",
        );
        std::env::set_var("ZION_BRIDGE_VALIDATOR_PUBKEYS", allowlist);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_THRESHOLD", "3");
        let params = json!({
            "recipient": recipient,
            "amount_flowers": 1_000_000u64,
            "burn_id": "burn-replay",
            "evm_chain": "base-sepolia",
            "evm_tx_hash": "0xreplay",
            "validator_proofs": validator_proofs
        });

        let first = rpc_call(&router, "submitBridgeUnlock", params.clone());
        assert!(
            first.error.is_none(),
            "initial unlock failed: {:?}",
            first.error
        );

        let second = rpc_call(&router, "submitBridgeUnlock", params);
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_PUBKEYS");
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_THRESHOLD");
        assert!(second.error.is_some());
        let err = second.error.unwrap();
        assert_eq!(err.code, TX_REJECTED);
        assert!(err.message.contains("replay key"));
    }

    // ── F4 (audit): bridge multisig L1 enforcement, RPC layer ──────────

    #[test]
    fn live_submit_bridge_unlock_rejects_synthetic_proofs() {
        let _guard = BRIDGE_ENV_MUTEX.lock().expect("bridge env mutex lock");
        let router = bridge_unlock_ready_router(2_000_000_000_000);
        let recipient = crate::crypto::derive_address(&[13u8; 32]);
        let (mut validator_proofs, allowlist) = make_bridge_validator_proofs(
            &recipient,
            1_000_000u64,
            "base-sepolia",
            "burn-synth",
            "0xsynth",
        );
        // Flip one proof to `synthetic: true` — must be rejected even though
        // the other 2 carry valid signatures, because synthetic proofs are
        // never crypto-verified and therefore cannot count toward the
        // multisig threshold.
        validator_proofs[2]["synthetic"] = json!(true);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_PUBKEYS", allowlist);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_THRESHOLD", "3");
        let resp = rpc_call(
            &router,
            "submitBridgeUnlock",
            json!({
                "recipient": recipient,
                "amount_flowers": 1_000_000u64,
                "burn_id": "burn-synth",
                "evm_chain": "base-sepolia",
                "evm_tx_hash": "0xsynth",
                "validator_proofs": validator_proofs,
            }),
        );
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_PUBKEYS");
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_THRESHOLD");
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(
            err.message.contains("synthetic"),
            "expected synthetic-rejection, got: {}",
            err.message,
        );
    }

    #[test]
    fn live_submit_bridge_unlock_rejects_tampered_amount() {
        let _guard = BRIDGE_ENV_MUTEX.lock().expect("bridge env mutex lock");
        let router = bridge_unlock_ready_router(5_000_000_000_000);
        let recipient = crate::crypto::derive_address(&[15u8; 32]);
        // Build proofs that sign for amount = 1 trillion flowers …
        let signed_amount: u64 = 1_000_000;
        let (validator_proofs, allowlist) = make_bridge_validator_proofs(
            &recipient,
            signed_amount,
            "base-sepolia",
            "burn-tamper",
            "0xtamper",
        );
        // … but submit a request asking to unlock 4 trillion. The
        // operation_message reconstructed from the request will diverge
        // from what the validators signed, so signature verification must
        // fail.
        std::env::set_var("ZION_BRIDGE_VALIDATOR_PUBKEYS", allowlist);
        std::env::set_var("ZION_BRIDGE_VALIDATOR_THRESHOLD", "3");
        let resp = rpc_call(
            &router,
            "submitBridgeUnlock",
            json!({
                "recipient": recipient,
                "amount_flowers": 4_000_000_000_000u64,
                "burn_id": "burn-tamper",
                "evm_chain": "base-sepolia",
                "evm_tx_hash": "0xtamper",
                "validator_proofs": validator_proofs,
            }),
        );
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_PUBKEYS");
        std::env::remove_var("ZION_BRIDGE_VALIDATOR_THRESHOLD");
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(
            err.message.contains("operation_message mismatch")
                || err.message.contains("failed secp256k1"),
            "expected tamper-rejection, got: {}",
            err.message,
        );
    }
}
