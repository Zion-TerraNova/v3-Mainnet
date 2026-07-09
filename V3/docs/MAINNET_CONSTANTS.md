# ZION Mainnet Constants & Fixed Parameters

> **Canonical technical reference** — all values extracted directly from `V3/` source code as of 2026-07-01 (3.0.4 canonical).
> Source of truth: `V3/L1/core/src/` and `V3/L1/cosmic-harmony/src/`.

---

## 1. Emission & Supply (`emission.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `FLOWERS_PER_ZION` | `1_000_000` | 1 ZION = 10^6 flowers (6 decimals) (updated to 6-decimal in 3.0.3 fork) |
| `TOTAL_SUPPLY` | `144_000_000_000 * FLOWERS_PER_ZION` | Hard cap in flowers (~1.44e23) |
| `GENESIS_PREMINE` | `16_780_000_000 * FLOWERS_PER_ZION` | Genesis reserve in flowers |
| `MINING_EMISSION` | `TOTAL_SUPPLY - GENESIS_PREMINE` | ~127.22B ZION |
| `BLOCK_TIME_SECONDS` | `60` | Target block time |
| `BLOCKS_PER_YEAR` | `525_600` | 60 s × 365 days |
| `BLOCKS_PER_DECADE` | `10 * BLOCKS_PER_YEAR` = `5_256_000` | Decay interval |
| `DECAY_NUMERATOR` | `4` | Decay factor numerator (4/5 = 0.8) |
| `DECAY_DENOMINATOR` | `5` | Decay factor denominator |
| `MAX_DECAY_DECADES` | `10` | Decades before tail emission |
| `BASE_REWARD` | `5_400_067_000` flowers | Initial block reward (5,400.067 ZION) — 6-decimal scale (3.0.3 fork) |
| `GENESIS_HASH` | `d28dc404abfd4e22b313d3a7e8b680453328a77ace68b47466a14d18aff6df5d` | Current canonical genesis |
| `TAIL_REWARD` | `724_784_723` flowers | Perpetual tail emission (~724.7847 ZION) — 6-decimal scale (3.0.3 fork) |
| `COINBASE_MATURITY` | `100` | Blocks before coinbase spendable |
| `MINER_PCT` | `89` | Miner share of block reward (%) |
| `HUMANITARIAN_PCT` | `5` | Humanitarian fund share (%) |
| `ISSOBELLA_PCT` | `5` | L5/L6 Issobella fund share (%) |
| `POOL_FEE_PCT` | `1` | Pool operator fee share (%) |

**Reward split verification:** 89 + 5 + 5 + 1 = 100 %.

---

## 2. Genesis & Premine (`genesis.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `DAO_TREASURY_LOCK_HEIGHT` | `144_000` | ~100 days lock (changed in 3.0.3 fork from 525,600) |
| `GENESIS_TIMESTAMP` | `1_767_225_600` | Unix seconds (TBD at ceremony) |
| `GENESIS_MESSAGE` | Embedded ASCII dedication | See `GENESIS_MESSAGE.txt` |
| `PREMINE_OUTPUTS` | 13 outputs | See table below |

### Premine Distribution (13 outputs)

| # | Category | Amount (ZION) |
|---|----------|---------------|
| 1–5 | OASIS + Golden Egg/XP (5 × 1.65B) | 8,250,000,000 |
| 6 | DAO Treasury (main) | 2,500,000,000 |
| 7 | DAO Grants & Bounties | 1,000,000,000 |
| 8 | DAO Ecosystem Bootstrap | 500,000,000 |
| 9 | Core Development Fund | 1,000,000,000 |
| 10 | Network Infrastructure | 1,000,000,000 |
| 11 | Genesis Creator | 590,000,000 |
| 12 | Humanitarian — Children Future Fund | 1,440,000,000 |
| 13 | Bridge Seed Fund — EVM Bridge Liquidity | 400,000,000 |
| 14 | **Bridge Vault UTXO Seed — EVM Bridge Unlock Liquidity** | **100,000,000** |
| | **Total** | **16,780,000,000** |

### Subsidy Wallets (canonical addresses)

