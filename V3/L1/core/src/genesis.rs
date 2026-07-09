//! ZION V3 — Genesis Block & Premine
//!
//! Constitutional reference: `docs/mainnet/MAINNET_CONSTITUTION.md` §1–§2
//!
//! The genesis block (height 0) carries 14 premine outputs totalling
//! 16,780,000,000 ZION (11.65 % of the 144 B total supply).
//! Block subsidy at height 0 is 0 — the premine is the sole coinbase.
//!
//! Mining subsidy **89/5/5/1** routing and default-operator `zion1` addresses live in constants
//! below ([`MAINNET_CANONICAL_*`]) — they are **not** additional genesis outputs; see
//! `docs/mainnet/PREMINE_AND_CANONICAL_WALLETS_PUBLIC.txt`.

//! Source: `PREMINE_ADDRESSES_PUBLIC.txt` + `L1/core/src/blockchain/premine.rs`

use crate::tx::{self, TxOutput};
use crate::{difficulty, AcceptedBlock, MiningHeader, Transaction};
use zion_cosmic_harmony::{cosmic_harmony_ekam_deeksha, cosmic_harmony_with_height};

// ---------------------------------------------------------------------------
// Canonical mainnet subsidy & operator wallets (89/5/5/1 + default miner + pool payout)
// ---------------------------------------------------------------------------
//
// Issobella / pool-fee / default-miner / pool-payout addresses are derived deterministically from
// the UTF-8 labels below via `crypto::canonical_address_for_label` (BLAKE3 → StdRng → Ed25519).
// **Keys are reconstructible from this repository** — adequate only for bootstrap / open custody;
// operators who need exclusive control should generate fresh keys and override env vars.

/// Label → `MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET`.
pub const MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_LABEL: &str =
    "ZION_V3_MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_RECIPIENT_v2_2026-07-06-HARD-RESET";
/// Label → `MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET`.
pub const MAINNET_CANONICAL_POOL_FEE_SUBSIDY_LABEL: &str =
    "ZION_V3_MAINNET_CANONICAL_POOL_FEE_SUBSIDY_RECIPIENT_v2_2026-07-06-HARD-RESET";
/// Label → `MAINNET_CANONICAL_DEFAULT_MINER_WALLET` (89% share when split is on).
pub const MAINNET_CANONICAL_DEFAULT_MINER_LABEL: &str =
    "ZION_V3_MAINNET_CANONICAL_DEFAULT_SOLO_MINER_COINBASE_v2_2026-07-06-HARD-RESET";
/// Label → `MAINNET_CANONICAL_POOL_PAYOUT_WALLET` (PPLNS UTXO batch signer address).
pub const MAINNET_CANONICAL_POOL_PAYOUT_LABEL: &str =
    "ZION_V3_MAINNET_CANONICAL_POOL_PPLNS_PAYOUT_SIGNER_v2_2026-07-06-HARD-RESET";

/// Humanitarian 5% coinbase fee recipient (ongoing block subsidy).
/// Distinct from the premine humanitarian slot (slot 12 in PREMINE_OUTPUTS).
/// Hard reset 2026-07-06: fresh OS-random key with BIP39 mnemonic (encrypted archive).
pub const MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET: &str =
    "zion1e0u5q5s660k4m4a634p2c2v358r8g59564054z7";

pub const MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET: &str =
    "zion1f7y7l5k678y0v408e8s654d2282346k375526t2";
pub const MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET: &str =
    "zion1062522x6a083x6r4d24303l5h20698z7j8qk433";
pub const MAINNET_CANONICAL_DEFAULT_MINER_WALLET: &str =
    "zion1d6m0h2r8m7k8k2d8n072y7j3j4m0254323vq0e3";
pub const MAINNET_CANONICAL_POOL_PAYOUT_WALLET: &str =
    "zion1e4489793c5x2r0a0a4d8z7r4u5d6k0s4k3ht5m2";

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Block height at which DAO Treasury addresses unlock.
/// 144,000 blocks ≈ 100 days at 60 s/block (post-3.0.3 fork, was 525,600).
pub const DAO_TREASURY_LOCK_HEIGHT: u64 = 144_000;

/// Genesis timestamp (seconds since UNIX epoch).
/// 2026-01-01 00:00:00 UTC — frozen, all nodes must agree.
pub const GENESIS_TIMESTAMP: u64 = 1_767_225_600;

