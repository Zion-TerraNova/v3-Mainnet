# L5 System Design — Physical Layer Architecture

> **How Terra Nova physical communities integrate with the ZION L1–L4 stack.**

---

## 1. Overview

L5 is not a blockchain layer. It is a **socio-physical system** that uses the ZION protocol as its economic and coordination backbone. This document describes the architecture, interfaces, and data flows between L5 communities and the rest of the ZION stack.

```
┌─────────────────────────────────────────────────────────────────┐
│                         L4  OASIS                              │
│  Avatars · Quests · Knowledge Commons · Consciousness Mining   │
└────────────────────────────┬────────────────────────────────────┘
                             │  HTTP / gRPC
┌────────────────────────────▼────────────────────────────────────┐
│                         L3  WARP                               │
│  Cross-chain bridges · Fundraising · External asset pegs       │
└────────────────────────────┬────────────────────────────────────┘
                             │  Internal RPC
┌────────────────────────────▼────────────────────────────────────┐
│                         L2  DAO                                │
│  Proposals · Voting · Treasury · Governance                  │
└────────────────────────────┬────────────────────────────────────┘
                             │  SQLite / Axum API
┌────────────────────────────▼────────────────────────────────────┐
│                         L1  CORE                               │
│  Consensus · Mempool · State · P2P · RPC                     │
└────────────────────────────┬────────────────────────────────────┘
                             │  ZION P2P (port 8333)
┌────────────────────────────▼────────────────────────────────────┐
│                     L5  PHYSICAL                              │
│  Genesis Garden │ Dharma Temple │ [Future communities]        │
│  Energy · Water · Food · Shelter · Governance · Culture       │
└─────────────────────────────────────────────────────────────────┘
```

---

## 2. Design Principles

### 2.1 Autonomy with Accountability

L5 communities are **legally and physically autonomous**. They own their land, make their own decisions, and manage their own resources. However, they are **accountable to the ZION protocol** for:
- Revenue transparency (treasury on-chain)
- Humanitarian tithe (5% of protocol fees)
- Guardian node uptime (network health)

### 2.2 Offline-First, Sync-When-Online

L5 communities often have **intermittent connectivity**. The design assumes:
- Local state is authoritative while offline
- When connectivity returns, state syncs to L1
- Conflicts are resolved by timestamp + consensus

### 2.3 Human-Readable, Machine-Verifiable

All L5 governance decisions must be:
- **Readable** by community members (not just developers)
- **Verifiable** by the ZION node (signed, timestamped, on-chain)

---

## 3. Component Architecture

### 3.1 L5 Community Node (Physical)

```
┌────────────────────────────────────────┐
│  L5 Community                          │
│                                        │
│  ┌─────────────┐    ┌─────────────┐   │
│  │  Guardian   │    │   LoRa /    │   │
│  │  Node (L1)  │◄──►│  Mesh GW    │   │
│  │             │    │             │   │
│  │  - Validate │    │  - Local    │   │
│  │  - RPC      │    │    sensors  │   │
│  │  - Treasury │    │  - Guest    │   │
│  │    wallet   │    │    safety   │   │
│  └──────┬──────┘    └─────────────┘   │
│         │                              │
│         ▼                              │
│  ┌─────────────────────────────────┐   │
│  │      L5 Local Agent             │   │
│  │  (Rust daemon, community host)  │   │
│  │                                 │   │
│  │  - Reads node RPC              │   │
│  │  - Reads mesh sensors          │   │
│  │  - Writes to local DB        │   │
│  │  - Syncs to L2 DAO (batch)    │   │
│  └─────────────────────────────────┘   │
│                                        │
│  ┌─────────────┐    ┌─────────────┐   │
│  │  Community  │    │   Medical   │   │
│  │  Dashboard  │    │   Table DB  │   │
│  │  (local)    │    │  (local)    │   │
│  └─────────────┘    └─────────────┘   │
└────────────────────────────────────────┘
```

### 3.2 L5 Local Agent

The **L5 Local Agent** is a new component (not yet implemented). It is a lightweight Rust daemon that runs on the same hardware as the Guardian Node (or on a separate RPi):

