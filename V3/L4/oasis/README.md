# ZION OASIS — V3 L4 Consciousness Mining Game

> **AAA Spiritual MMORPG** built on ZION blockchain. Players earn XP through mining, meditation, quests with 51 sacred avatars, guild warfare, and the Golden Egg treasure hunt.

## Architecture

```
┌─────────────────────────────────────────────┐
│  UE5 Client (C++ + Blueprints)              │
│  • Open world (8 territories)                 │
│  • MetaHuman characters                       │
│  • UMG UI (wallet login, HUD, quest dialog) │
│  • Enhanced Input (WASD + gamepad)            │
└──────────────┬────────────────────────────────┘
               │ HTTP JSON / WebSocket
┌──────────────▼────────────────────────────────┐
│  zion-oasis — Rust Axum Server                │
│  • REST API (player, guild, leaderboard)      │
│  • WebSocket real-time events                 │
│  • SQLite persistence                         │
│  • Prometheus metrics                         │
└──────────────┬────────────────────────────────┘
               │ JSON-RPC
┌──────────────▼────────────────────────────────┐
│  zion-core — L1 Blockchain                    │
│  • Wallet verification                        │
│  • Balance queries                            │
│  • Block-mined XP hooks                       │
└───────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Unreal Engine 5.4](https://www.unrealengine.com/) (Epic Games Launcher)
- [Visual Studio 2022](https://visualstudio.microsoft.com/) with "Game dev with C++"
- [Docker](https://www.docker.com/) (optional, for containerized backend)

### 1. Clone & Build

```bash
# Build everything (backend + UE5 project files)
./Build-Oasis.sh        # Linux/macOS
.\Build-Oasis.ps1      # Windows
```

### 2. Start Backend

```bash
# Native
cargo run --manifest-path ../../Cargo.toml -p zion-oasis

# Or Docker
docker compose up -d
```

Backend endpoints:
- `http://localhost:8094` — REST API
- `ws://localhost:8095` — WebSocket events
- `http://localhost:9101/metrics` — Prometheus

### 3. Launch UE5 Editor

```bash
# Windows
.\ue5\RunEditor.ps1

# macOS/Linux
.\ue5\RunEditor.sh
```

### 4. Create Blueprints

See [`ue5/README_UE5.md`](ue5/README_UE5.md) for step-by-step Blueprint creation.

## Game Systems

### Consciousness Levels (9 Sefirot)

| Level | Name | Sefira | XP Required | Multiplier |
|-------|------|--------|-------------|------------|
| 1 | Physical | Malkuth | 0 | 1.0x |
| 2 | Emotional | Yesod | 1,000 | 1.2x |
| 3 | Mental | Hod/Netzach | 5,000 | 1.5x |
| 4 | Intuitional | Tiferet | 15,000 | 2.0x |
| 5 | Spiritual | Gevurah/Chesed | 50,000 | 3.0x |
| 6 | Cosmic | Binah | 150,000 | 5.0x |
| 7 | Divine | Chokmah | 500,000 | 8.0x |
| 8 | Unity | Da'at | 2,000,000 | 12.0x |
| 9 | On The Star | Keter | 10,000,000 | 15.0x |

### 51 Sacred Avatars

51 NFT avatars across 7 spiritual traditions:
- Hindu Deities (0-6): Krishna-Maitreya, Rama, Sita, Hanuman, Saraswati...
- Ascended Masters (7-16): El Morya, Saint Germain, Sanat Kumara...
- Buddhist Masters (17-20): Avalokiteshvara, Dalai Lama XIV...
- Christian Saints (21-24): Yeshua Sananda, Panna Maria...
- Historical Legends (25-30): King Arthur, Gandhi, Einstein, Karel IV...
- Matrix Heroes (31-34): Neo, Trinity, Morpheus, ZION
- ZION Originals (35-50): Issobela Guardian, Shanti, Sri Kalki Avatar...

Each avatar has 5 quests. Complete all = 255 quests total.

### Golden Egg Treasure Hunt

