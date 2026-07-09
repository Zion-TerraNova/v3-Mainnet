//! Relayer — submits cross-chain proofs for bridge operations.
//!
//! - L1→EVM: After L1 lock is confirmed + finalized, submits `submitLockProof()` to ZIONBridge.sol
//! - EVM→L1: After wZION burn is confirmed, submits L1 unlock TX + `confirmBurnRelease()` to ZIONBridge.sol

use crate::config::BridgeConfig;
use crate::config::ValidatorConfig;
use crate::db::BridgeDb;
use crate::evm_rpc::EvmHttpClient;
use crate::evm_tx::{
    build_and_sign_eip1559_tx, derive_evm_address, encode_confirm_burn_release,
    encode_execute_timelocked_mint, encode_submit_lock_proof, hash_to_bytes32,
};
use crate::metrics::BridgeMetrics;
use crate::rate_limiter::{RateLimitResult, RateLimiter};
use crate::types::{BridgeStatus, EvmBurnEvent, L1LockEvent};
use anyhow::Result;
use k256::ecdsa::{signature::Signer, Signature, SigningKey};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use zeroize::Zeroizing;

/// Gas limit safety margin (multiply estimate by this fraction numerator/denominator).
const GAS_MARGIN_NUM: u64 = 130; // 130%
const GAS_MARGIN_DEN: u64 = 100;

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    #[serde(default)]
    error: Option<Value>,
}

/// Load the validator private key securely.
///
/// Priority:
///   1. `ZION_VALIDATOR_PRIVATE_KEY` env var (preferred for containers/CI)
///   2. File at `config.validator.private_key_file`
///
/// The returned `Zeroizing<String>` is automatically wiped from memory when dropped.
fn load_validator_key(config: &ValidatorConfig) -> anyhow::Result<Zeroizing<String>> {
    if let Ok(key) = std::env::var("ZION_VALIDATOR_PRIVATE_KEY") {
        if !key.trim().is_empty() {
            tracing::debug!("🔑 Validator key loaded from ZION_VALIDATOR_PRIVATE_KEY env var");
            return Ok(Zeroizing::new(key.trim().to_string()));
        }
    }

    let path = &config.private_key_file;

    // Unix: enforce 0o600 file permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path)
            .map_err(|e| anyhow::anyhow!("Cannot stat key file {:?}: {}", path, e))?;
        let mode = meta.mode() & 0o777;
        if mode != 0o600 {
            anyhow::bail!(
                "🚨 Key file {:?} has insecure permissions {:o} — expected 0o600. \
                 Run: chmod 600 {:?}",
                path,
                mode,
                path
            );
        }
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Cannot read key file {:?}: {}", path, e))?;
    tracing::debug!("🔑 Validator key loaded from file {:?}", path);
    Ok(Zeroizing::new(raw.trim().to_string()))
}

/// Load all available validator private keys.
///
/// Reads `ZION_VALIDATOR_PRIVATE_KEY` (validator-1) plus optional
/// `ZION_VALIDATOR_PRIVATE_KEY_2` … `ZION_VALIDATOR_PRIVATE_KEY_5`.
/// The primary key is always first.  Additional keys are only used when
/// submitting multi-validator `submitLockProof` calls so a single relay
/// instance can satisfy a 5/5 threshold when all keys are co-located.
fn load_all_validator_keys(config: &ValidatorConfig) -> Vec<Zeroizing<String>> {
    let mut keys = Vec::new();
    if let Ok(k) = load_validator_key(config) {
        keys.push(k);
    }
    for n in 2..=5u8 {
        let var = format!("ZION_VALIDATOR_PRIVATE_KEY_{}", n);
        if let Ok(k) = std::env::var(&var) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                tracing::debug!("🔑 Additional validator key loaded from {} env var", var);
                keys.push(Zeroizing::new(k));
            }
        }
    }
    keys
}

/// Max relay retries before a lock/burn is permanently marked Failed.
const MAX_RELAY_RETRIES: u32 = 5;

/// Bridge relayer that processes lock and burn events.
pub struct Relayer {
    config: Arc<BridgeConfig>,
    rate_limiter: RateLimiter,
    metrics: Arc<BridgeMetrics>,
    db: Arc<BridgeDb>,
}

impl Relayer {
    pub fn new(config: Arc<BridgeConfig>, metrics: Arc<BridgeMetrics>, db: Arc<BridgeDb>) -> Self {
        let rate_limiter = RateLimiter::new(config.security.max_ops_per_hour);
        Self {
            config,
            rate_limiter,
            metrics,
            db,
        }
    }

