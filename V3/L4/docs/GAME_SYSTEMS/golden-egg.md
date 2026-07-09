# Golden Egg Treasure Hunt

> The **Golden Egg** is OASIS's endgame treasure hunt: 108 sacred clues hidden across the game world, assembled into 3 Master Keys, unlocking the Hiranyagarbha prize pool.

---

## Mechanics

### 108 Clues

| Category | Clues | Theme | Unlock CL |
|----------|-------|-------|-----------|
| Sacred Texts | 18 | Bhagavad Gita, Upanishads, Bible verses | 4 |
| Blockchain Riddles | 18 | Hash puzzles, merkle proofs, fee calculations | 5 |
| Geometry & Math | 18 | Fibonacci, golden ratio, Platonic solids | 5 |
| World History | 18 | Ancient civilizations, lost cities, sacred sites | 6 |
| Nature & Cosmos | 18 | Stars, elements, animals, plants | 6 |
| Meditation & Dreams | 18 | Symbolic visions, chakra colors, mantras | 7 |

### 3 Master Keys

| Key | Clues Required | Consciousness Level | Sanskrit Name | Path |
|-----|---------------|---------------------|---------------|------|
| Ramayana Key | 30 | CL 4 | Rama Kuñjī | Dharma |
| Mahabharata Key | 35 | CL 6 | Karma Kuñjī | Karma |
| Unity Key | 43 | CL 7 | Mokṣa Kuñjī | Moksha |

**Unity Key requires both previous keys.** It is the final gate.

---

## Prize Tiers

| Tier | Prize | Requirement |
|------|-------|-------------|
| 1st | 1,000,000,000 ZION | CL 9 + 108 clues + 3 keys + DAO approval |
| 2nd | 500,000,000 ZION | CL 9 + 108 clues + 3 keys + DAO approval |
| 3rd | 250,000,000 ZION | CL 9 + 108 clues + 3 keys + DAO approval |
| 4–10 | 100,000,000 ZION each | CL 8 + 90 clues + 2 keys |
| 11–100 | 10,000,000 ZION each | CL 7 + 72 clues + 1 key |
| 101–1,000 | 1,000,000 ZION each | CL 5 + 36 clues |
| 1,001–10,000 | 100,000 ZION each | CL 3 + 18 clues |

**Total prize pool:** 8.25B ZION.

---

## DAO Final Approval

The top 3 prizes require **DAO governance approval** to prevent:
- Bot / exploit farming
- Multi-account sybil attacks
- Premature prize claims

The DAO reviews:
1. Player reputation score
2. Guild verification
3. Humanitarian tithe history
4. Consciousness level audit (on-chain XP provenance)

---

## Technical Implementation

```rust
// From zion-oasis/src/golden_egg.rs
pub const TOTAL_CLUES: u32 = 108;
pub const RAMAYANA_CLUES: u32 = 30;
pub const MAHABHARATA_CLUES: u32 = 35;
pub const UNITY_CLUES: u32 = 43;

pub enum MasterKey {
    Ramayana,    // Dharma Path - 30 clues, CL 4
    Mahabharata, // Karma Path - 35 clues, CL 6
    Unity,       // Moksha Path - 43 clues, CL 7
}
```

Clue discovery is recorded on-chain via L1 transaction memos (`GOLDEN_EGG:<clue_id>`) for cryptographic provenance.
