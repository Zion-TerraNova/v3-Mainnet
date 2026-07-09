# L5 Community DAO Framework

> **Hybrid governance for Terra Nova physical communities: off-chain sociocratic circles + on-chain treasury and reputation.**

---

## 1. Philosophy

Blockchain governance excels at **allocating capital** and **recording decisions immutably**. It fails at **resolving conflicts**, **reading body language**, and **growing trust**.

Physical communities excel at **human connection**, **shared labor**, and **cultural continuity**. They fail at **transparent accounting**, **global coordination**, and **censorship-resistant record-keeping**.

The L5 DAO framework **combines both**: sociocracy for daily decisions, on-chain tools for money and memory.

> *"Votes are for money. Circles are for people. Never confuse the two."*

---

## 2. Off-Chain: Sociocratic Circles

### 2.1 Structure

Every L5 community organizes itself into **circles** — self-governing teams with a clear domain. Each circle:
- Has a **defined purpose** (why it exists)
- Has a **clear domain** (what it decides)
- Uses **consent-based decision-making** (not consensus)
- Elects a **facilitator** and a **secretary**
- Sends a **link** (representative) to the parent circle

```
General Circle (all Guardians + elected long-stay members)
    ├── Operations Circle
    │       ├── Agriculture Sub-circle
    │       ├── Hospitality Sub-circle
    │       └── Infrastructure Sub-circle
    ├── Finance Circle
    │       └── Treasury Sub-circle
    ├── Community Circle
    │       ├── Conflict Care Sub-circle
    │       └── Culture & Ritual Sub-circle
    └── Knowledge Circle
            ├── Seed Library Sub-circle
            └── Education Sub-circle
```

### 2.2 Consent-Based Decision-Making

**Not consensus.** Consent means: *"I have no reasoned objection to this proposal."*

**Process:**
1. **Presentation** — proposal owner explains the proposal (5 min)
2. **Clarifying questions** — anyone can ask (10 min)
3. **Reactions** — everyone shares feelings/reactions (1 min each)
4. **Amend & integrate** — proposal owner refines (5 min)
5. **Objection round** — anyone with a reasoned objection speaks (15 min)
6. **Resolve objections** — amend proposal or accept objection as valid concern
7. **Consent check** — facilitator asks: *"Does anyone have a reasoned objection?"*
8. **Record** — secretary records decision, rationale, and timestamp

**Valid objection criteria:**
- Does this proposal endanger community safety?
- Does this proposal violate our shared agreements?
- Does this proposal exceed the circle's domain?
- Is there a better way to achieve the same aim?

**Invalid objections:**
- "I don't like it" (personal preference without reason)
- "We tried this before and it failed" (past experience is not a principle)
- "I have a better idea" (propose it separately)

### 2.3 Meeting Rhythms

| Circle | Frequency | Duration | Attendance |
|--------|-----------|----------|------------|
| Operations Circle | Weekly | 60 min | All operational Guardians |
| Finance Circle | Bi-weekly | 90 min | Finance Guardian + elected members |
| Community Circle | Monthly | 120 min | All Guardians + long-stay members |
| Knowledge Circle | Monthly | 90 min | Knowledge Guardian + elected members |
| General Circle | Quarterly | 180 min | All Guardians + elected long-stay members |

---

## 3. On-Chain: ZION DAO

### 3.1 Treasury Multisig

Every L5 community maintains a **multisig treasury** on the ZION blockchain:

```
Community Treasury (3-of-5 multisig)
    ├── Signer 1: Finance Guardian (warm wallet, hardware)
    ├── Signer 2: Community Guardian (warm wallet, hardware)
    ├── Signer 3: Operations Guardian (warm wallet, hardware)
    ├── Signer 4: External trustee (cold wallet, steel backup)
    └── Signer 5: ZION Foundation representative (cold wallet, legal backup)
```