**Functions:**
1. **Sensor ingestion:** Read temperature, humidity, soil moisture, energy production from LoRa mesh
2. **State aggregation:** Store time-series data in local SQLite
3. **Batch sync:** When internet is available, push aggregated data to:
   - L2 DAO (treasury reporting, proposal triggers)
   - L4 OASIS (community quest progress, reputation)
   - External monitoring (Grafana, Prometheus)
4. **Offline governance:** Queue DAO votes locally, submit when online
5. **Alerting:** Local alerts (low battery, pump failure) via mesh broadcast

**Data schema (simplified):**
```sql
CREATE TABLE sensor_readings (
    id INTEGER PRIMARY KEY,
    sensor_id TEXT NOT NULL,      -- e.g., "solar-01", "soil-bed-3"
    reading_type TEXT NOT NULL,   -- "voltage", "temp_c", "moisture_pct"
    value REAL NOT NULL,
    unit TEXT,
    timestamp INTEGER NOT NULL,   -- Unix epoch
    synced INTEGER DEFAULT 0      -- 0 = local only, 1 = pushed to L2
);

CREATE TABLE dao_actions (
    id INTEGER PRIMARY KEY,
    action_type TEXT NOT NULL,    -- "spend", "vote", "member_add"
    payload TEXT NOT NULL,        -- JSON
    signed_by TEXT,               -- Guardian pubkey
    timestamp INTEGER NOT NULL,
    synced INTEGER DEFAULT 0
);
```

### 3.3 Interface to L1 (Core)

| Direction | Data | Mechanism | Frequency |
|-----------|------|-----------|-----------|
| L5 → L1 | Guardian node consensus participation | P2P protocol | Continuous |
| L1 → L5 | Block rewards, mempool status | RPC (local) | Per block |
| L5 → L1 | Treasury transactions | Signed TX via RPC | Ad hoc |

### 3.4 Interface to L2 (DAO)

| Direction | Data | Mechanism | Frequency |
|-----------|------|-----------|-----------|
| L5 → L2 | Treasury spending proposals | HTTP POST to L2 Axum API | Weekly |
| L5 → L2 | Community metrics (guests, harvest, energy) | Batch JSON | Daily |
| L2 → L5 | Approved proposals, treasury status | HTTP GET / webhook | Event-driven |

### 3.5 Interface to L3 (Warp)

| Direction | Data | Mechanism | Frequency |
|-----------|------|-----------|-----------|
| L5 → L3 | Cross-chain fundraising campaigns | HTTP POST | Campaign-based |
| L3 → L5 | Incoming bridge transactions (donations) | Webhook | Event-driven |

### 3.6 Interface to L4 (OASIS)

| Direction | Data | Mechanism | Frequency |
|-----------|------|-----------|-----------|
| L5 → L4 | Quest completion (e.g., "Plant 10 trees") | gRPC / HTTP | Event-driven |
| L5 → L4 | Guardian reputation updates | gRPC | Weekly |
| L4 → L5 | Quest assignments, visitor bookings | HTTP GET / webhook | Daily |

---

## 4. Data Flows

### 4.1 Revenue Flow

```
Block found (L1)
    ├── 89% → Miner payout (PPLNS, off-chain or L1 wallet)
    ├── 5%  → Humanitarian Tithe (L5 global fund, on-chain)
    ├── 5%  → Issobella Fund (L6, on-chain)
    └── 1%  → Pool fee

Guardian Node earns rewards (separate flow)
    ├── 90% → Node operator wallet (covers costs)
    └── 10% → Community treasury (multisig)

Community Treasury spends (L5 local)
    ├── 40% → Operations (food, energy, maintenance)
    ├── 25% → Infrastructure (buildings, tools)
    ├── 20% → Reserve
    ├── 10% → Humanitarian Tithe (forwarded to L5 global)
    └── 5%  → Education / knowledge commons
```

### 4.2 Guest Booking Flow

```
Guest visits L4 OASIS website
    ├── Selects "Genesis Garden retreat"
    ├── Pays deposit (ZION or fiat via L3 bridge)
    └── Booking recorded on L4

L4 pushes booking to L5 Local Agent (webhook)
    ├── Agent adds to local calendar DB
    ├── Agent sends confirmation to guest (email)
    └── Agent notifies Hospitality Guardian (mesh alert)

Guest arrives at L5
    ├── Check-in via local dashboard (no internet required)
    ├── Usage tracked: meals, energy, activities
    └── Checkout: final payment settled on L1
```