| Label | Address |
|-------|---------|
| `MAINNET_CANONICAL_HUMANITARIAN_SUBSIDY_WALLET` | `zion1s29403j538w6p6n0p783l6w5v6t254c0380c2d4` |
| `MAINNET_CANONICAL_ISSOBELLA_SUBSIDY_WALLET` | `zion140n8a8t6f3083232r0g6c498r6c0d423f4h9702` |
| `MAINNET_CANONICAL_POOL_FEE_SUBSIDY_WALLET` | `zion196m4n8x764v7a0s406j40094a8z5j8m6z7nk342` |
| `MAINNET_CANONICAL_DEFAULT_MINER_WALLET` | `zion1w523a76830x2t5m7f3j023w265e8g5c400a4790` |
| `MAINNET_CANONICAL_POOL_PAYOUT_WALLET` | `zion16825y2v5f3q507e5c2e0j8n666z43558l3zt604` |

> These addresses are hardcoded in `genesis.rs` and derived deterministically from canonical labels.
> Premine addresses are in `PREMINE_ADDRESSES_PUBLIC.txt`.

---

## 3. Fees & Addresses (`fee.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `MIN_TX_FEE` | `1` flower | Minimum transaction fee (0.000001 ZION) — 6-decimal scale (3.0.3 fork) |
| `MIN_FEE_RATE` | `1` flower/byte | Minimum fee rate |
| `MAX_TX_SIZE` | `100_000` bytes | Max transaction size |
| `MAX_OUTPUT_AMOUNT` | `u64::MAX` | Single output cap |
| `BURN_ADDRESS` | `zion1burn0000000000000000000000000000000dead` | Fee burn sink |
| `DAO_ADDRESS` | `zion1t4l2f5j737989828v295n7z4r3v5j8k895m56n4` | DAO treasury address (main governance) |
| `BRIDGE_VAULT_ADDRESS` | `zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7` | L1 bridge vault (from `BRIDGE_VAULT_SEED = "ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"`) |

---

## 4. Consensus & PoW — Ekam Deeksha v2 (`deeksha.rs`, `algorithms_npu.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `POW_PROFILE` | `"cosmic_harmony_ekam_deeksha_v2"` | Canonical consensus profile |
| `CHV_EKAM_FORK_HEIGHT` | `0` | Ekam activation (production) |
| `CHV_EKAM_V2_FORK_HEIGHT` | `0` | Ekam v2 activation (production) |
| `CHV42_DUAL_SPIN_FORK_HEIGHT` | `u64::MAX` | Future dual-spin gate (disabled) |
| `TX_HASH_V2_ACTIVATION_HEIGHT` | `0` | Transaction hash v2 from genesis |
| `BODY_ROOT_V2_ACTIVATION_HEIGHT` | `0` | BLAKE3 body Merkle from genesis |
| `EKAM_FUSION_ROUNDS` | `8` | Final hash reduction rounds |
| `EKAM_V2_SCRATCHPAD_SIZE` | `256 * 1024` = 262,144 bytes | Tier 1 ASIC resistance (256 KiB) |
| `EKAM_V2_PASSES` | `4` | Scratchpad passes |
| `EKAM_V2_RANDOM_READS` | `256` | Dependent memory reads per pass |
| `CHV4_MLP_GENESIS_SEED` | `b"ZION_CHv4_mixing_v1_genesis_seed"` | NPU epoch seed |
| `CHV4_NPU_FORK_HEIGHT` | `0` | NPU mixing from genesis |
| `NPU_EPOCH_LENGTH` | `2016` | Epoch blocks for MLP topology rotation |
| `SCRATCHPAD_SIZE` | `64 * 1024` = 65,536 bytes | Legacy v1 scratchpad |
| `SCRATCHPAD_SIZE_V2` | `256 * 1024` = 262,144 bytes | Current scratchpad |

### Test Vectors

| Constant | Value |
|----------|-------|
| `EKAM_CANONICAL_TEST_VECTOR_HEX` | Canonical hex vector (v1 gate) |
| `EKAM_V2_CANONICAL_TEST_VECTOR_HEX` | Canonical hex vector (v2 gate) |

---

