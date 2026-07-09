# ZION v3 — Mainnet

<div align="center">

**Ecosistema Dharma Multichain**

Blockchain de prueba de trabajo con consenso de algoritmo dual, bridge cross-chain, capa DeFi y gobernanza DAO.

[![Licencia: MIT](https://img.shields.io/badge/Licencia-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)

[English](../../README.md) · [Čeština](./README.cs.md) · **Español** · [Français](./README.fr.md) · [Português](./README.pt.md)

</div>

---

## Visión General

ZION es una infraestructura blockchain multicapa construida sobre consenso de prueba de trabajo con diseño de algoritmo dual (Ekam Deeksha). El mainnet v3 incluye:

- **L1 Consenso** — Nodo PoW en Rust con firmas Ed25519, hash BLAKE3, ajuste de dificultad LWMA, modelos de transacción UTXO + cuenta y red P2P
- **L2 DeFi** — Contratos inteligentes en Base Mainnet (Governance, Treasury, Staking, Farm) + relay de bridge cross-chain + atomic swap + gobernanza DAO
- **L2 Bridge** — Bridge ZION L1 ↔ EVM con quórum de validadores (threshold 5/5), desplegado en 6 cadenas EVM
- **RPC** — JSON-RPC 2.0 con más de 17 métodos, métricas Prometheus, health checks

## Arquitectura

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Consenso  │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)   │  │  Red     │  │  + Métricas   │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  UTXO +  │  │  Wallet  │  │   Mempool     │  │
│  │  Cuenta  │  │ (Ed25519)│  │ (prior. fee)  │  │
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

## Características Clave

### L1 Consenso
- **PoW de algoritmo dual** — Consenso Ekam Deeksha con minería GPU
- **Firmas Ed25519** — todas las transacciones firmadas con Ed25519
- **Hash BLAKE3** — hash rápido y seguro para TX IDs y raíces Merkle
- **Dificultad LWMA** — ventana de 60 bloques, clamp ±25%, solve time 30-120s
- **Modelos UTXO + Cuenta** — modelos de transacción duales con soporte memo
- **Red P2P** — Quinn/QUIC con limitación de tasa, sistema de baneo, orphan pool
- **Almacenamiento LMDB** — almacenamiento persistente en disco con escrituras atómicas
- **Fork choice** — por trabajo total, planificador de reorg (max depth 10), soft finality (60 bloques)

### L2 DeFi (Base Mainnet)
- **wZION** — Token ERC-20 ZION envuelto (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — Bridge con threshold de validadores 5/5 (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Votación ponderada por tokens, quórum 15%, periodo 14d
- **ZIONTreasury** — Multisig 3-de-3
- **ZIONStaking** — 12% APR, cooldown 7d
- **ZIONFarm** — 1 wZION/s, halving 90d
- **Los 7 contratos verificados en Basescan**

### Bridge
- 6 cadenas EVM: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Quórum de validadores: threshold 5/5
- L1 RPC: `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

## Estructura del Repositorio

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consenso, validación, RPC, P2P, almacenamiento
│   │   ├── pool/           # Pool de minería Stratum
│   │   ├── miner/          # Runtime del minero GPU
│   │   └── cosmic-harmony/ # Algoritmo PoW (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Contratos Solidity (Hardhat + Foundry)
│   │   ├── bridge/         # Daemon de relay del bridge
│   │   ├── dao/            # Daemon de gobernanza DAO
│   │   └── atomic-swap/    # Daemon de atomic swap HTLC
│   ├── L4/                 # Capa de gaming Oasis
│   ├── L5/                 # Capa comunitaria
│   └── docs/               # Documentación de arquitectura
├── docs/
│   ├── security/           # Divulgaciones de seguridad
│   └── lang/               # Traducciones de README
├── Cargo.toml              # Root del workspace de Rust
├── SECURITY.md             # Reporte de vulnerabilidades
├── CONTRIBUTING.md         # Guía de contribución
└── LICENSE                 # MIT
```

## Compilación

### Requisitos

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
# O con Foundry:
forge build
```

## Pruebas

```bash
# L1 core
cargo test -p zion-core --release

# L2 bridge relay
cargo test -p zion-bridge --release

# L2 DAO
cargo test -p zion-dao --release

# L2 atomic swap
cargo test -p zion-atomic-swap --release

# Todas las pruebas de Rust
cargo test --workspace --release

# Contratos Solidity
cd V3/L2/contracts && forge test
```

## Ejecutar un Nodo

### Configuración

Todos los valores sensibles se configuran mediante variables de entorno:

```bash
# Requerido
export ZION_NODE_ID="my-node"
export ZION_MINER_ADDRESS="zion1..."

# Opcional
export ZION_P2P_BIND="0.0.0.0:8333"
export ZION_RPC_BIND="127.0.0.1:8443"
export ZION_SEED_PEERS="peer1.example.com:8333"
```

**Nunca guarde claves privadas en archivos de configuración.** Use variables de entorno o keystores encriptados.

### Inicio

```bash
cargo run --release -p zion-core --bin zion-node
```

## Seguridad

- **Reportar vulnerabilidades:** Ver [SECURITY.md](../../SECURITY.md)
- **Vulnerabilidades conocidas:** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../security/SECURITY_DISCLOSURE_2026-07.md)
- **Todas las vulnerabilidades divulgadas (F1-F5, C1-C8) han sido remediadas**

## Constantes Canónicas

| Constante | Valor |
|-----------|-------|
| `FLOWERS_PER_ZION` | 1.000.000 (6 decimales) |
| `BASE_REWARD` | 5.400.067.000 flowers (5.400,067 ZION) |
| `TAIL_REWARD` | 724.784.723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| División de emisión | 89% minero / 5% humanitario / 5% issobella / 1% burn |
| Tiempo objetivo de bloque | 60 segundos |
| Ventana de dificultad | 60 bloques |
| Max reorg depth | 10 bloques |
| Soft finality | 60 bloques |

## Licencia

Este proyecto está licenciado bajo la [Licencia MIT](../../LICENSE).

## Enlaces

- **Sitio web:** [zionterranova.com](https://zionterranova.com)
- **Explorer:** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge:** [ZIONBridge en Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Ecosistema Dharma Multichain**

Construido con cuidado, asegurado por consenso.

</div>
