# ZION v3 — Mainnet

<div align="center">

**Multichain Dharma Ecosystem**

A proof-of-work blockchain with dual-algo consensus, cross-chain bridge, DeFi layer, and DAO governance.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)

**English** · [Čeština](./docs/lang/README.cs.md) · [Español](./docs/lang/README.es.md) · [Français](./docs/lang/README.fr.md) · [Português](./docs/lang/README.pt.md)

</div>

---

## Overview

ZION is a multi-layer blockchain infrastructure built on proof-of-work consensus with a dual-algorithm design (Ekam Deeksha). The v3 mainnet features:

- **L1 Consensus** — Rust-based PoW node with Ed25519 signatures, BLAKE3 hashing, LWMA difficulty adjustment, UTXO + account transaction models, and P2P networking
- **L2 DeFi** — Smart contracts on Base Mainnet (Governance, Treasury, Staking, Farm) + cross-chain bridge relay + atomic swap + DAO governance
- **L2 Bridge** — ZION L1 ↔ EVM bridge with validator quorum (5/5 threshold), deployed on 6 EVM chains
- **RPC** — JSON-RPC 2.0 with 17+ node methods, Prometheus metrics, health checks

## Architecture

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Consensus │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)    │  │ Network  │  │  + Metrics    │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  UTXO +  │  │  Wallet  │  │   Mempool     │  │
│  │ Account  │  │  (Ed25519)│  │  (fee-prior)  │  │
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
│  │     Smart Contracts (Base Mainnet)        │   │
│  │  Governance · Treasury · Staking · Farm   │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Key Features

### L1 Consensus
- **Dual-algo PoW** — Ekam Deeksha consensus with GPU mining
- **Ed25519 signatures** — all transactions signed with Ed25519
- **BLAKE3 hashing** — fast, secure hashing for tx IDs and block Merkle roots
- **LWMA difficulty** — 60-block window, ±25% clamp, 30-120s solve time
- **UTXO + Account models** — dual transaction models with memo support
- **P2P networking** — Quinn/QUIC-based with rate limiting, ban system, orphan pool
- **LMDB storage** — persistent on-disk storage with atomic writes
- **Fork choice** — by total work, reorg planner (max depth 10), soft finality (60 blocks)

