# L4 OASIS API Specification

> REST API + WebSocket reference for the `zion-oasis` Rust Axum server.

---

## Base URLs

| Protocol | URL | Default Port |
|----------|-----|-------------|
| REST | `http://localhost:8094` | 8094 |
| WebSocket | `ws://localhost:8095` | 8095 |
| Metrics | `http://localhost:9101/metrics` | 9101 |

---

## REST Endpoints

### Health

```
GET /health
```
Response: `{"status":"ok"}`

### Player Profile

```
GET    /api/v1/oasis/player/:address
POST   /api/v1/oasis/player/:address/xp
```

**Player profile fields:**

```json
{
  "address": "zion1...",
  "display_name": null,
  "total_xp": 0,
  "level": "Physical",
  "guild_id": null,
  "blocks_mined": 0,
  "zion_earned": 0,
  "achievements": [],
  "tithe_total": 0,
  "challenges_completed": 0,
  "daily_streak": 0,
  "best_streak": 0,
  "referrals": 0,
  "daily_xp": 0,
  "last_active": 1716336000,
  "created_at": 1716336000,
  "stats": {}
}
```

### Leaderboard

```
GET /api/v1/oasis/leaderboard?limit=100&filter=xp|blocks|tithe
```

### Avatars

```
GET /api/v1/oasis/avatars              → List avatars (query: ?ray=Blue&min_cl=4&rarity=Epic)
GET /api/v1/oasis/avatars/:id          → Avatar by ID
GET /api/v1/oasis/avatars/:id/quests   → Quests for avatar
```

**Avatar fields:**

```json
{
  "id": 1,
  "name": "Rama",
  "subtitle": "Dharma King",
  "ray": "Blue",
  "role": "Moral Compass / Dharma Teacher",
  "location": "Ayodhya Palace",
  "quest_line": "The Path of Righteousness",
  "teaching": "Duty before desire, truth before comfort",
  "ability": "Dharma Shield",
  "consciousness_level_required": 4,
  "consciousness_level_name": "Heart Opening",
  "key": "Ramayana Key (1/3)",
  "rarity": "Epic"
}
```

### Guilds

```
POST   /api/v1/oasis/guild              → Create guild
GET    /api/v1/oasis/guild/:id          → Guild info
POST   /api/v1/oasis/guild/:id/join     → Join guild
DELETE /api/v1/oasis/guild/:id/leave    → Leave guild
GET    /api/v1/oasis/guild/:id/wars     → Guild war history
```

### Map & Territories

```
GET /api/v1/oasis/map              → Territory map JSON
GET /api/v1/oasis/map/:territory   → Territory details
```

### Golden Egg

```
GET /api/v1/oasis/golden-egg/progress/:address   → Clue progress
GET /api/v1/oasis/golden-egg/clue/:id            → Clue hint (CL-gated)
POST /api/v1/oasis/golden-egg/submit/:id         → Submit clue answer
```

### Combat

```
POST /api/v1/oasis/combat/resolve
```
Body: `{ "attacker": "zion1...", "defender": "zion1...", "territory": "Kurukshetra" }`

### Rewards

```
GET /api/v1/oasis/rewards/pools       → Active reward pools
GET /api/v1/oasis/rewards/history/:address → Player reward history
```

---

## WebSocket Events

Connect to `ws://localhost:8095`.

### Client → Server

```json
{ "type": "subscribe", "channel": "guild:<guild_id>" }
{ "type": "subscribe", "channel": "territory:<territory_id>" }
{ "type": "subscribe", "channel": "global" }
```

### Server → Client

```json
{ "type": "xp_gain", "address": "zion1...", "amount": 100, "source": "mining" }
{ "type": "level_up", "address": "zion1...", "new_level": "Emotional" }
{ "type": "territory_capture", "territory": "Ayodhya", "guild": "Dharma Protectors", "old_guild": null }
{ "type": "guild_war_start", "war_id": "uuid", "attacker": "...", "defender": "..." }
{ "type": "clue_discovered", "address": "zion1...", "clue_id": 42, "category": "Sacred Texts" }
```

---

## Authentication

All player-scoped endpoints require a **ZION wallet signature** in the header:

```
X-Zion-Signature: <ed25519_sig_of_timestamp>
X-Zion-Timestamp: <unix_seconds>
X-Zion-Address: zion1...
```

The server verifies:
1. Signature is valid for the address + timestamp
2. Timestamp is within ±60 seconds of server time
3. Address exists on-chain (L1 RPC lookup)

---

## Prometheus Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `oasis_players_total` | Gauge | Registered players |
| `oasis_xp_awarded_total` | Counter | Total XP awarded |
| `oasis_guild_wars_total` | Counter | Guild wars completed |
| `oasis_territory_changes_total` | Counter | Territory captures |
| `oasis_golden_egg_clues_found_total` | Counter | Clues discovered |
| `oasis_ws_connections` | Gauge | Active WebSocket connections |
| `oasis_http_requests_duration_seconds` | Histogram | REST latency |
