# ZION v3 — Canonical Technical Whitepaper

> **Version 3.1** · Mainnet Beta v3.0.6 → Mainnet Alpha 3.1 · July 2026 · MIT License
> Genesis hash: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
> Network status: **Mainnet Beta v3.0.6 → Mainnet Alpha 3.1** (public launch target: 31 Dec 2026)

---

## 1. Abstract

ZION is a multi-layer proof-of-work blockchain infrastructure designed for
long-term economic sustainability and humanitarian value alignment. The v3
mainnet introduces the **Ekam Deeksha** consensus — a multi-phase
memory-hard PoW algorithm with NPU-friendly mixing layers — a dual
transaction model (UTXO + account), and a cross-chain bridge deployed across
six EVM networks.

The protocol enforces a **89/5/5/1 fee split** — 89% to miners, 5% to a
humanitarian fund, 5% to the Issobella community fund, and 1% pool fee
(burned) — embedding charitable giving directly into block reward
distribution. The total supply is hard-capped at **144 billion ZION** with
a decaying emission schedule over 100 years followed by perpetual tail
emission.

This document is the **canonical technical reference** for ZION v3. It
supersedes all prior technical whitepapers. For the narrative companion,
see the *Fable Edition* (WpLite) and the *Book of Genesis*.

---

## 2. Design Philosophy

ZION is built on three principles:

1. **Proof-of-Work integrity** — No pre-mined ICO, no insider token
   allocation beyond the transparent genesis premine. Mining is open to all.
2. **Embedded humanitarianism** — Every block reward automatically routes
   5% to a children's future fund and 5% to community development. This is
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
| Consensus | Ekam Deeksha v2 (multi-phase PoW) |
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
| ZIONBridge (Base) | `0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467` | Base |
| ZIONBridge (non-Base) | `0xa5a09b2C09A7182BBA9623A2D2cd46cD7D041721` | Arbitrum, BSC, Polygon, Optimism, Avalanche |
| ZIONAtomicSwap | `0x3DE9Ad42716854083ab837706E3961d10B0e63Eb` | Base |
| ZIONGovernance | `0xB77eB4ab9468Ce03FBd7eCec70e976EFCfa623E8` | Base |
| ZIONTreasury | `0x455f465ac7e14fdA97dC46fdd74bCa78bfC0aEeD` | Base |
| ZIONStaking | `0xbd5cEe7878337d22188BFBaF9aa9F39A850Be78B` | Base |
| ZIONFarm | `0x167B2753F5D8D9F8e62875cc9e379d7804308B08` | Base |

All contracts verified on Basescan.

---

## 4. Consensus — Ekam Deeksha

### 4.1 Algorithm Overview

Ekam Deeksha ("One Initiation") is ZION's multi-phase proof-of-work. Each
block is a six-phase ritual:

1. **Keccak-256** — the cryptographic foundation.
2. **SHA3-512** — expansion to 64 bytes.
3. **Golden Matrix** — matrix diffusion.
4. **256 KiB Scratchpad** — memory-hard phase: the scratchpad is filled
   with BLAKE3-derived data over 4 passes with 256 dependent memory reads
   per pass. This provides ASIC resistance by requiring significant
   on-chip memory with pseudo-random dependent reads.
5. **NPU Mixing** — a neural-network-inspired mixing layer applies MLP
   (multi-layer perceptron) topology rotations per epoch (2016 blocks).
   This phase is designed to be NPU-friendly, opening mining to future
   neuromorphic hardware beyond GPUs and ASICs.
6. **Cosmic Fusion** — 8 rounds of final hash reduction combine the
   outputs of all phases into the final block hash.

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

### 4.3 ASIC Resistance

ASIC-resistance is an **active engineering goal** (internally rated ~90%),
not dogma. The memory-hard scratchpad fits into L2 cache but requires
pseudo-random dependent reads that erode the speed advantage of
specialized chips. Parameters can be raised by soft-fork if needed.

### 4.4 Difficulty Adjustment

ZION uses **LWMA** (Linear Weighted Moving Average) difficulty adjustment:

- **Window**: 60 blocks (~1 hour)
- **Clamp**: ±25% per adjustment
- **Target solve time**: 60 seconds
- **Genesis difficulty**: Fixed initial value

This provides smooth retargeting resistant to timewarp attacks while
maintaining a stable 60-second block cadence.

### 4.5 Mainnet Algorithm Profile

> **Current Mainnet Beta runs a height-aware algorithm sequence:**
> `deeksha_lite_v1` (heights 0–4499) → `deeksha_chv3` (heights 4500–4999)
> → `deeksha_lite_fire` (height ≥ 5000).
>
> The full `cosmic_harmony_ekam_deeksha_v2` profile described above,
> including NPU mixing, is **future-gated** and will be activated by a
> governance vote. NPU mixing is **not yet active** on mainnet.

