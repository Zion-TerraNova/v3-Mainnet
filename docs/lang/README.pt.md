# ZION v3 — Mainnet

<div align="center">

**Ecossistema Dharma Multichain**

Blockchain de prova de trabalho com consenso de algoritmo dual, bridge cross-chain, camada DeFi e governança DAO.

[![Licença: MIT](https://img.shields.io/badge/Licença-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)

[English](../../README.md) · [Čeština](./README.cs.md) · [Español](./README.es.md) · [Français](./README.fr.md) · **Português**

</div>

---

## Visão Geral

ZION é uma infraestrutura blockchain multicamada construída sobre consenso de prova de trabalho com design de algoritmo dual (Ekam Deeksha). O mainnet v3 inclui:

- **L1 Consenso** — Nó PoW em Rust com assinaturas Ed25519, hash BLAKE3, ajuste de dificuldade LWMA, modelos de transação UTXO + conta e rede P2P
- **L2 DeFi** — Contratos inteligentes na Base Mainnet (Governance, Treasury, Staking, Farm) + relay de bridge cross-chain + atomic swap + governança DAO
- **L2 Bridge** — Bridge ZION L1 ↔ EVM com quórum de validadores (threshold 5/5), implantado em 6 cadeias EVM
- **RPC** — JSON-RPC 2.0 com mais de 17 métodos, métricas Prometheus, health checks

## Arquitetura

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Consenso  │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)   │  │  Rede    │  │  + Métricas   │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  UTXO +  │  │  Wallet  │  │   Mempool     │  │
│  │  Conta   │  │ (Ed25519)│  │ (prior. fee)  │  │
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
│  │     Contratos Inteligentes (Base)         │   │
│  │  Governance · Treasury · Staking · Farm   │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Funcionalidades Chave

### L1 Consenso
- **PoW algoritmo dual** — Consenso Ekam Deeksha com mineração GPU
- **Assinaturas Ed25519** — todas as transações assinadas com Ed25519
- **Hash BLAKE3** — hash rápido e seguro para TX IDs e raízes Merkle
- **Dificuldade LWMA** — janela de 60 blocos, clamp ±25%, solve time 30-120s
- **Modelos UTXO + Conta** — modelos de transação duais com suporte memo
- **Rede P2P** — Quinn/QUIC com limitação de taxa, sistema de banimento, orphan pool
- **Armazenamento LMDB** — armazenamento persistente em disco com escritas atômicas
- **Fork choice** — por trabalho total, planejador de reorg (max depth 10), soft finality (60 blocos)

### L2 DeFi (Base Mainnet)
- **wZION** — Token ERC-20 ZION envolvido (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — Bridge com threshold de validadores 5/5 (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Votação ponderada por tokens, quórum 15%, período 14d
- **ZIONTreasury** — Multisig 3-de-3
- **ZIONStaking** — 12% APR, cooldown 7d
- **ZIONFarm** — 1 wZION/s, halving 90d
- **Os 7 contratos verificados no Basescan**

### Bridge
- 6 cadeias EVM: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Quórum de validadores: threshold 5/5
- L1 RPC: `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

## Estrutura do Repositório

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consenso, validação, RPC, P2P, armazenamento
│   │   ├── pool/           # Pool de mineração Stratum
│   │   ├── miner/          # Runtime do minerador GPU
│   │   └── cosmic-harmony/ # Algoritmo PoW (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Contratos Solidity (Hardhat + Foundry)
│   │   ├── bridge/         # Daemon de relay do bridge
│   │   ├── dao/            # Daemon de governança DAO
│   │   └── atomic-swap/    # Daemon de atomic swap HTLC
│   ├── L4/                 # Camada de gaming Oasis
│   ├── L5/                 # Camada comunitária
│   └── docs/               # Documentação de arquitetura
├── docs/
│   ├── security/           # Divulgações de segurança
│   └── lang/               # Traduções do README
├── Cargo.toml              # Root do workspace Rust
├── SECURITY.md             # Relato de vulnerabilidades
├── CONTRIBUTING.md         # Guia de contribuição
└── LICENSE                 # MIT
```

## Compilação

### Pré-requisitos

- **Rust** (toolchain stable): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (para Solidity): `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (para scripts Hardhat): `nvm install 18`

### Compilar L1 (Rust)

```bash
cargo build --release
```

### Compilar L2 (Solidity)

```bash
cd V3/L2/contracts
npm install
npx hardhat compile
# Ou com Foundry:
forge build
```

## Testes

```bash
# L1 core
cargo test -p zion-core --release

# L2 bridge relay
cargo test -p zion-bridge --release

# L2 DAO
cargo test -p zion-dao --release

# L2 atomic swap
cargo test -p zion-atomic-swap --release

# Todos os testes Rust
cargo test --workspace --release

# Contratos Solidity
cd V3/L2/contracts && forge test
```

## Executar um Nó

### Configuração

Todos os valores sensíveis são configurados através de variáveis de ambiente:

```bash
# Obrigatório
export ZION_NODE_ID="my-node"
export ZION_MINER_ADDRESS="zion1..."

# Opcional
export ZION_P2P_BIND="0.0.0.0:8333"
export ZION_RPC_BIND="127.0.0.1:8443"
export ZION_SEED_PEERS="peer1.example.com:8333"
```

**Nunca armazene chaves privadas em arquivos de configuração.** Use variáveis de ambiente ou keystores criptografados.

### Início

```bash
cargo run --release -p zion-core --bin zion-node
```

## Segurança

- **Relatar vulnerabilidades:** Ver [SECURITY.md](../../SECURITY.md)
- **Vulnerabilidades conhecidas:** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../security/SECURITY_DISCLOSURE_2026-07.md)
- **Todas as vulnerabilidades divulgadas (F1-F5, C1-C8) foram remediadas**

## Constantes Canônicas

| Constante | Valor |
|-----------|-------|
| `FLOWERS_PER_ZION` | 1.000.000 (6 decimais) |
| `BASE_REWARD` | 5.400.067.000 flowers (5.400,067 ZION) |
| `TAIL_REWARD` | 724.784.723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| Divisão de emissão | 89% minerador / 5% humanitário / 5% issobella / 1% burn |
| Tempo alvo de bloco | 60 segundos |
| Janela de dificuldade | 60 blocos |
| Max reorg depth | 10 blocos |
| Soft finality | 60 blocos |

## Licença

Este projeto está licenciado sob a [Licença MIT](../../LICENSE).

## Links

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Explorer:** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge:** [ZIONBridge no Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Ecossistema Dharma Multichain**

Construído com cuidado, protegido por consenso.

</div>
