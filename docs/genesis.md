# ZION v3 — Genesis Block

> **Genesis hash**: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
> **Timestamp**: `1767225600` (2026-01-01 00:00:00 UTC)
> **Source**: [`V3/L1/core/src/genesis.rs`](../V3/L1/core/src/genesis.rs)

---

## Overview

The ZION v3 genesis block (height 0) is the foundational block of the
mainnet chain. It was regenerated during the **3.0.4 hard genesis reset**
(2026-07-06) following the disclosure and remediation of security
vulnerabilities F1 and F5.

The genesis block contains:
- **14 premine outputs** totaling 16,780,000,000 ZION (11.65% of 144B supply)
- **No mining subsidy** (subsidy = 0 at height 0; the premine is the sole coinbase)
- **13 account-model transactions** + **1 UTXO transaction** (bridge vault)
- An embedded **genesis message** with creator signature

### Block Header

| Field | Value |
|-------|-------|
| Height | 0 |
| Version | 3 |
| Previous hash | `0000000000000000000000000000000000000000000000000000000000000000` |
| Timestamp | `1767225600` (2026-01-01 00:00:00 UTC) |
| Algorithm | `deeksha_lite_v1` |
| Nonce | 0 |
| Template ID | 0 |
| Subsidy | 0 ZION |
| Miner reward | 0 ZION |

### Hash

```
4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e
```

This hash is **deterministic** — it is computed from the genesis block
construction and verified by the `genesis_hash_is_deterministic` test in
`genesis.rs`. All nodes must agree on this value. Any node producing a
different hash is on a different chain.

---

## Genesis Message

The genesis message is embedded in the first premine transaction's tag,
following the Bitcoin-style `scriptSig` heritage tradition.

### Short Form (embedded in TX hash)

```
ZION Mainet Launch v3 — For Sarah Issobel, Maitreya Buddha, Radha & Sita,
Meriam, Friends, Family, Freedom Humanity and all the children of this
world: ZION is yours. Build a better world where you reach for the Stars.
The Golden Age begins. Peace & One Love 4ever.
— Yose / Zion Creator
```

### Full Form (with ASCII art)

The full genesis message includes ASCII art of the Tree of Life and the
ZION logo. It is embedded at compile time via `include_str!("GENESIS_MESSAGE.txt")`.

See: [`V3/L1/core/src/GENESIS_MESSAGE.txt`](../V3/L1/core/src/GENESIS_MESSAGE.txt)

```
████████╗██╗ ██████╗███╗   ██╗
╚══███╔╝██║██╔═══██╗████╗  ██║
  ███╔╝ ██║██║   ██║██╔██╗ ██║
 ███╔╝  ██║██║   ██║██║╚██╗██║
███████╗██║╚██████╔╝██║ ╚████║
╚══════╝╚═╝ ╚═════╝ ╚═╝  ╚═══╝.  "Mainet Launch v3"

For Sarah Issobel, Maitreya Buddha, Radha & Sita, Meriam, Friends, Family,
Freedom Humanity and all the children of this world: ZION is yours.
Build a better world where you reach for the Stars. The Golden Age begins.
Peace & One Love 4ever.

— Yose / Zion Creator | Hooray to the Egg ! Om Namo Hiranyagarbha &
   Ekam Deeksha ! Thx Kalki/AmmaBhagavan !
```

---

## Premine Allocation

All 14 premine outputs are **admin-locked** (require 3-of-3 admin multisig
+ DAO vote to unlock). DAO Treasury outputs are additionally **time-locked**.

### OASIS + Golden Egg (5 slots × 1.65B = 8.25B ZION)

| # | Address | Amount (ZION) |
|---|---------|---------------|
| 1 | `zion1n3t6v6w3m8g4v6q8g7h7j4j6f7s8q2m7g7un8u0` | 1,650,000,000 |
| 2 | `zion16854w6h7a800k6h8n052s0h4k2v625x0w0z2320` | 1,650,000,000 |
| 3 | `zion1j8s2d6s6f248j7z3m80676p6m074x2q5p5er3w2` | 1,650,000,000 |
| 4 | `zion155k300w6x726p4x0w473s704d5k35865r2q75z8` | 1,650,000,000 |
| 5 | `zion1y293r8c6l5p3u0y7j8q8366372t7y070n3rp5r8` | 1,650,000,000 |

**Purpose**: OASIS platform rewards + Golden Egg/XP winner prizes.

### DAO Treasury (3 slots = 4.0B ZION) — time-locked until block 144,000

| # | Address | Amount (ZION) | Purpose |
|---|---------|---------------|---------|
| 6 | `zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0` | 2,500,000,000 | Community Governance (main) |
| 7 | `zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6` | 1,000,000,000 | Grants & Bounties |
| 8 | `zion102s8k4k0w783d657j255z865e47054s342u87v3` | 500,000,000 | Ecosystem Bootstrap |

**Time-lock**: Block 144,000 (~100 days at 60s/block).

### Infrastructure (3 slots = 2.59B ZION)

| # | Address | Amount (ZION) | Purpose |
|---|---------|---------------|---------|
| 9 | `zion1e8j5z6v8e4c6s5x7r0w7e2r673h8k3a6d4xx877` | 1,000,000,000 | Core Development Fund |
| 10 | `zion1f7z374q068r3p657m8z220v7y6k045q255xp2d3` | 1,000,000,000 | Network Infrastructure (P2P seed nodes) |
| 11 | `zion1s2j5s2a6f5k740k4d8s2k3y8v0t8d4k0u6my2k0` | 590,000,000 | Genesis Creator — Lifetime Rent |

### Humanitarian (1 slot = 1.44B ZION)

