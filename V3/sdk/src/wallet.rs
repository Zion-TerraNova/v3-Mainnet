//! High-level wallet client for ZION L1 — keypair management, balance queries,
//! and transaction signing (UTXO + account-model) via a connected node.
//!
//! Wraps [`NodeClient`] for RPC and `zion_core::wallet` for signing.
//!
//! ## Usage
//!
//! ```no_run
//! use zion_sdk::wallet::WalletClient;
//! use zion_sdk::node::NodeClient;
//!
//! # async fn demo() -> zion_sdk::Result<()> {
//! let node = NodeClient::builder("127.0.0.1", 8443).build();
//! let wallet = WalletClient::new(node);
//!
//! // Generate a new keypair
//! let kp = wallet.generate_keypair();
//! println!("address: {}", kp.address);
//!
//! // Check balance
//! let bal = wallet.balance_breakdown(&kp.address).await?;
//! println!("total: {:.6} ZION", bal.total_zion);
//!
//! // Send 1 ZION via account model (auto-fallback from UTXO)
//! let result = wallet.send(&kp.signing_key, &kp.address, "zion1...", 1.0, None).await?;
//! println!("txid: {}", result.txid);
//! # Ok(())
//! # }
//! ```

use ed25519_dalek::SigningKey;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Result, ZionSdkError};
use crate::node::NodeClient;
use zion_core::emission::FLOWERS_PER_ZION;
use zion_core::wallet::{self, SendParams, SpendableUtxo};

/// A generated or imported Ed25519 keypair with ZION address.
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub signing_key: SigningKey,
    pub address: String,
}

/// Balance breakdown for a hybrid (account + UTXO) wallet.
#[derive(Debug, Clone)]
pub struct BalanceBreakdown {
    pub total_flowers: u128,
    pub account_flowers: u128,
    pub utxo_flowers: u128,
    pub utxo_count: u64,
    pub total_zion: f64,
    pub account_zion: f64,
    pub utxo_zion: f64,
}

/// Result of a successful send.
#[derive(Debug, Clone)]
pub struct SendResult {
    pub txid: String,
    pub model: TxModel,
}

/// Which transaction model was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxModel {
    Utxo,
    Account,
}

/// High-level wallet client — wraps NodeClient with signing logic.
#[derive(Clone)]
pub struct WalletClient {
    node: NodeClient,
}

impl WalletClient {
    /// Create a new wallet client backed by the given node RPC client.
    pub fn new(node: NodeClient) -> Self {
        Self { node }
    }

    /// Access the underlying node client.
    pub fn node(&self) -> &NodeClient {
        &self.node
    }

    // ─── Keypair Management ───────────────────────────────────────────

    /// Generate a new random Ed25519 keypair with ZION address.
    pub fn generate_keypair(&self) -> KeyPair {
        let (signing_key, verifying_key) = zion_core::crypto::generate_keypair();
        let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
        KeyPair {
            signing_key,
            address,
        }
    }

    /// Derive a keypair from a BIP39 mnemonic (optionally with passphrase).
    pub fn keypair_from_mnemonic(mnemonic: &str, passphrase: &str) -> Result<KeyPair> {
        use bip39::{Language, Mnemonic};

        let mn = Mnemonic::parse_in_normalized(Language::English, mnemonic)
            .map_err(|e| ZionSdkError::Other(format!("invalid mnemonic: {e}")))?;
        let seed = mn.to_seed(passphrase);
        // First 32 bytes of BIP39 seed → Ed25519 private key
        let mut sk_bytes = [0u8; 32];
        sk_bytes.copy_from_slice(&seed[..32]);
        let signing_key = SigningKey::from_bytes(&sk_bytes);
        let verifying_key = signing_key.verifying_key();
        let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
        Ok(KeyPair {
            signing_key,
            address,
        })
    }