### L2 DeFi (Base Mainnet)
- **wZION** — ERC-20 wrapped ZION token (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — 5/5 validator threshold bridge (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Token-weighted voting, 15% quorum, 14d period
- **ZIONTreasury** — 3-of-3 multisig
- **ZIONStaking** — 12% APR, 7d cooldown
- **ZIONFarm** — 1 wZION/s, 90d halving
- **All 7 contracts verified on Basescan**

### Bridge
- 6 EVM chains: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Validator quorum: 5/5 threshold
- L1 RPC: `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

## Repository Structure

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consensus, validation, RPC, P2P, storage
│   │   ├── pool/           # Stratum mining pool
│   │   ├── miner/          # GPU miner runtime
│   │   └── cosmic-harmony/ # PoW algorithm (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Solidity contracts (Hardhat + Foundry)
│   │   ├── bridge/         # Bridge relay daemon
│   │   ├── dao/            # DAO governance daemon
│   │   └── atomic-swap/    # HTLC atomic swap daemon
│   ├── L4/                 # Oasis gaming layer
│   ├── L5/                 # Community layer
│   └── docs/               # Architecture documentation
├── docs/
│   ├── whitepaper.md       # Technical whitepaper
│   ├── ETHICS_PHILOSOPHY.md # Ethics & philosophy of 4 ZION books
│   ├── genesis.md          # Genesis block documentation
│   ├── LEGAL_DISCLAIMER.md # Legal disclaimer (no investment advice)
│   ├── TERMS_OF_USE.md     # Terms of use
│   ├── PRIVACY_POLICY.md   # Privacy policy
│   ├── JURISDICTION.md     # Jurisdiction & compliance
│   ├── TOKEN_DISCLOSURE.md # Token disclosure (no ICO, premine)
│   ├── security/           # Security disclosures
│   └── lang/               # Multilingual README translations
├── Cargo.toml              # Rust workspace root
├── SECURITY.md             # Vulnerability reporting
├── CONTRIBUTING.md         # Contribution guide
└── LICENSE                 # MIT
```

## Building

### Prerequisites

- **Rust** (stable toolchain): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (for Solidity): `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (for Hardhat scripts): `nvm install 18`

### Build L1 (Rust)

```bash
cargo build --release
```

### Build L2 (Solidity)

```bash
cd V3/L2/contracts
npm install
npx hardhat compile
# Or with Foundry:
forge build
```

## Testing

```bash
# L1 core
cargo test -p zion-core --release

# L2 bridge relay
cargo test -p zion-bridge --release

# L2 DAO
cargo test -p zion-dao --release

# L2 atomic swap
cargo test -p zion-atomic-swap --release

# All Rust tests
cargo test --workspace --release

# Solidity contracts
cd V3/L2/contracts && forge test
```

## Running a Node

### Configuration

All sensitive values are configured via environment variables:

```bash
# Required
export ZION_NODE_ID="my-node"
export ZION_MINER_ADDRESS="zion1..."

# Optional
export ZION_P2P_BIND="0.0.0.0:8333"
export ZION_RPC_BIND="127.0.0.1:8443"
export ZION_SEED_PEERS="peer1.example.com:8333"
```

**Never hardcode private keys in configuration files.** Use environment variables or encrypted keystores.

### Start

```bash
cargo run --release -p zion-core --bin zion-node
```

## Security

- **Reporting vulnerabilities:** See [SECURITY.md](./SECURITY.md)
- **Known vulnerabilities:** [docs/security/SECURITY_DISCLOSURE_2026-07.md](./docs/security/SECURITY_DISCLOSURE_2026-07.md)
- **All disclosed vulnerabilities (F1-F5, C1-C8) have been remediated**

## Canonical Constants

| Constant | Value |
|----------|-------|
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| `FLOWERS_PER_ZION` | 1,000,000 (6 decimals) |
| `BASE_REWARD` | 5,400,067,000 flowers (5,400.067 ZION) |
| `TAIL_REWARD` | 724,784,723 flowers (~724.785 ZION) |
| `MIN_TX_FEE` | 1 flower (0.000001 ZION) |
| Emission split | 89% miner / 5% humanitarian / 5% issobella / 1% burn |
| Block target | 60 seconds |
| Difficulty window | 60 blocks |
| Max reorg depth | 10 blocks |
| Soft finality | 60 blocks |

## Documentation

### Technical
- [Whitepaper](./docs/whitepaper.md) — Technical whitepaper (consensus, economics, architecture)
- [Ethics & Philosophy](./docs/ETHICS_PHILOSOPHY.md) — Four books of ZION: Genesis, Quantum Revolution, Ekam Deeksha, Terra Nova
- [evoluZion V2](./evoluZionV2.md) — PoW → Proof-of-Care evolution (10-year hybrid roadmap)
- [Genesis Block](./docs/genesis.md) — Genesis block, premine allocations, creator signature
- [Architecture](./V3/docs/) — L1/L2 architecture docs
- [Mainnet Constants](./V3/docs/MAINNET_CONSTANTS.md) — Canonical chain parameters
- [CLI Reference](./V3/docs/CLI_REFERENCE.md) — Full CLI command reference

### Legal
- [Legal Disclaimer](./docs/LEGAL_DISCLAIMER.md) — No investment advice, no warranty, risks
- [Terms of Use](./docs/TERMS_OF_USE.md) — Conditions for node operators, miners, users
- [Privacy Policy](./docs/PRIVACY_POLICY.md) — No personal data collected, pseudonymous network
- [Jurisdiction & Compliance](./docs/JURISDICTION.md) — Decentralized network, regulatory status
- [Token Disclosure](./docs/TOKEN_DISCLOSURE.md) — Transparent tokenomics, no ICO, premine details

### Security
- [Security Disclosures](./docs/security/) — Public vulnerability disclosures (F1-F5, C1-C8)
- [Security Policy](./SECURITY.md) — How to report vulnerabilities

### Community
- [Contributing](./CONTRIBUTING.md) — How to contribute
- [Code of Conduct](./CODE_OF_CONDUCT.md) — Community standards

## License

This project is licensed under the [MIT License](./LICENSE).

## Links

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Explorer:** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge:** [ZIONBridge on Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Multichain Dharma Ecosystem**

Built with care, secured by consensus.

</div>