| # | Address | Amount (ZION) | Purpose |
|---|---------|---------------|---------|
| 12 | `zion10797m0k3u356f2l443r062d4e49665f6n20j6x0` | 1,440,000,000 | Children Future Fund — Humanitarian DAO |

### Bridge Seed (1 slot = 0.4B ZION)

| # | Address | Amount (ZION) | Purpose |
|---|---------|---------------|---------|
| 13 | `zion1p3y7w4z7d2m3j0f00657r354y4f3q5k6y8ca0g7` | 400,000,000 | EVM Bridge Liquidity |

### Bridge Vault UTXO (1 slot = 0.1B ZION)

| # | Address | Amount (ZION) | Purpose |
|---|---------|---------------|---------|
| 14 | `zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7` | 100,000,000 | Bridge Vault UTXO — EVM Bridge Unlock Liquidity |

This output is a **UTXO transaction** (not account-model) with 6 outputs
to fit the amount within `u64` limits. Address derived from
`BRIDGE_VAULT_SEED = "ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"`.

### Summary

| Category | Slots | Amount (ZION) | % of Premine |
|----------|-------|---------------|--------------|
| OASIS + Golden Egg | 5 | 8,250,000,000 | 49.2% |
| DAO Treasury | 3 | 4,000,000,000 | 23.8% |
| Infrastructure | 3 | 2,590,000,000 | 15.4% |
| Humanitarian | 1 | 1,440,000,000 | 8.6% |
| Bridge Seed | 1 | 400,000,000 | 2.4% |
| Bridge Vault UTXO | 1 | 100,000,000 | 0.6% |
| **Total** | **14** | **16,780,000,000** | **100%** |

---

## Lock Mechanism

### Two-Layer Lock

All premine outputs use a **two-layer lock**:

1. **Time-lock** (`unlock_height`): Block height that must be reached.
   - DAO Treasury: block 144,000 (~100 days)
   - All others: no time-lock (immediate once admin-unlocked)

2. **Admin-lock** (`admin_locked`): Requires 3-of-3 admin multisig + DAO vote.
   - All 14 outputs are admin-locked.
   - The `admin_unlocked` closure checks on-chain unlock state.

**Both locks must be satisfied.** An admin-locked address cannot transfer
even if the time-lock has expired, until the admin multisig + DAO vote
unlocks it.

See: `is_premine_transfer_allowed()` in `genesis.rs`.

---

## Canonical Subsidy Wallets

These are **not** premine outputs — they are the ongoing block subsidy
recipients (89/5/5/1 fee split). They receive coins from every mined block.

| Label | Address |
|-------|---------|
| Humanitarian Subsidy (5%) | `zion1e0u5q5s660k4m4a634p2c2v358r8g59564054z7` |
| Issobella Subsidy (5%) | `zion1f7y7l5k678y0v408e8s654d2282346k375526t2` |
| Pool Fee Subsidy (1%, burned) | `zion1062522x6a083x6r4d24303l5h20698z7j8qk433` |
| Default Miner (89%) | `zion1d6m0h2r8m7k8k2d8n072y7j3j4m0254323vq0e3` |
| Pool PPLNS Payout | `zion1e4489793c5x2r0a0a4d8z7r4u5d6k0s4k3ht5m2` |

> The Issobella, pool-fee, default-miner, and pool-payout addresses are
> derived deterministically from UTF-8 labels via
> `crypto::canonical_address_for_label` (BLAKE3 → StdRng → Ed25519).
> Keys are reconstructible from the repository — adequate for bootstrap /
> open custody. Operators needing exclusive control should generate fresh
> keys and override via environment variables.

---

## Genesis Integrity Verification

The genesis hash is verified by three deterministic tests:

```
test genesis::tests::genesis_hash_is_deterministic ... ok
test genesis::tests::genesis_body_hash_is_deterministic ... ok
test launch::tests::frozen_genesis_hash_is_deterministic ... ok
```

Run: `cargo test -p zion-core --lib genesis launch::tests::frozen`

Any node that computes a different genesis hash is on a fork and will be
rejected by the network.

---

## Creator Signature

The genesis block and this document are signed by the ZION creator (**Yose**)
using PGP/GPG. The signature attests to the authenticity of the genesis
block, premine allocations, and the genesis message.

### Verification

```bash
# Import the creator's public key
gpg --import CREATOR_PUBKEY.asc

# Verify the genesis message signature
gpg --verify GENESIS_MESSAGE.txt.sig GENESIS_MESSAGE.txt

# Verify this document
gpg --verify genesis.md.sig genesis.md
```

### Creator Public Key

```
[PGP public key block to be added by creator — Yose]
```

> **Note**: The creator's PGP/GPG public key and detached signatures will
> be added in a follow-up commit. The signature is generated on an
> air-gapped machine to ensure key security.

---

## 3.0.4 Hard Reset Context

The v3 genesis was **regenerated** on 2026-07-06 as part of the 3.0.4 hard
genesis reset. This was necessitated by:

1. **F1 exploit** — Forged P2P account transaction signatures allowed an
   attacker to create fake transactions. Remediated by enforcing signature
   verification on all non-coinbase account transactions.

2. **F5 exploit** — Insufficient sender balance validation allowed
   unlimited inflation. Remediated by enforcing `sender_balance >= amount + fee`
   on all account transactions.

3. **Server compromise** — TeamViewer access and exposed services required
   a full server rebuild with hardened configuration.

The hard reset regenerated all premine addresses, canonical wallet addresses,
and the genesis hash. The previous genesis hash (`d28dc404...`) is **invalid**
and belongs to the compromised chain.

See: [`docs/security/SECURITY_DISCLOSURE_2026-07.md`](./security/SECURITY_DISCLOSURE_2026-07.md)

---

*— Yose / Zion Creator*
