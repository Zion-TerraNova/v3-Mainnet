# ZION v3 — Technical Whitepaper

> **Version 3.0.4** · July 2026 · MIT License
> Genesis hash: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`

---

## 1. Abstract

ZION is a multi-layer proof-of-work blockchain infrastructure designed for
long-term economic sustainability and humanitarian value alignment. The v3
mainnet introduces a dual-algorithm consensus (**Ekam Deeksha**) that combines
memory-hard hashing with NPU-friendly mixing layers, a dual transaction model
(UTXO + account), and a cross-chain bridge deployed across six EVM networks.

The protocol enforces a **89/5/5/1 fee split** — 89% to miners, 5% to a
humanitarian fund, 5% to the Issobella community fund, and 1% pool fee (burned)
— embedding charitable giving directly into block reward distribution. The
total supply is hard-capped at **144 billion ZION** with a decaying emission
schedule over 100 years followed by perpetual tail emission.

---

## 2. Design Philosophy

ZION is built on three principles:

1. **Proof-of-Work integrity** — No pre-mined ICO, no insider token allocation
   beyond the transparent genesis premine. Mining is open to all.
2. **Embedded humanitarianism** — Every block reward automatically routes 5%
   to a children's future fund and 5% to community development. This is
   consensus-enforced, not optional charity.
3. **Cross-chain openness** — ZION is not an island. The bridge connects L1
   to six EVM chains (Base, BSC, Polygon, Arbitrum, Optimism, Avalanche)
   with a 5/5 validator quorum, enabling wrapped ZION (wZION) circulation
   in DeFi ecosystems.

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────┐
│                    L1 Core (Rust)                    │
│                                                      │
│  Consensus    P2P Network    JSON-RPC    Mempool     │
│  (Ekam Deeksha) (QUIC/Quinn) (17+ methods) (fee-pri) │
│                                                      │
│  UTXO + Account TX    Wallet (Ed25519)    LMDB Store │
└────────────────────────┬────────────────────────────┘
                         │ Bridge Relay (5/5 quorum)
┌────────────────────────┴────────────────────────────┐
│              L2 DeFi (Base Mainnet)                  │
│                                                      │
│  wZION (ERC-20)    ZIONBridge    ZIONGovernance      │
│  ZIONTreasury      ZIONStaking   ZIONFarm            │
│  Atomic Swap (HTLC)   DAO (5 guardians)              │
└──────────────────────────────────────────────────────┘
```

### Layer 1 — Consensus

| Component | Technology |
|-----------|-----------|
| Language | Rust (stable) |
| Consensus | Ekam Deeksha v2 (dual-algo PoW) |
| Signatures | Ed25519 |
| Hashing | BLAKE3 (tx IDs, Merkle roots, body roots) |
| Difficulty | LWMA (60-block window, ±25% clamp) |
| Block time | 60 seconds (target) |
| Block size | 1 MiB max |
| Storage | LMDB (10 GiB map, schema v1) |
| P2P | Quinn/QUIC, 128 max connections, rate-limited |
| TX models | UTXO + account (dual, with memo field) |

### Layer 2 — DeFi

| Contract | Address | Chain |
|----------|---------|-------|
| wZION (ERC-20) | `0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6` | Base + 5 chains |
| ZIONBridge | `0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467` | Base |
| ZIONGovernance | `0xB77eB4ab9468Ce03FBd7eCec70e976EFCfa623E8` | Base |
| ZIONTreasury | `0x455f465ac7e14fdA97dC46fdd74bCa78bfC0aEeD` | Base |
| ZIONStaking | `0xbd5cEe7878337d22188BFBaF9aa9F39A850Be78B` | Base |
| ZIONFarm | `0x167B2753F5D8D9F8e62875cc9e379d7804308B08` | Base |

All contracts verified on Basescan.

---

## 4. Consensus — Ekam Deeksha

### 4.1 Algorithm Overview

Ekam Deeksha ("One Initiation") is ZION's dual-algorithm proof-of-work. It
combines two computational phases:

1. **Memory-hard phase (Tier 1)** — A 256 KiB scratchpad is filled with
   BLAKE3-derived data over 4 passes with 256 dependent memory reads per pass.
   This provides ASIC resistance by requiring significant on-chip memory.

2. **NPU mixing phase (Tier 2)** — A neural-network-inspired mixing layer
   applies MLP (multi-layer perceptron) topology rotations per epoch (2016
   blocks). This phase is designed to be NPU-friendly, opening mining to
   future neuromorphic hardware beyond GPUs and ASICs.

3. **Fusion phase** — 8 rounds of final hash reduction combine the outputs
   of both tiers into the final block hash.

### 4.2 Parameters

| Parameter | Value |
|-----------|-------|
| Profile | `cosmic_harmony_ekam_deeksha_v2` |
| Scratchpad size | 256 KiB (262,144 bytes) |
| Passes | 4 |
| Random reads/pass | 256 |
| Fusion rounds | 8 |
| NPU epoch length | 2,016 blocks |
| NPU genesis seed | `ZION_CHv4_mixing_v1_genesis_seed` |

