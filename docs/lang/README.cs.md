# ZION v3 — Mainnet Beta

<div align="center">

**Multichain Dharma Ecosystem**

Blockchain s proof-of-work konsenzem, cross-chain bridge, DeFi vrstvou a DAO governance.

[![License: MIT](https://img.shields.io/badge/Licence-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)
[![Status: Mainnet Beta](https://img.shields.io/badge/Status-Mainnet_Beta-orange.svg)](#status-sítě)

[English](../../README.md) · **Čeština** · [Español](./README.es.md) · [Français](./README.fr.md) · [Português](./README.pt.md)

</div>

---

## Status sítě

> **⚠️ MAINNET BETA — Těžba na vlastní nebezpečí**

ZION v3.0.4 je **živý a běží** jako Mainnet Beta. Síť je operační, bloky jsou produkovány a genesis řetězec je ustaven.

**Co to znamená:**
- ✅ Síť je živá a produkuje bloky
- ✅ Genesis blok a historie řetězce jsou **trvalé** — nebudou resetovány
- ✅ Všechny zveřejněné zranitelnosti (F1–F5, C1–C8) byly remediovány
- ✅ Všech 7 DeFi kontraktů verifikováno na Basescan
- ⚠️ Síť může stále obsahovat chyby — těžte a transakujte na vlastní nebezpečí
- ⚠️ Není poskytována žádná záruka — viz [Právní upozornění](../LEGAL_DISCLAIMER.md)

**Oficiální veřejný Launch: 31. prosince 2026**

Období Mainnet Beta trvá do oficiálního veřejného spuštění **31.12.2026** podle původního roadmapu. Během tohoto období:
- Síť prochází kontinuálním bezpečnostním ověřováním
- Pokud síť projde bezpečnostním ověřením, genesis blok a všechny vytěžené bloky **zůstanou trvale**
- Feedback komunity a hlášení chyb jsou vítány — viz [Contributing](../../CONTRIBUTING.md)
- Těžební odměny jsou reálné a nevrátitelné

| Parametr | Hodnota |
|-----------|---------|
| Status | **Mainnet Beta** |
| Protokol | 3.0.4 |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Oficiální launch | 2026-12-31 |
| Těžba | Aktivní (na vlastní nebezpečí) |

---

## Přehled

ZION je multi-vrstvá blockchain infrastruktura postavená na proof-of-work konsenzu s dual-algoritmem (Ekam Deeksha). V3 mainnet obsahuje:

- **L1 Konsenzus** — Rust-based PoW uzel s Ed25519 podpisy, BLAKE3 hashováním, LWMA obtížností, UTXO + account transakčními modely a P2P sítí
- **L2 DeFi** — Smart kontrakty na Base Mainnet (Governance, Treasury, Staking, Farm) + cross-chain bridge relay + atomic swap + DAO governance
- **L2 Bridge** — ZION L1 ↔ EVM bridge s validátorským kvórem (5/5 threshold), deploynutý na 6 EVM chainech
- **RPC** — JSON-RPC 2.0 s 17+ metodami, Prometheus metrikami, health checky

## Architektura

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Konsenzus │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)    │  │  Síť     │  │  + Metriky    │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  UTXO +  │  │  Wallet  │  │   Mempool     │  │
│  │ Account  │  │ (Ed25519)│  │ (fee-priorit.)│  │
│  └──────────┘  └──────────┘  └───────────────┘  │
└───────────────────────┬─────────────────────────┘
                        │ Bridge Relay
┌───────────────────────┴─────────────────────────┐
│                   L2 DeFi                        │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Bridge   │  │   DAO    │  │ Atomic Swap   │  │
│  │ (6 EVM)  │  │ (5 guard)│  │ (HTLC)        │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────────────────────────────────────┐   │
│  │     Smart Kontrakty (Base Mainnet)        │   │
│  │  Governance · Treasury · Staking · Farm   │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Klíčové vlastnosti

### L1 Konsenzus
- **Dual-algo PoW** — Ekam Deeksha konsenzus s GPU miningem
- **Ed25519 podpisy** — všechny transakce podepsané Ed25519
- **BLAKE3 hashování** — rychlé, bezpečné hashování pro TX ID a Merkle rooty
- **LWMA obtížnost** — 60-blokové okno, ±25% clamp, 30-120s solve time
- **UTXO + Account modely** — duální transakční modely s memo podporou
- **P2P síť** — Quinn/QUIC s rate limitingem, ban systémem, orphan poolem
- **LMDB úložiště** — perzistentní on-disk storage s atomickými zápisy
- **Fork choice** — podle total work, reorg planner (max depth 10), soft finality (60 bloků)

### L2 DeFi (Base Mainnet)
- **wZION** — ERC-20 wrapped ZION token (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — 5/5 validátorský threshold bridge (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Token-weighted hlasování, 15% kvórum, 14d perioda
- **ZIONTreasury** — 3-of-3 multisig
- **ZIONStaking** — 12% APR, 7d cooldown
- **ZIONFarm** — 1 wZION/s, 90d halving
- **Všech 7 kontraktů verifikováno na Basescan**

### Bridge
- 6 EVM chainů: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Validátorské kvórum: 5/5 threshold
- L1 RPC: `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

## Struktura repa

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Konsenzus, validace, RPC, P2P, storage
│   │   ├── pool/           # Stratum mining pool
│   │   ├── miner/          # GPU miner runtime
│   │   └── cosmic-harmony/ # PoW algoritmus (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Solidity kontrakty (Hardhat + Foundry)
│   │   ├── bridge/         # Bridge relay daemon
│   │   ├── dao/            # DAO governance daemon
│   │   └── atomic-swap/    # HTLC atomic swap daemon
│   ├── L4/                 # Oasis gaming vrstva
│   ├── L5/                 # Komunitní vrstva
│   └── docs/               # Architektonická dokumentace
├── docs/
│   ├── security/           # Security disclousures
│   └── lang/               # Překlady README
├── Cargo.toml              # Rust workspace root
├── SECURITY.md             # Nahlášení zranitelností
├── CONTRIBUTING.md         # Průvodce přispíváním
└── LICENSE                 # MIT
```

## Build

### Předpoklady

- **Rust** (stable toolchain): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (pro Solidity): `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (pro Hardhat skripty): `nvm install 18`

### Build L1 (Rust)

```bash
cargo build --release
```

### Build L2 (Solidity)

```bash
cd V3/L2/contracts
npm install
npx hardhat compile
# Nebo s Foundry:
forge build
```

## Testování

```bash
# L1 core
cargo test -p zion-core --release

# L2 bridge relay
cargo test -p zion-bridge --release

# L2 DAO
cargo test -p zion-dao --release

# L2 atomic swap
cargo test -p zion-atomic-swap --release

# Všechny Rust testy
cargo test --workspace --release

# Solidity kontrakty
cd V3/L2/contracts && forge test
```

## Spuštění uzlu

### Konfigurace

Všechny citlivé hodnoty se konfigurují přes environment variables:

```bash
# Povinné
export ZION_NODE_ID="my-node"
export ZION_MINER_ADDRESS="zion1..."

# Volitelné
export ZION_P2P_BIND="0.0.0.0:8333"
export ZION_RPC_BIND="127.0.0.1:8443"
export ZION_SEED_PEERS="peer1.example.com:8333"
```

**Nikdy nezapisujte private klíče do konfiguračních souborů.** Používejte environment variables nebo šifrované keystores.

### Start

```bash
cargo run --release -p zion-core --bin zion-node
```

## Bezpečnost

- **Nahlášení zranitelností:** Viz [SECURITY.md](../../SECURITY.md)
- **Známé zranitelnosti:** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../security/SECURITY_DISCLOSURE_2026-07.md)
- **Všechny disclosed zranitelnosti (F1-F5, C1-C8) byly remediovány**

## Kanonické konstanty

| Konstanta | Hodnota |
|-----------|---------|
| `FLOWERS_PER_ZION` | 1 000 000 (6 desetinných míst) |
| `BASE_REWARD` | 5 400 067 000 flowers (5 400,067 ZION) |
| `TAIL_REWARD` | 724 784 723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| Emission split | 89% miner / 5% humanitární / 5% issobella / 1% burn |
| Cílový čas bloku | 60 sekund |
| Okno obtížnosti | 60 bloků |
| Max reorg depth | 10 bloků |
| Soft finality | 60 bloků |

## Licence

Tento projekt je licencován pod [MIT License](../../LICENSE).

## Odkazy

- **Web:** [zionterranova.com](https://zionterranova.com)
- **Explorer:** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge:** [ZIONBridge na Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Multichain Dharma Ecosystem**

Postaveno s péčí, zabezpečeno konsenzem.

</div>