## 5. P2P & Network Security (`p2p_security.rs`, `peer_manager.rs`, `orphan.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_CONNECTIONS` | `128` | Global max TCP connections |
| `MAX_MESSAGES_PER_WINDOW` | `100` | Rate limit per peer per window |
| `RATE_LIMIT_WINDOW_SECS` | `60` | Rate limit window |
| `BAN_DURATIONS` | `[300, 1800, 7200]` | Escalating ban seconds (5m, 30m, 2h) |
| `MAX_BAN_STRIKES` | `3` | Permanent ban after 3 strikes |
| `MAX_PEERS` | `128` | Peer manager max peers |
| `MIN_OUTBOUND` | `8` | Minimum outbound connections |
| `MAX_PER_SUBNET` | `4` | Subnet diversity limit |
| `HEARTBEAT_INTERVAL` | `60` s | Peer heartbeat |
| `PEER_IDLE_TIMEOUT` | `300` s | Idle disconnect |
| `INITIAL_SCORE` | `100` | Peer reputation start |
| `BAN_THRESHOLD` | `-100` | Auto-ban score floor |
| `PENALTY_INVALID_BLOCK` | `-50` | Score penalty |
| `PENALTY_INVALID_TX` | `-10` | Score penalty |
| `PENALTY_PROTOCOL_VIOLATION` | `-30` | Score penalty |
| `REWARD_VALID_BLOCK` | `+20` | Score reward |
| `REWARD_VALID_TX` | `+1` | Score reward |
| `REWARD_FAST_RESPONSE` | `+5` | Score reward |
| `CHAIN_ID` | `"zion-mainnet-1"` | Network identity |
| `MAX_ORPHAN_BLOCKS` | `200` | Orphan buffer size |
| `ORPHAN_EXPIRY_SECS` | `600` | Orphan eviction timeout |

---

## 6. Mempool & Validation (`mempool_v2.rs`, `validation.rs`, `tx.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_MEMPOOL_SIZE` | `10_000` | Max mempool transactions |
| `MAX_MEMPOOL_BYTES` | `20_971_520` | Max mempool bytes (20 MiB) |
| `COINBASE_MATURITY` | `100` | From `emission.rs` |
| `MAX_BLOCK_SIZE` | `1_048_576` | 1 MiB block size limit |
| `MAX_TIMESTAMP_DRIFT` | `7_200` | ±2 hours timestamp sanity |
| `TX_HASH_V2_VERSION` | `2` | Current transaction version |
| `MAX_TEMPLATE_TRANSACTIONS` | `16` | Max TX per block template |
| `MAX_MEMPOOL_TRANSACTIONS` | `4_096` | Max TX in mempool (alternate limit) |
| `MAX_TEMPLATE_UTXO_TRANSACTIONS` | `16` | Max UTXO TX per template |

---

## 7. Chain & Reorg (`lib.rs`, chain constants)

| Constant | Value | Description |
|----------|-------|-------------|
| `HEADER_SIZE` | `80` | Block header bytes |
| `NODE_PROTOCOL_VERSION` | `"zion-v3-node/0.1"` | P2P version string |
| `MAX_REORG_DEPTH` | `10` | Constitutional max reorg |
| `SOFT_FINALITY_DEPTH` | `60` | Soft finality in blocks |
| `BRIDGE_MIN_VALIDATOR_PROOFS` | `3` | Minimum validator signatures |

---

## 8. Storage & IBD (`storage.rs`, `ibd.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `DEFAULT_MAP_SIZE_BYTES` | `10 * 1024 * 1024 * 1024` = 10 GiB | LMDB map size |
| `SCHEMA_VERSION` | `1` | Database schema version |
| `IBD_THRESHOLD` | `50` | Blocks behind to trigger IBD |
| `IBD_BATCH_SIZE` | `500` | Blocks per sync batch |
| `IBD_STALL_TIMEOUT` | `120` s | Stall detection timeout |
| `IBD_MAX_RETRIES` | `3` | Retry count before peer demotion |
| `IBD_MAX_INFLIGHT` | `4` | Parallel IBD requests |

---

## 9. Revenue System (`revenue.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `ZION_ALLOCATION` | `0.50` | 50 % canonical ZION mining |
| `MULTI_ALGO_ALLOCATION` | `0.25` | 25 % multi-algo external |
| `NCL_ALLOCATION` | `0.25` | 25 % NCL AI compute |
| `MIN_ZION_ALLOCATION` | `0.50` | Minimum ZION allocation floor |
| `MERGED_MINING_FEE` | `0.05` | 5 % fee for merged mining |
| `PROFIT_SWITCH_FEE` | `0.02` | 2 % fee for profit switching |
| `BLAKE3_EXTERNAL_FEE` | `0.02` | 2 % fee for Blake3 external |
| `NCL_FEE` | `0.10` | 10 % fee for NCL tasks |
| `ZION_MINER_PCT` | `89` | Miner PPLNS share |
| `ZION_HUMANITARIAN_PCT` | `5` | Humanitarian tithe |
| `ZION_ISSOBELLA_PCT` | `5` | Issobella fund |
| `ZION_POOL_PCT` | `1` | Pool operator fee |
| `CIRCUIT_BREAKER_THRESHOLD` | `10` | Consecutive failures to open circuit |
| `CIRCUIT_BREAKER_RESET_SECS` | `60` | Cooldown before auto-reset |
| `SEEN_HEIGHTS_WINDOW` | `100_000` | Bounded idempotency window |