/// Genesis message embedded in the coinbase (Bitcoin-style scriptSig heritage).
/// Short form used in tx hashing; full form with ASCII art available via `genesis_message_full()`.
pub const GENESIS_MESSAGE: &str = concat!(
    "ZION Mainet Launch v3 — ",
    "For Sarah Issobel, Maitreya Buddha, Radha & Sita, Meriam, Friends, Family, ",
    "Freedom Humanity and all the children of this world: ZION is yours. ",
    "Build a better world where you reach for the Stars. The Golden Age begins. ",
    "Peace & One Love 4ever. ",
    "— Yose / Zion Creator"
);

/// Full genesis message including ASCII art, embedded at compile time.
pub const GENESIS_MESSAGE_FULL: &str = include_str!("GENESIS_MESSAGE.txt");

// ---------------------------------------------------------------------------
// Premine allocations — frozen data from PREMINE_ADDRESSES_PUBLIC.txt
// ---------------------------------------------------------------------------

/// A single premine output in the genesis block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PremineOutput {
    pub address: &'static str,
    pub purpose: &'static str,
    pub amount_zion: u64,
    pub amount_flowers: u128,
    pub category: &'static str,
    pub unlock_height: Option<u64>,
    /// If `true`, transfers from this address require 3-of-3 admin
    /// multisig + DAO vote to unlock (in addition to any `unlock_height`
    /// time-lock).  All mainnet premine outputs are admin-locked.
    pub admin_locked: bool,
}

/// All 14 premine allocations, ordered by category then slot.
/// Total: 16,780,000,000 ZION = 16,780,000,000,000,000,000,000 flowers.
pub const PREMINE_OUTPUTS: &[PremineOutput] = &[
    // --- OASIS + Golden Egg (5 × 1.65B = 8.25B) ---
    PremineOutput {
        address: "zion1n3t6v6w3m8g4v6q8g7h7j4j6f7s8q2m7g7un8u0",
        purpose: "ZION OASIS + Winners Golden Egg/Xp (Slot 1)",
        amount_zion: 1_650_000_000,
        amount_flowers: 1_650_000_000_000_000_000_000,
        category: "oasis_golden_egg",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion16854w6h7a800k6h8n052s0h4k2v625x0w0z2320",
        purpose: "ZION OASIS + Winners Golden Egg/Xp (Slot 2)",
        amount_zion: 1_650_000_000,
        amount_flowers: 1_650_000_000_000_000_000_000,
        category: "oasis_golden_egg",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion1j8s2d6s6f248j7z3m80676p6m074x2q5p5er3w2",
        purpose: "ZION OASIS + Winners Golden Egg/Xp (Slot 3)",
        amount_zion: 1_650_000_000,
        amount_flowers: 1_650_000_000_000_000_000_000,
        category: "oasis_golden_egg",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion155k300w6x726p4x0w473s704d5k35865r2q75z8",
        purpose: "ZION OASIS + Winners Golden Egg/Xp (Slot 4)",
        amount_zion: 1_650_000_000,
        amount_flowers: 1_650_000_000_000_000_000_000,
        category: "oasis_golden_egg",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion1y293r8c6l5p3u0y7j8q8366372t7y070n3rp5r8",
        purpose: "ZION OASIS + Winners Golden Egg/Xp (Slot 5)",
        amount_zion: 1_650_000_000,
        amount_flowers: 1_650_000_000_000_000_000_000,
        category: "oasis_golden_egg",
        unlock_height: None,
        admin_locked: true,
    },
    // --- DAO Treasury (3 slots = 4.0B) — locked until height 144,000 ---
    PremineOutput {
        address: "zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0",
        purpose: "DAO Treasury — Community Governance (main)",
        amount_zion: 2_500_000_000,
        amount_flowers: 2_500_000_000_000_000_000_000,
        category: "dao_treasury",
        unlock_height: Some(DAO_TREASURY_LOCK_HEIGHT),
        admin_locked: true,
    },
    PremineOutput {
        address: "zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6",
        purpose: "DAO Treasury — Grants & Bounties",
        amount_zion: 1_000_000_000,
        amount_flowers: 1_000_000_000_000_000_000_000,
        category: "dao_treasury",
        unlock_height: Some(DAO_TREASURY_LOCK_HEIGHT),
        admin_locked: true,
    },
    PremineOutput {
        address: "zion102s8k4k0w783d657j255z865e47054s342u87v3",
        purpose: "DAO Treasury — Ecosystem Bootstrap",
        amount_zion: 500_000_000,
        amount_flowers: 500_000_000_000_000_000_000,
        category: "dao_treasury",
        unlock_height: Some(DAO_TREASURY_LOCK_HEIGHT),
        admin_locked: true,
    },
    // --- Infrastructure (3 slots = 2.59B) ---
    PremineOutput {
        address: "zion1e8j5z6v8e4c6s5x7r0w7e2r673h8k3a6d4xx877",
        purpose: "Core Development Fund",
        amount_zion: 1_000_000_000,
        amount_flowers: 1_000_000_000_000_000_000_000,
        category: "infrastructure",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion1f7z374q068r3p657m8z220v7y6k045q255xp2d3",
        purpose: "Network Infrastructure — P2P Seed Nodes",
        amount_zion: 1_000_000_000,
        amount_flowers: 1_000_000_000_000_000_000_000,
        category: "infrastructure",
        unlock_height: None,
        admin_locked: true,
    },
    PremineOutput {
        address: "zion1s2j5s2a6f5k740k4d8s2k3y8v0t8d4k0u6my2k0",
        purpose: "Genesis Creator — Lifetime Rent",
        amount_zion: 590_000_000,
        amount_flowers: 590_000_000_000_000_000_000,
        category: "infrastructure",
        unlock_height: None,
        admin_locked: true,
    },
    // --- Humanitarian (1 slot = 1.44B) ---
    PremineOutput {
        address: "zion10797m0k3u356f2l443r062d4e49665f6n20j6x0",
        purpose: "Children Future Fund — Humanitarian DAO",
        amount_zion: 1_440_000_000,
        amount_flowers: 1_440_000_000_000_000_000_000,
        category: "humanitarian",
        unlock_height: None,
        admin_locked: true,
    },
    // --- Bridge Seed Fund (1 slot = 0.4B) — immediate unlock for EVM bridge liquidity ---
    PremineOutput {
        address: "zion1p3y7w4z7d2m3j0f00657r354y4f3q5k6y8ca0g7",
        purpose: "Bridge Seed Fund — EVM Bridge Liquidity",
        amount_zion: 400_000_000,
        amount_flowers: 400_000_000_000_000_000_000,
        category: "bridge_seed",
        unlock_height: None,
        admin_locked: true,
    },
    // --- Bridge Vault UTXO Seed (1 slot = 0.1B) — UTXO liquidity for bridge unlocks ---
    // Address derived from new BRIDGE_VAULT_SEED v2 (keyless, deterministic).
    PremineOutput {
        address: "zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7",
        purpose: "Bridge Vault UTXO Seed — EVM Bridge Unlock Liquidity",
        amount_zion: 100_000_000,
        amount_flowers: 100_000_000_000_000_000_000,
        category: "bridge_vault_utxo",
        unlock_height: None,
        admin_locked: true,
    },
];

