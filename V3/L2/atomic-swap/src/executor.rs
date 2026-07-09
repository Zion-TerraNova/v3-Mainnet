//! Transaction executor — signs and submits L1 ZION releases from the escrow.
//!
//! Mirrors the V3 UTXO wallet flow in L1 core:
//! fetches UTXOs for the escrow address, coin-selects, builds a TX,
//! signs with the escrow Ed25519 key, and submits through raw TCP JSON-RPC.

use crate::config::SwapConfig;
use crate::db::SwapDb;
use crate::error::{SwapError, SwapResult};
use crate::types::{
    bytes_to_hex, canonical_utxo_tx_hash, normalize_rpc_addr, zion_address_from_public_key,
    L1SpendableUtxo, L1TxInput, L1TxOutput, L1UtxoTransaction, RpcResponse, SwapHash, SwapPreimage,
};
use ed25519_dalek::{Signer, SigningKey};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::{info, warn};

// ─── SwapExecutor ─────────────────────────────────────────────────────────────

pub struct SwapExecutor {
    cfg: Arc<SwapConfig>,
    /// Cached escrow address (derived from key on startup).
    pub escrow_address: String,
    /// Ed25519 signing key bytes.
    signing_key_bytes: [u8; 32],
}

impl Drop for SwapExecutor {
    /// Zero the signing key bytes on drop so they are not recoverable
    /// from a core/process dump after the executor is shut down (L2 audit).
    fn drop(&mut self) {
        // Use a simple volatile-style zero; we avoid pulling in the
        // `zeroize` crate to keep the dependency footprint unchanged.
        for b in self.signing_key_bytes.iter_mut() {
            // Write in a way the optimizer is unlikely to elide.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

impl SwapExecutor {
    /// Create dummy executor for tests (in-memory mode, fake address).
    pub fn new_dummy() -> Self {
        Self {
            cfg: Arc::new(SwapConfig {
                swap: crate::config::SwapIdentity {
                    name: "Dummy Executor".into(),
                    network: "testnet".into(),
                    min_lock_flowers: 1_000,
                    max_lock_atomic: 10_000_000_000_000,
                    release_fee_atomic: 2_000,
                },
                l1: crate::config::L1Config {
                    rpc_url: "dummy".into(),
                    rpc_token: None,
                    escrow_key_hex: None,
                    scan_batch_size: 10,
                    poll_interval_secs: 5,
                },
                database: Default::default(),
                api: Default::default(),
                refund: Default::default(),
                evm_watcher: None,
            }),
            escrow_address: "zion1dummyescrowaddress000000000000000001".into(),
            signing_key_bytes: [42u8; 32],
        }
    }

    /// Create executor — reads escrow key from config / env.
    pub fn new(cfg: Arc<SwapConfig>) -> SwapResult<Self> {
        let key_hex = cfg
            .escrow_key_hex()
            .ok_or_else(|| SwapError::InvalidEscrowKey {
                msg: "ZION_SWAP_ESCROW_KEY not set".into(),
            })?;
        if key_hex.len() != 64 {
            return Err(SwapError::InvalidEscrowKey {
                msg: "Key must be 64 hex chars (32 bytes)".into(),
            });
        }
        let key_bytes: Vec<u8> =
            hex::decode(&key_hex).map_err(|_| SwapError::InvalidEscrowKey {
                msg: "Key is not valid hex".into(),
            })?;
        let signing_key_bytes: [u8; 32] =
            key_bytes
                .try_into()
                .map_err(|_| SwapError::InvalidEscrowKey {
                    msg: "Key must be exactly 32 bytes".into(),
                })?;

        let signing_key = SigningKey::from_bytes(&signing_key_bytes);
        let pk = signing_key.verifying_key();
        let escrow_address = zion_address_from_public_key(pk.as_bytes()).ok_or_else(|| {
            SwapError::InvalidEscrowKey {
                msg: "Escrow key produced invalid zion1 address".into(),
            }
        })?;

        Ok(Self {
            cfg,
            escrow_address,
            signing_key_bytes,
        })
    }

    // ── Claim (S-03) ──────────────────────────────────────────────────────

    /// Release ZION to the claimer after verifying the preimage.
    pub async fn execute_claim(
        &self,
        db: &SwapDb,
        hash_hex: &str,
        preimage_hex: &str,
        recipient: &str,
    ) -> SwapResult<()> {
        // 1. Fetch HTLC record
        let rec = db
            .get_htlc(hash_hex)?
            .ok_or_else(|| SwapError::HtlcNotFound {
                hash_hex: hash_hex.to_string(),
            })?;

        // 2. Guard: already settled?
        if rec.is_terminal() {
            return Err(SwapError::AlreadySettled {
                hash_hex: hash_hex.to_string(),
                state: rec.state.to_string(),
            });
        }

        // 3. Guard: timelock expired?
        if rec.is_expired() {
            return Err(SwapError::TimelockExpired {
                hash_hex: hash_hex.to_string(),
            });
        }

        // 3b. Guard: pre-committed claimant (C1 security patch).
        // When the LOCK memo carried a claimant_address, only that address
        // may receive the released ZION — prevents front-running by observers
        // who steal the preimage from the counterparty chain.
        if let Some(ref expected) = rec.claimant_address {
            if expected != recipient {
                warn!(
                    "CLAIM {hash_hex} recipient {recipient} != committed claimant {expected} — rejected (C1)"
                );
                return Err(SwapError::Internal(format!(
                    "recipient {recipient} does not match committed claimant {expected}"
                )));
            }
        }

        // 4. Verify preimage
        let preimage_bytes = hex::decode(preimage_hex)?;
        let preimage_arr: [u8; 32] = preimage_bytes
            .try_into()
            .map_err(|_| SwapError::Internal("Preimage must be 32 bytes".into()))?;
        let preimage = SwapPreimage(preimage_arr);
        let computed_hash = preimage.hash();
        let expected_hash = SwapHash::from_hex(hash_hex)
            .ok_or_else(|| SwapError::Internal("Invalid hash_hex".into()))?;
        if computed_hash != expected_hash {
            return Err(SwapError::PreimageMismatch);
        }

        // 5. Release ZION
        let release_fee = self.cfg.swap.release_fee_atomic;
        let amount = rec.amount_flowers.saturating_sub(release_fee);
        let tx_id = self
            .submit_release(amount, recipient, Some(hash_hex))
            .await?;

        // 6. Persist
        db.mark_claimed(hash_hex, &tx_id, recipient, preimage_hex)?;
        info!("✅ CLAIM settled hash={hash_hex} → recipient={recipient} tx={tx_id}");
        Ok(())
    }

    // ── Refund (S-04) ─────────────────────────────────────────────────────

    /// Refund ZION to the original locker after timelock expiry.
    pub async fn execute_refund(&self, db: &SwapDb, hash_hex: &str) -> SwapResult<()> {
        let rec = db
            .get_htlc(hash_hex)?
            .ok_or_else(|| SwapError::HtlcNotFound {
                hash_hex: hash_hex.to_string(),
            })?;

        if rec.is_terminal() {
            return Err(SwapError::AlreadySettled {
                hash_hex: hash_hex.to_string(),
                state: rec.state.to_string(),
            });
        }

        if !rec.is_expired() {
            return Err(SwapError::TimelockActive {
                hash_hex: hash_hex.to_string(),
                expires_at: rec.expires_at,
            });
        }

        let release_fee = self.cfg.swap.release_fee_atomic;
        let amount = rec.amount_flowers.saturating_sub(release_fee);
        let tx_id = self
            .submit_release(amount, &rec.locker_address, Some(hash_hex))
            .await?;

        db.mark_refunded(hash_hex, &tx_id)?;
        info!(
            "↩️  REFUND settled hash={hash_hex} → locker={} tx={tx_id}",
            rec.locker_address
        );
        Ok(())
    }

    // ── L1 TX builder + submitter ─────────────────────────────────────────

    async fn submit_release(
        &self,
        amount: u64,
        recipient: &str,
        memo_hash: Option<&str>,
    ) -> SwapResult<String> {
        let signing_key = SigningKey::from_bytes(&self.signing_key_bytes);
        let pk = signing_key.verifying_key();
        let public_key = pk.as_bytes().to_vec();

        // ── Fetch UTXOs for escrow ────────────────────────────────────────
        let utxos = self.fetch_utxos(&self.escrow_address).await?;

        let fee: u64 = 1_000;
        let needed = amount.saturating_add(fee);

        let mut sorted = utxos;
        sorted.sort_by(|a, b| b.amount.cmp(&a.amount));

        let mut selected: Vec<L1SpendableUtxo> = Vec::new();
        let mut total_in: u64 = 0;
        for utxo in sorted {
            total_in += utxo.amount;
            selected.push(utxo);
            if total_in >= needed {
                break;
            }
        }

        if total_in < needed {
            return Err(SwapError::InsufficientBalance {
                have: total_in,
                need: needed,
            });
        }

        let change = total_in - amount - fee;

        // ── Build TX JSON ─────────────────────────────────────────────────
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let inputs: Vec<L1TxInput> = selected
            .iter()
            .map(|utxo| {
                let prev_tx_hash = decode_tx_hash(&utxo.tx_hash)?;
                Ok(L1TxInput {
                    prev_tx_hash,
                    output_index: utxo.output_index,
                    signature: vec![],
                    public_key: public_key.clone(),
                })
            })
            .collect::<SwapResult<Vec<_>>>()?;

        let memo_str = memo_hash.map(|h| format!("SWAP:RELEASE:{h}"));
        let mut outputs = vec![L1TxOutput {
            amount,
            address: recipient.to_string(),
            memo: memo_str,
        }];
        if change > 0 {
            outputs.push(L1TxOutput {
                amount: change,
                address: self.escrow_address.clone(),
                memo: None,
            });
        }

        let mut tx = L1UtxoTransaction {
            id: [0u8; 32],
            version: 1,
            inputs,
            outputs,
            fee,
            timestamp: ts,
        };
        tx.id = canonical_utxo_tx_hash(tx.version, &tx.inputs, &tx.outputs, tx.fee, tx.timestamp);

        for input in &mut tx.inputs {
            input.signature = signing_key.sign(&tx.id).to_bytes().to_vec();
        }

        self.submit_transaction(&tx).await
    }

    // ── UTXO fetch ────────────────────────────────────────────────────────

    async fn fetch_utxos(&self, address: &str) -> SwapResult<Vec<L1SpendableUtxo>> {
        if self.cfg.l1.rpc_url == "dummy" {
            return Ok(vec![L1SpendableUtxo {
                tx_hash: "0".repeat(64),
                output_index: 0,
                amount: 10_000_000_000_000, // 10 ZION
                address: address.to_string(),
                height: 100,
            }]);
        }

        #[derive(Deserialize)]
        struct UtxosResponse {
            utxos: Vec<L1SpendableUtxo>,
        }

        let response: UtxosResponse = self.rpc("getUtxos", json!({ "address": address })).await?;
        Ok(response.utxos)
    }

    async fn submit_transaction(&self, transaction: &L1UtxoTransaction) -> SwapResult<String> {
        if self.cfg.l1.rpc_url == "dummy" {
            return Ok(bytes_to_hex(&transaction.id));
        }

        #[derive(Deserialize)]
        struct SubmitTransactionResponse {
            accepted: bool,
            tx_id: String,
        }

        let response: SubmitTransactionResponse = self
            .rpc("sendRawTransaction", json!({ "transaction": transaction }))
            .await?;
        if !response.accepted {
            warn!("sendRawTransaction returned accepted=false");
            return Err(SwapError::L1Rpc(
                "sendRawTransaction returned accepted=false".into(),
            ));
        }
        Ok(if response.tx_id.is_empty() {
            bytes_to_hex(&transaction.id)
        } else {
            response.tx_id
        })
    }

    async fn rpc<T: DeserializeOwned>(&self, method: &str, params: Value) -> SwapResult<T> {
        let address = normalize_rpc_addr(&self.cfg.l1.rpc_url);
        let mut stream = TcpStream::connect(&address)
            .await
            .map_err(|e| SwapError::L1Rpc(format!("RPC connect failed: {e}")))?;

        let request = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        }))
        .map_err(|e| SwapError::L1Rpc(format!("RPC encode failed: {e}")))?;

        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| SwapError::L1Rpc(format!("RPC write failed: {e}")))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| SwapError::L1Rpc(format!("RPC newline write failed: {e}")))?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| SwapError::L1Rpc(format!("RPC read failed: {e}")))?;

        let rpc_resp: RpcResponse<T> = serde_json::from_str(line.trim())
            .map_err(|e| SwapError::L1Rpc(format!("RPC parse failed: {e}")))?;
        if let Some(err) = rpc_resp.error {
            return Err(SwapError::L1Rpc(format!("RPC error: {err}")));
        }

        rpc_resp
            .result
            .ok_or_else(|| SwapError::L1Rpc("RPC returned null result".into()))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn decode_tx_hash(value: &str) -> SwapResult<[u8; 32]> {
    let bytes = hex::decode(value)?;
    bytes
        .try_into()
        .map_err(|_| SwapError::Internal(format!("Invalid UTXO tx hash length for {value}")))
}