### 4.3 Difficulty Adjustment

ZION uses **LWMA** (Linear Weighted Moving Average) difficulty adjustment:

- **Window**: 60 blocks (~1 hour)
- **Clamp**: ±25% per adjustment
- **Target solve time**: 60 seconds
- **Genesis difficulty**: Fixed initial value

This provides smooth retargeting resistant to timewarp attacks while
maintaining a stable 60-second block cadence.

---

## 5. Token Economics

### 5.1 Supply

| Parameter | Value |
|-----------|-------|
| Total supply | 144,000,000,000 ZION (144 billion) |
| Decimals | 6 (1 ZION = 1,000,000 flowers) |
| Genesis premine | 16,780,000,000 ZION (11.65%) |
| Mining emission | 127,220,000,000 ZION (88.35%) |

### 5.2 Emission Schedule

Block rewards decay by a factor of **4/5 (0.8)** every decade (5,256,000
blocks). After 10 decades (~100 years), a perpetual **tail emission** kicks
in to sustain miner incentives indefinitely.

| Decade | Block Reward (ZION) |
|--------|---------------------|
| 1 | 5,400.067 |
| 2 | 4,320.054 |
| 3 | 3,456.043 |
| 4 | 2,764.834 |
| 5 | 2,211.867 |
| 6 | 1,769.494 |
| 7 | 1,415.595 |
| 8 | 1,132.476 |
| 9 | 905.981 |
| 10+ (tail) | 724.785 (perpetual) |

### 5.3 Fee Split (Consensus-Enforced)

Every block reward is automatically split:

| Recipient | Share | Description |
|-----------|-------|-------------|
| Miner | 89% | Proof-of-work reward |
| Humanitarian Fund | 5% | Children's Future Fund |
| Issobella Fund | 5% | Community/L5 development |
| Pool Fee | 1% | Burned (deflationary) |

**Total: 100%** — verified in `emission.rs` and enforced at the consensus layer.

### 5.4 Coinbase Maturity

Mined coins require **100 blocks** (~100 minutes) of maturity before they
can be spent. This prevents reorg-based double-spending of freshly mined
rewards.

---

## 6. Genesis Block

### 6.1 Overview

The genesis block (height 0) contains **14 premine outputs** totaling
16,780,000,000 ZION. There is no mining subsidy at height 0 — the premine
is the sole coinbase.

- **Genesis hash**: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
- **Timestamp**: `1767225600` (2026-01-01 00:00:00 UTC)
- **Previous hash**: `0000...0000` (all zeros)
- **Algorithm**: `deeksha_lite_v1`

See [`genesis.md`](./genesis.md) for the full premine allocation table and
genesis message.

### 6.2 Premine Distribution

| Category | Amount (ZION) | % of Premine |
|----------|---------------|--------------|
| OASIS + Golden Egg (5 slots) | 8,250,000,000 | 49.2% |
| DAO Treasury (3 slots) | 4,000,000,000 | 23.8% |
| Infrastructure (3 slots) | 2,590,000,000 | 15.4% |
| Humanitarian (1 slot) | 1,440,000,000 | 8.6% |
| Bridge Seed (1 slot) | 400,000,000 | 2.4% |
| Bridge Vault UTXO (1 slot) | 100,000,000 | 0.6% |
| **Total** | **16,780,000,000** | **100%** |

All premine outputs are **admin-locked** (require 3-of-3 multisig + DAO vote
to unlock). DAO Treasury slots are additionally **time-locked** until block
144,000 (~100 days).

---

## 7. Transaction Model

ZION supports a **dual transaction model**:

### 7.1 Account Model
- Ed25519-signed transactions with `from`/`to`/`amount`/`fee`/`nonce`
- Memo field for arbitrary metadata (height-gated activation)
- Sender balance validation (F5 security fix, active from genesis in 3.0.4)
- Max TX amount cap: `TOTAL_SUPPLY` (144B ZION) — prevents inflation bugs

### 7.2 UTXO Model
- Bitcoin-style inputs/outputs
- Used for coinbase rewards and bridge vault operations
- Ed25519 signatures on inputs
- TX hash v2 (BLAKE3-based) from genesis

### 7.3 Fees

| Parameter | Value |
|-----------|-------|
| Min fee | 1 flower (0.000001 ZION) |
| Min fee rate | 1 flower/byte |
| Max TX size | 100,000 bytes |
| Burn address | `zion1burn0000000000000000000000000000000dead` |

Fees are burned (deflationary pressure).

---

## 8. P2P Network

### 8.1 Protocol

- **Transport**: QUIC (via Quinn)
- **Max connections**: 128
- **Min outbound**: 8
- **Max per subnet**: 4 (diversity enforcement)
- **Chain ID**: `zion-mainnet-1`

### 8.2 Security