**Thresholds:**
- 2-of-3 for daily operations wallet (sub-account)
- 3-of-5 for treasury reserve (cold storage)
- 4-of-5 for constitutional changes ( Guardianship removal, legal entity change)

### 3.2 Spending Tiers

| Tier | Amount | Decision path | On-chain? | Time |
|------|--------|-------------|-----------|------|
| Micro | < 100 EUR | Operations Circle lead | No (off-chain accounting) | Immediate |
| Small | 100–500 EUR | Parent circle consent | Yes (recorded, not executed) | 24h |
| Medium | 500–5,000 EUR | Finance Circle → General Circle | Yes (multisig 2-of-3) | 48h |
| Large | 5,000–20,000 EUR | General Circle consent → multisig | Yes (multisig 3-of-5) | 7 days |
| Extraordinary | > 20,000 EUR | General Circle + on-chain community vote | Yes (quadratic voting, 14 days) | 14 days |

### 3.3 Proposal Format

Every on-chain proposal must include:

```json
{
  "proposal_id": "genesis-2027-03-015",
  "community": "genesis-garden",
  "title": "Purchase 10 kWp solar expansion",
  "author": "operations-guardian-01",
  "circle": "Operations Circle",
  "tier": "large",
  "amount_eur": 12000,
  "amount_zion": "TBD at execution time",
  "recipient": "supplier-wallet-or-escrow",
  "rationale": "Current 5 kWp system insufficient for Phase 2. Expansion allows...",
  "circle_consent_date": "2027-03-01",
  "circle_consent_proof": "hash-of-meeting-minutes",
  "timeline": "2027-04-01 to 2027-05-15",
  "milestones": [
    {"date": "2027-04-15", "deliverable": "Panels delivered", "release_pct": 50},
    {"date": "2027-05-15", "deliverable": "Installation complete", "release_pct": 50}
  ],
  "kpis": [
    "Daily energy production > 40 kWh",
    "Guest capacity increased by 10"
  ],
  "risk_mitigation": "Supplier vetted. Backup supplier identified. Insurance covers transport.",
  "review_date": "2027-06-01"
}
```

### 3.4 Reputation and Roles

Guardians earn **on-chain reputation** through participation:

| Action | Reputation Points | Cap per period |
|--------|-------------------|----------------|
| Attend circle meeting (with proof) | 10 | Weekly |
| Submit proposal (approved) | 50 | Monthly |
| Submit proposal (rejected, but valid) | 10 | Monthly |
| Mentor new Guardian | 100 | Per mentorship |
| Resolve conflict (verified) | 150 | Per case |
| Node uptime > 99% | 200 | Quarterly |
| Humanitarian contribution (verified) | Variable | Annual |

**Reputation unlocks:**
- 500 points: Can propose without sponsorship
- 1,000 points: Can serve as multisig signer
- 2,000 points: Can represent community at L5 network council
- 5,000 points: Can propose constitutional changes

### 3.5 Slashing Conditions

Guardians can lose reputation or be removed for:

| Offense | Consequence | Appeals |
|---------|-------------|---------|
| Unauthorized treasury spend | Reputation reset, removal from multisig | 14-day appeal to General Circle |
| Node downtime > 7 days without notice | -500 reputation, warning | 7-day appeal to Tech Circle |
| Conflict of interest (undisclosed) | -300 reputation, recusal required | Community Circle mediation |
| Harassment / abuse | Immediate suspension, 30-day investigation | External arbitration (TBD) |
| False proposal data | Proposal rejected, -200 reputation | 7-day appeal to Finance Circle |

---

## 4. Hybrid Decision Examples

### Example 1: Buying a Tractor

**Off-chain:**
- Operations Circle discusses need, researches models, gets quotes
- Agriculture Sub-circle tests tractor at supplier
- Operations Circle reaches consent: "Buy tractor X from supplier Y for EUR 8,000"