// ---------------------------------------------------------------------------
// Genesis block construction
// ---------------------------------------------------------------------------

/// Build the canonical genesis `AcceptedBlock`.
///
/// The genesis block has:
/// - height 0, template_id 0, nonce 0
/// - timestamp = `GENESIS_TIMESTAMP`
/// - difficulty = `GENESIS_DIFFICULTY`
/// - 13 account-model premine transactions + 1 UTXO coinbase
/// - subsidy = 0 (no mining reward at height 0)
/// - miner_reward = 0
pub fn genesis_block() -> AcceptedBlock {
    let mut transactions: Vec<Transaction> = Vec::new();
    let mut utxo_transactions: Vec<tx::Transaction> = Vec::new();

    for (i, output) in PREMINE_OUTPUTS.iter().enumerate() {
        if output.category == "bridge_vault_utxo" {
            // UTXO coinbase for bridge vault — 100M ZION split into 6 outputs
            // so each fits in u64.
            const VAULT_AMOUNT_PER_OUTPUT: u64 = 16_666_666_666_666_666_666;
            const VAULT_AMOUNT_LAST: u64 = 16_666_666_666_666_666_670;
            let mut utxo = tx::Transaction {
                id: [0u8; 32],
                version: tx::TX_HASH_V2_VERSION,
                inputs: vec![],
                outputs: vec![],
                fee: 0,
                timestamp: GENESIS_TIMESTAMP,
            };
            for _ in 0..5 {
                utxo.outputs.push(TxOutput {
                    amount: VAULT_AMOUNT_PER_OUTPUT,
                    address: output.address.to_string(),
                    memo: None,
                });
            }
            utxo.outputs.push(TxOutput {
                amount: VAULT_AMOUNT_LAST,
                address: output.address.to_string(),
                memo: None,
            });
            utxo.id = utxo.calculate_hash();
            utxo_transactions.push(utxo);
        } else {
            // Standard account-model genesis transaction
            let tag = if i == 0 {
                format!(
                    "genesis-premine-{i:02}:{}:{}",
                    output.address, GENESIS_MESSAGE
                )
            } else {
                format!("genesis-premine-{i:02}:{}", output.address)
            };
            let tx_id = genesis_tx_id(&tag, i as u64);
            transactions.push(Transaction {
                tx_id,
                from: "genesis".to_string(),
                to: output.address.to_string(),
                amount_zion: output.amount_flowers,
                fee_zion: 0,
                nonce: i as u64,
                signature: String::new(),
                public_key: String::new(),
                memo: None,
            });
        }
    }

    let transaction_ids: Vec<String> = transactions.iter().map(|tx| tx.tx_id.clone()).collect();
    let utxo_transaction_ids: Vec<String> = utxo_transactions
        .iter()
        .map(|tx| crate::hex(&tx.id))
        .collect();

    // Build the genesis header, hash it, and produce the canonical hash.
    let genesis_target = difficulty::difficulty_to_target(difficulty::GENESIS_DIFFICULTY);
    let genesis_bits = difficulty::target_to_compact(&genesis_target);
    let merkle_root =
        crate::derive_template_merkle_root_v2_blake3(&transactions, &utxo_transactions);

    let header = MiningHeader {
        version: 3,
        previous_hash: [0u8; 32],
        merkle_root,
        timestamp: GENESIS_TIMESTAMP,
        difficulty_bits: genesis_bits,
    };

    let hash = cosmic_harmony_with_height(&header.to_bytes(), 0, 0);
    let hash_hex = crate::hex(&hash.data);
    let header_hex = crate::hex(&header.to_bytes());

    let body_hash = genesis_body_hash(&transactions);

    AcceptedBlock {
        template_id: 0,
        height: 0,
        timestamp: GENESIS_TIMESTAMP,
        difficulty: difficulty::GENESIS_DIFFICULTY,
        nonce: 0,
        hash_hex,
        header_hex,
        previous_hash_hex: crate::hex(&[0u8; 32]),
        algorithm: "deeksha_lite_v1".to_string(),
        transaction_ids,
        transactions,
        total_fees_zion: 0,
        body_hash_hex: crate::hex(&body_hash),
        subsidy_zion: 0,
        miner_reward_zion: 0,
        miner_address: String::new(),
        humanitarian_address: String::new(),
        issobella_address: String::new(),
        pool_fee_address: String::new(),
        utxo_transaction_ids,
        utxo_transactions,
    }
}

