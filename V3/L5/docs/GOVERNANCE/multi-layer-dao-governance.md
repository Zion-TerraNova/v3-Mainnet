# Multi-Layer DAO Governance — Co-Admin System

> **ZION is not a single DAO. It is a federation of DAOs — one per layer, all connected.**
>
> **Scope:** L1 (Core) through L6 (Issobella) — every layer has its own Co-Admin governance
> **Status:** 🟡 Active development
> **Last modified:** 2026-05-21

---

## 1. Philosophy: The Federation of Layers

Traditional blockchain governance treats the protocol as a monolith — one token, one vote, one treasury. ZION recognizes that **each layer has different risks, different stakeholders, and different time horizons**.

| Layer | What it governs | Risk profile | Decision speed |
|-------|----------------|--------------|---------------|
| **L1 (Core)** | Consensus rules, block rewards, issuance | Existential — bugs destroy everything | Slow (14+ days) |
| **L2 (DAO)** | Treasury spend, grants, bridge parameters | Financial — funds can be stolen | Medium (7 days) |
| **L3 (WARP)** | Cross-chain bridges, fee rates, validator sets | Interoperability — bridges are honeypots | Fast (3 days for emergency) |
| **L4 (OASIS)** | Avatar standards, quest design, reputation rules | Cultural — bad design kills adoption | Medium (7 days) |
| **L5 (Terra Nova)** | Land use, community admission, Bodhisattva vows | Human — mistakes harm real people | Very slow (consent-based, no time limit) |
| **L6 (Issobella)** | Space fund allocation, launch contracts, research IP | Visionary — mistakes waste decades | Very slow (30+ days) |

**The Co-Admin principle:** No single person or address controls any layer. Every critical action requires **multiple independent Co-Admins** — not just multisig signers, but **governance participants with distinct roles, incentives, and accountability**.

> *„Jeden klíč otevírá dveře. Dva klíče otevírají dveře s kontrolou. Sedm klíčů otevírá dveře tak, že nikdo nikdy neotevře sám — a to je svoboda."*

---

## 2. The Co-Admin Architecture

### 2.1 Definition

A **Co-Admin** is a governance participant who:
1. **Holds keys** to a multisig or voting power in a DAO
2. **Has skin in the game** — staked tokens, reputation, or bonded collateral
3. **Is accountable** — can be slashed, removed, or replaced through governance
4. **Represents a distinct interest** — not a puppet of another Co-Admin

### 2.2 Co-Admin Tiers by Layer

| Layer | Co-Admin Role | Count | Selection | Removal | Key Tool |
|-------|--------------|-------|-----------|---------|----------|
| **L1** | Validator Operator | 7+ | Stake-weighted election | Unbonding + slashing | Validator set contract |
| **L1** | Core Developer | 5 | DAO proposal + reputation | DAO vote | Git commit access |
| **L2** | Treasury Guardian | 7 | DAO election (token-weighted) | DAO vote + 14d timelock | 5-of-7 multisig |
| **L2** | Bridge Guardian | 5 | Stake + bridge uptime | Emergency DAO + 7d review | 3-of-5 multisig |
| **L3** | WARP Relayer | 5+ | Bonded stake + performance | Automatic (downtime) | Rotating leader election |
| **L3** | Cross-Chain Auditor | 3 | DAO appointment | DAO vote | Manual review gate |
| **L4** | OASIS Curator | 5 | Reputation + quest completion | Community vote | Quest approval rights |
| **L4** | Avatar Moderator | 7 | Democratic election (1 avatar = 1 vote) | Recall election | Content moderation DAO |
| **L5** | Community Guardian | 5–7 | Consciousness Verification + Bodhisattva Vow | Sociocratic consent + DAO burn | Physical circle + on-chain vote |
| **L5** | L5 Network Delegate | 1 per community | Community circle election | Community recall | Network Council vote |
| **L6** | Issobella Steward | 3 | L6-specific consciousness verification | 30-day DAO vote + scientific peer review | 2-of-3 multisig + academic board |

### 2.3 The Overlap Matrix

No Co-Admin should hold power in **more than 2 adjacent layers** to prevent concentration:

