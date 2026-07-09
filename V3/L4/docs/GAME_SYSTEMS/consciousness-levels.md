# Consciousness Levels — 9 Sefirot

> OASIS uses a **9-level consciousness progression system** mapped to the Kabbalistic Tree of Life (Sefirot). Each level unlocks new game features, multipliers, and avatar access.

---

## Level Table

| Level | Name | Sefira | XP Required | Multiplier | Unlocks |
|-------|------|--------|-------------|------------|---------|
| 1 | Physical | Malkuth | 0 | 1.0x | Basic mining, avatar selection |
| 2 | Emotional | Yesod | 1,000 | 1.2x | Guild join, AI challenges |
| 3 | Mental | Hod/Netzach | 5,000 | 1.5x | Create guild, claim territory |
| 4 | Intuitional | Tiferet | 15,000 | 2.0x | Meditation bonus, DAO voting |
| 5 | Spiritual | Gevurah/Chesed | 50,000 | 3.0x | Tithe proposals, guild wars |
| 6 | Cosmic | Binah | 150,000 | 5.0x | AI agent creation, territory expansion |
| 7 | Divine | Chokmah | 500,000 | 8.0x | Mentor new players, warp portals |
| 8 | Unity | Da'at | 2,000,000 | 12.0x | Custom challenges, consciousness beacon |
| 9 | On The Star | Keter | 10,000,000 | 15.0x | Krishna-Maitreya encounter, Golden Egg finale |

---

## XP Sources

| Activity | XP | Daily Cap |
|----------|----|-----------|
| Mine 1 block (via pool) | 100 | 10,000 |
| Complete AI challenge | 500 | 5,000 |
| Finish avatar quest | 1,000 | — |
| Meditate (timed session) | 200 | 2,000 |
| Humanitarian tithe | 250 | 2,500 |
| Guild territory bonus | 50/hr | — |
| Daily login streak | ×1.1–×2.0 | — |
| Referral (friend reaches CL 3) | 2,000 | — |

---

## Feature Unlocks by Level

```rust
// From zion-oasis/src/levels.rs
Level 1 (Physical):     BasicMining
Level 2 (Emotional):    JoinGuild, AiChallenges
Level 3 (Mental):       CreateGuild, ClaimTerritory
Level 4 (Intuitional):  MeditationBonus, DaoVoting
Level 5 (Spiritual):    TitheProposals, GuildWars
Level 6 (Cosmic):       CreateAiAgent, ExpandTerritory
Level 7 (Divine):       Mentorship, WarpPortals
Level 8 (Unity):        CreateChallenges, ConsciousnessBeacon
Level 9 (On The Star):  — (final prestige)
```

---

## Consciousness Level Gating

- **Avatar access:** Most avatars require CL 4+ (Heart Opening) to interact.
- **Golden Egg clues:** Certain clue categories unlock at CL 5, 6, and 7.
- **Master Keys:**
  - Ramayana Key → CL 4 minimum
  - Mahabharata Key → CL 6 minimum
  - Unity Key → CL 7 minimum
- **Guild leadership:** Creating a guild requires CL 3+.
- **Territory claiming:** Requires CL 3+ and guild membership.