/// The frozen genesis block hash. Computed once, then hard-coded.
/// All nodes must agree on this value.
pub fn genesis_hash() -> String {
    genesis_block().hash_hex.clone()
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate premine totals at compile-time-like granularity.
pub fn validate_premine() -> Result<(), String> {
    let oasis: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "oasis_golden_egg")
        .map(|o| o.amount_flowers)
        .sum();
    if oasis != 8_250_000_000_000_000_000_000 {
        return Err(format!("OASIS total {oasis} != 8.25B flowers"));
    }

    let dao: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "dao_treasury")
        .map(|o| o.amount_flowers)
        .sum();
    if dao != 4_000_000_000_000_000_000_000 {
        return Err(format!("DAO Treasury total {dao} != 4.0B flowers"));
    }

    let infra: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "infrastructure")
        .map(|o| o.amount_flowers)
        .sum();
    if infra != 2_590_000_000_000_000_000_000 {
        return Err(format!("Infrastructure total {infra} != 2.59B flowers"));
    }

    let humanitarian: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "humanitarian")
        .map(|o| o.amount_flowers)
        .sum();
    if humanitarian != 1_440_000_000_000_000_000_000 {
        return Err(format!(
            "Humanitarian total {humanitarian} != 1.44B flowers"
        ));
    }

    let bridge_seed: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "bridge_seed")
        .map(|o| o.amount_flowers)
        .sum();
    if bridge_seed != 400_000_000_000_000_000_000 {
        return Err(format!("Bridge Seed total {bridge_seed} != 0.4B flowers"));
    }

    let bridge_vault_utxo: u128 = PREMINE_OUTPUTS
        .iter()
        .filter(|o| o.category == "bridge_vault_utxo")
        .map(|o| o.amount_flowers)
        .sum();
    if bridge_vault_utxo != 100_000_000_000_000_000_000 {
        return Err(format!(
            "Bridge Vault UTXO total {bridge_vault_utxo} != 0.1B flowers"
        ));
    }

    let grand_total: u128 = PREMINE_OUTPUTS.iter().map(|o| o.amount_flowers).sum();
    // Premine amounts are in LEGACY flower scale (10¹²); compare to LEGACY_GENESIS_PREMINE.
    // Post-migration (block H+1), these get divided by 10⁶ to match new GENESIS_PREMINE.
    if grand_total != crate::emission::LEGACY_GENESIS_PREMINE {
        return Err(format!(
            "Grand total {grand_total} != LEGACY_GENESIS_PREMINE {}",
            crate::emission::LEGACY_GENESIS_PREMINE
        ));
    }

    Ok(())
}