---

## 5. Token Economics

### 5.1 Supply

| Parameter | Value |
|-----------|-------|
| Total supply | 144,000,000,000 ZION (144 billion) |
| Decimals | 6 (1 ZION = 1,000,000 flowers) |
| Genesis premine | 16,780,000,000 ZION (11.65%) |
| Mining emission | 127,220,000,000 ZION (88.35%) |

### 5.2 Emission Schedule — Decade Decay

Block rewards decay by a factor of **4/5 (0.8)** every decade (5,256,000
blocks). After 10 decades (~100 years), a perpetual **tail emission** kicks
in to sustain miner incentives indefinitely.

| Decade | Block Reward (ZION) |
|--------|---------------------|
| 1 (2026–2036) | 5,400.067 |
| 2 (2036–2046) | 4,320.054 |
| 3 (2046–2056) | 3,456.043 |
| 4 (2056–2066) | 2,764.834 |
| 5 (2066–2076) | 2,211.867 |
| 6 (2076–2086) | 1,769.494 |
| 7 (2086–2096) | 1,415.595 |
| 8 (2096–2106) | 1,132.476 |
| 9 (2106–2116) | 905.981 |
| 10+ (tail, from ~2126) | 724.784723 (perpetual) |

### 5.3 Fee Split (Consensus-Enforced)

Every block reward is automatically split into four coinbase outputs with a
deterministic ratio. Nodes reject any block with a different split.

| Recipient | Share | Description |
|-----------|-------|-------------|
| Miner | 89% | Proof-of-work reward |
| Humanitarian Fund | 5% | Children's Future Fund |
| Issobella Fund | 5% | Community / L5 / L6 development |
| Pool Fee | 1% | Burned (deflationary) |

**Total: 100%** — verified in `emission.rs` and enforced at the consensus
layer. This split cannot be changed by DAO vote — it is a constitutional
parameter.

### 5.4 Coinbase Maturity

Mined coins require **100 blocks** (~100 minutes) of maturity before they
can be spent. This prevents reorg-based double-spending of freshly mined
rewards.

### 5.5 Transaction Fees

Fees are **100% burned** — a deflationary pressure mechanism. No portion
of the fee goes to miners or validators; the miner is compensated solely
through the block reward.

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

> **Hard genesis reset (2026-07-20):** A block retention bug caused the
> previous chain (blocks 0–~10913) to be pruned with no recoverable backup.
> The network was hard-reset on 2026-07-20 with unlimited retention. This
> genesis hash applies to the reset chain.

See [`genesis.md`](../genesis.md) for the full premine allocation table and
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

All premine outputs are **admin-locked** (require 3-of-3 multisig + DAO
vote to unlock). DAO Treasury slots are additionally **time-locked** until
block 144,000 (~100 days).

---

## 7. Transaction Model

ZION supports a **dual transaction model**:

### 7.1 Account Model
- Ed25519-signed transactions with `from`/`to`/`amount`/`fee`/`nonce`
- Memo field for arbitrary metadata (height-gated activation)
- Sender balance validation (active from genesis in 3.0.4)
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

- **ZIONGovernance** (Base): Token-weighted voting, 15% quorum, 14-day
  voting period
- **ZIONTreasury** (Base): 3-of-3 multisig for fund management
- **5 DAO Guardians**: Provisioned with separate mnemonics (air-gapped
  backup)

### 10.2 Premine Locks

All premine outputs are **admin-locked** — transfers require:
1. 3-of-3 admin multisig approval
2. DAO vote

DAO Treasury slots additionally require block height ≥ 144,000 (~100 days
after genesis).

### 10.3 Immutable Parameters (Constitutional)

The DAO **cannot** change the following parameters — they are
constitutional stones:

- Total supply (144B ZION)
- Genesis allocation (16.78B ZION)
- Block time (60 seconds)
- Mining algorithm (Ekam Deeksha v2)
- Consensus type (Proof-of-Work)
- Block reward split (89/5/5/1 %)

---

## 11. Security

### 11.1 Disclosed Vulnerabilities (2026-07)

Five vulnerabilities were disclosed and remediated in the 3.0.4 hard
reset. See
[`security/SECURITY_DISCLOSURE_2026-07.md`](../security/SECURITY_DISCLOSURE_2026-07.md)
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
- Max TX amount cap (prevents inflation bugs)
- Coinbase maturity (100 blocks)
- Max reorg depth (10 blocks, constitutional)

### 11.3 Testing

