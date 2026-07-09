//! Print `.env`-style lines for repo-pinned mainnet subsidy addresses.
//!
//! The wallet constants come from offline mnemonic seeds (see `genesis.rs` comments) and
//! intentionally do NOT match `crypto::canonical_address_for_label()` derivation. The pool
//! payout signing key must be taken from the offline mnemonic backup, not from this binary.

fn main() {
    use zion_core::genesis::{
        MAINNET_CANONICAL_DEFAULT_MINER_WALLET, MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET,
        MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET, MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET,
        MAINNET_CANONICAL_POOL_PAYOUT_WALLET,
    };

    // NOTE: MAINNET_CANONICAL_*_WALLET constants are generated from offline
    // mnemonic seeds, not from the canonical_address_for_label() derivation. They
    // intentionally do NOT match the label-derived addresses. Do not add
    // debug_assert_eq! checks here; they will panic in debug builds. See
    // hardforkfix.md §4 for the pool wallet resolution (2026-07-02).

    println!("# Mainnet canonical addresses from offline mnemonics");
    println!("# Pool payout SK must be set from the offline mnemonic backup, NOT from a label derivation.");
    println!("ZION_MINER_ADDRESS={MAINNET_CANONICAL_DEFAULT_MINER_WALLET}");
    println!("ZION_HUMANITARIAN_WALLET={MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET}");
    println!("ZION_ISSOBELLA_WALLET={MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET}");
    println!("ZION_POOL_FEE_WALLET={MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET}");
    println!("ZION_POOL_WALLET={MAINNET_CANONICAL_POOL_PAYOUT_WALLET}");
}
