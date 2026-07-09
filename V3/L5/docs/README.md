# V3/L5 — Terra Nova Physical Communities

> **Layer 5 (L5)** of the ZION ecosystem: **Physical communities** that bridge the digital protocol with the land.

---

## What is L5?

L5 is the **terrestrial layer** of ZION — where blockchain consensus meets soil, water, and human governance. It is not a "feature" of the protocol. It is a **separate but connected system** that uses ZION as its native economic and coordination layer.

### Core principle

> *"Code runs on servers. Communities run on land. Both need each other."*

- **L1** (Core) provides the **ledger** — who owns what, who contributed what.
- **L2** (DAO) provides **governance tools** — proposals, voting, treasury.
- **L3** (Warp) provides **cross-chain** access to external capital.
- **L4** (OASIS) provides the **digital interface** — avatars, quests, reputation.
- **L5** (this layer) provides the **physical substrate** — farms, sanctuaries, workshops, homes.

### The L5 Trinity

| Node | Element | Archetype | Function |
|------|---------|-----------|----------|
| **Genesis Garden** | Earth | Root / Base Camp | Foundation, agriculture, entry point |
| **Dharma Temple** | Fire | Trunk / Sanctuary | Practice, transformation, depth |
| **Te Pīko Ora** | Water | Crown / Paradise | Fruition, abundance, integration |

---

## Directory Structure

```
V3/L5/docs/
├── README.md                      ← You are here
├── ARCHITECTURE/
│   └── l5-system-design.md        ← How L5 connects to L1–L4
├── GOVERNANCE/
│   ├── community-dao-framework.md         ← On-chain + off-chain governance model
│   ├── consciousness-admission-framework.md ← Age-based entry, 5 Dharmic principles, Bodhisattva vow
│   └── multi-layer-dao-governance.md      ← Co-Admin system across L1–L6, cross-layer voting, slashing
├── PROTOCOLS/
│   └── resonance-protocol.md      ← Sound, Time & Intergenerational Bridge (Resonance Council, Fibonacci Time Capsule, Light Language Registry)
├── TECH/
│   ├── zion-node-spec.md          ← Hardware/software spec for Guardian nodes
│   ├── mesh-network.md            ← LoRa/Meshtastic off-grid communication
│   └── medical-table.md           ← Health protocol specification
├── COMMUNITIES/
│   ├── genesis-garden.md          ← Zahrada Genesis, Portugal
│   ├── dharma-temple.md           ← Dharma Temple, La Palma
│   └── te-piko-ora.md             ← Te Pīko Ora, French Polynesia
└── TEMPLATES/
    └── community-blueprint.md     ← Generic template for new L5 communities
```

---

## Active Communities

| Community | Location | Archetype | Status | L5 Docs |
|-----------|----------|-----------|--------|---------|
| **Genesis Garden** | Algarve, Portugal | Base Camp — movement, ocean, farm | 🟡 Active development | [`COMMUNITIES/genesis-garden.md`](./COMMUNITIES/genesis-garden.md) |
| **Dharma Temple** | La Palma, Canary Islands | Sanctuary — silence, meditation, volcano | 🔵 Preparation | [`COMMUNITIES/dharma-temple.md`](./COMMUNITIES/dharma-temple.md) |
| **Te Pīko Ora** | Raiatea / Tahiti, French Polynesia | Crown — paradise, marine permaculture, wayfinding | 🔵 Vision / Preparation | [`COMMUNITIES/te-piko-ora.md`](./COMMUNITIES/te-piko-ora.md) |

---

## Shared Protocols (All L5 Nodes)

Every L5 community implements the same **baseline protocols**, ensuring interoperability across the network:

| Protocol | Purpose | L1/L2 Integration |
|----------|---------|---------------------|
| **ZION Guardian Node** | Validate blocks, earn rewards, fund community treasury | Revenue split: 10% of node rewards → community fund |
| **Seed Library** | Exchange local seed varieties between nodes | Off-chain logistics, on-chain provenance (future) |
| **Medical Table** | Holistic health protocols, herbal medicine, wellness | Off-chain practice, on-chain reputation |
| **LoRa / Meshtastic Mesh** | Off-grid communication within and between communities | Message relay, no blockchain dependency |
| **Sociocratic DAO** | Hybrid governance: off-chain circles + on-chain treasury votes | L2 DAO proposals for capital allocation |
| **Consciousness Admission** | Age-based entry (free <18), Dharmic principles, Bodhisattva vow for Guardians | Off-chain verification, on-chain registry (Soulbound token) |
| **Resonance Protocol** | Sound attunement before governance, Fibonacci Time Capsules, Youth–Elder Bridge, Light Language Registry | L2 DAO seal requirement, Co-Admin frequency signatures, cross-layer HRV proof |

---

## Revenue Model

L5 communities are **economically autonomous** but **protocol-aligned**:

```
Block Reward (L1)
    ├── 89% → Miner payout (PPLNS)
    ├── 5%  → Humanitarian Tithe (L5 global fund)
    ├── 5%  → Issobella Fund (L6 space fund)
    └── 1%  → Pool operator fee

Guardian Node (L5 local)
    ├── 90% → Node operator (covers hardware, electricity, bandwidth)
    └── 10% → Community treasury (local projects, maintenance, reserves)

Community Treasury (L5 local, DAO-governed)
    ├── 40% → Operations (food, energy, maintenance)
    ├── 25% → Infrastructure (buildings, tools, expansion)
    ├── 20% → Reserve fund (safety, emergencies)
    ├── 10% → Humanitarian Tithe (forwarded to L5 global)
    └── 5%  → Education / knowledge commons
```

---

## Development Phases (All Communities)

| Phase | Name | Goal | Typical Duration |
|-------|------|------|----------------|
| 0 | **Seed** | Legal foundation, first Guardians, land access | 3–12 months |
| 1 | **Roots** | Energy, water, basic food, shelter | 6–18 months |
| 2 | **Community** | Permanent residents, governance, ZION node | 12–24 months |
| 3 | **Network** | Connection to other L5 nodes, shared protocols | 18–36 months |
| 4 | **Radiance** | Retreats, education, international visitors | 24–60 months |

---

## Relationship to Legacy L5/L6

The legacy `L5/` and `L6/` directories in the repo root contain **early vision documents** (pre-V3). All actively maintained L5 documentation lives here in `V3/L5/docs/`. The legacy files are preserved for historical context but are **not current**.

| Legacy | V3 Replacement |
|--------|--------------|
| `L5/README.md` (vision) | This document + `ARCHITECTURE/l5-system-design.md` |
| `L6/README.md` (Issobella vision) | `V3/L1/cosmic-harmony/src/revenue.rs` (5% Issobella fund) + future `V3/L6/` |
| `docs/TerraNova/Projects/*.md` (concept lists) | `V3/L5/docs/COMMUNITIES/*.md` (living implementation docs) |

---

## Contributing

To propose a new L5 community:

1. Copy [`TEMPLATES/community-blueprint.md`](./TEMPLATES/community-blueprint.md)
2. Fill in all sections (do not leave TBDs in the final version)
3. Open a PR to `V3/L5/docs/COMMUNITIES/[your-community].md`
4. Include: location proof, Guardian introductions, legal structure, budget

To update an existing community:

1. Edit the relevant `COMMUNITIES/*.md` file
2. Update the "Last modified" date
3. PR with photos/evidence where possible

---

> *"Freedom is not given — it is built, block by block, seed by seed."*

*V3/L5 · Terra Nova Physical Layer · 2026*