| Mechanism | Value |
|-----------|-------|
| Rate limit | 100 msg / 60s per peer |
| Ban escalation | 5min → 30min → 2h → permanent |
| Max strikes | 3 (then permanent ban) |
| Peer reputation | Score-based (-100 = auto-ban) |
| Invalid block penalty | -50 |
| Invalid TX penalty | -10 |
| Valid block reward | +20 |
| Heartbeat | 60s |
| Idle timeout | 300s |

### 8.3 Fork Choice & Finality

- **Fork choice**: Greatest cumulative work (Nakamoto consensus)
- **Max reorg depth**: 10 blocks (constitutional limit)
- **Soft finality**: 60 blocks (~1 hour)
- **Orphan pool**: 200 blocks max, 600s expiry

---

## 9. Bridge & Cross-Chain

### 9.1 Architecture

The ZION bridge connects L1 to EVM chains using a **validator quorum** model:

- **Threshold**: 5/5 validators must sign unlock proofs
- **Chains**: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- **Token**: wZION (ERC-20, same address on all 6 chains via deterministic deploy)
- **Peg**: 1:1 (1 ZION L1 = 1 wZION EVM)

### 9.2 Flow

**Outbound (L1 → EVM):**
1. User sends ZION to `BRIDGE_VAULT_ADDRESS` with memo `BRIDGE:<chain>:<recipient>`
2. Bridge relay detects lock, validators sign proof
3. wZION minted on destination chain

**Inbound (EVM → L1):**
1. User burns wZION on EVM chain via ZIONBridge contract
2. Bridge relay detects burn, validators sign unlock proof
3. `submitBridgeUnlock` RPC called on L1, ZION released from vault

### 9.3 Atomic Swaps

HTLC-based atomic swaps enable trustless peer-to-peer ZION ↔ EVM token
exchanges. Escrow is funded on L1 with on-chain claim/refund logic.

---

## 10. DAO Governance

### 10.1 On-Chain Governance

- **ZIONGovernance** (Base): Token-weighted voting, 15% quorum, 14-day voting period
- **ZIONTreasury** (Base): 3-of-3 multisig for fund management
- **5 DAO Guardians**: Provisioned with separate mnemonics (air-gapped backup)

### 10.2 Premine Locks

All premine outputs are **admin-locked** — transfers require:
1. 3-of-3 admin multisig approval
2. DAO vote

DAO Treasury slots additionally require block height ≥ 144,000 (~100 days
after genesis).

---

## 11. Security

### 11.1 Disclosed Vulnerabilities (2026-07)

Five vulnerabilities were disclosed and remediated in the 3.0.4 hard reset.
See [`docs/security/SECURITY_DISCLOSURE_2026-07.md`](./security/SECURITY_DISCLOSURE_2026-07.md)
for full details.

| ID | Severity | Description | Status |
|----|----------|-------------|--------|
| F1 | Critical | Forged P2P account TX signatures | Fixed (signature verification on all non-coinbase account TX) |
| F5 | Critical | Unlimited inflation via insufficient balance check | Fixed (sender balance validation, active from genesis) |
| C1-C8 | High | Server exposure (ports, keys, services) | Fixed (all services on 127.0.0.1, UFW, AppArmor, key scrub) |
| — | High | TeamViewer compromise | Removed |
| — | Medium | EVM key compromise | Rotated |

### 11.2 Hardening Measures

- All services bind to `127.0.0.1` (no public RPC)
- UFW firewall (SSH/HTTP/HTTPS only)
- AppArmor profile for zion-node
- SSH key-only authentication
- File permissions 600 on all sensitive files
- Private keys scrubbed from source
- RPC audit logging
- 3 monitoring cron jobs
- Max TX amount cap (prevents inflation bugs)
- Coinbase maturity (100 blocks)
- Max reorg depth (10 blocks, constitutional)

---

## 12. Roadmap

| Version | Focus | Status |
|---------|-------|--------|
| 3.0.4 | Hard genesis reset, security hardening, DeFi deploy | ✅ Complete |
| 3.1.0 | Wallet SDK, mobile app, TX history, L4 Oasis | Planned |
| 3.2.0 | NPU mining hardware integration | Research |
| 4.0.0 | Proof-of-Care consensus (NPU-based caring computation) | Vision |

---

## 13. References

- Source code: [V3/](../V3/) directory in this repository
- Mainnet constants: [`V3/docs/MAINNET_CONSTANTS.md`](../V3/docs/MAINNET_CONSTANTS.md)
- Genesis documentation: [`genesis.md`](./genesis.md)
- Security disclosures: [`security/SECURITY_DISCLOSURE_2026-07.md`](./security/SECURITY_DISCLOSURE_2026-07.md)
- CLI reference: [`V3/docs/CLI_REFERENCE.md`](../V3/docs/CLI_REFERENCE.md)

---

## License

ZION v3 is released under the **MIT License**.

---

*— Yose / Zion Creator*