**On-chain:**
- Finance Guardian submits proposal to L2 DAO with circle consent proof
- 48h review period; no objections from other Guardians
- 3-of-5 multisig executes payment to escrow
- Milestone 1 (delivery): 50% released
- Milestone 2 (acceptance): 50% released
- KPI review after 3 months: fuel efficiency, hours used, maintenance cost

### Example 2: Admitting a New Guardian

**Off-chain:**
- Applicant completes 30-day probatory stay
- Community Circle interviews applicant
- General Circle reaches consent: "Admit [Name] as Farm Guardian"

**On-chain:**
- Community Guardian submits membership proposal
- 14-day review: any Guardian can raise objection (with reason)
- If no objection: automatic on-chain record + reputation assignment
- If objection: General Circle resolves within 7 days; if unresolved, escalates to L5 Network Council

### Example 3: Emergency Spend (Volcanic Evacuation)

**Off-chain:**
- Emergency declared by any Guardian
- Immediate action: 2-of-3 operations wallet spends up to EUR 2,000
- Retroactive consent sought from General Circle within 48h

**On-chain:**
- Emergency spend recorded as "retroactive proposal"
- General Circle reviews: if legitimate, ratified; if not, Guardian responsible for repayment

---

## 5. L5 Network Council

When 3+ L5 communities exist, they form a **Network Council** — a meta-governance body for cross-community decisions.

### 5.1 Domains

- Shared protocol standards (mesh frequency, seed exchange format)
- Dispute resolution between communities
- Allocation of L5 global humanitarian fund
- Admission / expulsion of communities from network

### 5.2 Representation

Each community sends **1 elected delegate** to the Network Council. Decisions require **consent of 2/3 of represented communities**.

### 5.3 On-Chain

Network Council decisions are recorded on-chain as **multi-community proposals**:
- Each delegate signs with their community multisig key
- Threshold: 2/3 of community signatures
- Execution: via L2 DAO smart contract (when implemented)

---

## 6. Tools and Infrastructure

### 6.1 Off-Chain Tools

| Tool | Purpose | Open Source? |
|------|---------|--------------|
| **Sociocracy 3.0 patterns** | Meeting formats, consent process | Yes (sociocracy30.org) |
| **Loomio** | Async decision support | Yes (loomio.org) |
| **CryptPad** | Encrypted collaborative documents | Yes (cryptpad.fr) |
| **Signal / Matrix** | Secure group chat | Yes (Signal: clients; Matrix: protocol) |
| **Physical minutes book** | Legal backup, culture | N/A (paper, signed) |

### 6.2 On-Chain Tools

| Tool | Purpose | Status |
|------|---------|--------|
| **ZION core node** | Block validation, RPC, wallet | ✅ Implemented |
| **L2 DAO (Axum)** | Proposals, voting, treasury tracking | ✅ Implemented |
| **Multisig wallet** | Threshold spending | ✅ Implemented (script-based) |
| **Reputation registry** | Guardian points, history | 🔵 Planned (2027) |
| **Quadratic voting** | Large / constitutional decisions | 🔵 Planned (2028) |
| **Escrow contracts** | Milestone-based payments | 🔵 Planned (2028) |

---

## 7. Constitutional Principles

These principles **cannot be changed** without 4-of-5 multisig + 14-day community vote:

1. **Land sovereignty:** The community's physical land is held in trust for future generations. It cannot be sold for private profit.
2. **Open source:** All community tools, methods, and documentation are shared freely.
3. **Non-discrimination:** No one is excluded based on origin, belief, gender, or economic status. (Behavioral standards apply.)
4. **Environmental responsibility:** Every decision must consider its impact on soil, water, and biodiversity.
5. **Protocol alignment:** The community remains aligned with ZION consensus rules and the 5% humanitarian tithe.

---

> *"Democracy is two wolves and a sheep voting on dinner. Sociocracy is the sheep saying 'I have a reasoned objection.' And the wolves listening."*

*V3/L5/GOVERNANCE · Community DAO Framework · 2026*