- **108 clues** across 8 categories
- **3 master keys** (36 clues each)
- **10 prize tiers** (total 8.25B ZION reward pool)
- First 3 players to CL9 + 108 clues + 3 keys win:
  - 1st: 1,000,000,000 ZION
  - 2nd: 500,000,000 ZION
  - 3rd: 250,000,000 ZION

### Guilds & Territories

- 8 spiritual orders (Blue Ray, Yellow Ray, Pink Ray, etc.)
- Guild level cap: 50
- Max members: 100
- Territory control = mining/XP bonuses

## REST API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/api/v1/oasis/player/:address` | Player profile |
| POST | `/api/v1/oasis/player/:address/xp` | Award XP |
| GET | `/api/v1/oasis/leaderboard` | Top players |
| POST | `/api/v1/oasis/guild` | Create guild |
| GET | `/api/v1/oasis/guild/:id` | Guild info |
| POST | `/api/v1/oasis/guild/:id/join` | Join guild |
| GET | `/api/v1/oasis/map` | Territory map |
| GET | `/api/v1/oasis/rewards/pools` | Reward status |
| GET | `/api/v1/oasis/golden-egg/progress/:address` | Egg progress |
| POST | `/api/v1/oasis/combat/resolve` | Resolve combat |

Full API docs in `src/server.rs`.

## Docker Stack

```bash
cd V3/L4/oasis
docker compose up -d

# Services:
#   oasis      → localhost:8094 (REST) / :8095 (WS)
#   nginx      → localhost:80 (proxy)
#   prometheus → localhost:9090 (metrics)
```

## Project Structure

```
oasis/
├── src/                        # Rust backend
│   ├── main.rs                 # Entrypoint
│   ├── server.rs               # Axum router (15+ endpoints)
│   ├── api.rs                  # API response types
│   ├── config.rs               # Game world config
│   ├── db.rs                   # SQLite persistence
│   ├── consciousness.rs        # 9-level consciousness system
│   ├── xp.rs                   # XP calculation
│   ├── quests.rs               # Avatar quest registry
│   ├── guild.rs                # Guild CRUD
│   ├── combat.rs               # Turn-based combat
│   ├── golden_egg.rs           # Treasure hunt logic
│   ├── territory.rs            # 8-genesis map
│   ├── rewards.rs              # Prize pool distribution
│   ├── leaderboard.rs          # Ranking system
│   ├── player.rs               # Player profile
│   ├── raid_team.rs            # Raid groups
│   ├── challenges.rs           # AI quiz challenges
│   ├── tithe.rs                # DAO tithe tracking
│   ├── levels.rs               # Level curve
│   ├── rate_limit.rs           # DDoS protection
│   ├── metrics.rs              # Prometheus gauges
│   ├── websocket.rs            # WS broadcast hub
│   └── error.rs                # Error types
├── data/                       # Static game data
│   ├── avatars.json            # 51 avatar definitions
│   ├── golden_egg.json         # 108 clue definitions
│   ├── prize_tiers.json        # Reward distribution
│   └── world.json              # Territory genesis map
├── scripts/                    # Python helpers
│   ├── gen_ue5_avatar_pipeline.py
│   └── parse_avatars.py
├── ue5/                        # Unreal Engine 5 project
│   ├── Source/ZionOasis/       # C++ game module
│   ├── Content/                # Blueprints, maps, DataTables
│   ├── Config/                 # Engine .ini files
│   ├── ZionOasis.uproject
│   └── README_UE5.md           # UE5 setup guide
├── docker-compose.yml
├── Dockerfile
├── Build-Oasis.sh / .ps1
└── README.md                   # This file
```

## Testing

```bash
# Rust unit tests
cargo test --manifest-path ../../Cargo.toml -p zion-oasis

# Health check
curl http://localhost:8094/health

# Create player + award XP
curl -X POST http://localhost:8094/api/v1/oasis/player/zion1test/xp \
  -H "Content-Type: application/json" \
  -d '{"source":"BlockMined","amount":100}'
```

## License

MIT — Copyright 2026 ZION TerraNova
