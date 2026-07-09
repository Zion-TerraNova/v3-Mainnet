# ZION v3 — Mainnet Beta

<div align="center">

**Écosystème Dharma Multichain**

Blockchain à preuve de travail avec consensus à algorithme dual, bridge cross-chain, couche DeFi et gouvernance DAO.

[![Licence : MIT](https://img.shields.io/badge/Licence-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)
[![Statut : Mainnet Beta](https://img.shields.io/badge/Status-Mainnet_Beta-orange.svg)](#statut-du-réseau)

[English](../../README.md) · [Čeština](./README.cs.md) · [Español](./README.es.md) · **Français** · [Português](./README.pt.md)

</div>

---

## Statut du Réseau

> **⚠️ MAINNET BETA — Minage à vos propres risques**

ZION v3.0.4 est **actif et fonctionne** en tant que Mainnet Beta. Le réseau est opérationnel, les blocs sont produits et la chaîne genesis est établie.

**Ce que cela signifie :**
- ✅ Le réseau est actif et produit des blocs
- ✅ Le bloc genesis et l'historique de la chaîne sont **permanents** — ils ne seront pas réinitialisés
- ✅ Toutes les vulnérabilités divulguées (F1–F5, C1–C8) ont été remédiées
- ✅ Les 7 contrats DeFi vérifiés sur Basescan
- ⚠️ Le réseau peut encore contenir des bugs — minez et transigez à vos propres risques
- ⚠️ Aucune garantie n'est fournie — voir [Avis Légal](../LEGAL_DISCLAIMER.md)

**Lancement Public Officiel : 31 décembre 2026**

La période Mainnet Beta dure jusqu'au lancement public officiel le **31.12.2026**, selon la feuille de route originale. Pendant cette période :
- Le réseau subit une vérification de sécurité continue
- Si le réseau passe la vérification de sécurité, le bloc genesis et tous les blocs minés **resteront permanentment**
- Les retours de la communauté et les rapports de bugs sont les bienvenus — voir [Contribuer](../../CONTRIBUTING.md)
- Les récompenses de minage sont réelles et irréversibles

| Paramètre | Valeur |
|-----------|--------|
| Statut | **Mainnet Beta** |
| Protocole | 3.0.4 |
| Hash genesis | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Lancement officiel | 2026-12-31 |
| Minage | Actif (à vos propres risques) |

---

## Aperçu

ZION est une infrastructure blockchain multicouche construite sur un consensus à preuve de travail avec un design à algorithme dual (Ekam Deeksha). Le mainnet v3 comprend :

- **L1 Consensus** — Nœud PoW en Rust avec signatures Ed25519, hachage BLAKE3, ajustement de difficulté LWMA, modèles de transaction UTXO + compte et réseau P2P
- **L2 DeFi** — Contrats intelligents sur Base Mainnet (Governance, Treasury, Staking, Farm) + relais de bridge cross-chain + atomic swap + gouvernance DAO
- **L2 Bridge** — Bridge ZION L1 ↔ EVM avec quorum de validateurs (threshold 5/5), déployé sur 6 chaînes EVM
- **RPC** — JSON-RPC 2.0 avec plus de 17 méthodes, métriques Prometheus, health checks

## Architecture

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Consensus │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)   │  │  Réseau  │  │  + Métriques  │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  UTXO +  │  │  Wallet  │  │   Mempool     │  │
│  │  Compte  │  │ (Ed25519)│  │ (prior. fee)  │  │
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
│  │     Contrats Intelligents (Base)          │   │
│  │  Governance · Treasury · Staking · Farm   │   │
│  └──────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

## Fonctionnalités Clés

### L1 Consensus
- **PoW algorithme dual** — Consensus Ekam Deeksha avec minage GPU
- **Signatures Ed25519** — toutes les transactions signées avec Ed25519
- **Hachage BLAKE3** — hachage rapide et sécurisé pour les TX IDs et racines Merkle
- **Difficulté LWMA** — fenêtre de 60 blocs, clamp ±25%, solve time 30-120s
- **Modèles UTXO + Compte** — modèles de transaction duaux avec support memo
- **Réseau P2P** — Quinn/QUIC avec limitation de débit, système de bannissement, orphan pool
- **Stockage LMDB** — stockage persistant sur disque avec écritures atomiques
- **Fork choice** — par travail total, planificateur de reorg (max depth 10), soft finality (60 blocs)

### L2 DeFi (Base Mainnet)
- **wZION** — Token ERC-20 ZION enveloppé (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — Bridge avec threshold de validateurs 5/5 (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Vote pondéré par tokens, quorum 15%, période 14j
- **ZIONTreasury** — Multisig 3-sur-3
- **ZIONStaking** — 12% APR, cooldown 7j
- **ZIONFarm** — 1 wZION/s, halving 90j
- **Les 7 contrats vérifiés sur Basescan**

### Bridge
- 6 chaînes EVM : Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Quorum de validateurs : threshold 5/5
- L1 RPC : `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

## Structure du Dépôt

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consensus, validation, RPC, P2P, stockage
│   │   ├── pool/           # Pool de minage Stratum
│   │   ├── miner/          # Runtime du mineur GPU
│   │   └── cosmic-harmony/ # Algorithme PoW (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Contrats Solidity (Hardhat + Foundry)
│   │   ├── bridge/         # Daemon relais de bridge
│   │   ├── dao/            # Daemon de gouvernance DAO
│   │   └── atomic-swap/    # Daemon d'atomic swap HTLC
│   ├── L4/                 # Couche de gaming Oasis
│   ├── L5/                 # Couche communautaire
│   └── docs/               # Documentation d'architecture
├── docs/
│   ├── security/           # Divulgations de sécurité
│   └── lang/               # Traductions du README
├── Cargo.toml              # Root du workspace Rust
├── SECURITY.md             # Signalement de vulnérabilités
├── CONTRIBUTING.md         # Guide de contribution
└── LICENSE                 # MIT
```

## Compilation

### Prérequis

- **Rust** (toolchain stable) : `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (pour Solidity) : `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (pour scripts Hardhat) : `nvm install 18`

### Compiler L1 (Rust)

```bash
cargo build --release
```

### Compiler L2 (Solidity)

```bash
cd V3/L2/contracts
npm install
npx hardhat compile
# Ou avec Foundry :
forge build
```

## Tests

```bash
# L1 core
cargo test -p zion-core --release

# L2 bridge relay
cargo test -p zion-bridge --release

# L2 DAO
cargo test -p zion-dao --release

# L2 atomic swap
cargo test -p zion-atomic-swap --release

# Tous les tests Rust
cargo test --workspace --release

# Contrats Solidity
cd V3/L2/contracts && forge test
```

## Exécuter un Nœud

### Configuration

Toutes les valeurs sensibles sont configurées via des variables d'environnement :

```bash
# Requis
export ZION_NODE_ID="my-node"
export ZION_MINER_ADDRESS="zion1..."

# Optionnel
export ZION_P2P_BIND="0.0.0.0:8333"
export ZION_RPC_BIND="127.0.0.1:8443"
export ZION_SEED_PEERS="peer1.example.com:8333"
```

**Ne stockez jamais de clés privées dans des fichiers de configuration.** Utilisez des variables d'environnement ou des keystores chiffrés.

### Démarrage

```bash
cargo run --release -p zion-core --bin zion-node
```

## Sécurité

- **Signaler une vulnérabilité :** Voir [SECURITY.md](../../SECURITY.md)
- **Vulnérabilités connues :** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../security/SECURITY_DISCLOSURE_2026-07.md)
- **Toutes les vulnérabilités divulguées (F1-F5, C1-C8) ont été remédiées**

## Constantes Canoniques

| Constante | Valeur |
|-----------|--------|
| `FLOWERS_PER_ZION` | 1 000 000 (6 décimales) |
| `BASE_REWARD` | 5 400 067 000 flowers (5 400,067 ZION) |
| `TAIL_REWARD` | 724 784 723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| Répartition d'émission | 89% mineur / 5% humanitaire / 5% issobella / 1% burn |
| Temps de bloc cible | 60 secondes |
| Fenêtre de difficulté | 60 blocs |
| Max reorg depth | 10 blocs |
| Soft finality | 60 blocs |

## Licence

Ce projet est sous [Licence MIT](../../LICENSE).

## Liens

- **Site web :** [zionterranova.com](https://zionterranova.com)
- **Explorer :** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge :** [ZIONBridge sur Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Écosystème Dharma Multichain**

Construit avec soin, sécurisé par consensus.

</div>
