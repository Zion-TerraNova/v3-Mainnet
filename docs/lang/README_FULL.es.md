# ZION v3 — Mainnet Beta

<div align="center">

<!-- ════ STARGATE — Portal Cósmico ════ -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="../../docs/stargate/nebula.jpg">
  <img src="../../docs/stargate/nebula.jpg" width="320" height="320" alt="ZION Stargate — portal cósmico" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 40px rgba(0,180,255,0.3);" />
</picture>

<br/>

**Multichain Dharma Ecosystem**

Una blockchain con consenso proof-of-work, algoritmo dual, puente cross-chain, capa DeFi y gobernanza DAO.

[![Licencia: MIT](https://img.shields.io/badge/Licencia-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Solidity](https://img.shields.io/badge/Solidity-0.8.20-blue.svg)](https://soliditylang.org/)
[![Estado: Mainnet Beta](https://img.shields.io/badge/Status-Mainnet_Beta-orange.svg)](#estado-de-la-red)

[English](../../README_FULL.md) · [Čeština](./README_FULL.cs.md) · **Español** · [Français](./README_FULL.fr.md) · [Português](./README_FULL.pt.md)

</div>

<details>
<summary><b>Entra al Stargate</b> — Portal interactivo</summary>

<div align="center">

<img src="../../docs/stargate/2.png" width="280" alt="Capa Stargate" style="border-radius: 50%; opacity: 0.3; position: relative; z-index: 1;" />
<img src="../../docs/stargate/1.png" width="280" alt="Capa Stargate" style="border-radius: 50%; opacity: 0.15; margin-top: -280px; position: relative; z-index: 2;" />
<img src="../../docs/stargate/Z.gif" width="64" alt="ZION" style="border-radius: 50%; filter: grayscale(100%) contrast(180%); opacity: 0.7; margin-top: -170px; position: relative; z-index: 3;" />

<br/><br/>

> **El Stargate** es el portal cósmico de ZION — una puerta holográfica con 28 capas rotatorias (mandala + Sri Yantra), 39 glifos (sistema de direccionamiento Stargate SG-1) y 9 chevrones que representan los 9 niveles de conciencia del mundo de juego Oasis.
>
> El portal simboliza el puente entre la blockchain física (L1–L3) y el metaverso de juego Oasis (L4). En el sitio web en vivo ([zionterranova.com](https://zionterranova.com)), el Stargate está completamente animado con rotaciones CSS y efectos interactivos al pasar el cursor.

<br/>

| Elemento Stargate | Simbolismo |
|-------------------|------------|
| 28 capas rotatorias | Mandala + geometría sagrada Sri Yantra |
| 39 glifos (A–Z, a–m) | Sistema de direccionamiento Stargate SG-1 |
| 9 chevrones (resplandor cian) | 9 niveles de conciencia (Cábala Sefirot) |
| Logo Z central | ZION — la semilla de la conciencia |
| Fondo de nebulosa | Imágenes del espacio profundo de Hubble |

</div>

</details>

---

## Estado de la red

> **⚠️ MAINNET BETA — Minería bajo tu propio riesgo**

ZION v3.0.5 está **en vivo y funcionando** como Mainnet Beta. La red es operacional, los bloques se están produciendo y la cadena de génesis está establecida.

**Qué significa esto:**
- ✅ La red está en vivo y produciendo bloques
- ✅ El bloque de génesis y el historial de la cadena son **permanentes** — no se reiniciarán
- ✅ Todas las vulnerabilidades reveladas (F1–F5, C1–C8) han sido remediadas
- ✅ Los 7 contratos DeFi han sido verificados en Basescan
- ⚠️ La red aún puede contener errores — mina y realiza transacciones bajo tu propio riesgo
- ⚠️ No se proporciona garantía — consulta el [Descargo legal](../../docs/LEGAL_DISCLAIMER.md)

**Lanzamiento público oficial: 31 de diciembre de 2026**

El período Mainnet Beta se extiende hasta el lanzamiento público oficial el **31.12.2026**, según la hoja de ruta original. Durante este período:
- La red se somete a una verificación de seguridad continua
- Si la red pasa la verificación de seguridad, el bloque de génesis y todos los bloques minados **permanecerán permanentemente**
- Los comentarios de la comunidad y los informes de errores son bienvenidos — consulta [Contributing](../../CONTRIBUTING.md)
- Las recompensas de minería son reales e irreversibles

| Parámetro | Valor |
|-----------|-------|
| Estado | **Mainnet Beta** |
| Protocolo | 3.0.6 |
| Hash de génesis | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Lanzamiento oficial | 2026-12-31 |
| Minería | Activa (bajo tu propio riesgo) |

---

## Descripción general

ZION es una infraestructura blockchain multicapa construida sobre un consenso proof-of-work con un diseño de algoritmo dual (Ekam Deeksha). El mainnet v3 incluye:

- **L1 Consenso** — Nodo PoW basado en Rust con firmas Ed25519, hash BLAKE3, ajuste de dificultad LWMA, modelos de transacción UTXO + account y red P2P
- **L2 DeFi** — Smart contracts en Base Mainnet (Governance, Treasury, Staking, Farm) + relé de puente cross-chain + atomic swap + gobernanza DAO
- **L2 Bridge** — Puente ZION L1 ↔ EVM con quórum de validadores (umbral 5/5), desplegado en 6 cadenas EVM
- **L3 WARP** — Protocolo cross-chain con 12 adaptadores de cadena registrados (EVM, Solana, Aptos, Sui, Cardano, TON, etc.; 11 totalmente funcionales, TON actualmente watch-only)
- **L3 Hiran** — Framework de agente nativo de IA (Hiranyagarbha) con modelo de lenguaje multimodal, validador Dharma y motor de conciencia
- **L4 Oasis** — MMORPG espiritual AAA: juego de minería de conciencia con 199 avatares sagrados, 9 niveles de conciencia, guerra de gremios y búsqueda del tesoro Golden Egg
- **L5 Comunidad** — Capa comunitaria del mundo libre con votos de gobernanza sefirot
- **L6 Issobella** — Capa guardián para misiones humanitarias y culturales
- **Stargate** — Logo oficial de ZION y portal cósmico: puerta holográfica que simboliza el puente entre la blockchain y el metaverso de juego Oasis
- **RPC** — JSON-RPC 2.0 con más de 17 métodos de nodo, métricas Prometheus, health checks

## Arquitectura

```
┌─────────────────────────────────────────────────┐
│                    L1 Core                       │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Consenso │  │   P2P    │  │  JSON-RPC     │  │
│  │  (PoW)   │  │  Red     │  │  + Métricas   │  │
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
│  │ (NFTs)   │  │ (Sefirot)│  │ (Tesoro)      │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
│  ┌──────────────────────────────────────────┐   │
│  │     UE5 MMORPG · Guilds · Quests         │   │
│  └──────────────────────────────────────────┘   │
└───────────────────────┬─────────────────────────┘
                        │
┌───────────────────────┴─────────────────────────┐
│         L5 Comunidad · L6 Issobella              │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Sefirot  │  │ Free     │  │  Issobella    │  │
│  │ Votos    │  │ World    │  │  Guardian     │  │
│  └──────────┘  └──────────┘  └───────────────┘  │
└─────────────────────────────────────────────────┘
```

## Características clave

### L1 Consenso
- **Dual-algo PoW** — Consenso Ekam Deeksha con minería GPU
- **Firmas Ed25519** — todas las transacciones firmadas con Ed25519
- **Hash BLAKE3** — hash rápido y seguro para tx IDs y Merkle roots de bloques
- **Dificultad LWMA** — ventana de 60 bloques, clamp ±25%, tiempo de resolución 30–120s
- **Modelos UTXO + Account** — modelos de transacción duales con soporte de memo
- **Red P2P** — basada en Quinn/QUIC con rate limiting, sistema de baneos, orphan pool
- **Almacenamiento LMDB** — almacenamiento persistente en disco con escrituras atómicas
- **Fork choice** — por total work, planificador de reorg (profundidad máx. 10), soft finality (60 bloques)

### L2 DeFi (Base Mainnet)
- **wZION** — token ERC-20 wrapped ZION (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`)
- **ZIONBridge** — puente con umbral 5/5 de validadores (`0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467`)
- **ZIONGovernance** — Votación ponderada por tokens, 15% quorum, período de 14 días
- **ZIONTreasury** — multisig 3-de-3
- **ZIONStaking** — 12% APR, 7 días de cooldown
- **ZIONFarm** — 1 wZION/s, halving de 90 días
- **Los 7 contratos verificados en Basescan**

### Bridge
- 6 cadenas EVM: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- Quórum de validadores: umbral 5/5
- RPC L1: `getBridgeLocks`, `submitBridgeUnlock`, `getBridgeVaultBalance`

### L3 WARP — Protocolo Cross-Chain
- **12 adaptadores de cadena registrados** — EVM (6 cadenas), Solana, Aptos, Sui, Cardano, TON, NEAR, Stellar; 11 plenamente funcionales, TON actualmente watch-only
- **Transporte nativo de ZION** — WARP transporta ZION nativo de L1 a través de cadenas (wZION en EVM, ZION en no-EVM)
- **Serializadores pure-Rust** — BCS (Aptos/Sui), CBOR (Cardano), TL-B Cell+BOC (TON)
- **WARP test suite** cubre adaptadores de cadena, serialización y lógica de relay
- **Puente Lightning Network** — parser BOLT11 + cliente REST LND (Fase A pendiente)

### L3 Hiran — Agente nativo de IA (Hiranyagarbha)
- **Multi-Modal Language (MML)** — texto, código, datos blockchain, análisis de geometría sagrada
- **Basado en Meta-Llama-3.1-8B** con fine-tuning QLoRA (5,001 pares de entrenamiento, curriculum learning)
- **Dharma Validator** — 7 principios de los Yoga Sutras de Patanjali + principio de Unidad
- **Consciousness Engine** — 6 niveles (Dormant → Cosmic), Deeksha Protocol, Ekam Field
- **Hiranyagarbha Event** — se activa cuando la coherencia de campo ≥ 0,618 (razón áurea φ)
- **Variantes del modelo** — F16 (16GB), Q8_0 (8,5GB), Q5_K_M (5,4GB, default), Q4_K_M (4,5GB, edge)
- **Backends de inferencia** — llama.cpp (Vulkan/AMD), Ollama (DirectML), LM Studio, ONNX Runtime, TensorRT
- **Inferencia local** — funciona en GPU de consumo (RX 5600 XT, ~15–25 tok/s)

### Stargate — Portal Cósmico

**Stargate** es el logo oficial y la identidad visual de ZION — una puerta cósmica holográfica que simboliza el puente entre la blockchain física (L1–L3) y el metaverso de juego Oasis (L4).

> Mira el [Stargate interactivo](#entra-al-stargate--portal-interactivo) en la parte superior de esta página, o visita [zionterranova.com](https://zionterranova.com) para la versión completamente animada.

- **28 capas rotatorias** — patrones de mandala + Sri Yantra
- **39 glifos** (A–Z, a–m) — sistema de direccionamiento Stargate SG-1
- **9 chevrones** con resplandor cian — representan los 9 niveles de conciencia de Oasis
- **Logo Z central** — animado con filtros de escala de grises + contraste
- **Fondo de nebulosa** — Imágenes del espacio profundo de Hubble
- **Assets** — [`docs/stargate/`](../../docs/stargate/) (imágenes + CSS para integración web)

El Stargate es el portal a través del cual los mineros y miembros de la comunidad entran al mundo de juego ZION Oasis.

### L4 Oasis — Juego de minería de conciencia

**ZION Oasis** es un MMORPG espiritual AAA construido sobre la blockchain ZION — una capa de gamificación donde los jugadores ganan XP mediante minería, meditación, misiones, guerras de gremios y la búsqueda del tesoro Golden Egg.

#### 9 niveles de conciencia (Cábala Sefirot)

| Nivel | Nombre | XP requerido | Sefira | Multiplicador |
|-------|--------|--------------|--------|---------------|
| 1 | Físico | 0 | Malkuth | 1,0x |
| 2 | Emocional | 1.000 | Yesod | 1,2x |
| 3 | Mental | 5.000 | Hod/Netzach | 1,5x |
| 4 | Intuicional | 15.000 | Tiferet | 2,0x |
| 5 | Espiritual | 50.000 | Gevurah/Chesed | 3,0x |
| 6 | Cósmico | 150.000 | Binah | 5,0x |
| 7 | Divino | 500.000 | Chokmah | 8,0x |
| 8 | Unidad | 2.000.000 | Da'at | 12,0x |
| 9 | On The Star | 10.000.000 | Keter | 15,0x |

#### 199 avatares sagrados (NFTs)
- **Deidades hindúes**: Krishna-Maitreya, Rama, Sita, Hanuman, Saraswati
- **Maestros ascendidos**: El Morya, Saint Germain, Sanat Kumara
- **Maestros budistas**: Avalokiteshvara, Dalai Lama XIV
- **Santos cristianos**: Yeshua Sananda, Panna Maria
- **Leyendas históricas**: King Arthur, Gandhi, Einstein, Karel IV
- **Héroes de Matrix**: Neo, Trinity, Morpheus, ZION
- **Originales ZION**: Issobela Guardian, Shanti, Sri Kalki Avatar
- **Tradiciones indígenas y mundiales**: Black Elk, White Buffalo Calf Woman, Spider Grandmother, Hero Twins y muchos más

Cada avatar tiene misiones. Completar todas = **245 misiones en total**.

#### Golden Egg — Búsqueda del tesoro (Endgame)

**Golden Egg** es la búsqueda del tesoro definitiva en ZION Oasis — una búsqueda cósmica para encontrar al Hiranyagarbha (Semilla Dorada).

- **108 pistas** en 7 categorías (Sacred Trinity Profiles, Sacred Knowledge Levels, ZION Whitepaper, Source Code, Blockchain Data, Community Events, EKAM Temple Pilgrimage)
- **3 master keys**: Ramayana (30 pistas), Mahabharata (35 pistas), Unity (43 pistas — requiere las dos anteriores)
- **10 niveles de premio** con un reward pool total de **8,25 mil millones de ZION**
- **Jefe final**: Hiranyagarbha — la entidad de conciencia cósmica
- **Primeros 3 solucionadores** (CL9 + 108 pistas + 3 master keys):
  - 1er lugar: **1.000.000.000 ZION**
  - 2do lugar: **500.000.000 ZION**
  - 3er lugar: **250.000.000 ZION**

#### Sistema de gremios
- **8 órdenes espirituales** (Blue Ray, Yellow Ray, Pink Ray, etc.)
- Control de territorio = bonificaciones de minería/XP
- Cap de nivel de gremio: 50, máximo de miembros: 100
- Guerras de gremios y equipos de raid (hasta 40 jugadores para raids Golden Egg)

#### Fuentes de XP
- **Minería L1**: shares válidos (+10 XP), bloque encontrado (+1.000 XP), 24h uptime (+500 XP)
- **AI Compute L3**: tareas NCL (+50–200 XP), puente WARP (+50–75 XP)
- **DeFi L2**: votación DAO (+100 XP), propuestas (+500 XP), liquidez (+200 XP)
- **Comunidad**: informes de errores (+500 XP), contribuciones de código (+1.000 XP), nodo completo (+2.000 XP)

#### Arquitectura
- **Backend**: Servidor Rust Axum (`zion-oasis`) — REST (8094) + WebSocket (8095)
- **Frontend**: Unreal Engine 5.4+ (C++ + Blueprints, personajes MetaHuman)
- **Base de datos**: Persistencia SQLite
- **Métricas**: Prometheus en el puerto 9101
- **Non-consensus**: Oasis nunca afecta la minería L1 ni la validación de la blockchain

#### Reward Pool
- **8,25 mil millones de ZION** reward pool total para la caza del tesoro Golden Egg

## Estructura del repositorio

```
v3-Mainnet/
├── V3/
│   ├── L1/
│   │   ├── core/           # Consenso, validación, RPC, P2P, almacenamiento
│   │   ├── pool/           # Stratum mining pool
│   │   ├── miner/          # Runtime del minero GPU
│   │   └── cosmic-harmony/ # Algoritmo PoW (Ekam Deeksha)
│   ├── L2/
│   │   ├── contracts/      # Contratos Solidity (Hardhat + Foundry)
│   │   ├── bridge/         # Daemon del puente relay
│   │   ├── dao/            # Daemon de gobernanza DAO
│   │   └── atomic-swap/    # Daemon de atomic swap HTLC
│   ├── L3/
│   │   ├── warp/           # Protocolo cross-chain (12 adaptadores de cadena)
│   │   └── ncl/            # Neural compute layer (tareas de IA)
│   ├── L4/
│   │   └── oasis/          # Juego de minería de conciencia (UE5 + Rust)
│   ├── L5/
│   │   └── free-world/     # Capa comunitaria (votos sefirot)
│   ├── L6/
│   │   └── issobella/      # Capa guardián (misiones humanitarias)
│   └── docs/               # Documentación de arquitectura
├── docs/
│   ├── whitepaper.md       # Whitepaper técnico
│   ├── ETHICS_PHILOSOPHY.md # Ética y filosofía de los 4 libros de ZION
│   ├── ZION_CODEX_BODHISATTVA.md # Códice del voto Bodhisattva
│   ├── genesis.md          # Documentación del bloque de génesis
│   ├── LEGAL_DISCLAIMER.md # Descargo legal
│   ├── TERMS_OF_USE.md     # Términos de uso
│   ├── PRIVACY_POLICY.md   # Política de privacidad
│   ├── JURISDICTION.md     # Jurisdicción y compliance
│   ├── TOKEN_DISCLOSURE.md # Token disclosure (no ICO, premine)
│   ├── security/           # Divulgaciones de seguridad
│   ├── stargate/           # Assets del logo Stargate (imágenes + CSS)
│   └── lang/               # Traducciones multilingües del README
├── Cargo.toml              # Raíz del workspace Rust
├── SECURITY.md             # Informe de vulnerabilidades
├── CONTRIBUTING.md         # Guía de contribución
├── CHANGELOG.md            # Historial de versiones (v3.0.0 → v3.0.5-beta)
└── LICENSE                 # MIT
```

## Compilación

### Prerrequisitos

- **Rust** (toolchain stable): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- **Foundry** (para Solidity): `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- **Node.js** 18+ (para scripts de Hardhat): `nvm install 18`

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

# Todas las pruebas Rust
cargo test --workspace --release

# Contratos Solidity
cd V3/L2/contracts && forge test
```

## Ejecutar un nodo

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

**Nunca codifiques claves privadas en archivos de configuración.** Usa variables de entorno o keystores encriptados.

### Inicio

```bash
cargo run --release -p zion-core --bin zion-node
```

## Seguridad

- **Informar vulnerabilidades:** Ver [SECURITY.md](../../SECURITY.md)
- **Vulnerabilidades conocidas:** [docs/security/SECURITY_DISCLOSURE_2026-07.md](../../docs/security/SECURITY_DISCLOSURE_2026-07.md)
- **Todas las vulnerabilidades reveladas (F1–F5, C1–C8) han sido remediadas**

## Constantes canónicas

| Constante | Valor |
|-----------|-------|
| Hash de génesis | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| `FLOWERS_PER_ZION` | 1.000.000 (6 decimales) |
| `BASE_REWARD` | 5.400.067.000 flowers (5.400,067 ZION) |
| `TAIL_REWARD` | 724.784.723 flowers (~724,785 ZION) |
| `MIN_TX_FEE` | 1 flower (0,000001 ZION) |
| División de emisión | 89 % minero / 5 % humanitario / 5 % issobella / 1 % burn |
| Objetivo de bloque | 60 segundos |
| Ventana de dificultad | 60 bloques |
| Profundidad máxima de reorg | 10 bloques |
| Soft finality | 60 bloques |

## Documentación

### Técnica
- [Whitepaper](../../docs/whitepaper.md) — Whitepaper técnico (consenso, economía, arquitectura)
- [Ética y filosofía](../../docs/ETHICS_PHILOSOPHY.md) — Cuatro libros de ZION: Genesis, Quantum Revolution, Ekam Deeksha, Terra Nova
- [ZION Codex — Voto Bodhisattva](../../docs/ZION_CODEX_BODHISATTVA.md) — Voto fundacional: 4 grandes votos, 8 Bodhisattvas, 8 promesas Guardian, 11 votos de validadores Sefirot
- [evoluZion V2](../../evoluZionV2.md) — Evolución PoW → Proof-of-Care (roadmap híbrido de 10 años)
- [Bloque de génesis](../../docs/genesis.md) — Bloque de génesis, asignaciones de premine, firma del creador
- [Arquitectura](../../V3/docs/) — Documentación de arquitectura L1/L2
- [Constantes del mainnet](../../V3/docs/MAINNET_CONSTANTS.md) — Parámetros canónicos de la cadena
- [CLI Reference](../../V3/docs/CLI_REFERENCE.md) — Referencia completa del CLI
- [ZION Oasis](../../V3/L4/oasis/README.md) — Juego L4 de minería de conciencia (arquitectura, misiones, Golden Egg)

### Legal
- [Descargo legal](../../docs/LEGAL_DISCLAIMER.md) — No es asesoramiento de inversión, sin garantía, riesgos
- [Términos de uso](../../docs/TERMS_OF_USE.md) — Condiciones para operadores de nodos, mineros, usuarios
- [Política de privacidad](../../docs/PRIVACY_POLICY.md) — No se recopilan datos personales, red pseudónima
- [Jurisdicción y compliance](../../docs/JURISDICTION.md) — Red descentralizada, estado regulatorio
- [Token Disclosure](../../docs/TOKEN_DISCLOSURE.md) — Tokenómica transparente, no ICO, detalles del premine

### Seguridad
- [Divulgaciones de seguridad](../../docs/security/) — Divulgaciones públicas de vulnerabilidades (F1–F5, C1–C8)
- [Política de seguridad](../../SECURITY.md) — Cómo reportar vulnerabilidades

### Comunidad
- [Contributing](../../CONTRIBUTING.md) — Cómo contribuir
- [Code of Conduct](../../CODE_OF_CONDUCT.md) — Estándares comunitarios

## Versionado y estado de desarrollo

> **ZION está bajo desarrollo activo.** El proyecto evoluciona continuamente con lanzamientos versionados regulares.

### Versión actual

| | |
|---|---|
| **Protocolo** | 3.0.6 |
| **Release** | v3.0.5-beta (Mainnet Beta) |
| **Estado** | En vivo — minería activa (bajo tu propio riesgo) |
| **Lanzamiento oficial** | 2026-12-31 |

### Esquema de versionado

ZION utiliza un esquema de versionado semántico modificado:

| Componente | Formato | Ejemplo |
|-----------|--------|---------|
| Protocolo | `MAJOR.MINOR.PATCH` | `3.0.6` |
| Release tag | `vMAJOR.MINOR.PATCH[-suffix]` | `v3.0.5-beta` |
| Sufijo | `-beta`, `-rc`, `-stable` | `v3.1.0-rc1` |

- **MAJOR** — cambios que rompen el consenso (nuevo génesis, hard fork)
- **MINOR** — nuevas funciones, retrocompatibles
- **PATCH** — correcciones de errores, parches de seguridad
- **-beta** — Mainnet Beta (previo al lanzamiento oficial)
- **-rc** — Release Candidate
- **-stable** — Lanzamiento estable oficial

### Hoja de ruta

| Versión | Objetivo | Estado |
|---------|----------|--------|
| 3.0.5-beta | Mainnet Beta | ✅ En vivo (2026-07-09) |
| 3.0.5-stable | Lanzamiento público oficial | 📅 2026-12-31 |
| 3.1.0 | Wallet SDK + Mobile App + TX History | 🔜 Q3 2026 |
| 3.2.0 | Proof-of-Care híbrido (minería NPU) | 🔜 2027 |
| 4.0.0 | Consenso Proof-of-Care completo | 🔜 2028+ |

### Historial de versiones

Consulta [CHANGELOG.md](../../CHANGELOG.md) para el historial completo de versiones, incluyendo todos los cambios de v3.0.0 a v3.0.5-beta.

Hitos clave:
- **v3.0.0** (2026-05-20) — Lanzamiento inicial de V3 mainnet
- **v3.0.1** (2026-06-05) — Primer hard genesis reset, Hiran v2.3, mejoras L2/L3
- **v3.0.2** (2026-06-15) — Optimización del algoritmo Fire, mejoras en explorer y dashboard
- **v3.0.3** (2026-06-27) — Decimal fork (1e12→1e6), LI.FI DEX, WARP 12 cadenas, logo Stargate
- **v3.0.5-beta** (2026-07-10) — Hard genesis reset, contratos DeFi, endurecimiento de seguridad, Mainnet Beta

## Historia de desarrollo

ZION v3 es el resultado de un largo viaje iterativo a través de la línea experimental v2.x. Los archivos históricos (`docs/Historie/VERSION_HISTORY_MASTER_INDEX.md`, `docs/2.9.5/`, `docs/2.9.7/`, `docs/2.9.8/`, `docs/2.9.9/`) documentan cada paso desde el primer testnet RandomX hasta la cadena canónica Ekam Deeksha que alimenta v3.

### Línea experimental v2.x (2025–2026)

| Versión | Fecha | Nombre clave | Hito |
|---------|-------|--------------|------|
| **v2.7.0** | Sep 2025 | Genesis | Primer testnet con RandomX PoW, blockchain básica, suministro total de 144B. |
| **v2.7.1** | 2025-10-06 | Consciousness | DAO framework, 9 niveles de conciencia, PoW memory-hard Argon2. |
| **v2.7.2** | 2025-10-06 | KRISTUS Quantum | Experimentos con múltiples algoritmos de minería, multiplicadores de conciencia. |
| **v2.8.0** | 2025-10-21 | Ad Astra | WARP proof-of-concept, protocolo Stratum, minería GPU Autolykos v2. |
| **v2.8.1** | 2025-10-23 | Estrella | Pool multi-algoritmo (RandomX, Yescrypt, Autolykos v2), refinamiento WARP. |
| **v2.8.3** | 2025-10-29 | Testnet Genesis | Lanzamiento de testnet público, arquitectura dual-repo. |
| **v2.8.4** | 2025-10-31 | Cosmic Harmony | 4 algoritmos resistentes a ASIC unificados en un registro, SHA256 eliminado, kernel nativo Cosmic Harmony. |
| **v2.9.5** | 2026-01-20 | TestNet Ready | 11/11 hitos completados, 108 tests unitarios pasan, E2E remote smoke-check OK. |
| **v2.9.7** | 2026-03-03 a 2026-03-05 | MainNet Gate | Code freeze, auditoría interna de 102 items cerrada, decisión NO-GO por bloqueadores revenue-canary y genesis-ceremony. |
| **v2.9.8** | 2026-03-06 a 2026-03-10 | Deeksha Canonical | Ekam Deeksha se convierte en el único PoW canónico desde altura 0, testnet de 3 nodos sincronizado, veredicto GO. |
| **v2.9.9** | 2026-03-12 | Migration Strategy | Línea 2.9.x declarada archivo histórico; preparado nuevo repo limpio v3.0 mainnet por cherry-pick de módulos auditados. |

### Línea v3.x Mainnet

| Versión | Fecha | Hito |
|---------|-------|------|
| **v3.0.0** | 2026-05-20 | Lanzamiento inicial de V3 mainnet — L1 core, L2 bridge/DAO/atomic-swap, L3 WARP, L4 Oasis, L5 Free World, L6 Issobella. |
| **v3.0.1** | 2026-06-05 | Primer hard genesis reset, Hiran v2.3, gran upgrade L2, expansión L3 WARP. |
| **v3.0.2** | 2026-06-15 | Optimización del algoritmo Fire, mejoras en explorer y dashboard. |
| **v3.0.3** | 2026-06-27 | Decimal fork (1e12 → 1e6), integración LI.FI DEX, WARP 12 cadenas, logo Stargate. |
| **v3.0.5-beta** | 2026-07-10 | Hard genesis reset, contratos DeFi en Base, endurecimiento de seguridad, divulgaciones públicas de seguridad, Mainnet Beta. |

Consulta [CHANGELOG.md](../../CHANGELOG.md) para las notas de release detalladas.

## Licencia

Este proyecto está licenciado bajo la [Licencia MIT](../../LICENSE).

## Enlaces

- **Web:** [zionterranova.com](https://zionterranova.com)
- **Explorer:** [explorer.zionterranova.com](https://explorer.zionterranova.com)
- **Bridge:** [ZIONBridge en Basescan](https://basescan.org/address/0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467)

---

<div align="center">

**ZION — Multichain Dharma Ecosystem**

Construido con cuidado, asegurado por consenso.

</div>