/// Check whether a transfer from a premine address is allowed at the given height.
///
/// **Two-layer lock:**
/// 1. **Time-lock** (`unlock_height`): Block height that must be reached.
/// 2. **Admin-lock** (`admin_locked`): Requires 3-of-3 admin multisig + DAO vote
///    to unlock.  The `admin_unlocked` closure returns `true` if the address
///    has been admin-unlocked on-chain.
///
/// Both locks must be satisfied.  An address that is admin-locked but not
/// yet admin-unlocked cannot transfer, even if the time-lock has expired.
pub fn is_premine_transfer_allowed(
    address: &str,
    current_height: u64,
    admin_unlocked: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    if let Some(output) = PREMINE_OUTPUTS.iter().find(|o| o.address == address) {
        // Layer 1: time-lock
        if let Some(unlock) = output.unlock_height {
            if current_height < unlock {
                return Err(format!(
                    "premine address {} time-locked until block {} (current: {})",
                    address, unlock, current_height
                ));
            }
        }
        // Layer 2: admin-lock
        if output.admin_locked && !admin_unlocked(address) {
            return Err(format!(
                "premine address {} admin-locked — requires 3-of-3 admin multisig \
                 + DAO vote to unlock (not yet unlocked)",
                address
            ));
        }
    }
    Ok(())
}

/// Convenience wrapper for callers that have no admin-unlock registry
/// (e.g. tests, IBD).  All admin-locked addresses will be rejected.
pub fn is_premine_transfer_allowed_no_admin(
    address: &str,
    current_height: u64,
) -> Result<(), String> {
    is_premine_transfer_allowed(address, current_height, &|_| false)
}