    /// Start the relayer — listens for events from both watchers and submits proofs.
    ///
    /// Also runs a periodic `executeTimelockedMint` poller every 5 minutes.
    pub async fn run(
        &self,
        mut lock_rx: mpsc::Receiver<L1LockEvent>,
        mut burn_rx: mpsc::Receiver<EvmBurnEvent>,
    ) -> Result<()> {
        info!("🔗 Bridge Relayer started — processing lock and burn events");

        // Timelock poller: fires every 5 minutes
        let mut timelock_interval = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
        // Skip the first (immediate) tick so we don't fire before DB is warm
        timelock_interval.tick().await;

        loop {
            tokio::select! {
                // Process L1 lock events → submit mint proof to EVM
                Some(lock) = lock_rx.recv() => {
                    match self.handle_l1_lock(lock).await {
                        Ok(()) => {}
                        Err(e) => error!("Failed to handle L1 lock: {:?}", e),
                    }
                }

                // Process EVM burn events → submit unlock to L1
                Some(burn) = burn_rx.recv() => {
                    match self.handle_evm_burn(burn).await {
                        Ok(()) => {}
                        Err(e) => error!("Failed to handle EVM burn: {:?}", e),
                    }
                }

                // Periodic timelock poller
                _ = timelock_interval.tick() => {
                    if let Err(e) = self.poll_timelocked_ops().await {
                        error!("Timelock poller error: {:?}", e);
                    }
                }

                else => {
                    warn!("Relayer: all channels closed, shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handle an L1 lock event: encode + sign + submit `submitLockProof()` to ZIONBridge.
    async fn handle_l1_lock(&self, lock: L1LockEvent) -> Result<()> {
        info!(
            "📤 Processing L1→EVM lock: {} ZION → {} on {} (TX: {})",
            crate::types::conversion::flowers_to_zion_display_at(
                lock.amount_flowers,
                lock.l1_block_height
            ),
            lock.evm_recipient,
            lock.target_chain,
            lock.l1_tx_hash,
        );

        // Persist lock to DB so it survives restarts (INSERT OR IGNORE — safe to call again)
        if let Err(e) = self.db.insert_lock(&lock) {
            warn!("DB: failed to persist lock {}: {}", lock.l1_tx_hash, e);
        }
        let _ = self
            .db
            .update_lock_status(&lock.l1_tx_hash, BridgeStatus::Executing);

        // ── Rate limit ────────────────────────────────────────────────
        match self.rate_limiter.check_and_record(&lock.l1_sender) {
            RateLimitResult::Allowed => {}
            RateLimitResult::GlobalLimitReached { current, max } => {
                self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!(
                    "🚫 Rate limit: global hourly limit reached ({}/{}) — skipping lock TX: {}",
                    current,
                    max,
                    lock.l1_tx_hash,
                );
            }
            RateLimitResult::AddressLimitReached {
                address,
                current,
                max,
            } => {
                self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!(
                    "🚫 Rate limit: address {} exceeded per-address limit ({}/{}) — skipping lock TX: {}",
                    address, current, max, lock.l1_tx_hash,
                );
            }
        }

        // ── Validate recipient EVM address format ─────────────────────
        validate_evm_address(&lock.evm_recipient).map_err(|e| {
            anyhow::anyhow!("🚫 Invalid evm_recipient: {} — TX: {}", e, lock.l1_tx_hash)
        })?;

        // ── Amount security checks ────────────────────────────────────
        let wei: u128 = lock.amount_wzion_wei.parse().unwrap_or(0);
        let max_single: u128 = self
            .config
            .security
            .max_single_amount
            .parse()
            .unwrap_or(u128::MAX);
        let min_amount: u128 = self.config.security.min_bridge_amount.parse().unwrap_or(0);

        if wei < min_amount {
            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!(
                "🚫 Amount below minimum: {} < {} (min_bridge_amount) — TX: {}",
                lock.amount_wzion_wei,
                self.config.security.min_bridge_amount,
                lock.l1_tx_hash,
            );
        }
        if wei > max_single {
            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!(
                "🚫 Amount exceeds max_single_amount: {} > {} — TX: {}",
                lock.amount_wzion_wei,
                self.config.security.max_single_amount,
                lock.l1_tx_hash,
            );
        }

        // ── Find EVM chain config ─────────────────────────────────────
        let chain_config = self
            .config
            .evm_chains
            .iter()
            .find(|c| c.chain_id == lock.target_chain && c.enabled)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Target chain '{}' not configured or disabled",
                    lock.target_chain
                )
            })?;

        // ── Reject bridge contract as recipient (prevents self-mint) ──
        if lock
            .evm_recipient
            .eq_ignore_ascii_case(&chain_config.bridge_contract_address)
        {
            anyhow::bail!(
                "🚫 evm_recipient is the bridge contract itself ({}). \
                 Lock TX memo must contain the user's EVM wallet, not the bridge address. \
                 Skipping TX: {}",
                lock.evm_recipient,
                lock.l1_tx_hash,
            );
        }
        if lock
            .evm_recipient
            .eq_ignore_ascii_case(&chain_config.wzion_address)
        {
            anyhow::bail!(
                "🚫 evm_recipient is the wZION token contract ({}). \
                 Lock TX memo must contain the user's EVM wallet, not the token address. \
                 Skipping TX: {}",
                lock.evm_recipient,
                lock.l1_tx_hash,
            );
        }

        // ── Load all available validator keys (multi-validator support) ──
        let all_keys = load_all_validator_keys(&self.config.validator);
        if all_keys.is_empty() {
            anyhow::bail!("No validator keys available — set ZION_VALIDATOR_PRIVATE_KEY");
        }
        info!("   Submitting with {} validator key(s)", all_keys.len());

        // ── Build calldata (same for all validators) ──────────────────
        let l1_tx_hash_bytes = hash_to_bytes32(&lock.l1_tx_hash);
        let calldata = encode_submit_lock_proof(
            &l1_tx_hash_bytes,
            &lock.evm_recipient,
            &lock.amount_wzion_wei,
            lock.l1_block_height,
            &lock.l1_sender,
        )?;
        let calldata_hex = format!("0x{}", hex::encode(&calldata));

        info!(
            "   Calldata: {} bytes — bridge: {}",
            calldata.len(),
            chain_config.bridge_contract_address
        );

        // ── Setup EVM HTTP client ─────────────────────────────────────
        let rpc_url = chain_config.effective_rpc_url(&self.config.ankr);
        let evm = EvmHttpClient::from_rpc_url(&rpc_url);

        // ── Get gas params once (shared across all validator TXs) ────
        // M3: retry once before falling back to conservative defaults so a
        // transient RPC blip does not underpay during congestion.
        let base_fee = match evm.get_gas_price().await {
            Ok(f) => f,
            Err(e) => {
                warn!("gas price RPC failed ({e}); retrying once");
                match evm.get_gas_price().await {
                    Ok(f) => f,
                    Err(e2) => {
                        warn!(
                            "gas price RPC retry failed ({e2}); using conservative 2 gwei fallback"
                        );
                        2_000_000_000
                    }
                }
            }
        };
        let priority_fee = match evm.get_max_priority_fee().await {
            Ok(f) => f,
            Err(e) => {
                warn!("priority fee RPC failed ({e}); retrying once");
                match evm.get_max_priority_fee().await {
                    Ok(f) => f,
                    Err(e2) => {
                        warn!("priority fee RPC retry failed ({e2}); using 1.5 gwei fallback");
                        1_500_000_000
                    }
                }
            }
        };
        let max_gas_gwei = chain_config.max_gas_gwei;
        let max_fee_cap = max_gas_gwei * 1_000_000_000;
        let max_fee = (2 * base_fee + priority_fee).min(max_fee_cap);
        let max_priority = priority_fee.min(max_fee);
        info!(
            "   Gas: base_fee={} gwei, priority={} gwei, max_fee={} gwei (cap={} gwei)",
            base_fee / 1_000_000_000,
            priority_fee / 1_000_000_000,
            max_fee / 1_000_000_000,
            max_gas_gwei,
        );

        // ── Submit submitLockProof for each validator key ─────────────
        let mut last_tx_hash = String::new();
        for (idx, key) in all_keys.iter().enumerate() {
            let validator_address = match derive_evm_address(key.as_str()) {
                Ok(a) => a,
                Err(e) => {
                    warn!(
                        "   Validator key {} — failed to derive address: {}",
                        idx + 1,
                        e
                    );
                    continue;
                }
            };
            info!("   Validator-{} address: {}", idx + 1, validator_address);

            let nonce = match evm.get_nonce(&validator_address).await {
                Ok(n) => n,
                Err(e) => {
                    warn!("   Validator-{} — failed to get nonce: {}", idx + 1, e);
                    continue;
                }
            };
            info!("   Validator-{} nonce: {}", idx + 1, nonce);

            let gas_estimate = evm
                .estimate_gas(
                    &validator_address,
                    &chain_config.bridge_contract_address,
                    &calldata_hex,
                )
                .await
                .unwrap_or(200_000);
            let gas_limit = gas_estimate * GAS_MARGIN_NUM / GAS_MARGIN_DEN;

            let raw_tx = match build_and_sign_eip1559_tx(
                chain_config.evm_chain_id,
                nonce,
                max_priority,
                max_fee,
                gas_limit,
                &chain_config.bridge_contract_address,
                &calldata,
                key.as_str(),
            ) {
                Ok(t) => t,
                Err(e) => {
                    warn!("   Validator-{} — failed to sign TX: {}", idx + 1, e);
                    continue;
                }
            };

            match evm.send_raw_transaction(&raw_tx).await {
                Ok(tx_hash) => {
                    info!(
                        "   ✅ submitLockProof TX submitted! hash: {} | validator-{} | chain: {} | bridge: {}",
                        tx_hash, idx + 1, chain_config.name, chain_config.bridge_contract_address,
                    );
                    last_tx_hash = tx_hash;
                    self.metrics
                        .evm_mints_submitted
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    warn!("   Validator-{} — submitLockProof failed: {}", idx + 1, e);
                }
            }

            // Small delay between submissions to avoid nonce races
            if idx + 1 < all_keys.len() {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        if last_tx_hash.is_empty() {
            let err_msg = format!(
                "All validator key submissions failed for TX: {}",
                lock.l1_tx_hash
            );
            // Persist failure so recovery loop can retry
            let retry_count = self
                .db
                .increment_lock_retry(&lock.l1_tx_hash, &err_msg)
                .unwrap_or(0);
            if retry_count >= MAX_RELAY_RETRIES {
                error!("   ☠️ Lock {} permanently failed after {} retries — manual intervention required",
                    lock.l1_tx_hash, retry_count);
                let _ = self
                    .db
                    .update_lock_status(&lock.l1_tx_hash, BridgeStatus::Failed);
            } else {
                warn!(
                    "   ⚠️ Lock {} failed (retry {}/{}), will be retried on next startup",
                    lock.l1_tx_hash, retry_count, MAX_RELAY_RETRIES
                );
                let _ = self
                    .db
                    .update_lock_status(&lock.l1_tx_hash, BridgeStatus::Failed);
            }
            self.metrics
                .errors
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            anyhow::bail!("{}", err_msg);
        }
        // Mark as executing (confirmed submission)
        let _ = self
            .db
            .update_lock_status(&lock.l1_tx_hash, BridgeStatus::Executing);
        let tx_hash = last_tx_hash;

        // ── Poll for receipt ──────────────────────────────────────────
        tokio::spawn({
            let evm_url = rpc_url.to_string();
            let tx = tx_hash.clone();
            let chain_name = chain_config.name.clone();
            let metrics = Arc::clone(&self.metrics);
            let db = Arc::clone(&self.db);
            let l1_tx_hash = lock.l1_tx_hash.clone();
            async move {
                let evm2 = EvmHttpClient::from_rpc_url(&evm_url);
                for attempt in 1..=20 {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                    match evm2.get_receipt(&tx).await {
                        Ok(Some(receipt)) => {
                            let status = receipt["status"].as_str().unwrap_or("0x0");
                            if status == "0x1" {
                                info!(
                                    "   🟢 submitLockProof CONFIRMED on {} (attempt {}) — tx: {}",
                                    chain_name, attempt, tx
                                );
                                metrics.evm_mints_confirmed.fetch_add(1, Ordering::Relaxed);
                                let _ = db.update_lock_status(&l1_tx_hash, BridgeStatus::Completed);
                            } else {
                                error!(
                                    "   🔴 submitLockProof REVERTED on {} (attempt {}) — tx: {}",
                                    chain_name, attempt, tx
                                );
                                metrics.errors.fetch_add(1, Ordering::Relaxed);
                                let _ = db.update_lock_status(&l1_tx_hash, BridgeStatus::Failed);
                            }
                            return;
                        }
                        Ok(None) => {
                            if attempt < 20 {
                                continue; // not mined yet
                            }
                            warn!("   ⏱️ submitLockProof receipt not found after 20 attempts — tx: {}", tx);
                            let _ = db.update_lock_status(&l1_tx_hash, BridgeStatus::Failed);
                        }
                        Err(e) => {
                            warn!("   Receipt poll error (attempt {}): {}", attempt, e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle an EVM burn event: submit L1 unlock TX and confirm on ZIONBridge.
    async fn handle_evm_burn(&self, burn: EvmBurnEvent) -> Result<()> {
        info!(
            "📤 Processing EVM→L1 burn: {} wZION → {} on L1 (chain: {}, burn_id: {})",
            burn.amount_wzion_wei, burn.l1_recipient, burn.evm_chain, burn.burn_id,
        );

        // Persist burn to DB (INSERT OR IGNORE — safe to call multiple times)
        if let Err(e) = self.db.insert_burn(&burn) {
            warn!("DB: failed to persist burn {}: {}", burn.burn_id, e);
        }
        let _ = self
            .db
            .update_burn_status(&burn.burn_id, BridgeStatus::Executing);

        // ── Rate limit ────────────────────────────────────────────────
        match self.rate_limiter.check_and_record(&burn.evm_burner) {
            RateLimitResult::Allowed => {}
            RateLimitResult::GlobalLimitReached { current, max } => {
                self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!(
                    "🚫 Rate limit: global hourly limit reached ({}/{}) — skipping burn: {}",
                    current,
                    max,
                    burn.burn_id,
                );
            }
            RateLimitResult::AddressLimitReached {
                address,
                current,
                max,
            } => {
                self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                anyhow::bail!(
                    "🚫 Rate limit: address {} exceeded per-address limit ({}/{}) — skipping burn: {}",
                    address, current, max, burn.burn_id,
                );
            }
        }

        // ── Validate L1 recipient address format ──────────────────────
        validate_l1_address(&burn.l1_recipient).map_err(|e| {
            anyhow::anyhow!("🚫 Invalid l1_recipient: {} — burn_id: {}", e, burn.burn_id)
        })?;

        // ── Amount security checks ────────────────────────────────────
        let wei: u128 = burn.amount_wzion_wei.parse().unwrap_or(0);
        let max_single: u128 = self
            .config
            .security
            .max_single_amount
            .parse()
            .unwrap_or(u128::MAX);
        let min_amount: u128 = self.config.security.min_bridge_amount.parse().unwrap_or(0);

        if wei < min_amount {
            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!(
                "🚫 Burn amount below minimum: {} < {} — burn_id: {}",
                burn.amount_wzion_wei,
                self.config.security.min_bridge_amount,
                burn.burn_id,
            );
        }
        if wei > max_single {
            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::bail!(
                "🚫 Burn amount exceeds max_single_amount: {} > {} — burn_id: {}",
                burn.amount_wzion_wei,
                self.config.security.max_single_amount,
                burn.burn_id,
            );
        }

        let l1_amount = burn.amount_flowers;
        info!(
            "   Step 1: Submitting L1 unlock TX for {} ZION ({} atomic) to {}",
            crate::types::conversion::flowers_to_zion_display(l1_amount),
            l1_amount,
            burn.l1_recipient,
        );

        // Build real multisig proofs. Fail-closed: if fewer than `threshold`
        // real validator keys are available we abort before we touch L1.
        // Synthetic placeholder proofs were removed in this PR — L1 (F4, PR
        // #22) rejects them anyway, so emitting them is pure noise and wastes
        // an RPC round-trip.
        let validator_proofs = self.build_validator_proofs(&burn, l1_amount).map_err(|e| {
            self.metrics.errors.fetch_add(1, Ordering::Relaxed);
            anyhow::anyhow!(
                "🚫 Bridge unlock aborted: {} — burn_id: {}",
                e,
                burn.burn_id
            )
        })?;
        let proof_count = validator_proofs.len();

        let unlock_request = json!({
            "recipient": burn.l1_recipient,
            "amount_flowers": l1_amount,
            "burn_id": burn.burn_id,
            "evm_chain": burn.evm_chain,
            "evm_tx_hash": burn.evm_tx_hash,
            "validator_proofs": validator_proofs,
        });

        info!(
            "   Step 1a: Aggregated {} real validator signatures (threshold met). \
             Synthetic placeholder proofs are disabled — L1 would reject them.",
            proof_count
        );

        let l1_result: Value = self
            .l1_rpc("submitBridgeUnlock", unlock_request)
            .await
            .map_err(|e| anyhow::anyhow!("submitBridgeUnlock failed: {}", e))?;
        let l1_tx_hash = l1_result
            .get("tx_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        info!(
            "   ✅ L1 unlock TX submitted — L1 TX: {}, amount: {} ZION",
            l1_tx_hash,
            crate::types::conversion::flowers_to_zion_display(l1_amount),
        );
        self.metrics
            .l1_unlocks_submitted
            .fetch_add(1, Ordering::Relaxed);

        // ── Step 2: Confirm burn release on ZIONBridge EVM contract via Ankr ──
        let chain_config = self
            .config
            .evm_chains
            .iter()
            .find(|c| c.chain_id == burn.evm_chain && c.enabled)
            .ok_or_else(|| anyhow::anyhow!("Burn chain '{}' not configured", burn.evm_chain))?;

        let rpc_url = chain_config.effective_rpc_url(&self.config.ankr);
        let evm = EvmHttpClient::from_rpc_url(&rpc_url);

        // Verify burn tx receipt exists on EVM
        match evm.get_receipt(&burn.evm_tx_hash).await {
            Ok(Some(receipt)) => {
                let status = receipt["status"].as_str().unwrap_or("0x0");
                if status == "0x1" {
                    info!(
                        "   ✅ Burn TX confirmed on {} — tx: {}",
                        chain_config.name, burn.evm_tx_hash
                    );
                } else {
                    warn!(
                        "   ⚠️ Burn TX {} reverted on {} — skipping confirmBurnRelease",
                        burn.evm_tx_hash, chain_config.name
                    );
                    return Ok(());
                }
            }
            Ok(None) => {
                warn!(
                    "   ⚠️ Burn TX {} not yet mined on {} — deferring",
                    burn.evm_tx_hash, chain_config.name
                );
                return Ok(());
            }
            Err(e) => {
                warn!("   ⚠️ Failed to fetch burn receipt: {}", e);
            }
        }

        // ── Step 3: Submit confirmBurnRelease() to ZIONBridge EVM contract ──
        let key = load_validator_key(&self.config.validator)
            .map_err(|e| anyhow::anyhow!("Failed to load validator key: {}", e))?;
        let validator_address = derive_evm_address(key.as_str())?;
        info!("   Validator address: {}", validator_address);

        // ABI-encode confirmBurnRelease(bytes32 burnId, address evmBurner, uint256 amount, string l1Recipient)
        let burn_id_bytes = hash_to_bytes32(&burn.burn_id);
        let calldata = encode_confirm_burn_release(
            &burn_id_bytes,
            &burn.evm_burner,
            &burn.amount_wzion_wei,
            &burn.l1_recipient,
        )?;
        let calldata_hex = format!("0x{}", hex::encode(&calldata));

        info!(
            "   confirmBurnRelease calldata: {} bytes — bridge: {}",
            calldata.len(),
            chain_config.bridge_contract_address
        );

        // EVM HTTP client — use the chain's effective RPC (config override or Ankr fallback).
        let rpc_url = chain_config.effective_rpc_url(&self.config.ankr);
        let evm = EvmHttpClient::from_rpc_url(&rpc_url);

        // Get nonce + gas params
        let nonce = evm
            .get_nonce(&validator_address)
            .await
            .map_err(|e| anyhow::anyhow!("confirmBurnRelease: get_nonce failed: {}", e))?;
        let base_fee = evm.get_gas_price().await.unwrap_or(2_000_000_000);
        let priority_fee = evm.get_max_priority_fee().await.unwrap_or(1_500_000_000);
        let max_gas_gwei = chain_config.max_gas_gwei;
        let max_fee_cap = max_gas_gwei * 1_000_000_000;
        let max_fee = (2 * base_fee + priority_fee).min(max_fee_cap);
        let max_priority = priority_fee.min(max_fee);

        let gas_estimate = evm
            .estimate_gas(
                &validator_address,
                &chain_config.bridge_contract_address,
                &calldata_hex,
            )
            .await
            .unwrap_or(150_000);
        let gas_limit = gas_estimate * GAS_MARGIN_NUM / GAS_MARGIN_DEN;

        info!(
            "   Gas: nonce={} base_fee={} gwei priority={} gwei cap={} gwei estimate={} limit={}",
            nonce,
            base_fee / 1_000_000_000,
            priority_fee / 1_000_000_000,
            max_gas_gwei,
            gas_estimate,
            gas_limit,
        );

        // Build + sign + submit EIP-1559 TX
        let raw_tx = build_and_sign_eip1559_tx(
            chain_config.evm_chain_id,
            nonce,
            max_priority,
            max_fee,
            gas_limit,
            &chain_config.bridge_contract_address,
            &calldata,
            key.as_str(),
        )?;

        let cbr_tx_hash = evm
            .send_raw_transaction(&raw_tx)
            .await
            .map_err(|e| anyhow::anyhow!("confirmBurnRelease TX submit failed: {}", e))?;

        info!(
            "   ✅ confirmBurnRelease TX submitted! hash: {} | chain: {} | burn_id: {} | L1 TX: {}",
            cbr_tx_hash, chain_config.name, burn.burn_id, l1_tx_hash,
        );

        // Poll for receipt in background
        tokio::spawn({
            let evm_url = rpc_url.to_string();
            let tx = cbr_tx_hash.clone();
            let chain_name = chain_config.name.clone();
            let burn_id = burn.burn_id.clone();
            let metrics = Arc::clone(&self.metrics);
            let db = Arc::clone(&self.db);
            async move {
                let evm2 = EvmHttpClient::from_rpc_url(&evm_url);
                for attempt in 1..=20 {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                    match evm2.get_receipt(&tx).await {
                        Ok(Some(receipt)) => {
                            let status = receipt["status"].as_str().unwrap_or("0x0");
                            if status == "0x1" {
                                info!("   🟢 confirmBurnRelease CONFIRMED on {} (attempt {}) — burn_id: {} tx: {}",
                                    chain_name, attempt, burn_id, tx);
                                metrics.l1_unlocks_confirmed.fetch_add(1, Ordering::Relaxed);
                                let _ = db.update_burn_status(&burn_id, BridgeStatus::Completed);
                            } else {
                                error!("   🔴 confirmBurnRelease REVERTED on {} (attempt {}) — burn_id: {} tx: {}",
                                    chain_name, attempt, burn_id, tx);
                                metrics.errors.fetch_add(1, Ordering::Relaxed);
                                let _ = db.update_burn_status(&burn_id, BridgeStatus::Failed);
                            }
                            return;
                        }
                        Ok(None) => {
                            if attempt < 20 {
                                continue;
                            }
                            warn!("   ⏱️ confirmBurnRelease receipt not found after 20 attempts — tx: {}", tx);
                            let _ = db.update_burn_status(&burn_id, BridgeStatus::Failed);
                        }
                        Err(e) => {
                            warn!("   Receipt poll error (attempt {}): {}", attempt, e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Poll the `timelocked_ops` table and execute any that have passed their expiry.
    ///
    /// Should be called periodically (e.g. from a background task every 5 minutes).
    /// For each expired-but-pending timelocked op, this submits `executeTimelockedMint(bytes32)`
    /// to the ZIONBridge contract using validator-1's key.
    pub async fn poll_timelocked_ops(&self) -> Result<()> {
        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let expired = self.db.get_expired_timelocked_ops(&now)?;

        if expired.is_empty() {
            return Ok(());
        }

        info!(
            "⏰ Timelock poller: {} expired op(s) ready for executeTimelockedMint",
            expired.len()
        );

        for op in expired {
            info!(
                "⏰ Executing timelocked mint — l1_tx: {} chain: {} amount: {} recipient: {}",
                op.l1_tx_hash, op.evm_chain, op.amount_wzion_wei, op.evm_recipient,
            );

            // Find EVM chain config
            let chain_config = match self
                .config
                .evm_chains
                .iter()
                .find(|c| c.chain_id == op.evm_chain && c.enabled)
            {
                Some(c) => c,
                None => {
                    let e = format!("Chain '{}' not configured for timelocked op", op.evm_chain);
                    error!("   ⚠️ {}", e);
                    let _ = self.db.mark_timelocked_failed(&op.l1_tx_hash, &e);
                    continue;
                }
            };

            // Load validator-1 key
            let key = match load_validator_key(&self.config.validator) {
                Ok(k) => k,
                Err(e) => {
                    let msg = format!("Failed to load validator key: {}", e);
                    error!("   ⚠️ {}", msg);
                    let _ = self.db.mark_timelocked_failed(&op.l1_tx_hash, &msg);
                    continue;
                }
            };
            let validator_addr = match derive_evm_address(key.as_str()) {
                Ok(a) => a,
                Err(e) => {
                    error!("   ⚠️ Failed to derive validator address: {}", e);
                    continue;
                }
            };

            // Encode calldata: executeTimelockedMint(bytes32)
            let l1_hash_bytes = hash_to_bytes32(&op.l1_tx_hash);
            let calldata = encode_execute_timelocked_mint(&l1_hash_bytes);
            let calldata_hex = format!("0x{}", hex::encode(&calldata));

            let rpc_url = chain_config.effective_rpc_url(&self.config.ankr);
            let evm = EvmHttpClient::from_rpc_url(&rpc_url);

            let nonce = match evm.get_nonce(&validator_addr).await {
                Ok(n) => n,
                Err(e) => {
                    error!("   ⚠️ get_nonce failed for timelocked mint: {}", e);
                    continue;
                }
            };
            let base_fee = evm.get_gas_price().await.unwrap_or(2_000_000_000);
            let priority_fee = evm.get_max_priority_fee().await.unwrap_or(1_500_000_000);
            let max_fee_cap = chain_config.max_gas_gwei * 1_000_000_000;
            let max_fee = (2 * base_fee + priority_fee).min(max_fee_cap);
            let max_priority = priority_fee.min(max_fee);
            let gas_estimate = evm
                .estimate_gas(
                    &validator_addr,
                    &chain_config.bridge_contract_address,
                    &calldata_hex,
                )
                .await
                .unwrap_or(120_000);
            let gas_limit = gas_estimate * GAS_MARGIN_NUM / GAS_MARGIN_DEN;

            let raw_tx = match build_and_sign_eip1559_tx(
                chain_config.evm_chain_id,
                nonce,
                max_priority,
                max_fee,
                gas_limit,
                &chain_config.bridge_contract_address,
                &calldata,
                key.as_str(),
            ) {
                Ok(t) => t,
                Err(e) => {
                    error!("   ⚠️ Failed to sign executeTimelockedMint TX: {}", e);
                    continue;
                }
            };

            match evm.send_raw_transaction(&raw_tx).await {
                Ok(tx_hash) => {
                    info!(
                        "   ✅ executeTimelockedMint submitted! hash: {} | l1_tx: {} | chain: {}",
                        tx_hash, op.l1_tx_hash, op.evm_chain,
                    );
                    let _ = self.db.mark_timelocked_executed(&op.l1_tx_hash, &tx_hash);
                    self.metrics
                        .evm_mints_submitted
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    let msg = format!("executeTimelockedMint TX submit failed: {}", e);
                    error!("   ⚠️ {}", msg);
                    let _ = self.db.mark_timelocked_failed(&op.l1_tx_hash, &msg);
                    self.metrics.errors.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        Ok(())
    }

    /// Build the `validator_proofs` array for a `submitBridgeUnlock` call.
    ///
    /// **Fail-closed.** If the relayer has fewer than `threshold` real
    /// validator signing keys configured, this returns `Err` and the caller
    /// aborts the unlock before hitting L1 — we no longer pad with synthetic
    /// placeholder proofs. L1 (F4, PR #22) rejects `synthetic: true` entries
    /// anyway, so emitting them is wasted work + noise.
    ///
    /// Configure `ZION_VALIDATOR_EXTRA_KEYS` (comma-separated hex) plus
    /// `ZION_VALIDATOR_EXTRA_IDS` (comma-separated, optional) so that the
    /// local key + extras meet `threshold`.
    fn build_validator_proofs(
        &self,
        burn: &EvmBurnEvent,
        amount_flowers: u64,
    ) -> Result<Vec<Value>> {
        let threshold = usize::from(self.config.validator.threshold);

        let operation_message = format!(
            "unlock|recipient={}|amount={}|chain={}|burn_id={}|evm_tx={}",
            burn.l1_recipient, amount_flowers, burn.evm_chain, burn.burn_id, burn.evm_tx_hash,
        );

        let signers = self.load_validator_signers()?;

        build_validator_proofs_checked(
            signers,
            &self.config.validator.validator_addresses,
            threshold,
            &operation_message,
        )
    }

    fn load_validator_signers(&self) -> Result<Vec<(String, SigningKey)>> {
        let mut signers = Vec::new();

        let local_key = load_validator_key(&self.config.validator)?;
        let local_id = if self.config.validator.validator_id.trim().is_empty() {
            "validator-1".to_string()
        } else {
            self.config.validator.validator_id.clone()
        };
        signers.push((local_id, Self::signing_key_from_hex(local_key.as_str())?));

        if let Ok(extra_raw) = std::env::var("ZION_VALIDATOR_EXTRA_KEYS") {
            let extra_ids: Vec<String> = std::env::var("ZION_VALIDATOR_EXTRA_IDS")
                .ok()
                .map(|ids| ids.split(',').map(|v| v.trim().to_string()).collect())
                .unwrap_or_default();

            for (index, raw_key) in extra_raw
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .enumerate()
            {
                let id = extra_ids
                    .get(index)
                    .cloned()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| format!("validator-extra-{}", index + 1));
                signers.push((id, Self::signing_key_from_hex(raw_key)?));
            }
        }

        Ok(signers)
    }

    fn signing_key_from_hex(raw: &str) -> Result<SigningKey> {
        let pk_hex = raw.trim().strip_prefix("0x").unwrap_or(raw.trim());
        let pk_bytes = hex::decode(pk_hex)
            .map_err(|e| anyhow::anyhow!("Invalid validator private key hex: {e}"))?;
        SigningKey::from_slice(&pk_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid secp256k1 private key: {e}"))
    }

    async fn l1_rpc<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T> {
        let address = normalize_rpc_addr(&self.config.l1.rpc_url);
        let mut stream = TcpStream::connect(&address)
            .await
            .map_err(|e| anyhow::anyhow!("RPC connect failed to {}: {}", address, e))?;

        let request = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1,
        }))?;

        stream.write_all(request.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: RpcResponse<T> = serde_json::from_str(line.trim())?;
        if let Some(err) = response.error {
            return Err(anyhow::anyhow!("RPC error: {}", err));
        }

        response
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC returned null result"))
    }
}

/// Sign an operation message with a secp256k1 signing key and return
/// `(signature_hex, compressed_public_key_hex)`, both prefixed with `0x`.
///
/// Exposed as a free function so unit tests can exercise it without
/// constructing a full `Relayer` (which requires a populated
/// `BridgeConfig` + a filesystem-backed validator key file).
fn sign_operation_with_key(signing_key: &SigningKey, message: &str) -> (String, String) {
    let signature: Signature = signing_key.sign(message.as_bytes());
    let public_key = signing_key.verifying_key();
    let sec1 = public_key.to_encoded_point(true);
    (
        format!("0x{}", hex::encode(signature.to_bytes())),
        format!("0x{}", hex::encode(sec1.as_bytes())),
    )
}

/// Pure, testable core of [`Relayer::build_validator_proofs`].
///
/// Fail-closed contract:
///
/// - Returns `Err` if `signers.len() < threshold` — caller must abort
///   the unlock. Synthetic placeholder proofs are **never** emitted.
/// - Returns `Err` if any `validator_id` appears more than once — a
///   duplicate quorum member cannot contribute to threshold.
/// - Otherwise returns exactly `threshold` proofs (the first `threshold`
///   signers in the supplied order), each carrying a real secp256k1 ECDSA
///   signature over `operation_message` and `"synthetic": false`.
///
/// `validator_addresses[i]` is looked up for the i-th proof and reported
/// as `validator_address` (or `null` if out of range).
fn build_validator_proofs_checked(
    signers: Vec<(String, SigningKey)>,
    validator_addresses: &[String],
    threshold: usize,
    operation_message: &str,
) -> Result<Vec<Value>> {
    if signers.len() < threshold {
        anyhow::bail!(
            "validator_signatures_insufficient: have={}, need={}. \
             Configure ZION_VALIDATOR_EXTRA_KEYS (+ optional \
             ZION_VALIDATOR_EXTRA_IDS) so that the relayer holds at least \
             `threshold` distinct real validator signing keys. Synthetic \
             placeholder proofs are disabled — L1 rejects them and the \
             submitBridgeUnlock call would fail on-chain.",
            signers.len(),
            threshold,
        );
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (id, _) in &signers {
        if !seen.insert(id.clone()) {
            anyhow::bail!(
                "validator_signatures_duplicate_id: {id} — each validator \
                 may appear at most once in the quorum."
            );
        }
    }

    let mut proofs = Vec::with_capacity(threshold);
    for (index, (validator_id, signing_key)) in signers.into_iter().enumerate().take(threshold) {
        let validator_address = validator_addresses.get(index).cloned();
        let (signature, validator_public_key) =
            sign_operation_with_key(&signing_key, operation_message);

        proofs.push(json!({
            "validator_id": validator_id,
            "validator_address": validator_address,
            "validator_public_key": validator_public_key,
            "signature": signature,
            "signature_scheme": "secp256k1-ecdsa",
            "operation_message": operation_message,
            "synthetic": false,
        }));
    }

    Ok(proofs)
}

fn normalize_rpc_addr(value: &str) -> String {
    let trimmed = value.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix("/jsonrpc").unwrap_or(trimmed);
    trimmed
        .strip_prefix("tcp://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("https://"))
        .unwrap_or(trimmed)
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Address validation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Validate that an EVM address is well-formed: `0x` prefix + 40 hex characters.
/// Rejects the zero address (`0x0000…0000`).
fn validate_evm_address(addr: &str) -> Result<()> {
    let addr = addr.trim();
    if !addr.starts_with("0x") && !addr.starts_with("0X") {
        anyhow::bail!("EVM address must start with 0x, got: {}", addr);
    }
    let hex_part = &addr[2..];
    if hex_part.len() != 40 {
        anyhow::bail!(
            "EVM address must be 40 hex chars after 0x, got {} chars: {}",
            hex_part.len(),
            addr,
        );
    }
    if hex_part.chars().any(|c| !c.is_ascii_hexdigit()) {
        anyhow::bail!("EVM address contains non-hex characters: {}", addr);
    }
    // Reject zero address
    if hex_part.chars().all(|c| c == '0') {
        anyhow::bail!("EVM address is the zero address: {}", addr);
    }
    Ok(())
}

/// Validate that an L1 address is well-formed: starts with `zion1` and is
/// 40–60 characters of alphanumeric content.
fn validate_l1_address(addr: &str) -> Result<()> {
    let addr = addr.trim();
    if !addr.starts_with("zion1") {
        anyhow::bail!("L1 address must start with zion1, got: {}", addr);
    }
    if addr.len() < 40 || addr.len() > 60 {
        anyhow::bail!(
            "L1 address length {} out of range [40, 60]: {}",
            addr.len(),
            addr,
        );
    }
    if addr[5..].chars().any(|c| !c.is_ascii_alphanumeric()) {
        anyhow::bail!("L1 address contains invalid characters: {}", addr);
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_evm_address_valid() {
        assert!(validate_evm_address("0xdde17506BC2D2dCE1d594bD1D85B0BAbb389D186").is_ok());
        assert!(validate_evm_address("0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6").is_ok());
    }

    #[test]
    fn test_validate_evm_address_rejects_zero() {
        assert!(validate_evm_address("0x0000000000000000000000000000000000000000").is_err());
    }

    #[test]
    fn test_validate_evm_address_rejects_short() {
        assert!(validate_evm_address("0xabc").is_err());
    }

    #[test]
    fn test_validate_evm_address_rejects_no_prefix() {
        assert!(validate_evm_address("dde17506BC2D2dCE1d594bD1D85B0BAbb389D186").is_err());
    }

    #[test]
    fn test_validate_l1_address_valid() {
        assert!(validate_l1_address("zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7").is_ok());
    }

    #[test]
    fn test_validate_l1_address_rejects_wrong_prefix() {
        assert!(validate_l1_address("btc1abc123456789012345678901234567890").is_err());
    }

    #[test]
    fn test_validate_l1_address_rejects_too_short() {
        assert!(validate_l1_address("zion1abc").is_err());
    }

    #[test]
    fn test_normalize_rpc_addr() {
        assert_eq!(normalize_rpc_addr("tcp://127.0.0.1:8443"), "127.0.0.1:8443");
        assert_eq!(
            normalize_rpc_addr("http://127.0.0.1:8443/jsonrpc"),
            "127.0.0.1:8443"
        );
        assert_eq!(
            normalize_rpc_addr("204.168.245.175:8443"),
            "204.168.245.175:8443"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // build_validator_proofs_checked (synthetic-proof kill regression)
    // ──────────────────────────────────────────────────────────────────

    fn mk_signer(id: &str, seed: u8) -> (String, SigningKey) {
        // Deterministic 32-byte seed from a single byte — fine for tests.
        let bytes = [seed; 32];
        let sk = SigningKey::from_slice(&bytes).expect("valid test secp256k1 scalar");
        (id.to_string(), sk)
    }

    fn mk_addrs(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("0x{:040x}", 0xde_ad_be_ef_u32 + i as u32))
            .collect()
    }

    #[test]
    fn build_validator_proofs_checked_rejects_insufficient_signers() {
        let signers = vec![mk_signer("validator-1", 1), mk_signer("validator-2", 2)];
        let addrs = mk_addrs(3);
        let err = build_validator_proofs_checked(signers, &addrs, 3, "op|test")
            .expect_err("must fail-closed when below threshold");
        let msg = format!("{err}");
        assert!(
            msg.contains("validator_signatures_insufficient"),
            "error must name the failure class for ops triage: {msg}"
        );
        assert!(
            msg.contains("have=2") && msg.contains("need=3"),
            "error must report the observed and required counts: {msg}"
        );
        assert!(
            msg.contains("Synthetic placeholder proofs are disabled"),
            "error must document why padding isn't used anymore: {msg}"
        );
    }

    #[test]
    fn build_validator_proofs_checked_accepts_at_threshold() {
        let signers = vec![
            mk_signer("validator-1", 1),
            mk_signer("validator-2", 2),
            mk_signer("validator-3", 3),
        ];
        let addrs = mk_addrs(3);
        let proofs = build_validator_proofs_checked(signers, &addrs, 3, "op|test")
            .expect("must produce real proofs when threshold is met");
        assert_eq!(proofs.len(), 3, "must emit exactly `threshold` proofs");

        for proof in &proofs {
            assert_eq!(
                proof["synthetic"],
                json!(false),
                "no proof may be marked synthetic under the fail-closed contract: {proof}"
            );
            let sig = proof["signature"]
                .as_str()
                .expect("signature must be a string");
            assert!(sig.starts_with("0x"), "signature must be 0x-prefixed hex");
            assert_ne!(
                sig, "synthetic-proof-slot",
                "synthetic placeholder sentinel must not appear"
            );
            assert!(
                proof["validator_public_key"]
                    .as_str()
                    .map(|s| s.starts_with("0x"))
                    .unwrap_or(false),
                "every real proof must carry a compressed secp256k1 public key"
            );
        }
    }

    #[test]
    fn build_validator_proofs_checked_rejects_duplicate_validator_id() {
        let signers = vec![
            mk_signer("validator-1", 1),
            mk_signer("validator-1", 7),
            mk_signer("validator-2", 2),
        ];
        let addrs = mk_addrs(3);
        let err = build_validator_proofs_checked(signers, &addrs, 3, "op|test")
            .expect_err("duplicate validator_id must not contribute to threshold");
        let msg = format!("{err}");
        assert!(
            msg.contains("validator_signatures_duplicate_id"),
            "error must name the duplicate-id failure: {msg}"
        );
        assert!(
            msg.contains("validator-1"),
            "error must identify the duplicated id: {msg}"
        );
    }

    #[test]
    fn build_validator_proofs_checked_takes_exactly_threshold_not_more() {
        let signers = vec![
            mk_signer("validator-1", 1),
            mk_signer("validator-2", 2),
            mk_signer("validator-3", 3),
            mk_signer("validator-4", 4),
            mk_signer("validator-5", 5),
        ];
        let addrs = mk_addrs(5);
        let proofs = build_validator_proofs_checked(signers, &addrs, 3, "op|test")
            .expect("5 signers / threshold 3 must succeed");
        assert_eq!(
            proofs.len(),
            3,
            "must cap at threshold even when more signers are available"
        );
        let ids: Vec<_> = proofs
            .iter()
            .filter_map(|p| p["validator_id"].as_str())
            .collect();
        assert_eq!(ids, vec!["validator-1", "validator-2", "validator-3"]);
    }

    #[test]
    fn build_validator_proofs_checked_never_emits_synthetic_marker() {
        // Worst case for the old pad-with-synthetics behaviour: 0 signers,
        // threshold 3. Must NOT produce 3 synthetic placeholders.
        let err = build_validator_proofs_checked(Vec::new(), &mk_addrs(3), 3, "op|test")
            .expect_err("zero signers must fail-closed");
        assert!(
            format!("{err}").contains("validator_signatures_insufficient"),
            "0 signers must never degrade to synthetic padding"
        );
    }
}
