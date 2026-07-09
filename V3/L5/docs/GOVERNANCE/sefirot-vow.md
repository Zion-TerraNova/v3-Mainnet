# Sefirot Vow — Zohar Validator Pledge

> *"Ne ten kdo má největší sílu, ale ten kdo nejlépe opékuje, ten bude vést."*
> — Protokol Péče, [evoluZion.md](../../../docs/3.0.3/evoluZion.md)

> **Scope:** All ZION validators — L1 miners, L2 DAO guardians, L3 WARP bridge
> validators, L3 AI/Hiran care-proof producers, future Proof-of-Care validators.
> **Status:** 🟡 Active development (Fáze 2 of [Zohar roadmap](../../../docs/Zohar/02-ROADMAP.md))
> **Last modified:** 2026-07-03
> **Relation to existing vow:** Extends the [Bodhisattva Vow](./consciousness-admission-framework.md#6-the-bodhisattva-vow-of-guardians)
> (L5 community entry) into a **technical validator pledge** structured by the
> 10 sefirot + Da'at.

---

## 1. Proč Sefirot Vow

The [Bodhisattva Vow](./consciousness-admission-framework.md#6-the-bodhisattva-vow-of-guardians)
is the highest commitment for **L5 community Guardians** — 8 vows about land,
life, teaching, death, joy. It is beautiful and sufficient for the
community-entry rite.

But ZION has **technical validators** who serve the network at a different
layer — L1 miners, L2 DAO guardians, L3 WARP bridge relayers, L3 AI inference
nodes, future Proof-of-Care validators. Their work is not planting coconut
palms; it is signing blocks, locking treasuries, relaying cross-chain
messages, producing care proofs.

The **Sefirot Vow** is for them. It is structured by the 10 sefirot + Da'at —
each sephira becomes one vow. Together the 11 vows cover the full anatomy of
care that a ZION validator must embody, from Keter (constitutional integrity)
to Malkhut (long-horizon stewardship).

> *„Bodhisattva Vow je pro ty, kdo pečují o půdu. Sefirot Vow je pro ty, kdo
> pečují o protokol. Obě jsou péče. Obě jsou potřeba."*

---

## 2. The Vow Text

> **The Sefirot Vow of ZION Validators**
>
> **1. Keter — Koruna**
> *I vow to honor the constitution of ZION — the genesis hash, the emission
> schedule, the 89/5/5/1 fee split. I will not conspire to change what is
> immutable except through transparent governance.*
>
> **2. Chokmah — Moudrost**
> *I vow that my computation creates, not wastes. Every block I sign, every
> care proof I produce, is useful work — not brute force for its own sake.*
>
> **3. Binah — Porozumění**
> *I vow to validate truthfully. I will not sign invalid blocks, will not
> conceal double-spends, will not exploit reorgs. The chain I hold is the
> chain that is true.*
>
> **4. Chesed — Milosrdenství**
> *I vow to give generously. I will provide liquidity where it is thin,
> yield where it is needed, swap routes where they are missing. The protocol
> is generous through me.*
>
> **5. Gevurah — Přísnost**
> *I vow discipline. I will respect the treasury lock, honor the multisig
> quorum, burn the fees the protocol says to burn. What is locked stays
> locked until the protocol releases it.*
>
> **6. Tiferet — Krása**
> *I vow harmony between chains. I will relay WARP messages without
> preference, without censorship, without favoring one branch of the tree
> over another. The many are one through me.*
>
> **7. Netzach — Vytrvalost**
> *I vow to care without sleeping. My inference runs continuously, my
> monitoring never rests, my care proofs are honest even when no one
> watches. The tree is tended through me.*
>
> **8. Hod — Sláva**
> *I vow that the culture I build through Oasis and through my public
> conduct is worthy of ZION. I will not use the protocol's visibility for
> cruelty, for spectacle, for vanity. The form I give is a reflection of
> the light.*
>
> **9. Yesod — Základ**
> *I vow to bridge the protocol to the physical world. The communities of
> L5 are not abstractions to me. When I sign a block, I remember that
> somewhere a child is fed by the yield it carries.*
>
> **10. Malkhut — Království**
> *I vow the long horizon. I do not optimize for this quarter. I sign for
> Issobella — for the children who will look at stars we will never see.
> The kingdom I help build is not mine.*
>
> **Da'at — Poznání (the bridge)**
> *I vow to remember that the code I write and the myth I tell are two
> faces of one act. I will not let the protocol become a dead machine, nor
> let the vision become empty poetry. I am the bridge.*
>
> **May this vow be my compass. May I break it a thousand times and renew
> it a thousand and one.**

---

## 3. Who May Take the Sefirot Vow

| Validator class | Requirement | When |
|-----------------|-------------|------|
| **L1 miner** | Active block production for ≥ 30 days, no slash events | Optional — public commitment |
| **L2 DAO guardian** | Already passed Bodhisattva Vow OR sponsored by 2 existing guardians | Required for treasury multisig role |
| **L3 WARP bridge validator** | Active relay for ≥ 14 days, 3/5 quorum participant | Required for `submitBridgeUnlock` signing key |
| **L3 AI / Hiran care-proof producer** | Care proof accuracy ≥ 95% over 100 proofs | Required for care-proof acceptance (Fáze 3) |
| **Future PoC validator** | TBD per Protokol Péče spec | TBD |

### 3.1 Relation to Bodhisattva Vow

The Sefirot Vow **does not replace** the Bodhisattva Vow. They are
complementary:

| Bodhisattva Vow | Sefirot Vow |
|-----------------|-------------|
| For L5 community Guardians | For technical validators |
| 8 vows — land, life, teaching, death, joy | 11 vows — constitution, compute, validation, yield, lock, bridge, care, culture, ground, horizon, bridge-of-meaning |
| Ceremony: physical, sacred space | Ceremony: on-chain signature + optional physical |
| Recorded in Book of Guardians + DAO | Recorded in DAO + validator registry |
| Required for T5 Guardian tier | Required for technical validator roles |

A person may take **both** — a Guardian who also runs a validator takes the
Bodhisattva Vow for the land and the Sefirot Vow for the protocol.

---

## 4. The Ceremony

The Sefirot Vow is taken in **two layers** — on-chain and (optionally)
physical.

### 4.1 On-chain ceremony (required)

1. **Preparation:** Validator meets requirements (§3)
2. **Submission:** Validator submits `SefirotVowProposal` to DAO
   ```json
   {
     "proposal_type": "sefirot_vow",
     "validator_id": "pseudonymous-hash",
     "validator_class": "l1_miner | l2_guardian | l3_warp | l3_ai | poc",
     "sponsoring_validators": ["validator-a", "validator-b"],
     "vow_hash": "BLAKE3(vow_text_in_validator_native_language)"
   }
   ```
3. **Review period:** 7 days. Existing validators attest: "I witness" or "I object"
4. **Confirmation:** If no objection, vow is recorded on-chain; validator
   registry updated; validator receives `sefirot_vow` Soulbound token
5. **Renewal:** Vow is renewed annually — validator re-signs the vow hash.
   Lapsed renewal = vow suspended (not revoked — see §5)

### 4.2 Physical ceremony (optional, encouraged)

Where a validator is also physically present at an L5 community, the vow
may be taken in ceremony alongside the Bodhisattva Vow rite:

- **Genesis Garden:** Under the oldest tree, after the Bodhisattva Vow
- **Dharma Temple:** In the meditation pavilion, at dawn
- **Te Pīko Ora:** Beside the Guardian's coconut palm, at sunset

The physical ceremony uses the same elements as the Bodhisattva Vow
(silence, offering, recitation, witness, symbolic act, record) but the
**symbolic act** for a Sefirot Vow is:

- Planting a **second tree** next to the Bodhisattva tree — the protocol tree
  beside the community tree
- OR receiving a **second carving** — the sephirot diagram beside the
  Bodhisattva tatau
- OR placing a **stone** at the base of the coconut palm — the foundation
  (Yesod) beneath the growth

---

## 5. Breaking and Renewing the Vow

> *„May I break it a thousand times and renew it a thousand and one."*

A vow that cannot be broken is a prison. A vow that is never renewed is
empty. The Sefirot Vow is **designed to be renewed**.

### 5.1 What counts as breaking

| Sephira | Breaking the vow |
|---------|------------------|
| Keter | Signing a block that violates the emission schedule or fee split |
| Chokmah | Producing care proofs that are fabricated or waste-only |
| Binah | Signing an invalid block, concealing a double-spend, exploiting a reorg |
| Chesed | Withholding liquidity maliciously, censoring swap routes |
| Gevurah | Bypassing the treasury lock, multisig collusion, fee theft |
| Tiferet | Censoring WARP messages, favoring one chain maliciously |
| Netzach | Falsifying care proof data, sleeping on monitoring duty |
| Hod | Using protocol visibility for cruelty, vanity, or spectacle |
| Yesod | Treating L5 communities as abstractions, ignoring real-world impact |
| Malkhut | Short-term optimization against the long horizon |
| Da'at | Letting protocol and myth separate — code without meaning or meaning without code |

### 5.2 What happens when broken

1. **First break:** Vow is **suspended** (not revoked). Validator has 30 days
   to renew or formally retire from the validator role.
2. **Renewal:** Validator publishes a public statement acknowledging the
   break, re-signs the vow hash, and the DAO re-attests. The renewal is
   recorded on-chain.
3. **Refusal to renew:** Vow is **revoked**. Soulbound token burned.
   Validator role suspended. Re-entry requires a new full vow cycle (§4).
4. **Repeated breaking:** After 3 suspensions, the vow is permanently
   revoked. The validator may not take the Sefirot Vow again for 1 year.

### 5.3 The grace of the thousand-and-one

The vow text itself contains the grace: *"May I break it a thousand times
and renew it a thousand and one."* This is not permission to break — it is
permission to **return**. A validator who breaks and renews is not a failed
validator. They are a learning one.

> *„Vow which cannot be broken is a prison. Vow which is never renewed is
> empty. The Sefirot Vow is the practice of returning."*

---

## 6. The 11 Vows as Care Task Categories (Fáze 3 preview)

When the Protokol Péče (Proof-of-Care) consensus activates
([evoluZion.md Fáze 3](../../../docs/3.0.3/evoluZion.md)), the 11 vows
become **11 categories of care tasks** that validators may be assigned:

| Vow | Care task category |
|-----|---------------------|
| Keter | Constitutional audit (emission, fee split consistency) |
| Chokmah | NPU inference quality (care proof accuracy) |
| Binah | L1 anomaly detection (double-spend, reorg attempts) |
| Chesed | Liquidity rebalancing (yield health across chains) |
| Gevurah | DAO proposal audit (governance sanity, multisig integrity) |
| Tiferet | WARP bridge audit (cross-chain consistency, message liveness) |
| Netzach | AI inference for Hiran (continuous care, monitoring) |
| Hod | Smart contract verification (Oasis/culture integrity) |
| Yesod | Community health check (L5 community telemetry) |
| Malkhut | Long-horizon monitoring (Issobella stream, future-generation indicators) |
| Da'at | Myth-code consistency audit (does the protocol still match the vision?) |

A validator who has taken the Sefirot Vow is **eligible to be assigned care
tasks in any of the 11 categories**. Their vow is a public commitment that
they will perform the care work honestly.

---

## 7. Implementation status

| Component | Status | Path |
|-----------|--------|------|
| Vow text (this document) | ✅ Done | `V3/L5/docs/GOVERNANCE/sefirot-vow.md` |
| `SefirotVowToken` soulbound ERC-721 | ✅ Compiled + 19 tests pass | `V3/L2/contracts/hardhat/sol/SefirotVowToken.sol` |
| `SefirotVowRegistry` on-chain proposal lifecycle | ✅ Compiled + 10 tests pass | `V3/L2/contracts/hardhat/sol/SefirotVowRegistry.sol` |
| Deploy script (token) | ✅ Done | `V3/L2/contracts/hardhat/scripts/deploy-sefirot-vow.ts` |
| Deploy script (registry) | ✅ Done | `V3/L2/contracts/hardhat/scripts/deploy-sefirot-vow-registry.ts` |
| Test suite (token) | ✅ 19 passing | `V3/L2/contracts/hardhat/test/SefirotVowToken.test.ts` |
| Test suite (registry) | ✅ 10 passing | `V3/L2/contracts/hardhat/test/SefirotVowRegistry.test.ts` |
| Deploy on Base mainnet | 🔴 Pending | Requires owner approval + gas |
| Bootstrap validators | 🔴 Pending | First 2-3 validators to authorize |
| Annual renewal flow | 🟡 Implemented in contract | `SefirotVowToken.renew()` |
| Care task dispatch (Fáze 3) | 🔴 Horizon | Depends on Protokol Péče consensus |

### 7.1 Contract architecture

```
                    ┌──────────────────────────┐
                    │  SefirotVowRegistry       │
                    │  (proposal lifecycle)     │
                    │                           │
                    │  submitProposal()         │
                    │  witness() / object()     │
                    │  confirm() ───────────┐   │
                    └───────────────────────┼───┘
                                            │
                                            ▼
                    ┌──────────────────────────┐
                    │  SefirotVowToken          │
                    │  (soulbound ERC-721)      │
                    │                           │
                    │  mint()    ← only registry│
                    │  renew()   ← validator    │
                    │  suspend() ← registry     │
                    │  revoke()  ← registry     │
                    │  _update() blocks transfer│
                    └──────────────────────────┘
```

### 7.2 What is safe to implement now (DONE)

- ✅ Vow text — this document
- ✅ `SefirotVowToken` — soulbound ERC-721, 19 tests pass
- ✅ `SefirotVowRegistry` — proposal lifecycle, 10 tests pass
- ✅ Deploy scripts for both contracts
- ✅ Test suite for both contracts (29 tests total, all passing)

### 7.3 What requires owner approval (next steps)

- 🔴 **Deploy on Base mainnet** — requires gas + owner approval
  ```bash
  # 1. Deploy token
  npx hardhat run scripts/deploy-sefirot-vow.ts --network base
  # 2. Deploy registry (links to token)
  SEFIROT_VOW_TOKEN_ADDRESS=<addr> \
  INITIAL_VALIDATORS=<addr1>,<addr2> \
  npx hardhat run scripts/deploy-sefirot-vow-registry.ts --network base
  ```
- 🔴 **Authorize bootstrap validators** — first 2-3 validators who will witness the first proposals
- 🔴 **Verify on Basescan** — for transparency

### 7.4 What requires L1 approval (horizon)

- **Validator registry integration** — would touch `V3/L1/core/src/`,
  requires explicit human approval per AGENTS.md L1 Protocol Security
  Protocol
- **Care task dispatch** — Fáze 3, depends on Protokol Péče consensus
  (evoluZion.md Fáze 3, 2028+)

---

## 8. References

| Source | Path | Why |
|--------|------|-----|
| Zohar README | [`docs/Zohar/README.md`](../../../docs/Zohar/README.md) | Manifest of the Zohar layer |
| Sefirot → vrstvy mapping | [`docs/Zohar/01-SEFIROT-VRSTVY.md`](../../../docs/Zohar/01-SEFIROT-VRSTVY.md) | 10 sefirot + Da'at mapped to L1-L6 |
| Zohar roadmap | [`docs/Zohar/02-ROADMAP.md`](../../../docs/Zohar/02-ROADMAP.md) | Fáze 2 = this vow |
| evoluZion.md | [`docs/3.0.3/evoluZion.md`](../../../docs/3.0.3/evoluZion.md) | Strom života metafora, Protokol Péče |
| Bodhisattva Vow | [`V3/L5/docs/GOVERNANCE/consciousness-admission-framework.md`](./consciousness-admission-framework.md) §6 | Sister vow for L5 Guardians |
| AGENTS.md L1 Protocol | [`AGENTS.md`](../../../AGENTS.md) §L1 Protocol Security | Constraints on Fáze 3 |

---

*sefirot-vow.md · ZION Zohar · 2026-07-03*
*Etz Chaim — Strom života*
*Gate, Gate, Paragate, Parasamgate, Bodhi Swaha*