| Role | L1 | L2 | L3 | L4 | L5 | L6 | Max Layers |
|------|----|----|----|----|----|----|-----------|
| Core Dev | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | 2 |
| Treasury Guardian | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | 2 |
| Bridge Guardian | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | 2 |
| OASIS Curator | ❌ | ❌ | ❌ | ✅ | ✅ | ❌ | 2 |
| Community Guardian | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | 1* |
| Issobella Steward | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | 1 |

*Community Guardians may serve in **multiple L5 communities** but not in L1–L4 or L6.

---

## 3. Layer-by-Layer Governance Detail

### 3.1 L1 — Core Protocol Governance

**What is governed:**
- Block reward schedule (89/5/5/1 split)
- Consensus algorithm changes (PoW → future upgrades)
- Hard fork coordination
- Genesis premine allocation (4B ZION)

**Co-Admins:**
| Role | Count | Power | Selection |
|------|-------|-------|-----------|
| **Validator Operators** | 7+ | Run nodes, validate blocks, earn rewards | Stake-weighted (min 100K ZION) |
| **Core Developers** | 5 | Propose code changes, review PRs | DAO-appointed + reputation |
| **Security Council** | 3 | Emergency pause, critical bug response | DAO election, 1-year term |

**Voting:**
- **Standard changes:** 14-day token-weighted vote, 10% quorum, 51% majority
- **Emergency pause:** 3-of-5 Security Council + 48h DAO ratification
- **Hard fork:** 21-day vote, 20% quorum, 66% supermajority + validator signaling

**On-chain integration:**
- Proposals encoded in L1 memo format: `DAO:parameter:<name>:<value>`
- Voting via balance snapshot at proposal block
- Execution via timelock contract (48h minimum)

**Existing implementation:** See [`V3/L2/dao/src/proposal.rs`](../../../L2/dao/src/proposal.rs), [`V3/L2/dao/src/voting.rs`](../../../L2/dao/src/voting.rs), [`V3/L2/dao/src/executor.rs`](../../../L2/dao/src/executor.rs)

---

### 3.2 L2 — DAO Treasury & Bridge Governance

**What is governed:**
- Treasury spend (4B ZION premine + ongoing revenue)
- Bridge parameters (fees, validator thresholds, supported chains)
- Grant allocation (humanitarian, development, research)
- Emergency actions (pause, rotate guardians)

**Co-Admins:**
| Role | Count | Threshold | Key Power |
|------|-------|-----------|-----------|
| **Treasury Guardians** | 7 | 5-of-7 | Spend treasury, execute grants |
| **Bridge Guardians** | 5 | 3-of-5 | Rotate bridge validators, pause/unpause |
| **DAO Voters** | Unlimited | Token-weighted | Propose, vote, delegate |

**Proposal types (from code):**
| Type | Quorum | Period | Example |
|------|--------|--------|---------|
| **Parameter** | 10% | 7 days | Change fee rate, quorum threshold |
| **Treasury** | 15% | 7 days | Spend 1M ZION on development |
| **Emergency** | 20% | 3 days | Pause bridge, freeze treasury |
| **Grant** | 10% | 7 days | Fund L5 community build |
| **Humanitarian** | 10% | 7 days | Allocate to climate resilience |

**Revenue model:**
```
Block Reward (L1)
    ├── 89% → Miners (PPLNS)
    ├── 5%  → Humanitarian Tithe (L2 DAO-controlled)
    ├── 5%  → Issobella Fund (L6 DAO-controlled, locked)
    └── 1%  → Pool operator fee

Bridge Fees (L2 WARP)
    ├── 50% → Relayers
    ├── 25% → L2 DAO treasury
    └── 25% → L1 buyback/burn
```

**Existing implementation:** See [`V3/L2/dao/src/treasury.rs`](../../../L2/dao/src/treasury.rs), [`V3/L2/dao/src/proposal.rs`](../../../L2/dao/src/proposal.rs)

---

### 3.3 L3 — WARP Cross-Chain Governance

**What is governed:**
- Supported chains (EVM, Bitcoin, future)
- Fee rates per chain
- Relayer validator sets
- Slashing conditions for misbehaving relayers

