# Guilds & Territories

> **8 Spiritual Orders** compete for territory control, mining bonuses, and guild-wide consciousness ascension.

---

## Guild System

### Creation

- **CL required:** 3 (Mental)
- **Cost:** 10,000 ZION (burned)
- **Max members:** 100
- **Level cap:** 50

### Guild Levels

| Level | Members | Territories | Bonus |
|-------|---------|--------------|-------|
| 1 | 10 | 1 | +2 % mining XP |
| 10 | 25 | 2 | +5 % mining XP, +1 territory bonus |
| 25 | 50 | 3 | +10 % mining XP, guild wars unlocked |
| 50 | 100 | 5 | +25 % mining XP, consciousness beacon |

### Ray Affiliation

Each guild chooses a **primary Ray** (1–7). Members of matching-ray avatars get +10 % XP inside guild territory.

**Full-spectrum guilds** (all 7 rays represented) get +15 % territory control bonus.

---

## 8 Territories

| Territory | Element | Ray | Bonus | Guardian Avatar |
|-----------|---------|-----|-------|-----------------|
| EKAM Temple | Ether | All | +20 % meditation XP | Krishna-Maitreya |
| Ayodhya | Earth | Blue | +15 % dharma quests | Rama |
| Vrindavan | Water | Pink | +15 % devotion XP | Radha |
| Kurukshetra | Fire | Blue | +15 % combat XP | Arjuna |
| Himalayas | Air | Yellow | +15 % wisdom quests | Vyasa-Kamil |
| ZION City | Metal | White | +10 % all XP | Maitreya |
| Ashram Grove | Wood | Ruby-Gold | +15 % peace quests | Shanti |
| Hidden Dimension | Void | Violet | +25 % clue discovery | — |

---

## Guild Wars

- **Unlock:** Guild level 25+
- **Frequency:** Weekly (Saturdays)
- **Format:** Territory capture the flag + consciousness puzzle duels
- **Rewards:**
  - Winner: Territory control + 7 days of +20 % mining bonus
  - Loser: +5 % mining bonus (consolation)
  - MVP: 50,000 XP + rare avatar fragment

### War Rules

1. Minimum 5 vs 5 players
2. No bots — human verification via AI challenge
3. Territory must be held for 10 minutes to capture
4. Consciousness puzzle duel: first to solve 3 chakra-alignment puzzles wins the tiebreaker

---

## Territory Control

- **Claiming:** Requires CL 3 + guild membership + 5,000 ZION deposit (escrow)
- **Defense:** Guild members can build "consciousness beacons" (CL 8 unlock) that slow enemy capture
- **Tax:** 1 % of all mining XP in territory goes to guild treasury
- **Decay:** Unheld territories revert to neutral after 30 days

---

## Technical Notes

Guild and territory state is stored in SQLite (`zion-oasis` backend) with L1 anchoring for:
- Guild creation transaction (burn proof)
- Territory claim transaction (escrow proof)
- Guild war results (DAO-verified outcome hash)