---

## 10. RPC (`rpc.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `JSONRPC_VERSION` | `"2.0"` | Protocol version |
| `PARSE_ERROR` | `-32700` | JSON-RPC standard error |
| `INVALID_REQUEST` | `-32600` | JSON-RPC standard error |
| `METHOD_NOT_FOUND` | `-32601` | JSON-RPC standard error |
| `INVALID_PARAMS` | `-32602` | JSON-RPC standard error |
| `INTERNAL_ERROR` | `-32603` | JSON-RPC standard error |
| `BLOCK_NOT_FOUND` | `-32001` | ZION custom error |
| `TX_NOT_FOUND` | `-32002` | ZION custom error |
| `INVALID_ADDRESS` | `-32003` | ZION custom error |
| `TX_REJECTED` | `-32004` | ZION custom error |
| `NOT_SYNCED` | `-32005` | ZION custom error |

---

## 11. Wallet & Batch (`wallet.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_BATCH_RECIPIENTS` | `200` | Max PPLNS payout recipients per TX |
| `MIN_PAYOUT_AMOUNT` | `10_000_000_000_000` flowers | 10 ZION minimum payout |

---

## 12. Propagation (`propagation.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `MAX_SEEN_BLOCKS` | `2_048` | Dedup cache for block relay |
| `MAX_SEEN_TXS` | `8_192` | Dedup cache for tx relay |

---

## 13. Discovery (`discovery.rs`)

| Constant | Value | Description |
|----------|-------|-------------|
| `DISCOVERY_PORT` | `8335` | Peer discovery UDP/TCP port |
| `DISCOVERY_INTERVAL` | `300` s | Discovery broadcast interval |
| `MAX_DISCOVERED` | `1_000` | Max discovered peers buffer |
| `PEER_EXPIRY` | `86_400` s | Peer record TTL (24 h) |
| `MAX_ANNOUNCE_AGE` | `600` s | Max age for valid announce |
| `DNS_SEEDS` | `&[]` | Configurable at runtime |

---

## 14. Difficulty (`difficulty.rs` — inferred)

| Parameter | Value |
|-----------|-------|
| DAA type | LWMA (Linearly Weighted Moving Average) |
| Window | 60 blocks |
| Max change per block | ±25 % |
| Solve-time clamp | 30–120 s |
| Min difficulty | 1 000 |

---

## 15. Summary Tables

### Constitutional Immutable Parameters

| Parameter | Value | Location |
|-----------|-------|----------|
| Total supply | 144,000,000,000 ZION | `emission.rs` |
| Initial block reward | 5,400.067 ZION | `emission.rs` |
| Decay schedule | −20 % / 10 years (×4/5) | `emission.rs` |
| Tail emission | ~724.7847 ZION/block | `emission.rs` |
| Block time | 60 s | `emission.rs` |
| Chain ID | `zion-mainnet-1` | `orphan.rs` |
| Max reorg depth | 10 blocks | `chain.rs` / `lib.rs` |
| Coinbase maturity | 100 blocks | `emission.rs` / `validation.rs` |
| DAO Treasury lock | 144,000 blocks (~100 days) | `genesis.rs` |
| Fee model | 100 % burn | `fee.rs` |
| Reward split | 89/5/5/1 % | `emission.rs` / `revenue.rs` |
| Consensus | Ekam Deeksha v2 | `deeksha.rs` |
| Address prefix | `zion1` | `crypto.rs` |

### Security Limits

| Limit | Value |
|-------|-------|
| Max block size | 1,048,576 bytes |
| Max tx size | 100,000 bytes |
| Max mempool TX | 10,000 |
| Max mempool bytes | 20,971,520 |
| Max peers | 128 |
| Max connections | 128 |
| Max per subnet | 4 |
| Min outbound | 8 |
| Max orphan blocks | 200 |
| Ban escalation | 300s → 1800s → 7200s → permanent |

---

*Generated from `V3/` source code — 2026-07-01 (3.0.4 canonical)*
*Verify against current code before mainnet launch*