/// Convenience wrapper for callers where all admin-locks have been
/// satisfied (e.g. test fixtures).  Only time-locks are checked.
pub fn is_premine_transfer_allowed_admin_ok(
    address: &str,
    current_height: u64,
) -> Result<(), String> {
    is_premine_transfer_allowed(address, current_height, &|_| true)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Deterministic tx_id from a tag string and nonce (64 hex chars).
fn genesis_tx_id(tag: &str, nonce: u64) -> String {
    let hash = cosmic_harmony_ekam_deeksha(tag.as_bytes(), nonce);
    crate::hex(&hash.data)
}

/// Merkle root for genesis transactions (same derivation as regular blocks).
#[allow(dead_code)]
fn genesis_merkle_root(transactions: &[Transaction]) -> [u8; 32] {
    let mut seed = [0u8; 32];
    for tx in transactions {
        let tx_hash = cosmic_harmony_ekam_deeksha(tx.tx_id.as_bytes(), tx.nonce);
        for (slot, value) in seed.iter_mut().zip(tx_hash.data.iter()) {
            *slot ^= *value;
        }
    }
    cosmic_harmony_ekam_deeksha(&seed, transactions.len() as u64).data
}

/// Body hash for genesis (XOR-fold of tx hashes, then final hash).
fn genesis_body_hash(transactions: &[Transaction]) -> [u8; 32] {
    let mut seed = [0u8; 32];
    for tx in transactions {
        let tx_hash = cosmic_harmony_ekam_deeksha(tx.tx_id.as_bytes(), tx.nonce);
        for (slot, value) in seed.iter_mut().zip(tx_hash.data.iter()) {
            *slot ^= *value;
        }
    }
    cosmic_harmony_ekam_deeksha(&seed, transactions.len() as u64).data
}

/// Verify flower amounts are consistent with ZION amounts.
/// Uses LEGACY_FLOWERS_PER_ZION because PREMINE_OUTPUTS store old-scale (10¹²) values.
#[cfg(test)]
fn amounts_consistent(zion: u64, flowers: u128) -> bool {
    flowers == zion as u128 * crate::emission::LEGACY_FLOWERS_PER_ZION as u128
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premine_has_14_outputs() {
        assert_eq!(PREMINE_OUTPUTS.len(), 14);
    }

    #[test]
    fn premine_totals_validate() {
        validate_premine().expect("premine validation should pass");
    }

    #[test]
    fn premine_total_is_16_78b_zion() {
        let total_zion: u64 = PREMINE_OUTPUTS.iter().map(|o| o.amount_zion).sum();
        assert_eq!(total_zion, 16_780_000_000);
    }

    #[test]
    fn premine_total_flowers_matches_emission_constant() {
        // PREMINE_OUTPUTS store amounts in LEGACY flower scale (10¹², pre-3.0.3).
        // Compare to LEGACY_GENESIS_PREMINE, not the new-scale GENESIS_PREMINE.
        let total_flowers: u128 = PREMINE_OUTPUTS.iter().map(|o| o.amount_flowers).sum();
        assert_eq!(total_flowers, crate::emission::LEGACY_GENESIS_PREMINE);
    }

    #[test]
    fn premine_zion_and_flowers_consistent() {
        for output in PREMINE_OUTPUTS {
            assert!(
                amounts_consistent(output.amount_zion, output.amount_flowers),
                "{}: {} ZION != {} flowers",
                output.address,
                output.amount_zion,
                output.amount_flowers
            );
        }
    }

    #[test]
    fn no_duplicate_premine_addresses() {
        let mut seen = std::collections::HashSet::new();
        for output in PREMINE_OUTPUTS {
            assert!(
                seen.insert(output.address),
                "duplicate premine address: {}",
                output.address
            );
        }
    }

    #[test]
    fn oasis_category_totals_8_25b() {
        let total: u64 = PREMINE_OUTPUTS
            .iter()
            .filter(|o| o.category == "oasis_golden_egg")
            .map(|o| o.amount_zion)
            .sum();
        assert_eq!(total, 8_250_000_000);
    }

    #[test]
    fn dao_treasury_locked_until_144000() {
        for output in PREMINE_OUTPUTS
            .iter()
            .filter(|o| o.category == "dao_treasury")
        {
            assert_eq!(output.unlock_height, Some(DAO_TREASURY_LOCK_HEIGHT));
        }
    }

    #[test]
    fn non_dao_premine_unlocked() {
        for output in PREMINE_OUTPUTS
            .iter()
            .filter(|o| o.category != "dao_treasury")
        {
            assert_eq!(output.unlock_height, None);
        }
    }

    #[test]
    fn dao_lock_enforced() {
        let dao_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "dao_treasury")
            .unwrap()
            .address;
        // Time-locked + admin-locked: both layers must pass
        // Without admin unlock → always rejected (even after time-lock)
        assert!(is_premine_transfer_allowed(dao_addr, 0, &|_| false).is_err());
        assert!(is_premine_transfer_allowed(dao_addr, 143_999, &|_| false).is_err());
        assert!(is_premine_transfer_allowed(dao_addr, 144_000, &|_| false).is_err());
        // With admin unlock → time-lock still applies
        assert!(is_premine_transfer_allowed(dao_addr, 0, &|_| true).is_err());
        assert!(is_premine_transfer_allowed(dao_addr, 143_999, &|_| true).is_err());
        // Both satisfied → OK
        assert!(is_premine_transfer_allowed(dao_addr, 144_000, &|_| true).is_ok());
    }

    #[test]
    fn non_dao_unlocked_immediately() {
        let addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "infrastructure")
            .unwrap()
            .address;
        // No time-lock, but admin-locked
        // Without admin unlock → rejected
        assert!(is_premine_transfer_allowed(addr, 0, &|_| false).is_err());
        // With admin unlock → OK
        assert!(is_premine_transfer_allowed(addr, 0, &|_| true).is_ok());
    }

    #[test]
    fn all_premine_admin_locked() {
        // All 14 premine outputs must be admin-locked
        for output in PREMINE_OUTPUTS {
            assert!(
                output.admin_locked,
                "premine slot {} ({}) is NOT admin-locked — all must be locked",
                output.address,
                output.category
            );
        }
    }

    #[test]
    fn admin_lock_blocks_even_after_time_lock() {
        // Even after DAO_TREASURY_LOCK_HEIGHT, admin-lock still blocks
        let dao_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "dao_treasury")
            .unwrap()
            .address;
        let far_future = DAO_TREASURY_LOCK_HEIGHT + 1_000_000;
        // Without admin unlock → still blocked
        assert!(is_premine_transfer_allowed(dao_addr, far_future, &|_| false).is_err());
        // With admin unlock → OK
        assert!(is_premine_transfer_allowed(dao_addr, far_future, &|_| true).is_ok());
    }

    #[test]
    fn admin_unlock_is_per_address() {
        let dao_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "dao_treasury")
            .unwrap()
            .address;
        let infra_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "infrastructure")
            .unwrap()
            .address;

        // Unlock only DAO address, not infra
        let unlock_set = std::collections::HashSet::from([dao_addr.to_string()]);
        let check = |addr: &str| unlock_set.contains(addr);

        // DAO: time-lock passed + admin-unlocked → OK
        assert!(is_premine_transfer_allowed(dao_addr, 144_000, &check).is_ok());
        // Infra: no time-lock but NOT admin-unlocked → blocked
        assert!(is_premine_transfer_allowed(infra_addr, 0, &check).is_err());
    }

    #[test]
    fn convenience_wrappers_work() {
        let dao_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "dao_treasury")
            .unwrap()
            .address;
        let infra_addr = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "infrastructure")
            .unwrap()
            .address;

        // no_admin: all admin-locked → all rejected (even without time-lock)
        assert!(is_premine_transfer_allowed_no_admin(infra_addr, 0).is_err());
        assert!(is_premine_transfer_allowed_no_admin(dao_addr, 144_000).is_err());

        // admin_ok: only time-locks checked
        assert!(is_premine_transfer_allowed_admin_ok(infra_addr, 0).is_ok());
        assert!(is_premine_transfer_allowed_admin_ok(dao_addr, 0).is_err());
        assert!(is_premine_transfer_allowed_admin_ok(dao_addr, 144_000).is_ok());
    }

    #[test]
    fn genesis_block_has_correct_structure() {
        let block = genesis_block();
        assert_eq!(block.height, 0);
        assert_eq!(block.template_id, 0);
        assert_eq!(block.nonce, 0);
        assert_eq!(block.timestamp, GENESIS_TIMESTAMP);
        assert_eq!(block.difficulty, difficulty::GENESIS_DIFFICULTY);
        assert_eq!(block.subsidy_zion, 0);
        assert_eq!(block.miner_reward_zion, 0);
        assert_eq!(block.total_fees_zion, 0);
        assert_eq!(block.transactions.len(), 13);
        assert_eq!(block.transaction_ids.len(), 13);
        assert_eq!(block.utxo_transactions.len(), 1);
        assert_eq!(block.utxo_transaction_ids.len(), 1);
    }

    #[test]
    fn genesis_block_outputs_match_premine() {
        let block = genesis_block();
        let mut account_idx = 0;
        for output in PREMINE_OUTPUTS.iter() {
            if output.category == "bridge_vault_utxo" {
                // UTXO premine output is in utxo_transactions, not transactions
                let utxo_tx = block
                    .utxo_transactions
                    .first()
                    .expect("bridge vault utxo should exist");
                let total_utxo: u128 = utxo_tx.outputs.iter().map(|o| o.amount as u128).sum();
                assert_eq!(total_utxo, output.amount_flowers);
                assert!(utxo_tx.outputs.iter().all(|o| o.address == output.address));
            } else {
                let tx = &block.transactions[account_idx];
                assert_eq!(tx.to, output.address);
                assert_eq!(tx.amount_zion, output.amount_flowers);
                assert_eq!(tx.from, "genesis");
                assert_eq!(tx.fee_zion, 0);
                account_idx += 1;
            }
        }
        assert_eq!(account_idx, block.transactions.len());
    }

    #[test]
    fn genesis_vault_utxo_has_six_outputs() {
        let block = genesis_block();
        let utxo_tx = block
            .utxo_transactions
            .first()
            .expect("vault utxo should exist");
        assert_eq!(utxo_tx.outputs.len(), 6);
        assert!(
            utxo_tx.inputs.is_empty(),
            "genesis UTXO should be coinbase (no inputs)"
        );
        assert_eq!(utxo_tx.fee, 0);
        let total: u128 = utxo_tx.outputs.iter().map(|o| o.amount as u128).sum();
        assert_eq!(total, 100_000_000_000_000_000_000_u128);
    }

    #[test]
    fn genesis_hash_is_deterministic() {
        let h1 = genesis_hash();
        let h2 = genesis_hash();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn genesis_block_hash_is_nonzero() {
        let block = genesis_block();
        assert_ne!(block.hash_hex, crate::hex(&[0u8; 32]));
    }

    #[test]
    fn genesis_body_hash_is_deterministic() {
        let b1 = genesis_block();
        let b2 = genesis_block();
        assert_eq!(b1.body_hash_hex, b2.body_hash_hex);
    }

    #[test]
    fn genesis_tx_ids_are_unique() {
        let block = genesis_block();
        let mut seen = std::collections::HashSet::new();
        for tx in &block.transactions {
            assert!(
                seen.insert(&tx.tx_id),
                "duplicate genesis tx_id: {}",
                tx.tx_id
            );
        }
    }

    #[test]
    fn genesis_message_is_embedded() {
        assert!(GENESIS_MESSAGE.contains("ZION Mainet Launch v3"));
        assert!(GENESIS_MESSAGE.contains("Peace & One Love 4ever"));
        assert!(GENESIS_MESSAGE.contains("Yose / Zion Creator"));
    }

    #[test]
    fn genesis_message_full_contains_ascii_art() {
        assert!(GENESIS_MESSAGE_FULL.contains("ZION"));
        assert!(GENESIS_MESSAGE_FULL.contains("Mainet Launch v3"));
        assert!(GENESIS_MESSAGE_FULL.contains("Golden Age begins"));
    }

    #[test]
    fn canonical_mainnet_subsidy_wallets_track_label_derivation() {
        use crate::crypto;
        // Labels must produce valid addresses (they are deterministic from the
        // repo-pinned label strings).  The actual canonical addresses in
        // MAINNET_CANONICAL_*_WALLET were generated from an offline mnemonic
        // seed during genesis regeneration, so they will NOT match the
        // label-derived addresses.  This test only guards that the label
        // derivation function itself works.
        for label in [
            MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_LABEL,
            MAINNET_CANONICAL_POOL_FEE_SUBSIDY_LABEL,
            MAINNET_CANONICAL_DEFAULT_MINER_LABEL,
            MAINNET_CANONICAL_POOL_PAYOUT_LABEL,
        ] {
            let addr = crypto::canonical_address_for_label(label);
            assert!(
                crypto::is_valid_address(&addr),
                "label '{label}' produced invalid address: {addr}"
            );
        }
    }

    #[test]
    fn canonical_mainnet_addresses_are_valid_zion1() {
        for addr in [
            MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET,
            MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET,
            MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET,
            MAINNET_CANONICAL_DEFAULT_MINER_WALLET,
            MAINNET_CANONICAL_POOL_PAYOUT_WALLET,
        ] {
            assert!(
                crate::crypto::is_valid_address(addr),
                "invalid canonical address: {addr}"
            );
        }
        // Validate that the premine humanitarian address is also a valid zion1 address.
        let premine_humanitarian = PREMINE_OUTPUTS
            .iter()
            .find(|o| o.category == "humanitarian")
            .unwrap()
            .address;
        assert!(
            crate::crypto::is_valid_address(premine_humanitarian),
            "invalid premine humanitarian address: {premine_humanitarian}"
        );
    }

    #[test]
    fn canonical_subsidy_wallets_are_distinct_and_not_duplicate_premine_slots() {
        let canon = [
            MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET,
            MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET,
            MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET,
            MAINNET_CANONICAL_DEFAULT_MINER_WALLET,
            MAINNET_CANONICAL_POOL_PAYOUT_WALLET,
        ];
        let mut seen = std::collections::HashSet::new();
        for a in canon {
            assert!(seen.insert(a), "duplicate canonical address: {a}");
            assert!(
                !PREMINE_OUTPUTS.iter().any(|o| o.address == a),
                "canonical operator address must not duplicate a genesis premine recipient: {a}"
            );
        }
    }
    #[test]
    fn genesis_coinbase_tx_includes_message() {
        // The first tx's tx_id should differ from a plain "genesis-premine-00" hash
        // because it includes GENESIS_MESSAGE in its tag
        let block = genesis_block();
        let plain_tag = "genesis-premine-00";
        let plain_hash = crate::hex(&cosmic_harmony_ekam_deeksha(plain_tag.as_bytes(), 0).data);
        assert_ne!(
            block.transactions[0].tx_id, plain_hash,
            "coinbase tx_id must include genesis message"
        );
    }
}