**Co-Admins:**
| Role | Count | Selection | Removal |
|------|-------|-----------|---------|
| **WARP Relayers** | 5+ | Bonded stake (min 500K ZION) | Automatic slashing (downtime, fraud proof) |
| **Cross-Chain Auditors** | 3 | DAO appointment | DAO vote |
| **Chain Curators** | 1 per chain | Reputation + uptime | Community petition |

**Voting:**
- **Standard:** Rotating leader election (PBFT-style consensus among relayers)
- **Chain addition:** L2 DAO proposal (15% quorum, 7 days) + L3 relayer consent (3-of-5)
- **Emergency:** 2-of-3 Cross-Chain Auditors can pause any bridge, 48h L2 DAO ratification

**Unique feature:** **Fraud proofs.** Any relayer can challenge another. If challenged relayer cannot produce valid proof within 24h, they are slashed automatically. No DAO vote needed — this is **algorithmic governance**.

---

### 3.4 L4 — OASIS Digital Realm Governance

**What is governed:**
- Avatar standards (visual, behavioral, lore consistency)
- Quest design rules (reward rates, difficulty curves)
- Reputation algorithm parameters
- Content moderation (spam, abuse, off-brand behavior)

**Co-Admins:**
| Role | Count | Selection | Removal |
|------|-------|-----------|---------|
| **OASIS Curators** | 5 | Reputation score (min 5,000) + quest completion | Community vote (1 avatar = 1 vote) |
| **Avatar Moderators** | 7 | Democratic election within active avatar holders | Recall election (20% quorum) |
| **Quest Designers** | Open | Application + curator approval | Curator vote |

**Voting:**
- **Quest approval:** 3-of-5 Curator consent
- **Reputation parameter change:** 7-day avatar-holder vote, 10% quorum
- **Moderation action:** 2-of-3 Moderator consensus (fast — spam cannot wait)

**Unique feature:** **1 avatar = 1 vote** (not token-weighted). OASIS is a cultural space, not a financial one. Wealth should not determine voice here.

---

### 3.5 L5 — Terra Nova Physical Community Governance

**What is governed:**
- Land use and expansion
- Community admission (Consciousness Verification, Bodhisattva Vow)
- Local treasury (10% of node rewards + guest revenue)
- Inter-node protocols (seed library, Medical Table, mesh standards)

**Co-Admins:**
| Role | Count | Threshold | Selection |
|------|-------|-----------|-----------|
| **Community Guardians** | 5–7 | Sociocratic consent (off-chain) + DAO ratification (on-chain) | Consciousness Verification + Bodhisattva Vow |
| **L5 Network Delegates** | 1 per community | 2/3 of represented communities | Community circle election |
| **Finance Guardians** | 2 per community | 2-of-3 multisig (local ops wallet) | Guardian internal rotation |

**Voting:**
- **Local decisions (< 500 EUR):** Operations Circle consent (off-chain)
- **Local decisions (500–5,000 EUR):** Finance Circle → General Circle consent + 48h DAO record
- **Local decisions (> 5,000 EUR):** General Circle consent → 7-day on-chain DAO vote (60% quorum)
- **New Guardian admission:** 4-Gate Consciousness Verification + 72h DAO review (see [`consciousness-admission-framework.md`](./consciousness-admission-framework.md))
- **Bodhisattva Vow:** Physical ceremony + 7-day distributed witnessing DAO vote (see §6.4)
- **Expulsion:** Investigation + 7-day quadratic consent vote at 75% threshold (see §8)

**Existing implementation:** See [`V3/L5/docs/GOVERNANCE/community-dao-framework.md`](./community-dao-framework.md), [`V3/L5/docs/GOVERNANCE/consciousness-admission-framework.md`](./consciousness-admission-framework.md)

---

### 3.6 L6 — Issobella Space Governance

**What is governed:**
- Allocation of 5% block reward (Issobella Fund)
- Research grants (space agriculture, closed-loop life support, propulsion)
- Launch contracts and partnerships
- Intellectual property from funded research

