use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, Context, Result};
use bip39::{Language, Mnemonic};
use clap::Subcommand;
use ed25519_dalek::SigningKey;
use pbkdf2::pbkdf2_hmac;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::ui;

const WALLET_KDF_ITERATIONS: u32 = 210_000;
const ENCRYPTION_ALGORITHM: &str = "aes-256-gcm+pbkdf2-sha256";

#[derive(Subcommand)]
pub enum WalletCmd {
    /// Generate a new ZION wallet (keypair)
    New {
        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,

        /// Generate a mnemonic-backed wallet instead of a random raw secret.
        #[arg(long, default_value_t = false)]
        mnemonic: bool,

        /// Word count for mnemonic generation (12, 15, 18, 21, 24).
        #[arg(long, default_value_t = 24)]
        words: u8,

        /// Optional BIP39 passphrase for mnemonic-backed wallet generation.
        #[arg(long, default_value = "")]
        passphrase: String,

        /// Also persist the generated address into miner.wallet in ~/.zion/zion.toml.
        #[arg(long, default_value_t = false)]
        set_default: bool,

        /// Read the wallet encryption password from this environment variable.
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Import a mnemonic and write a wallet file.
    ImportMnemonic {
        /// Mnemonic words.
        #[arg(long)]
        mnemonic: String,

        /// Optional BIP39 passphrase.
        #[arg(long, default_value = "")]
        passphrase: String,

        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,

        /// Also persist the generated address into miner.wallet in ~/.zion/zion.toml.
        #[arg(long, default_value_t = false)]
        set_default: bool,

        /// Read the wallet encryption password from this environment variable.
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Import a raw 32-byte Ed25519 secret key and write a wallet file.
    ImportSecretKey {
        /// Secret key hex (64 hex chars, 32 bytes).
        #[arg(long)]
        secret_key_hex: String,

        /// Output wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        out: PathBuf,

        /// Overwrite output file if it exists.
        #[arg(long, default_value_t = false)]
        force: bool,

        /// Print wallet JSON to stdout as well.
        #[arg(long, default_value_t = false)]
        print: bool,

        /// Also persist the generated address into miner.wallet in ~/.zion/zion.toml.
        #[arg(long, default_value_t = false)]
        set_default: bool,

        /// Read the wallet encryption password from this environment variable.
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Show metadata for a wallet file.
    Info {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,
    },
    /// Print the stored wallet JSON.
    Export {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,
    },
    /// Reveal decrypted secrets from a wallet file.
    Reveal {
        /// Wallet file path.
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,

        /// Read the wallet decryption password from this environment variable.
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Show current wallet address from config
    Address,
    /// Query balance from node
    Balance {
        #[arg(long)]
        address: Option<String>,
    },
    /// Send ZION to an address (submits via node RPC)
    Send {
        #[arg(long)]
        to: String,
        #[arg(long)]
        amount: f64,
        /// Optional memo / note
        #[arg(long)]
        memo: Option<String>,
        /// Wallet file path (source of signing key).
        #[arg(short, long, default_value = "zion-wallet.json")]
        wallet: PathBuf,
        /// Environment variable holding the wallet decryption password.
        #[arg(long)]
        password_env: Option<String>,
    },
    /// Show config wallet address + tithe distribution
    Tithe,
}

#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct WalletFile {
    format: String,
    format_version: u32,
    public_key_hex: String,
    address: String,
    mnemonic_present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    secret_key_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mnemonic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encryption: Option<WalletEncryption>,
    created_at_utc: String,
}

impl WalletFile {
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn is_encrypted(&self) -> bool {
        self.encryption.is_some()
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct WalletEncryption {
    algorithm: String,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
    pbkdf2_iterations: u32,
}

#[derive(Serialize, Deserialize, Debug)]
struct WalletSecretPayload {
    secret_key_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mnemonic: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WalletReveal {
    format: String,
    format_version: u32,
    public_key_hex: String,
    address: String,
    secret_key_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mnemonic: Option<String>,
    created_at_utc: String,
}

pub async fn run(cfg: &Config, cmd: WalletCmd) -> Result<()> {
    match cmd {
        WalletCmd::New {
            out,
            force,
            print,
            mnemonic,
            words,
            passphrase,
            set_default,
            password_env,
        } => {
            ui::print_header("New Wallet");

            let wallet = if mnemonic {
                generate_mnemonic_wallet_file(words, &passphrase)?
            } else {
                generate_wallet_file()
            };
            persist_wallet_file(
                wallet,
                &out,
                force,
                print,
                set_default,
                password_env.as_deref(),
            )
        }
        WalletCmd::ImportMnemonic {
            mnemonic,
            passphrase,
            out,
            force,
            print,
            set_default,
            password_env,
        } => {
            ui::print_header("Import Wallet From Mnemonic");
            let wallet = import_mnemonic_wallet_file(&mnemonic, &passphrase)?;
            persist_wallet_file(
                wallet,
                &out,
                force,
                print,
                set_default,
                password_env.as_deref(),
            )
        }
        WalletCmd::ImportSecretKey {
            secret_key_hex,
            out,
            force,
            print,
            set_default,
            password_env,
        } => {
            ui::print_header("Import Wallet From Secret Key");
            let wallet = import_secret_key_wallet_file(&secret_key_hex)?;
            persist_wallet_file(
                wallet,
                &out,
                force,
                print,
                set_default,
                password_env.as_deref(),
            )
        }
        WalletCmd::Info { wallet } => {
            ui::print_header("Wallet File Info");
            let parsed = read_wallet_file(&wallet)?;
            ui::print_row("Wallet file", &wallet.display().to_string());
            ui::print_row("Format", &parsed.format);
            ui::print_row("Address", &parsed.address);
            ui::print_row("Public key", &parsed.public_key_hex);
            ui::print_row(
                "Encrypted",
                if parsed.encryption.is_some() {
                    "yes"
                } else {
                    "no"
                },
            );
            ui::print_row(
                "Mnemonic stored",
                if parsed.mnemonic_present { "yes" } else { "no" },
            );
            ui::print_row("Created", &parsed.created_at_utc);
            println!();
            Ok(())
        }
        WalletCmd::Export { wallet } => {
            let raw = fs::read_to_string(&wallet)
                .with_context(|| format!("read {}", wallet.display()))?;
            println!("{}", raw);
            Ok(())
        }
        WalletCmd::Reveal {
            wallet,
            password_env,
        } => {
            let parsed = read_wallet_file(&wallet)?;
            let secrets = resolve_wallet_secrets(&parsed, password_env.as_deref())?;
            let reveal = WalletReveal {
                format: parsed.format,
                format_version: parsed.format_version,
                public_key_hex: parsed.public_key_hex,
                address: parsed.address,
                secret_key_hex: secrets.secret_key_hex,
                mnemonic: secrets.mnemonic,
                created_at_utc: parsed.created_at_utc,
            };
            println!("{}", serde_json::to_string_pretty(&reveal)?);
            Ok(())
        }
        WalletCmd::Address => {
            ui::print_header("Wallet Address");
            if cfg.miner.wallet.is_empty() {
                ui::print_warn("No wallet configured. Run: zion config set miner.wallet <address>");
            } else {
                ui::print_row("Address", &cfg.miner.wallet);
            }
            println!();
            Ok(())
        }
        WalletCmd::Balance { address } => {
            let addr = address.unwrap_or_else(|| cfg.miner.wallet.clone());
            if addr.is_empty() {
                ui::print_warn("No address. Use --address <addr> or set miner.wallet in config.");
                return Ok(());
            }
            ui::print_header(&format!("Balance: {}", addr));
            // Query node RPC for balance
            let result = crate::rpc::node_rpc::call(
                &cfg.node.rpc_host,
                cfg.node.rpc_port,
                "getBalance",
                serde_json::json!({ "address": addr }),
            )
            .await;
            match result {
                Ok(v) => {
                    // V3 returns account_balance_flowers + utxo_balance_flowers as strings (u128)
                    let account = v["account_balance_flowers"]
                        .as_str()
                        .and_then(|s| s.parse::<u128>().ok())
                        .unwrap_or(0);
                    let utxo = v["utxo_balance_flowers"]
                        .as_str()
                        .and_then(|s| s.parse::<u128>().ok())
                        .unwrap_or(0);
                    let total = account + utxo;
                    let flowers_per_zion = zion_core::emission::FLOWERS_PER_ZION as u128;
                    let total_zion = total as f64 / flowers_per_zion as f64;
                    let account_zion = account as f64 / flowers_per_zion as f64;
                    let utxo_zion = utxo as f64 / flowers_per_zion as f64;
                    ui::print_row("Total", &format!("{:.6} ZION", total_zion));
                    ui::print_row("Account", &format!("{:.6} ZION", account_zion));
                    ui::print_row("UTXO", &format!("{:.6} ZION", utxo_zion));
                }
                Err(e) => ui::print_warn(&format!("Cannot fetch balance: {}", e)),
            }
            println!();
            Ok(())
        }
        WalletCmd::Send {
            to,
            amount,
            memo,
            wallet,
            password_env,
        } => {
            if cfg.miner.wallet.is_empty() {
                ui::print_warn("No wallet configured. Set miner.wallet in config first.");
                return Ok(());
            }
            ui::print_header("Send ZION");
            ui::print_row("From", &cfg.miner.wallet);
            ui::print_row("To", &to);
            ui::print_row("Amount", &format!("{:.8} ZION", amount));
            if let Some(ref m) = memo {
                ui::print_row("Memo", m);
            }

            // ── Load wallet & signing key ────────────────────────────────
            let wallet_file = read_wallet_file(&wallet)?;
            let secrets = resolve_wallet_secrets(&wallet_file, password_env.as_deref())?;
            let sk_bytes = zion_core::crypto::from_hex(&secrets.secret_key_hex)
                .ok_or_else(|| anyhow!("invalid secret key hex in wallet"))?;
            let sk_bytes: [u8; 32] = sk_bytes
                .try_into()
                .map_err(|_| anyhow!("secret key must be 32 bytes"))?;
            let signing_key = SigningKey::from_bytes(&sk_bytes);

            // Convert ZION → flowers (post-3.0.3: 1 ZION = 1e6 flowers)
            let amount_flowers = (amount * zion_core::emission::FLOWERS_PER_ZION as f64) as u64;
            let fee = zion_core::fee::MIN_TX_FEE;

            // ── Check UTXOs ────────────────────────────────────────────────
            let utxos_resp = crate::rpc::node_rpc::call(
                &cfg.node.rpc_host,
                cfg.node.rpc_port,
                "getUtxos",
                serde_json::json!({ "address": &cfg.miner.wallet }),
            )
            .await;

            let mut spendable_utxos: Vec<zion_core::wallet::SpendableUtxo> = Vec::new();
            if let Ok(ref v) = utxos_resp {
                if let Some(utxo_array) = v.get("utxos").and_then(|u| u.as_array()) {
                    for item in utxo_array {
                        let tx_hash_hex =
                            item.get("tx_hash").and_then(|v| v.as_str()).unwrap_or("");
                        let output_index = item
                            .get("output_index")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        // V3 returns amount as string (u128) or number
                        let amt = item
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
                                spendable_utxos.push(zion_core::wallet::SpendableUtxo {
                                    tx_hash,
                                    output_index,
                                    amount: amt,
                                    address: cfg.miner.wallet.clone(),
                                });
                            }
                        }
                    }
                }
            }

            let result = if !spendable_utxos.is_empty() {
                // ── UTXO send ──────────────────────────────────────────────
                let params = zion_core::wallet::SendParams {
                    to_address: to.clone(),
                    amount: amount_flowers,
                    fee,
                    memo: memo.clone(),
                };
                let built = zion_core::wallet::build_and_sign(
                    &signing_key,
                    &cfg.miner.wallet,
                    &params,
                    &spendable_utxos,
                    0, // chain_tip_height not needed for fee here
                )
                .map_err(|e| anyhow!("{e}"))?;
                crate::rpc::node_rpc::call(
                    &cfg.node.rpc_host,
                    cfg.node.rpc_port,
                    "submitTransaction",
                    serde_json::json!({ "transaction": built.transaction }),
                )
                .await
            } else {
                // ── Account-model fallback ───────────────────────────────
                ui::print_info("No spendable UTXOs; falling back to account-model send.");
                let balance_resp = crate::rpc::node_rpc::call(
                    &cfg.node.rpc_host,
                    cfg.node.rpc_port,
                    "getBalance",
                    serde_json::json!({ "address": &cfg.miner.wallet }),
                )
                .await;
                let account_balance = if let Ok(ref v) = balance_resp {
                    v.get("account_balance_flowers")
                        .and_then(|b| b.as_str())
                        .and_then(|s| s.parse::<u128>().ok())
                        .unwrap_or(0)
                } else {
                    0
                };
                let total_needed = (amount_flowers as u128).saturating_add(fee as u128);
                if account_balance < total_needed {
                    return Err(anyhow!(
                        "insufficient funds: account balance {} flowers, need {} flowers",
                        account_balance,
                        total_needed
                    ));
                }
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let chain_info = crate::rpc::node_rpc::call(
                    &cfg.node.rpc_host,
                    cfg.node.rpc_port,
                    "getChainInfo",
                    serde_json::json!({}),
                )
                .await?;
                let chain_tip = chain_info["chain_height"].as_u64().unwrap_or(0);
                let tx = zion_core::wallet::build_and_sign_account(
                    &signing_key,
                    &cfg.miner.wallet,
                    &to,
                    amount_flowers as u128,
                    fee,
                    nonce,
                    memo.clone(),
                    chain_tip,
                )
                .map_err(|e| anyhow!("{e}"))?;
                crate::rpc::node_rpc::call(
                    &cfg.node.rpc_host,
                    cfg.node.rpc_port,
                    "submitAccountTransaction",
                    serde_json::json!({ "transaction": tx }),
                )
                .await
            };

            match result {
                Ok(v) => {
                    let txid = v.get("tx_id").and_then(|t| t.as_str()).unwrap_or("?");
                    ui::print_ok(&format!("Submitted! txid: {}", txid));
                }
                Err(e) => ui::print_err(&format!("TX failed: {}", e)),
            }
            println!();
            Ok(())
        }
        WalletCmd::Tithe => {
            ui::print_header("Tithe Wallets");
            ui::print_info("Tithe wallet distribution (from TITHE_WALLETS_BACKUP.txt).");
            ui::print_info("See project root for the full list.");
            println!();
            Ok(())
        }
    }
}

fn generate_wallet_file() -> WalletFile {
    let (signing_key, verifying_key) = zion_core::crypto::generate_keypair();
    let address = zion_core::crypto::derive_address(verifying_key.as_bytes());

    WalletFile {
        format: "zion_wallet_ed25519".to_string(),
        format_version: 1,
        public_key_hex: zion_core::crypto::to_hex(verifying_key.as_bytes()),
        address,
        mnemonic_present: false,
        secret_key_hex: Some(zion_core::crypto::to_hex(&signing_key.to_bytes())),
        mnemonic: None,
        encryption: None,
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    }
}

pub(crate) fn create_wallet_at(
    out: &Path,
    use_mnemonic: bool,
    words: u8,
    force: bool,
    encryption_password: Option<&str>,
) -> Result<WalletFile> {
    ensure_output_path(out, force)?;
    let wallet = if use_mnemonic {
        generate_mnemonic_wallet_file(words, "")?
    } else {
        generate_wallet_file()
    };
    let wallet = match encryption_password {
        Some(password) => encrypt_wallet_file(wallet, password)?,
        None => wallet,
    };
    let json = serde_json::to_string_pretty(&wallet)?;
    write_wallet_file(out, &json)?;
    Ok(wallet)
}

fn generate_mnemonic_wallet_file(words: u8, passphrase: &str) -> Result<WalletFile> {
    let word_count = match words {
        12 | 15 | 18 | 21 | 24 => words as usize,
        _ => {
            return Err(anyhow!(
                "Unsupported word count: {} (use 12/15/18/21/24)",
                words
            ))
        }
    };

    let mnemonic = Mnemonic::generate_in_with(&mut OsRng, Language::English, word_count)
        .map_err(|error| anyhow!("Failed to generate mnemonic: {error}"))?;
    import_mnemonic_wallet_file(&mnemonic.to_string(), passphrase)
}

fn import_mnemonic_wallet_file(mnemonic: &str, passphrase: &str) -> Result<WalletFile> {
    let mnemonic = Mnemonic::parse_in(Language::English, mnemonic)
        .map_err(|error| anyhow!("Invalid mnemonic: {error}"))?;

    let seed = mnemonic.to_seed(passphrase);
    let secret: [u8; 32] = seed[0..32]
        .try_into()
        .map_err(|_| anyhow!("Seed slice conversion failed"))?;
    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();
    let address = zion_core::crypto::derive_address(verifying_key.as_bytes());

    Ok(WalletFile {
        format: "zion_wallet_ed25519".to_string(),
        format_version: 1,
        public_key_hex: zion_core::crypto::to_hex(verifying_key.as_bytes()),
        address,
        mnemonic_present: true,
        secret_key_hex: Some(zion_core::crypto::to_hex(&secret)),
        mnemonic: Some(mnemonic.to_string()),
        encryption: None,
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn import_secret_key_wallet_file(secret_key_hex: &str) -> Result<WalletFile> {
    let secret = hex_to_32(secret_key_hex).context("secret_key_hex must be 32-byte hex")?;
    let signing_key = SigningKey::from_bytes(&secret);
    let verifying_key = signing_key.verifying_key();
    let address = zion_core::crypto::derive_address(verifying_key.as_bytes());

    Ok(WalletFile {
        format: "zion_wallet_ed25519".to_string(),
        format_version: 1,
        public_key_hex: zion_core::crypto::to_hex(verifying_key.as_bytes()),
        address,
        mnemonic_present: false,
        secret_key_hex: Some(zion_core::crypto::to_hex(&secret)),
        mnemonic: None,
        encryption: None,
        created_at_utc: chrono::Utc::now().to_rfc3339(),
    })
}

fn persist_wallet_file(
    wallet: WalletFile,
    out: &Path,
    force: bool,
    print: bool,
    set_default: bool,
    password_env: Option<&str>,
) -> Result<()> {
    ensure_output_path(out, force)?;
    let wallet = maybe_encrypt_wallet_file(wallet, password_env)?;
    let json = serde_json::to_string_pretty(&wallet)?;
    write_wallet_file(out, &json)?;

    ui::print_ok(&format!("Wrote wallet  {}", out.display()));
    ui::print_row("Address", &wallet.address);
    if wallet.encryption.is_some() {
        ui::print_ok("Secrets are encrypted in the wallet file.");
    } else {
        ui::print_warn("Secret key is stored in plaintext JSON; protect this file.");
    }

    if set_default {
        config::set_value("miner.wallet", &wallet.address)?;
    } else {
        ui::print_info(&format!(
            "Next: zion config set miner.wallet {}",
            wallet.address
        ));
    }

    if print {
        println!();
        println!("{}", json);
    }
    println!();
    Ok(())
}

fn ensure_output_path(out: &Path, force: bool) -> Result<()> {
    if out.exists() && !force {
        return Err(anyhow!(
            "Refusing to overwrite existing file: {} (use --force)",
            out.display()
        ));
    }
    Ok(())
}

fn maybe_encrypt_wallet_file(wallet: WalletFile, password_env: Option<&str>) -> Result<WalletFile> {
    let Some(password_env) = password_env else {
        return Ok(wallet);
    };

    let password = env::var(password_env).with_context(|| {
        format!(
            "Environment variable {} is required for wallet encryption",
            password_env
        )
    })?;
    if password.is_empty() {
        return Err(anyhow!(
            "Environment variable {} is set but empty",
            password_env
        ));
    }

    encrypt_wallet_file(wallet, &password)
}

fn encrypt_wallet_file(mut wallet: WalletFile, password: &str) -> Result<WalletFile> {
    if password.is_empty() {
        return Err(anyhow!("wallet encryption password must not be empty"));
    }

    let secret_key_hex = wallet
        .secret_key_hex
        .take()
        .ok_or_else(|| anyhow!("wallet has no plaintext secret key to encrypt"))?;
    let payload = WalletSecretPayload {
        secret_key_hex,
        mnemonic: wallet.mnemonic.take(),
    };
    let plaintext = serde_json::to_vec(&payload)?;

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let mut key = [0u8; 32];
    pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, WALLET_KDF_ITERATIONS, &mut key);

    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| anyhow!("failed to initialize wallet cipher: {error}"))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
        .map_err(|error| anyhow!("wallet encryption failed: {error}"))?;
    key.fill(0);

    wallet.encryption = Some(WalletEncryption {
        algorithm: ENCRYPTION_ALGORITHM.to_string(),
        salt_hex: zion_core::crypto::to_hex(&salt),
        nonce_hex: zion_core::crypto::to_hex(&nonce_bytes),
        ciphertext_hex: zion_core::crypto::to_hex(&ciphertext),
        pbkdf2_iterations: WALLET_KDF_ITERATIONS,
    });

    Ok(wallet)
}

fn resolve_wallet_secrets(
    wallet: &WalletFile,
    password_env: Option<&str>,
) -> Result<WalletSecretPayload> {
    if let Some(secret_key_hex) = &wallet.secret_key_hex {
        return Ok(WalletSecretPayload {
            secret_key_hex: secret_key_hex.clone(),
            mnemonic: wallet.mnemonic.clone(),
        });
    }

    let encryption = wallet
        .encryption
        .as_ref()
        .ok_or_else(|| anyhow!("wallet file has neither plaintext nor encrypted secrets"))?;
    let password_env = password_env.ok_or_else(|| {
        anyhow!("wallet file is encrypted; pass --password-env <ENV_VAR> to reveal secrets")
    })?;
    let password = env::var(password_env).with_context(|| {
        format!(
            "Environment variable {} is required for wallet decryption",
            password_env
        )
    })?;
    if password.is_empty() {
        return Err(anyhow!(
            "Environment variable {} is set but empty",
            password_env
        ));
    }

    decrypt_wallet_secret_payload(encryption, &password)
}

fn decrypt_wallet_secret_payload(
    encryption: &WalletEncryption,
    password: &str,
) -> Result<WalletSecretPayload> {
    if encryption.algorithm != ENCRYPTION_ALGORITHM {
        return Err(anyhow!(
            "unsupported wallet encryption algorithm: {}",
            encryption.algorithm
        ));
    }

    let salt = zion_core::crypto::from_hex(&encryption.salt_hex)
        .ok_or_else(|| anyhow!("wallet encryption salt is not valid hex"))?;
    let nonce_bytes = zion_core::crypto::from_hex(&encryption.nonce_hex)
        .ok_or_else(|| anyhow!("wallet encryption nonce is not valid hex"))?;
    let ciphertext = zion_core::crypto::from_hex(&encryption.ciphertext_hex)
        .ok_or_else(|| anyhow!("wallet encryption ciphertext is not valid hex"))?;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<sha2::Sha256>(
        password.as_bytes(),
        &salt,
        encryption.pbkdf2_iterations,
        &mut key,
    );
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|error| anyhow!("failed to initialize wallet cipher: {error}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
        .map_err(|_| anyhow!("wallet decryption failed; check the password"))?;
    key.fill(0);

    serde_json::from_slice(&plaintext).context("decrypt wallet secret payload")
}

fn write_wallet_file(path: &Path, json: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, json)?;
    Ok(())
}

fn read_wallet_file(path: &Path) -> Result<WalletFile> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed: WalletFile = serde_json::from_str(&raw).context("parse wallet JSON")?;
    Ok(parsed)
}

fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    let bytes = zion_core::crypto::from_hex(s)?;
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{
        create_wallet_at, generate_mnemonic_wallet_file, generate_wallet_file,
        import_mnemonic_wallet_file, maybe_encrypt_wallet_file, resolve_wallet_secrets,
        write_wallet_file,
    };

    #[test]
    fn generated_wallet_has_valid_shape() {
        let wallet = generate_wallet_file();
        assert_eq!(wallet.format, "zion_wallet_ed25519");
        assert_eq!(wallet.format_version, 1);
        assert_eq!(
            wallet.secret_key_hex.as_deref().unwrap_or_default().len(),
            64
        );
        assert_eq!(wallet.public_key_hex.len(), 64);
        assert!(!wallet.mnemonic_present);
        assert!(zion_core::crypto::is_valid_address(&wallet.address));
    }

    #[test]
    fn generated_mnemonic_wallet_has_phrase() {
        let wallet =
            generate_mnemonic_wallet_file(24, "").expect("mnemonic wallet should generate");
        assert!(wallet.mnemonic_present);
        assert!(wallet.mnemonic.is_some());
        assert!(zion_core::crypto::is_valid_address(&wallet.address));
    }

    #[test]
    fn import_mnemonic_is_deterministic() {
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let first = import_mnemonic_wallet_file(phrase, "").expect("first import should work");
        let second = import_mnemonic_wallet_file(phrase, "").expect("second import should work");
        assert_eq!(first.address, second.address);
        assert_eq!(first.public_key_hex, second.public_key_hex);
    }

    #[test]
    fn encrypt_wallet_moves_secrets_out_of_plaintext_fields() {
        let env_name = format!("ZION_TEST_WALLET_PASSWORD_{}", std::process::id());
        std::env::set_var(&env_name, "test-password");

        let encrypted = maybe_encrypt_wallet_file(generate_wallet_file(), Some(&env_name))
            .expect("wallet encryption should succeed");
        assert!(encrypted.secret_key_hex.is_none());
        assert!(encrypted.encryption.is_some());

        std::env::remove_var(&env_name);
    }

    #[test]
    fn encrypted_wallet_can_be_revealed() {
        let env_name = format!("ZION_TEST_WALLET_PASSWORD_REVEAL_{}", std::process::id());
        std::env::set_var(&env_name, "test-password");

        let encrypted = maybe_encrypt_wallet_file(
            generate_mnemonic_wallet_file(12, "").expect("mnemonic wallet should generate"),
            Some(&env_name),
        )
        .expect("wallet encryption should succeed");
        let revealed = resolve_wallet_secrets(&encrypted, Some(&env_name))
            .expect("wallet reveal should succeed");
        assert_eq!(revealed.secret_key_hex.len(), 64);
        assert!(revealed.mnemonic.is_some());

        std::env::remove_var(&env_name);
    }

    #[test]
    fn write_wallet_file_creates_parent_directories() {
        let temp_dir = std::env::temp_dir().join(format!(
            "zion-cli-wallet-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let wallet_path = temp_dir.join("nested").join("wallet.json");

        write_wallet_file(&wallet_path, "{}")
            .expect("wallet file should be written with parent directories created");

        assert!(wallet_path.exists());
        let content =
            std::fs::read_to_string(&wallet_path).expect("wallet file should be readable");
        assert_eq!(content, "{}");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn create_wallet_at_persists_file() {
        let temp_dir = std::env::temp_dir().join(format!(
            "zion-cli-wallet-create-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let wallet_path = temp_dir.join("wallet.json");

        let wallet = create_wallet_at(&wallet_path, true, 12, false, None)
            .expect("wallet helper should create a file");
        assert!(wallet_path.exists());
        assert!(wallet.mnemonic_present);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
