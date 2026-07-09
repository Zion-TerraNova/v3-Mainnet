fn main() {
    println!("New canonical addresses from v2 labels:");
    println!();

    let issobella_label =
        "ZION_V3_MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_RECIPIENT_v2_2026-06-03-GENESIS-RESET";
    let pool_fee_label =
        "ZION_V3_MAINNET_CANONICAL_POOL_FEE_SUBSIDY_RECIPIENT_v2_2026-06-03-GENESIS-RESET";
    let default_miner_label =
        "ZION_V3_MAINNET_CANONICAL_DEFAULT_SOLO_MINER_COINBASE_v2_2026-06-03-GENESIS-RESET";
    let pool_payout_label =
        "ZION_V3_MAINNET_CANONICAL_POOL_PPLNS_PAYOUT_SIGNER_v2_2026-06-03-GENESIS-RESET";

    let issobella_addr = zion_core::crypto::canonical_address_for_label(issobella_label);
    let pool_fee_addr = zion_core::crypto::canonical_address_for_label(pool_fee_label);
    let default_miner_addr = zion_core::crypto::canonical_address_for_label(default_miner_label);
    let pool_payout_addr = zion_core::crypto::canonical_address_for_label(pool_payout_label);

    println!("ISSOBELLA_WALLET={}", issobella_addr);
    println!("POOL_FEE_WALLET={}", pool_fee_addr);
    println!("DEFAULT_MINER_WALLET={}", default_miner_addr);
    println!("POOL_PAYOUT_WALLET={}", pool_payout_addr);
}