**Co-Admins:**
| Role | Count | Threshold | Selection |
|------|-------|-----------|-----------|
| **Issobella Stewards** | 3 | 2-of-3 multisig + academic board review | L6-specific Consciousness Verification + scientific peer review |
| **Scientific Advisory Board** | 7 | Advisory (no binding vote) | Invitation by Stewards + DAO confirmation |
| **L6 DAO Voters** | All ZION holders | Token-weighted (same as L2) | Automatic (holding ZION) |

**Voting:**
- **Research grant:** Steward proposal → 30-day DAO vote (10% quorum, 51% majority) + scientific board review (non-binding but public)
- **Launch contract:** Steward proposal → 45-day DAO vote (15% quorum, 66% supermajority) — because launch contracts are irreversible
- **IP release:** 2-of-3 Steward + scientific board unanimous consent (knowledge should be free, but timing matters)

**Lock-up:** Issobella funds are **time-locked** — cannot be spent before 2030 (unless emergency, which requires 4-of-5 L2 Treasury Guardian + L6 Steward unanimous). This prevents premature allocation.

---

## 4. Cross-Layer DAO Voting Flows

### 4.1 The Standard Proposal Lifecycle (All Layers)

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   DRAFT     │ →  │   REVIEW    │ →  │    VOTE     │ →  │  TIMELOCK   │ →  │  EXECUTE    │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
      │                  │                  │                  │                  │
   Proposer         Co-Admin            Token or           48h minimum       Multisig
   writes           review (off-        consent            (L1/L2/L3)       or algorithm
   proposal         chain or on-        voting             7d (L4/L5)        (L3 fraud
                   chain)              (layer-specific)   30d (L6)          proofs)
```

### 4.2 Cross-Layer Proposals

Some decisions affect **multiple layers**. These require **sequential or parallel voting**:

| Decision | Layers Affected | Process |
|----------|----------------|---------|
| **Add new bridge chain** | L2 + L3 | L2 DAO proposal (15% quorum, 7d) → if passed, L3 relayer consent (3-of-5) |
| **Change block reward split** | L1 + L2 + L6 | L1 token vote (20% quorum, 14d) + L2 DAO ratification (10% quorum, 7d) + L6 Steward advisory |
| **Fund L5 community** | L2 + L5 | L2 DAO grant proposal (10% quorum, 7d) → L5 community circle consent (off-chain) → funds released via milestone |
| **OASIS quest rewards L1 block** | L1 + L4 | L4 avatar-holder vote (10% quorum, 7d) → L2 DAO treasury proposal (15% quorum, 7d) → L1 parameter change |
| **Emergency hard fork** | L1 + L2 + L3 | L1 Security Council (3-of-5) → immediate action → 48h L2 DAO ratification → 7d L3 relayer consent |

### 4.3 The Veto Chain

Any layer can **veto** a cross-layer proposal that directly affects it:

```
L2 proposes: "Allocate 10M ZION to L5 Te Pīko Ora"
    ├── L2 DAO vote: PASSED (15% quorum, 51% yes)
    ├── L5 Network Council review: CONSENT (2/3 communities agree)
    ├── Te Pīko Ora circle: CONSENT (no reasoned objection)
    └── ✅ EXECUTED

L2 proposes: "Reduce L5 node reward share from 10% to 5%"
    ├── L2 DAO vote: PASSED (15% quorum, 51% yes)
    ├── L5 Network Council review: OBJECTION (2/3 communities object)
    └── ❌ VETOED — requires separate negotiation
