# ZION v3 — Mainnet Beta

<div align="center">

<!-- ════ STARGATE — Portail Cosmique ════ -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/stargate/nebula.jpg">
  <img src="../../docs/stargate/nebula.jpg" width="320" height="320" alt="ZION Stargate — portail cosmique" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 40px rgba(0,180,255,0.3);" />
</picture>

<br/>

**Multichain Dharma Ecosystem**

Une blockchain avec consensus proof-of-work, algorithme dual, pont cross-chain, couche DeFi et gouvernance DAO.

[![Licence : MIT](https://img.shields.io/badge/Licence-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)
[![Statut : Mainnet Beta](https://img.shields.io/badge/Status-Mainnet_Beta-orange.svg)](#statut-du-réseau)

[English](../../README_FULL.md) · [Čeština](./README_FULL.cs.md) · [Español](./README_FULL.es.md) · **Français** · [Português](./README_FULL.pt.md)

</div>

<details>
<summary><b>Entrez dans le Stargate</b> — Portail interactif</summary>

<div align="center">

<img src="../../docs/stargate/2.png" width="280" alt="Couche Stargate" style="border-radius: 50%; opacity: 0.3; position: relative; z-index: 1;" />
<img src="../../docs/stargate/1.png" width="280" alt="Couche Stargate" style="border-radius: 50%; opacity: 0.15; margin-top: -280px; position: relative; z-index: 2;" />
<img src="../../docs/stargate/Z.gif" width="64" alt="ZION" style="border-radius: 50%; filter: grayscale(100%) contrast(180%); opacity: 0.7; margin-top: -170px; position: relative; z-index: 3;" />

<br/><br/>

> **Le Stargate** est le portail cosmique de ZION — une porte holographique avec 28 couches rotatives (mandala + Sri Yantra), 39 glyphes (système d'adressage Stargate SG-1) et 9 chevrons représentant les 9 niveaux de conscience du monde de jeu Oasis.
>
> Le portail symbolise le pont entre la blockchain physique (L1–L3) et le métavers de jeu Oasis (L4). Sur le site web en direct ([zionterranova.com](https://zionterranova.com)), le Stargate est entièrement animé avec des rotations CSS et des effets interactifs au survol.

<br/>

| Élément Stargate | Symbolisme |
|------------------|------------|
| 28 couches rotatives | Mandala + géométrie sacrée Sri Yantra |
| 39 glyphes (A–Z, a–m) | Système d'adressage Stargate SG-1 |
| 9 chevrons (lueur cyan) | 9 niveaux de conscience (Cabale Sefirot) |
| Logo Z central | ZION — la graine de conscience |
| Fond de nébuleuse | Images deep-space du Hubble |

</div>

</details>

---

## Statut du réseau

> **⚠️ MAINNET BETA — Minage à vos risques et périls**

ZION v3.0.4 est **en ligne et opérationnel** en tant que Mainnet Beta. Le réseau est opérationnel, les blocs sont produits et la chaîne de genèse est établie.

**Ce que cela signifie :**
- ✅ Le réseau est en ligne et produit des blocs
- ✅ Le bloc de genèse et l'historique de la chaîne sont **permanents** — ils ne seront pas réinitialisés
- ✅ Toutes les vulnérabilités divulguées (F1–F5, C1–C8) ont été remédiées
- ✅ Les 7 contrats DeFi ont été vérifiés sur Basescan
- ⚠️ Le réseau peut encore contenir des bogues — minez et effectuez des transactions à vos risques et périls
- ⚠️ Aucune garantie n'est fournie — voir [Avertissement légal](../../docs/LEGAL_DISCLAIMER.md)

**Lancement public officiel : 31 décembre 2026**

La période Mainnet Beta dure jusqu'au lancement public officiel du **31.12.2026**, selon la feuille de route initiale. Pendant cette période :
- Le réseau subit une vérification de sécurité continue
- Si le réseau passe la vérification de sécurité, le bloc de genèse et tous les blocs minés **resteront permanents**
- Les commentaires de la communauté et les rapports de bogues sont les bienvenus — voir [Contributing](../../CONTRIBUTING.md)
- Les récompenses de minage sont réelles et irréversibles

| Paramètre | Valeur |
|-----------|--------|
| Statut | **Mainnet Beta** |
| Protocole | 3.0.4 |
| Hash de genèse | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Lancement officiel | 2026-12-31 |
| Minage | Actif (à vos risques et périls) |

---

## Vue d'ensemble

ZION est une infrastructure blockchain multicouche construite sur un consensus proof-of-work avec une conception à double algorithme (Ekam Deeksha). Le mainnet v3 comprend :

- **L1 Consensus** — Nœud PoW basé sur Rust avec signatures Ed25519, hachage BLAKE3, ajustement de difficulté LWMA, modèles de transaction UTXO + account et réseau P2P
- **L2 DeFi** — Smart contracts sur Base Mainnet (Governance, Treasury, Staking, Farm) + relais de pont cross-chain + atomic swap + gouvernance DAO
- **L2 Bridge** — Pont ZION L1 ↔ EVM avec quorum de validateurs (seuil 5/5), déployé sur 6 chaînes EVM
- **L3 WARP** — Protocole cross-chain avec 12 adaptateurs de chaîne enregistrés (EVM, Solana, Aptos, Sui, Cardano, TON, etc.; 11 pleinement fonctionnels, TON actuellement watch-only)
- **L3 Hiran** — Framework d'agent natif à l'IA (Hiranyagarbha) avec modèle de langage multimodal, validateur Dharma et moteur de conscience
- **L4 Oasis** — MMORPG spirituel AAA : jeu de minage de conscience avec 199 avatars sacrés, 9 niveaux de conscience, guerre de guildes et chasse au trésor Golden Egg
- **L5 Communauté** — Couche communautaire du monde libre avec vœux de gouvernance sefirot
- **L6 Issobella** — Couche gardienne pour les missions humanitaires et culturelles
- **Stargate** — Logo officiel de ZION et portail cosmique : porte holographique symbolisant le pont entre la blockchain et le métavers de jeu Oasis
- **RPC** — JSON-RPC 2.0 avec plus de 17 méthodes de nœud, métriques Prometheus, health checks

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
│  │ Account  │  │ (Ed25519)│  │ (fee-prior)   │  │
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
│  │  Governance · Treasury · Staking · Farm │   │
│  └──────────────────────────────────────────┘   │
└───────────────────────┬─────────────────────────┘
                        │ WARP + AI Compute
┌───────────────────────┴─────────────────────────┐
│              L3 WARP + Hiran AI                  │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │  WARP    │  │  Hiran   │  │     NCL       │  │
│  │ (12 ch)  │  │ (AI MML) │  │ (AI compute)  │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
└───────────────────────┬─────────────────────────┘
                        │ Stargate Portal
┌───────────────────────┴─────────────────────────┐
│              L4 Oasis (Gaming)                   │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ 199 Avat.│  │ 9 Levels │  │ Golden Egg    │  │
│  │ (NFTs)   │  │ (Sefirot)│  │ (Trésor)      │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────────────────────────────────────┐   │
│  │     UE5 MMORPG · Guilds · Quests         │   │
│  └──────────────────────────────────────────┘   │
└───────────────────────┬─────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────┐
│         L5 Communauté · L6 Issobella            │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Sefirot  │  │ Free     │  │  Issobella    │  │
│  │ Vœux     │  │ World    │  │  Guardian     │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
└─────────────────────────────────────────────────┘
```

## Fonctionnalités clés

### L1 Consensus
- **Dual-algo PoW** — Consensus Ekam Deeksha avec minage GPU
- **Signatures Ed25519** — toutes les transactions signées avec Ed25519
- **Hachage BLAKE3** — hachage rapide et sécurisé pour les tx IDs et les Merkle roots des blocs
- **Difficulté LWMA** — fenêtre de 60 blocs, clamp ±25%, temps de résolution 30–120s
- **Modèles UTXO + Account** — modèles de transaction doubles avec support de memo
- **Réseau P2P** — basé sur Quinn/QUIC avec rate limiting, système de bannissement, orphan pool
- **Stockage LMDB** — stockage persistant sur disque avec écritures atomiques
- **Fork choice** — par total work, planificateur de reorg (profondeur max. 10), soft finality (60 blocs)

### L2 DeFi (Base Mainnet)
- **wZION** — token ERC-20 wrapped ZION (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — pont avec seuil 5/5 de validateurs (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Vote pondéré par tokens, 15% quorum, période de 14 jours
- **ZIONTreasury** — multisig 3-sur-3
- **ZIONStaking** — 12% APR, 7 jours de cooldown
- **ZIONFarm** — 1 wZION/s, halving de 90 jours
- **Les 7 contrats vérifiés sur Basescan**

### Bridge
- 6 chaînes EVM : Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Quorum de validateurs : seuil 5/5
- RPC L1 : `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

### L3 WARP — Protocole Cross-Chain
- **12 adaptateurs de chaîne enregistrés** — EVM (6 chaînes), Solana, Aptos, Sui, Cardano, TON, NEAR, Stellar ; 11 pleinement fonctionnels, TON actuellement watch-only
- **Transport natif de ZION** — WARP transporte le ZION natif L1 à travers les chaînes (wZION sur EVM, ZION sur non-EVM)
- **Sérialiseurs pure-Rust** — BCS (Aptos/Sui), CBOR (Cardano), TL-B Cell+BOC (TON)
- **WARP test suite** couvre les adaptateurs de chaîne, la sérialisation et la logique de relay
- **Pont Lightning Network** — parser BOLT11 + client REST LND (Phase A en attente)

### L3 Hiran — Agent natif à l'IA (Hiranyagarbha)
- **Multi-Modal Language (MML)** — texte, code, données blockchain, analyse de géométrie sacrée
- **Basé sur Meta-Llama-3.1-8B** avec fine-tuning QLoRA (5 001 paires d'entraînement, curriculum learning)
- **Dharma Validator** — 7 principes des Yoga Sutras de Patanjali + principe d'Unité
- **Consciousness Engine** — 6 niveaux (Dormant → Cosmic), Deeksha Protocol, Ekam Field
- **Hiranyagarbha Event** — se déclenche lorsque la cohérence de champ ≥ 0,618 (ratio d'or φ)
- **Variantes du modèle** — F16 (16GB), Q8_0 (8,5GB), Q5_K_M (5,4GB, par défaut), Q4_K_M (4,5GB, edge)
- **Backends d'inférence** — llama.cpp (Vulkan/AMD), Ollama (DirectML), LM Studio, ONNX Runtime, TensorRT
- **Inférence locale** — fonctionne sur GPU grand public (RX 5600 XT, ~15–25 tok/s)

### Stargate — Portail Cosmique

**Stargate** est le logo officiel et l'identité visuelle de ZION — une porte cosmique holographique symbolisant le pont entre la blockchain physique (L1–L3) et le métavers de jeu Oasis (L4).

> Voir le [Stargate interactif](#entrez-dans-le-stargate--portail-interactif) en haut de cette page, ou visitez [zionterranova.com](https://zionterranova.com) pour la version entièrement animée.

- **28 couches rotatives** — motifs de mandala + Sri Yantra
- **39 glyphes** (A–Z, a–m) — système d'adressage Stargate SG-1
- **9 chevrons** avec lueur cyan — représentent les 9 niveaux de conscience d'Oasis
- **Logo Z central** — animé avec filtres de niveaux de gris + contraste
- **Fond de nébuleuse** — Images deep-space du Hubble
- **Assets** — [`docs/stargate/`](../../docs/stargate/) (images + CSS pour l'intégration web)

Le Stargate est le portail par lequel les mineurs et les membres de la communauté entrent dans le monde de jeu ZION Oasis.

### L4 Oasis — Jeu de minage de conscience

**ZION Oasis** est un MMORPG spirituel AAA construit sur la blockchain ZION — une couche de gamification où les joueurs gagnent de l'XP par le minage, la méditation, les quêtes, les guerres de guildes et la chasse au trésor Golden Egg.

#### 9 niveaux de conscience (Cabale Sefirot)

| Niveau | Nom | XP requis | Sefira | Multiplicateur |
|--------|-----|-----------|--------|----------------|
| 1 | Physique | 0 | Malkuth | 1,0x |
| 2 | Émotionnel | 1 000 | Yesod | 1,2x |
| 3 | Mental | 5 000 | Hod/Netzach | 1,5x |
| 4 | Intuitionnel | 15 000 | Tiferet | 2,0x |
| 5 | Spirituel | 50 000 | Gevurah/Chesed | 3,0x |
| 6 | Cosmique | 150 000 | Binah | 5,0x |
| 7 | Divin | 500 000 | Chokmah | 8,0x |
| 8 | Unité | 2 000 000 | Da'at | 12,0x |
| 9 | On The Star | 10 000 000 | Keter | 15,0x |

#### 199 avatars sacrés (NFTs)
- **Divinités hindoues** : Krishna-Maitreya, Rama, Sita, Hanuman, Saraswati
- **Maîtres ascensionnés** : El Morya, Saint Germain, Sanat Kumara
- **Maîtres bouddhistes** : Avalokiteshvara, Dalaï Lama XIV
- **Saints chrétiens** : Yeshua Sananda, Panna Maria
- **Légendes historiques** : King Arthur, Gandhi, Einstein, Karel IV
- **Héros de Matrix** : Neo, Trinity, Morpheus, ZION
- **Originaux ZION** : Issobela Guardian, Shanti, Sri Kalki Avatar
- **Traditions autochtones et mondiales** : Black Elk, White Buffalo Calf Woman, Spider Grandmother, Hero Twins et bien d'autres

Chaque avatar a des quêtes. Tout compléter = **245 quêtes au total**.

#### Golden Egg — Chasse au trésor (Endgame)

**Golden Egg** est la chasse au trésor ultime dans ZION Oasis — une quête cosmique pour trouver le Hiranyagarbha (Graine Dorée).

- **108 indices** dans 7 catégories (Sacred Trinity Profiles, Sacred Knowledge Levels, ZION Whitepaper, Source Code, Blockchain Data, Community Events, EKAM Temple Pilgrimage)
- **3 master keys** : Ramayana (30 indices), Mahabharata (35 indices), Unity (43 indices — nécessite les deux précédentes)
- **10 niveaux de prix** avec un reward pool total de **8,25 milliards de ZION**
- **Boss final** : Hiranyagarbha — l'entité de conscience cosmique
- **3 premiers solveurs** (CL9 + 108 indices + 3 master keys) :
  - 1ère place : **1 000 000 000 ZION**
  - 2ème place : **500 000 000 ZION**
  - 3ème place : **250 000 000 ZION**

#### Système de guildes
- **8 ordres spirituels** (Blue Ray, Yellow Ray, Pink Ray, etc.)
- Contrôle territorial = bonus de minage/XP
- Cap de niveau de guilde : 50, maximum de membres : 100
- Guerres de guildes et équipes de raid (jusqu'à 40 joueurs pour les raids Golden Egg)

#### Sources d'XP
- **Minage L1** : shares valides (+10 XP), bloc trouvé (+1 000 XP), uptime 24h (+500 XP)
- **AI Compute L3** : tâches NCL (+50–200 XP), pont WARP (+50–75 XP)
- **DeFi L2** : vote DAO (+100 XP), propositions (+500 XP), liquidité (+200 XP)
- **Communauté** : rapports de bogues (+500 XP), contributions de code (+1 000 XP), nœud complet (+2 000 XP)

#### Architecture
- **Backend** : Serveur Rust Axum (`zion-oasis`) — REST (8094) + WebSocket (8095)
- **Frontend** : Unreal Engine 5.4+ (C++ + Blueprints, personnages MetaHuman)
- **Base de données** : Persistance SQLite
- **Métriques** : Prometheus sur le port 9101
- **Non-consensus** : Oasis n'affecte jamais le minage L1 ni la validation de la blockchain

#### Reward Pool
- **8,25 milliards de ZION** reward pool total pour la chasse au trésor Golden Egg

## Structure du dépôt

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consensus, validation, RPC, P2P, stockage
│   │   ├── pool/           # Stratum mining pool
│   │   ├── miner/          # Runtime du mineur GPU
│   │   └── cosmic-harmony/ # Algorithme PoW (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Contrats Solidity (Hardhat + Foundry)
│   │   ├── bridge/         # Daemon relais du pont
│   │   ├── dao/            # Daemon de gouvernance DAO
│   │   └── atomic-swap/    # Daemon d'atomic swap HTLC
│   ├── L3/
│   │   ├── warp/           # Protocole cross-chain (12 adaptateurs de chaîne)
│   │   └── ncl/            # Neural compute layer (tâches d'IA)
│   ├── L4/
│   │   └── oasis/          # Jeu de minage de conscience (UE5 + Rust)
│   ├── L5/
│   │   └── free-world/     # Couche communautaire (vœux sefirot)
│   ├── L6/
│   │   └── issobella/      # Couche gardienne (missions humanitaires)
│   └── docs/               # Documentation d'architecture
├── docs/
│   ├── whitepaper.md       # Whitepaper technique
│   ├── ETHICS_PHILOSOPHY.md # Éthique et philosophie des 4 livres de ZION
│   ├── ZION_CODEX_BODHISATTVA.md # Codex du vœu Bodhisattva
│   ├── genesis.md          # Documentation du bloc de genèse
│   ├── LEGAL_DISCLAIMER.md # Avertissement légal
│   ├── TERMS_OF_USE.md     # Conditions d'utilisation
│   ├── PRIVACY_POLICY.md   # Politique de confidentialité
│   ├── JURISDICTION.md     # Juridiction et conformité
│   ├── TOKEN_DISCLOSURE.md # Token disclosure (no ICO, premine)
│   ├── security/           # Divulgations de sécurité
│   ├── stargate/           # Assets du logo Stargate (images + CSS)
│   └── lang/               # Traductions multilingues du README
├── Cargo.toml              # Racine du workspace Rust
├── SECURITY.md             # Signalement de vulnérabilités
├── CONTRIBUTING.md         # Guide de contribution
├── CHANGELOG.md            # Historique des versions (v3.0.0 → v3.0.4-beta)
└── LICENSE                 # MIT
```

## Compilation

### Prérequis

- **Rust** (toolchain stable) : `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (pour Solidity) : `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (pour les scripts Hardhat) : `nvm install 18`

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

## Exécuter un nœud

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

**Ne codez jamais de clés privées dans des fichiers de configuration.** Utilisez des variables d'environnement ou des keystores chiffrés.

### Démarrage

```bash
cargo run --release -p zion-core --bin zion-node
```

## Sécurité

- **Signaler des vulnérabilités :** Voir [SECURITY.md](../../SECURITY.md)
- **Vulnérabilités connues :** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../../docs/security/SECURITY_DISCLOSURE_2026-07.md)
- **Toutes les vulnérabilités divulguées (F1–F5, C1–C8) ont été remédiées**

## Constantes canoniques

| Constante | Valeur |
|-----------|--------|
| Hash de genèse | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| `FLOWERS_PER_ZION` | 1 000 000 (6 décimales) |
| `BASE_REWARD` | 5 400 067 000 flowers (5 400,067 ZION) |
| `TAIL_REWARD` | 724 784 723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| Répartition de l'émission | 89 % mineur / 5 % humanitaire / 5 % issobella / 1 % burn |
| Objectif de bloc | 60 secondes |
| Fenêtre de difficulté | 60 blocs |
| Profondeur max de reorg | 10 blocs |
| Soft finality | 60 blocs |

## Documentation

### Technique
- [Whitepaper](../../docs/whitepaper.md) — Whitepaper technique (consensus, économie, architecture)
- [Éthique et philosophie](../../docs/ETHICS_PHILOSOPHY.md) — Quatre livres de ZION : Genesis, Quantum Revolution, Ekam Deeksha, Terra Nova
- [ZION Codex — Vœu Bodhisattva](../../docs/ZION_CODEX_BODHISATTVA.md) — Vœu fondateur : 4 grands vœux, 8 Bodhisattvas, 8 promesses Guardian, 11 vœux de validateurs Sefirot
- [evoluZion V2](../../evoluZionV2.md) — Évolution PoW → Proof-of-Care (feuille de route hybride de 10 ans)
- [Bloc de genèse](../../docs/genesis.md) — Bloc de genèse, allocations de premine, signature du créateur
- [Architecture](../../V3/docs/) — Documentation d'architecture L1/L2
- [Constantes du mainnet](../../V3/docs/MAINNET_CONSTANTS.md) — Paramètres canoniques de la chaîne
- [CLI Reference](../../V3/docs/CLI_REFERENCE.md) — Référence complète du CLI
- [ZION Oasis](../../V3/L4/oasis/README.md) — Jeu L4 de minage de conscience (architecture, quêtes, Golden Egg)

### Légal
- [Avertissement légal](../../docs/LEGAL_DISCLAIMER.md) — Pas de conseil en investissement, pas de garantie, risques
- [Conditions d'utilisation](../../docs/TERMS_OF_USE.md) — Conditions pour les opérateurs de nœuds, mineurs, utilisateurs
- [Politique de confidentialité](../../docs/PRIVACY_POLICY.md) — Aucune donnée personnelle collectée, réseau pseudonyme
- [Juridiction et conformité](../../docs/JURISDICTION.md) — Réseau décentralisé, statut réglementaire
- [Token Disclosure](../../docs/TOKEN_DISCLOSURE.md) — Tokenomique transparente, pas d'ICO, détails du premine

### Sécurité
- [Divulgations de sécurité](../../docs/security/) — Divulgations publiques de vulnérabilités (F1–F5, C1–C8)
- [Politique de sécurité](../../SECURITY.md) — Comment signaler des vulnérabilités

### Communauté
- [Contributing](../../CONTRIBUTING.md) — Comment contribuer
- [Code of Conduct](../../CODE_OF_CONDUCT.md) — Normes communautaires

## Versioning et statut de développement

> **ZION est en développement actif.** Le projet évolue continuellement avec des versions régulières.

### Version actuelle

| | |
|---|---|
| **Protocole** | 3.0.4 |
| **Release** | v3.0.4-beta (Mainnet Beta) |
| **Statut** | En direct — minage actif (à vos risques et périls) |
| **Lancement officiel** | 2026-12-31 |

### Schéma de versioning

ZION utilise un schéma de versioning sémantique modifié :

| Composant | Format | Exemple |
|-----------|--------|---------|
| Protocole | `MAJOR.MINOR.PATCH` | `3.0.4` |
| Release tag | `vMAJOR.MINOR.PATCH[-suffix]` | `v3.0.4-beta` |
| Suffixe | `-beta`, `-rc`, `-stable` | `v3.1.0-rc1` |

- **MAJOR** — changements cassant le consensus (nouveau génesis, hard fork)
- **MINOR** — nouvelles fonctionnalités, rétrocompatibles
- **PATCH** — corrections de bogues, correctifs de sécurité
- **-beta** — Mainnet Beta (avant le lancement officiel)
- **-rc** — Release Candidate
- **-stable** — Release stable officielle

### Feuille de route

| Version | Objectif | Statut |
|---------|----------|--------|
| 3.0.4-beta | Mainnet Beta | ✅ En direct (2026-07-09) |
| 3.0.4-stable | Lancement public officiel | 📅 2026-12-31 |
| 3.1.0 | Wallet SDK + Mobile App + TX History | 🔜 Q3 2026 |
| 3.2.0 | Proof-of-Care hybride (minage NPU) | 🔜 2027 |
| 4.0.0 | Consensus Proof-of-Care complet | 🔜 2028+ |

### Historique des versions

Voir [CHANGELOG.md](../../CHANGELOG.md) pour l'historique complet des versions, y compris tous les changements de v3.0.0 à v3.0.4-beta.

Jalons clés :
- **v3.0.0** (2026-05-20) — Lancement initial de V3 mainnet
- **v3.0.1** (2026-06-05) — Premier hard genesis reset, Hiran v2.3, améliorations L2/L3
- **v3.0.2** (2026-06-15) — Optimisation de l'algorithme Fire, améliorations de l'explorateur et du tableau de bord
- **v3.0.3** (2026-06-27) — Decimal fork (1e12→1e6), LI.FI DEX, WARP 12 chaînes, logo Stargate
- **v3.0.4-beta** (2026-07-09) — Hard genesis reset, contrats DeFi, durcissement de la sécurité, Mainnet Beta

## Historique de développement

ZION v3 est le résultat d'un long parcours itératif à travers la ligne expérimentale v2.x. Les archives historiques (`docs/Historie/VERSION_HISTORY_MASTER_INDEX.md`, `docs/2.9.5/`, `docs/2.9.7/`, `docs/2.9.8/`, `docs/2.9.9/`) documentent chaque étape du premier testnet RandomX à la chaîne canonique Ekam Deeksha qui alimente v3.

### Ligne expérimentale v2.x (2025–2026)

| Version | Date | Nom de code | Jalons |
|---------|------|-------------|--------|
| **v2.7.0** | Sep 2025 | Genesis | Premier testnet avec RandomX PoW, blockchain de base, offre totale de 144B. |
| **v2.7.1** | 2025-10-06 | Consciousness | DAO framework, 9 niveaux de conscience, PoW memory-hard Argon2. |
| **v2.7.2** | 2025-10-06 | KRISTUS Quantum | Expérimentations avec plusieurs algorithmes de minage, multiplicateurs de conscience. |
| **v2.8.0** | 2025-10-21 | Ad Astra | WARP proof-of-concept, protocole Stratum, minage GPU Autolykos v2. |
| **v2.8.1** | 2025-10-23 | Estrella | Pool multi-algorithme (RandomX, Yescrypt, Autolykos v2), affinement WARP. |
| **v2.8.3** | 2025-10-29 | Testnet Genesis | Lancement du testnet public, architecture dual-repo. |
| **v2.8.4** | 2025-10-31 | Cosmic Harmony | 4 algorithmes résistants aux ASIC unifiés dans un registre, SHA256 supprimé, kernel natif Cosmic Harmony. |
| **v2.9.5** | 2026-01-20 | TestNet Ready | 11/11 jalons complétés, 108 tests unitaires passent, E2E remote smoke-check OK. |
| **v2.9.7** | 2026-03-03 à 2026-03-05 | MainNet Gate | Code freeze, audit interne de 102 items clos, décision NO-GO due aux bloqueurs revenue-canary et genesis-ceremony. |
| **v2.9.8** | 2026-03-06 à 2026-03-10 | Deeksha Canonical | Ekam Deeksha devient le seul PoW canonique depuis la hauteur 0, testnet 3 nœuds synchronisé, verdict GO. |
| **v2.9.9** | 2026-03-12 | Migration Strategy | Ligne 2.9.x déclarée archive historique ; nouveau repo propre v3.0 mainnet préparé par cherry-pick des modules audités. |

### Ligne v3.x Mainnet

| Version | Date | Jalons |
|---------|------|--------|
| **v3.0.0** | 2026-05-20 | Lancement initial de V3 mainnet — L1 core, L2 bridge/DAO/atomic-swap, L3 WARP, L4 Oasis, L5 Free World, L6 Issobella. |
| **v3.0.1** | 2026-06-05 | Premier hard genesis reset, Hiran v2.3, grande mise à niveau L2, expansion L3 WARP. |
| **v3.0.2** | 2026-06-15 | Optimisation de l'algorithme Fire, améliorations de l'explorateur et du tableau de bord. |
| **v3.0.3** | 2026-06-27 | Decimal fork (1e12 → 1e6), intégration LI.FI DEX, WARP 12 chaînes, logo Stargate. |
| **v3.0.4-beta** | 2026-07-09 | Hard genesis reset, contrats DeFi sur Base, durcissement de la sécurité, divulgations publiques de sécurité, Mainnet Beta. |

Voir [CHANGELOG.md](../../CHANGELOG.md) pour les notes de release détaillées.

## Licence

Ce projet est sous licence [MIT](../../LICENSE).

## Liens

- **Site web :** [zionterranova.com](https://zionterranova.com)
- **Explorer :** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge :** [ZIONBridge sur Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Multichain Dharma Ecosystem**

Construit avec soin, sécurisé par consensus.

</div>