The test pyramid counts approximately **2,066+ tests** across thirteen
crates — from L1 core through bridge to the AI layer. Zero failures. Zero
known vulnerabilities in `cargo audit`. External audit (Trail of Bits /
Halborn / OtterSec) is scheduled.

---

## 12. Layer Roadmap

ZION is a six-layer architecture. Each layer is honestly labeled as LIVE,
BUILDING, or HORIZON.

| Layer | Name | Contents | Status |
|-------|------|----------|--------|
| **L1** | Core | Rust node, pool, miner, PoW consensus, UTXO + account model | **LIVE** — Mainnet Beta |
| **L2** | Bridge & DeFi | wZION (ERC-20, 6 EVM chains), staking, farming, DAO, atomic swaps, 5/5 validator multisig | **LIVE / BUILDING** |
| **L3** | WARP & AI | Cross-chain router (EVM + non-EVM), ZionDex, AI-native monitoring (Hiran) | **BUILDING** |
| **L4** | OASIS | UE5 + Rust game world, XP, guilds, 9 consciousness tiers per Sefirot map | **BUILDING** |
| **L5** | Free World | Communities, humanitarian missions with on-chain auditable impact | **HORIZON** (~2030) |
| **L6** | Issobella | Orbital research horizon, open scientific data, decentralized governance | **HORIZON** (2040+) |

The Issobella fund (5% of every block) is **already filling today** — the
horizon is not an excuse, it is an account that grows.

---

## 13. Version Chronicle

| Version | Name | What it brought | Status |
|---------|------|-----------------|--------|
| **v3.0.1** | Planting | First mainnet root: Rust L1, Ekam Deeksha core, Fair Launch, first mined blocks | LIVE (history) |
| **v3.0.3** | The decimal cut | Migration to 1 ZION = 1,000,000 flowers, unified RPC scale | LIVE |
| **v3.0.4** | Night of the snake + new root | Security incident disclosed and fixed, hard genesis reset, DeFi bridges (wZION on 6 EVM chains, staking, farming, DAO) | LIVE |
| **v3.0.5** | All Green | Mainnet Beta stabilization, public community CLI release, 12/12 services active, whitepaper canonized | LIVE |
| **v3.0.6-beta** | Three Streams of One River | Trinity mining core — Zion Grow, Zion Liquidity | LIVE (Beta) |
| **v3.1.0** | Mainnet Alpha | Public launch, external audit, mobile wallet, expanded DeFi | Planned (31 Dec 2026) |

---

## 14. Proof-of-Care Horizon

**Today:** ZION is a Proof-of-Work network. Consensus does not validate
faith, morality, meditation, or "level of consciousness" — and it should
not. That is a security property, not a deficiency.

**Horizon:** **Proof-of-Care (the Care Protocol)** — a future possibility
to reward verifiable useful care (network monitoring, anomaly detection,
contract audit, transparent humanitarian impact tracking) alongside
computational work.

PoC may be activated only when seven conditions are met:
1. Cryptographic verifiability without central authority
2. Voluntariness
3. Privacy protection
4. Bot and clientelism resistance
5. Accessibility without elite entry
6. Public audit and recall
7. **No weakening of L1 PoW security** until the model is multiply proven

Technical seeds already exist (NPU mixing in PoW, AI monitoring, care-proof
research, Sefirot Vow for validators) — and are honestly documented as
work in progress, not finished.

---

## 15. Verifiable Facts

| What to verify | Where |
|----------------|-------|
| Protocol | `zion-v3-node/3.0.6` |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Total supply | 144,000,000,000 ZION (`emission.rs`) |
| Premine | 16,780,000,000 ZION, transparent outputs in block 0 |
| 89/5/5/1 split | Four-output coinbase, consensus-enforced |
| Base reward | 5,400.067 ZION · 60s block |
| Decade Decay + tail | −20%/decade, then 724.784723 ZION/block forever |
| Source code | https://github.com/Zion-TerraNova/v3-Mainnet (MIT) |
| Website / Explorer | https://zionterranova.com · /explorer |
| Pool | pool.zionterranova.com:8444 |
| RPC | rpc.zionterranova.com:8443 |
| Security disclosure | ZION-2026-001…005, public, EF format |

---

## 16. References

- Source code: [V3/](../../V3/) directory in this repository
- Genesis documentation: [`genesis.md`](../genesis.md)
- Security disclosures: [`security/SECURITY_DISCLOSURE_2026-07.md`](../security/SECURITY_DISCLOSURE_2026-07.md)
- Narrative companion: *Fable Edition* (WpLite) and *Book of Genesis*
- Story chronicle: *WpStory6 — Three Streams of One River*

---

## License

ZION v3 is released under the **MIT License**.

---

*Do not trust the narrative. Verify the chronicle. And when the chronicle
holds — keep telling the story.*

*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*