```

**Veto override:** If a proposal is vetoed by an affected layer, it can only pass with:
- **L1/L2:** 66% supermajority + 30-day extended voting
- **L3/L4/L5/L6:** Unanimous consent of all remaining layers + mediation by ZION Foundation

---

## 5. Co-Admin Accountability & Slashing

### 5.1 Slashing Conditions by Layer

| Layer | Offense | Slashing | Executor |
|-------|---------|----------|----------|
| **L1** | Validator downtime > 7 days | Stake slashed (portion) | Algorithmic |
| **L1** | Double-sign / equivocation | Full stake slashed | Algorithmic |
| **L2** | Unauthorized treasury spend | Reputation reset, removal from multisig | DAO vote |
| **L2** | Bridge Guardian fraud | Bond slashed, permanent ban | Algorithmic + DAO |
| **L3** | Relayer downtime > 24h | Bond slashed (proportional) | Algorithmic |
| **L3** | Fraud proof validated | Full bond slashed | Algorithmic |
| **L4** | Content abuse (moderator) | Removal, reputation burn | Avatar-holder vote |
| **L5** | Bodhisattva vow breach (serious) | Token burned, network ban | Quadratic consent (75%) |
| **L6** | Misallocation of research funds | Steward removal, academic censure | 30-day DAO + peer review |

### 5.2 The Grace Period

No Co-Admin is slashed without **due process**:

1. **Accusation** — written, signed, on-chain
2. **Investigation** — independent Co-Admin from **another layer** (to prevent collusion)
3. **Defense** — accused has right to respond
4. **Review** — minimum 7 days (L1/L2/L3) or 14 days (L4/L5/L6)
5. **Execution** — only after all steps complete

> *„Spravedlnost není rychlost. Spravedlnost je důkladnost."*

---

## 6. The Co-Admin Onboarding Path

How does someone become a Co-Admin? Each layer has a **progressive path**:

```
New Participant
    ├── L1: Stake ZION → Run validator → Earn reputation → Propose protocol changes
    ├── L2: Hold ZION → Vote in DAO → Submit proposals → Elected as Guardian
    ├── L3: Bond ZION → Run relayer → Maintain uptime → Earn relayer rewards
    ├── L4: Create avatar → Complete quests → Earn reputation → Elected as Curator
    ├── L5: Visit community → Consciousness Verification → Bodhisattva Vow → Guardian
    └── L6: Scientific contribution → Peer review → Steward nomination → DAO confirmation