    /// Derive a keypair from a raw 32-byte hex private key.
    pub fn keypair_from_secret_hex(hex: &str) -> Result<KeyPair> {
        let bytes = zion_core::crypto::from_hex(hex)
            .ok_or_else(|| ZionSdkError::Other("invalid hex secret key".into()))?;
        let sk_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| ZionSdkError::Other("secret key must be 32 bytes".into()))?;
        let signing_key = SigningKey::from_bytes(&sk_bytes);
        let verifying_key = signing_key.verifying_key();
        let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
        Ok(KeyPair {
            signing_key,
            address,
        })
    }

    // ─── Balance ──────────────────────────────────────────────────────

    /// Query balance breakdown (account + UTXO) for an address.
    pub async fn balance_breakdown(&self, address: &str) -> Result<BalanceBreakdown> {
        let v: Value = self
            .node
            .call("getBalance", serde_json::json!({ "address": address }))
            .await?;

        let account = v
            .get("account_balance_flowers")
            .and_then(|b| b.as_str())
            .and_then(|s| s.parse::<u128>().ok())
            .unwrap_or(0);
        let utxo = v
            .get("utxo_balance_flowers")
            .and_then(|b| b.as_str())
            .and_then(|s| s.parse::<u128>().ok())
            .unwrap_or(0);
        let utxo_count = v.get("utxo_count").and_then(|c| c.as_u64()).unwrap_or(0);
        let total = account + utxo;
        let fpz = FLOWERS_PER_ZION as u128;

        Ok(BalanceBreakdown {
            total_flowers: total,
            account_flowers: account,
            utxo_flowers: utxo,
            utxo_count,
            total_zion: total as f64 / fpz as f64,
            account_zion: account as f64 / fpz as f64,
            utxo_zion: utxo as f64 / fpz as f64,
        })
    }

    /// Query total balance in ZION (account + UTXO).
    pub async fn balance(&self, address: &str) -> Result<f64> {
        Ok(self.balance_breakdown(address).await?.total_zion)
    }

    // ─── Sending ──────────────────────────────────────────────────────

    /// Send ZION to an address. Automatically tries UTXO model first,
    /// then falls back to account-model if no UTXOs are available.
    ///
    /// `amount_zion` is in ZION (float). Fee is MIN_TX_FEE.
    pub async fn send(
        &self,
        signing_key: &SigningKey,
        from_address: &str,
        to_address: &str,
        amount_zion: f64,
        memo: Option<String>,
    ) -> Result<SendResult> {
        let amount_flowers = (amount_zion * FLOWERS_PER_ZION as f64) as u64;
        let fee = zion_core::fee::MIN_TX_FEE;

        // Try UTXO model first
        let utxos = self.fetch_utxos(from_address).await?;
        if !utxos.is_empty() {
            return self
                .send_utxo(
                    signing_key,
                    from_address,
                    to_address,
                    amount_flowers,
                    fee,
                    memo,
                    &utxos,
                )
                .await;
        }

        // Account-model fallback
        self.send_account(
            signing_key,
            from_address,
            to_address,
            amount_flowers as u128,
            fee,
            memo,
        )
        .await
    }

    /// Send via UTXO model (explicit).
    #[allow(clippy::too_many_arguments)]
    pub async fn send_utxo(
        &self,
        signing_key: &SigningKey,
        from_address: &str,
        to_address: &str,
        amount_flowers: u64,
        fee: u64,
        memo: Option<String>,
        utxos: &[SpendableUtxo],
    ) -> Result<SendResult> {
        let params = SendParams {
            to_address: to_address.to_string(),
            amount: amount_flowers,
            fee,
            memo,
        };
        let built = wallet::build_and_sign(signing_key, from_address, &params, utxos, 0)
            .map_err(|e| ZionSdkError::Other(format!("UTXO build failed: {e}")))?;

        let tx_value = serde_json::to_value(&built.transaction)
            .map_err(|e| ZionSdkError::Other(format!("serialize UTXO tx: {e}")))?;
        let result: crate::types::SubmitAccepted = self
            .node
            .submit_transaction("submitTransaction", tx_value)
            .await?;

        Ok(SendResult {
            txid: result.tx_id.unwrap_or_default(),
            model: TxModel::Utxo,
        })
    }

    /// Send via account model (explicit).
    pub async fn send_account(
        &self,
        signing_key: &SigningKey,
        from_address: &str,
        to_address: &str,
        amount_flowers: u128,
        fee: u64,
        memo: Option<String>,
    ) -> Result<SendResult> {
        // Check account balance
        let bal = self.balance_breakdown(from_address).await?;
        let total_needed = amount_flowers.saturating_add(fee as u128);
        if bal.account_flowers < total_needed {
            return Err(ZionSdkError::Other(format!(
                "insufficient account balance: have {} flowers, need {} flowers",
                bal.account_flowers, total_needed
            )));
        }

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let chain_tip = self.node.chain_info().await?.chain_height;

        let tx = wallet::build_and_sign_account(
            signing_key,
            from_address,
            to_address,
            amount_flowers,
            fee,
            nonce,
            memo,
            chain_tip,
        )
        .map_err(|e| ZionSdkError::Other(format!("account build failed: {e}")))?;

        let tx_value = serde_json::to_value(&tx)
            .map_err(|e| ZionSdkError::Other(format!("serialize account tx: {e}")))?;
        let result: crate::types::SubmitAccepted = self
            .node
            .submit_transaction("submitAccountTransaction", tx_value)
            .await?;

        Ok(SendResult {
            txid: result.tx_id.unwrap_or_default(),
            model: TxModel::Account,
        })
    }

    // ─── UTXO Fetching ────────────────────────────────────────────────

    /// Fetch spendable UTXOs for an address from the node.
    pub async fn fetch_utxos(&self, address: &str) -> Result<Vec<SpendableUtxo>> {
        let v: Value = self
            .node
            .call("getUtxos", serde_json::json!({ "address": address }))
            .await?;

        let mut utxos = Vec::new();
        if let Some(arr) = v.get("utxos").and_then(|u| u.as_array()) {
            for item in arr {
                let tx_hash_hex = item.get("tx_hash").and_then(|v| v.as_str()).unwrap_or("");
                let output_index = item
                    .get("output_index")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0) as u32;
                let amount = item
                    .get("amount")
                    .and_then(|v| {
                        v.as_u64()
                            .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
                    })
                    .unwrap_or(0);

                if let Some(hash_bytes) = zion_core::crypto::from_hex(tx_hash_hex) {
                    if hash_bytes.len() == 32 {
                        let mut tx_hash = [0u8; 32];
                        tx_hash.copy_from_slice(&hash_bytes);
                        utxos.push(SpendableUtxo {
                            tx_hash,
                            output_index,
                            amount,
                            address: address.to_string(),
                        });
                    }
                }
            }
        }
        Ok(utxos)
    }

    // ─── Batch Payout (PPLNS pool payouts) ────────────────────────────

    /// Build and submit a batch payout to multiple recipients (UTXO model).
    /// Useful for pool operators paying miners.
    pub async fn batch_payout(
        &self,
        signing_key: &SigningKey,
        change_address: &str,
        recipients: &[wallet::BatchRecipient],
        fee: u64,
    ) -> Result<SendResult> {
        let utxos = self.fetch_utxos(change_address).await?;
        if utxos.is_empty() {
            return Err(ZionSdkError::Other(
                "no UTXOs available for batch payout".into(),
            ));
        }

        let built =
            wallet::build_batch_payout(signing_key, change_address, recipients, fee, &utxos, 0)
                .map_err(|e| ZionSdkError::Other(format!("batch payout build failed: {e}")))?;

        let tx_value = serde_json::to_value(&built.transaction)
            .map_err(|e| ZionSdkError::Other(format!("serialize batch tx: {e}")))?;
        let result: crate::types::SubmitAccepted = self
            .node
            .submit_transaction("submitTransaction", tx_value)
            .await?;

        Ok(SendResult {
            txid: result.tx_id.unwrap_or_default(),
            model: TxModel::Utxo,
        })
    }
}
