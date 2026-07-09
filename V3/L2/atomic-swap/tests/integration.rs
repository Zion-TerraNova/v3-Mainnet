//! E2E Integration Tests for ZION Atomic Swap Daemon.
//! Tests the full HTLC lifecycle: lock -> query -> claim (or lock -> refund).

use chrono::Utc;
use zion_atomic_swap::db::SwapDb;
use zion_atomic_swap::executor::SwapExecutor;
use zion_atomic_swap::types::{HtlcRecord, SwapState};

fn make_record(hash_hex: &str, preimage: Option<&str>, expires_in_sec: i64) -> HtlcRecord {
    HtlcRecord {
        hash_hex: hash_hex.to_string(),
        amount_flowers: 5_000_000_000_000, // 5 ZION
        locker_address: "zion1locker000000000000000000000000000001".to_string(),
        lock_tx_id: "0".repeat(64),
        lock_block_height: 100,
        expires_at: Utc::now().timestamp() + expires_in_sec,
        counterparty_chain: "base".to_string(),
        counterparty_addr: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        claimant_address: None,
        state: SwapState::Pending,
        preimage_hex: preimage.map(|p| p.to_string()),
        release_tx_id: None,
        release_recipient: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[tokio::test]
async fn test_full_htlc_claim_flow() {
    // 1. Initialize in-memory DB and executor
    let db = SwapDb::in_memory().unwrap();
    let executor = SwapExecutor::new_dummy();

    let preimage = "a".repeat(64);
    // preimage_hex "a".repeat(64) has a SHA-256 hash
    // We decode hex to bytes, then hash, then encode to hex
    use sha2::Digest;
    let preimage_bytes = hex::decode(&preimage).unwrap();
    let hash_bytes = sha2::Sha256::digest(&preimage_bytes);
    let hash = hex::encode(hash_bytes);

    let rec = make_record(&hash, None, 7200);

    // 2. Lock HTLC
    db.insert_htlc(&rec).unwrap();

    // 3. Verify it is pending
    let pending = db.list_pending(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].hash_hex, hash);

    // 4. Claim with correct preimage
    executor
        .execute_claim(&db, &hash, &preimage, "zion1bob")
        .await
        .unwrap();

    // 5. Verify it is claimed
    let fetched = db.get_htlc(&hash).unwrap().unwrap();
    assert_eq!(fetched.state, SwapState::Claimed);
    assert_eq!(fetched.preimage_hex.as_deref(), Some(preimage.as_str()));
    assert!(fetched.release_tx_id.is_some());
}

#[tokio::test]
async fn test_htlc_claim_preimage_mismatch() {
    let db = SwapDb::in_memory().unwrap();
    let executor = SwapExecutor::new_dummy();

    let hash = "c".repeat(64);
    let bad_preimage = "d".repeat(64);

    let rec = make_record(&hash, None, 7200);
    db.insert_htlc(&rec).unwrap();

    // Claim with bad preimage should fail
    let res = executor
        .execute_claim(&db, &hash, &bad_preimage, "zion1bob")
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_htlc_refund_active_fails() {
    let db = SwapDb::in_memory().unwrap();
    let executor = SwapExecutor::new_dummy();

    let hash = "e".repeat(64);

    // Lock with 2 hours active timelock
    let rec = make_record(&hash, None, 7200);
    db.insert_htlc(&rec).unwrap();

    // Refund active HTLC should fail
    let res = executor.execute_refund(&db, &hash).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn test_htlc_refund_expired_succeeds() {
    let db = SwapDb::in_memory().unwrap();
    let executor = SwapExecutor::new_dummy();

    let hash = "f".repeat(64);

    // Lock with expired timelock (-10 seconds)
    let rec = make_record(&hash, None, -10);
    db.insert_htlc(&rec).unwrap();

    // Refund expired HTLC should succeed
    executor.execute_refund(&db, &hash).await.unwrap();

    // Verify it is refunded
    let fetched = db.get_htlc(&hash).unwrap().unwrap();
    assert_eq!(fetched.state, SwapState::Refunded);
    assert!(fetched.release_tx_id.is_some());
}