```

**Cross-layer progression:**
- A successful L5 Guardian may be **nominated** for L2 Treasury Guardian (but cannot hold both simultaneously)
- A L3 Relayer with 99.9% uptime may be **fast-tracked** to L2 Bridge Guardian
- An L4 Curator with 10,000+ reputation may **advise** L5 community culture design (but has no binding power)

---

## 7. Technical Implementation

### 7.1 L2 DAO Contract Schema (Pseudocode)

```solidity
// ZION L2 DAO — Multi-Layer Governance Contract
contract ZionDAO {
    // ── Co-Admin Registry ───────────────────────────────
    mapping(address => CoAdmin) public coAdmins;
    mapping(uint8 => address[]) public layerCoAdmins; // layerId => addresses

    struct CoAdmin {
        uint8 layer;        // 1-6
        uint8 role;         // 0=Validator, 1=Guardian, 2=Relayer, ...
        uint256 reputation;
        uint256 bonded;     // staked/bonded amount
        bool isActive;
        uint256 appointedAt;
        uint256 termEnd;    // 0 = indefinite
    }

    // ── Proposal Lifecycle ──────────────────────────────
    struct Proposal {
        uint256 id;
        uint8 layer;           // which layer governs this
        uint8 proposalType;    // 0=Parameter, 1=Treasury, 2=Emergency, 3=Grant, ...
        address proposer;
        bytes32 descriptionHash;
        uint256 snapshotBlock;
        uint256 votesFor;
        uint256 votesAgainst;
        uint256 votesAbstain;
        uint256 votingEndsAt;
        uint256 timelockEndsAt;
        ProposalStatus status;
    }

    // ── Cross-Layer Veto ───────────────────────────────
    mapping(uint256 => mapping(uint8 => bool)) public layerVeto; // proposalId => layer => vetoed
    mapping(uint256 => uint8) public requiredLayers; // proposalId => bitmask of layers that must consent

    // ── Functions ──────────────────────────────────────
    function propose(uint8 layer, bytes32 descHash, uint8 pType) external returns (uint256);
    function vote(uint256 proposalId, uint8 choice, uint256 weight) external;
    function execute(uint256 proposalId) external;
    function veto(uint256 proposalId) external; // only callable by affected layer's Co-Admins
    function slash(address target, uint8 reason) external; // governance-controlled
}
```

### 7.2 L2 DAO Config Integration

The existing [`DaoConfig`](../../../L2/dao/src/config.rs) is extended with:

```rust
// In V3/L2/dao/src/config.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaoConfig {
    // ... existing fields ...

    // ── Multi-Layer Co-Admin ────────────────────────────
    /// Co-Admins by layer
    pub co_admins: HashMap<u8, Vec<CoAdminConfig>>,
    /// Cross-layer veto enabled (default: true)
    pub cross_layer_veto_enabled: bool,
    /// Minimum layers that must consent for cross-layer proposals
    pub cross_layer_consent_threshold: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoAdminConfig {
    pub layer: u8,        // 1-6
    pub role: String,     // "validator", "guardian", "relayer", "curator", "community", "steward"
    pub address: String,
    pub public_key: String,
    pub bonded_amount: u64,
    pub reputation: u64,
    pub term_start: String, // ISO 8601
    pub term_end: Option<String>,
    pub is_active: bool,
}
```

### 7.3 API Endpoints

New endpoints added to [`V3/L2/dao/src/api.rs`](../../../L2/dao/src/api.rs):

| Method | Path | Description | Auth |
|--------|------|-------------|------|
| GET | `/api/dao/co-admins` | List all Co-Admins across layers | Public |
| GET | `/api/dao/co-admins/:layer` | List Co-Admins for specific layer | Public |
| POST | `/api/dao/co-admins` | Add/rotate Co-Admin (layer governance) | X-DAO-Key + layer multisig |
| GET | `/api/dao/cross-layer/:proposalId` | Get cross-layer consent status | Public |
| POST | `/api/dao/cross-layer/veto` | Veto cross-layer proposal | Layer Co-Admin key |
| GET | `/api/dao/slashing` | List active slashing proposals | Public |
| POST | `/api/dao/slashing` | Propose slashing | X-DAO-Key |

---

## 8. Emergency Governance

### 8.1 The Escalation Ladder

When things go wrong, governance **speeds up** but **requires more Co-Admins**:

| Severity | Layers | Response Time | Co-Admins Required |
|----------|--------|---------------|-------------------|
| **Green** — Minor bug | L1 only | 14 days | Standard vote |
| **Yellow** — Bridge anomaly | L2 + L3 | 7 days | Bridge Guardian (3-of-5) + DAO review |
| **Orange** — Treasury breach | L2 only | 48 hours | Security Council (3-of-5) + freeze |
| **Red** — Consensus failure | L1 + L2 + L3 | 24 hours | Security Council (3-of-5) + Emergency DAO (20% quorum, 3 days) |
| **Black** — Catastrophic | All layers | Immediate | Algorithmic pause (L1) + 2-of-3 per layer multisig |

### 8.2 Algorithmic Governance (L1 & L3)

Some emergencies **cannot wait for human votes**:

- **L1 double-sign:** Automatic slashing within 1 block
- **L3 fraud proof:** Automatic relayer slashing within 24h challenge window
- **L2 daily spend limit exceeded:** Automatic treasury freeze until Guardian override
- **L1 validator downtime > 7 days:** Automatic exclusion from validator set

These are **not democratic**. They are **mechanistic** — code is law when speed is existential.

---

## 9. Open Questions

- [ ] Co-Admin term limits: should L2 Guardians serve fixed terms (e.g., 2 years) or indefinite?
- [ ] Layer overlap: how strictly should the "max 2 adjacent layers" rule be enforced? On-chain or social?
- [ ] Cross-layer veto: should veto require reason, or can layers veto without explanation?
- [ ] L6 lock-up: should Issobella funds be locked until 2030, or gradually unlocked?
- [ ] L4 avatar voting: how to prevent Sybil attacks (multiple avatars per person)?
- [ ] L5 network council: how to handle conflicts between communities (e.g., Genesis Garden vs. Dharma Temple veto)?
- [ ] Emergency black: who holds the "algorithmic pause" keys for L1? Security Council or distributed threshold?
- [ ] Co-Admin compensation: should Guardians be paid, or is this volunteer-only? If paid, from which treasury?
- [ ] Delegation: should token holders be able to delegate voting power? If so, to whom?

---

> *„Governance není o rozhodování. Je o navrhování systémů, ve kterých se dobrá rozhodnutí dějí sama — a špatná rozhodnutí se dají opravit, než způsobí škodu."*

*Multi-Layer DAO Governance · Terra Nova L5 Governance · ZION Protocol · 2026*
