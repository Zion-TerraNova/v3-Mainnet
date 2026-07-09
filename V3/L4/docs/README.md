# V3/L4 — OASIS: Consciousness Mining Game

> **Layer 4 (L4)** of the ZION ecosystem: the **digital realm** where blockchain consensus becomes playable experience.

---

## What is L4?

L4 is the **game layer** of ZION — a AAA Spiritual MMORPG built on the ZION blockchain. Players earn XP through mining, meditation, quests with sacred avatars, guild warfare, and the Golden Egg treasure hunt.

### Core principle

> *"Consciousness is the ultimate game mechanic."*

- **L1** (Core) provides the **ledger** — wallet identity, balance, block rewards.
- **L2** (DAO) provides **governance** — guild DAOs, territory votes, tithe proposals.
- **L3** (Warp/AI-Native) provides **cross-chain AI** — autonomous quest generation, marketplace.
- **L4** (this layer) provides the **playable interface** — avatars, quests, reputation, guilds.
- **L5** (Terra Nova) provides the **physical substrate** — player gatherings, LARPs, retreats.

---

## Directory Structure

```
V3/L4/docs/
├── README.md                          ← You are here
├── AVATARS/
│   ├── README.md                      ← Avatar system overview (51 core + 151 extended)
│   └── sacred-trinity.md              ← Core 17 Hindu deities (Trimurti + Shakti + Vedic)
├── GAME_SYSTEMS/
│   ├── consciousness-levels.md        ← 9 Sefirot progression (Malkuth → Keter)
│   ├── golden-egg.md                  ← Treasure hunt: 108 clues, 3 Master Keys
│   └── guilds-territories.md          ← Guilds, territories, warfare, rewards
└── TECH/
    ├── api-spec.md                    ← REST API + WebSocket reference
    └── ue5-integration.md             ← Unreal Engine 5.4 bridge & blueprints
```

---

## Architecture

```
┌─────────────────────────────────────────────┐
│  UE5 Client (C++ + Blueprints)              │
│  • Open world (8 territories)             │
│  • MetaHuman characters                     │
│  • UMG UI (wallet login, HUD, quest dialog) │
└──────────────┬──────────────────────────────┘
               │ HTTP JSON / WebSocket
┌──────────────▼──────────────────────────────┐
│  zion-oasis — Rust Axum Server              │
│  • REST API (player, guild, leaderboard)    │
│  • WebSocket real-time events               │
│  • SQLite persistence                       │
│  • Prometheus metrics                       │
└──────────────┬──────────────────────────────┘
               │ JSON-RPC
┌──────────────▼──────────────────────────────┐
│  zion-core — L1 Blockchain                  │
│  • Wallet verification                      │
│  • Balance queries                          │
│  • Block-mined XP hooks                     │
└─────────────────────────────────────────────┘
```

---

## Key Files

| Component | Path |
|-----------|------|
| Rust backend | `V3/L4/oasis/src/` |
| UE5 project | `V3/L4/oasis/ue5/` |
| Avatar data (JSON) | `V3/L4/oasis/data/avatars.json` |
| World data (JSON) | `V3/L4/oasis/data/world.json` |
| Golden Egg data (JSON) | `V3/L4/oasis/data/golden_egg.json` |
| Docker stack | `V3/L4/oasis/docker-compose.yml` |

---

## Status

- **Backend crate** `zion-oasis`: ✅ Builds, REST API + WebSocket ready.
- **UE5 project**: ✅ Base project, blueprints, maps, C++ bridge component.
- **Avatar docs**: ✅ This directory (2026-05-21).
- **Full launch target**: Q3–Q4 2026 (post external audit + L1/L2/L3 stabilization).