### 4.3 Seed Library Flow

```
Genesis Garden harvests seeds (L5 local)
    ├── Catalogs in local DB: species, variety, date, origin
    ├── Uploads to L4 Knowledge Commons (when online)
    └── Offers exchange to Dharma Temple (mesh message)

Dharma Temple requests seeds (mesh → L5 Agent)
    ├── Genesis Garden confirms availability
    ├── Physical exchange arranged (mail or visitor transport)
    └── Both nodes update local seed DB

Annual sync (when both online)
    ├── Both communities push seed exchange log to L2 DAO
    └── On-chain provenance record (future: NFT or simple hash)
```

---

## 5. Security Model

### 5.1 Threats

| Threat | Vector | Mitigation |
|--------|--------|------------|
| Node theft | Physical | Locked case, tamper alerts, no keys stored locally |
| Network partition | ISP failure | LoRa mesh for local comms, Starlink/4G fallback |
| Sybil attack (fake community) | On-chain | KYC for Guardian registration, site visit verification |
| Treasury theft | Key compromise | Multisig (3-of-5), cold storage, hardware wallets |
| Data loss | Disk failure | RAID 1 + quarterly off-site backup |
| Censorship | Government interference | Decentralized hosting, encrypted backups |

### 5.2 Guardian Node Key Management

```
Root key (BIP39 mnemonic)
    ├── Guardian node key (hot, on node, for validation)
    ├── Treasury key 1 (warm, hardware wallet, held by Finance Guardian)
    ├── Treasury key 2 (warm, hardware wallet, held by Community Guardian)
    ├── Treasury key 3 (cold, steel backup, held by external trustee)
    └── Recovery key (cold, held by ZION Foundation / trusted third party)
```

---

## 6. Scalability

### 6.1 Single Community

| Metric | Phase 1 | Phase 2 | Phase 3 |
|--------|---------|---------|---------|
| Guardian nodes | 1 | 1 | 1–2 (redundancy) |
| LoRa mesh nodes | 2–5 | 5–15 | 15–30 |
| Sensor data points/day | 1,000 | 10,000 | 50,000 |
| DAO proposals/year | 12 | 52 | 100+ |
| Treasury TX/year | 50 | 200 | 500 |

### 6.2 Network-Wide (All L5 Communities)

| Metric | 2026 | 2028 | 2030 |
|--------|------|------|------|
| Active communities | 2 | 5 | 12+ |
| Total Guardian nodes | 2 | 5 | 15+ |
| Inter-node mesh links | 0 | 1 (Genesis↔Dharma) | 5+ |
| Shared seed varieties | 20 | 100 | 500+ |

---

## 7. Implementation Status

| Component | Status | ETA |
|-----------|--------|-----|
| Guardian Node hardware spec | ✅ Done | `V3/L5/docs/TECH/zion-node-spec.md` |
| LoRa mesh spec | 🟡 Draft | `V3/L5/docs/TECH/mesh-network.md` (planned) |
| L5 Local Agent | 🔵 Not started | 2027 |
| Community dashboard | 🔵 Not started | 2027 |
| Seed library protocol | 🔵 Not started | 2028 |
| Medical Table v2 (Hiran) | 🔵 Not started | 2029 |
| Inter-node payment channels | 🔵 Not started | 2029 |

---

## 8. Glossary

| Term | Definition |
|------|------------|
| **Guardian Node** | ZION full node operated by an L5 community |
| **L5 Local Agent** | Rust daemon bridging physical sensors to ZION stack |
| **Community Treasury** | Multisig wallet holding community funds |
| **Humanitarian Tithe** | 5% of L1 block rewards → L5 global fund |
| **Seed Library** | Decentralized exchange of local plant varieties |
| **Medical Table** | Holistic health protocol and equipment standard |

---

> *"L5 is where the abstract becomes edible. Where a block hash becomes a seed in the ground. Where consensus becomes a circle of humans listening to each other."*

*V3/L5/ARCHITECTURE · System Design · 2026*
