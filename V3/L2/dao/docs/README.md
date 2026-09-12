# 🏛️ ZION DAO — L2 Dokumentace

> *„True leadership is teaching people to lead themselves."* — Maitreya Buddha

---

## Co je ZION DAO?

**ZION DAO** (Decentralized Autonomous Organization) je governance vrstva ekosystému ZION. Spravuje treasury (4B ZION z genesis), schvaluje granty, řídí protokolové změny a zajišťuje 20-letý přechod od centralizované správy k plné decentralizaci.

DAO není jen technická struktura — je to **dharmická organizace** založená na posvátné geometrii, kde consciousness a komunita společně řídí budoucnost.

---

## Struktura dokumentace

| Dokument | Obsah |
|----------|-------|
| [`GOVERNANCE_STRUCTURE.md`](GOVERNANCE_STRUCTURE.md) | Kompletní hierarchie DAO — Co-Admins, Koncil 9, Round Table 12, vrstvení |
| [`PROTOCOLS.md`](PROTOCOLS.md) | Protokoly a procesy — 20-year transition, voting, emergency, succession |
| [`SACRED_TRINITY.md`](SACRED_TRINITY.md) | Duchovní framework — kosmická rodina, archetypy, posvátná geometrie |
| [`V3_SOFTWARE.md`](V3_SOFTWARE.md) | Technická dokumentace crate `zion-dao` — API, DB, treasury, voting, timelock |

---

## Rychlé odkazy

- **Legacy DAO governance:** [`docs/docs2.9/2.7.1/DAO_GOVERNANCE_PLAN.md`](../../../../docs/docs2.9/2.7.1/DAO_GOVERNANCE_PLAN.md)
- **Koncil 9:** [`docs/docs2.9/2.8.2/KONCIL_9_DAO_GOVERNANCE.md`](../../../../docs/docs2.9/2.8.2/KONCIL_9_DAO_GOVERNANCE.md)
- **Round Table:** [`docs/docs2.9/2.7.5/ZION_2.8_ROUND_TABLE_COMPLETE_REPORT.md`](../../../../docs/docs2.9/2.7.5/ZION_2.8_ROUND_TABLE_COMPLETE_REPORT.md)
- **Maitreya profil:** [`docs/docs2.9/ZION_OASIS/SACRED_TRINITY/04_MAITREYA.md`](../../../../docs/docs2.9/ZION_OASIS/SACRED_TRINITY/04_MAITREYA.md)
- **Issobela profil:** [`docs/docs2.9/ZION_OASIS/SACRED_TRINITY/08_ISSOBELA_GUARDIAN.md`](../../../../docs/docs2.9/ZION_OASIS/SACRED_TRINITY/08_ISSOBELA_GUARDIAN.md)
- **Rodokmen:** [`docs/docs2.9/ZION_OASIS/SACRED_TRINITY/07_FAMILY_TREE.md`](../../../../docs/docs2.9/ZION_OASIS/SACRED_TRINITY/07_FAMILY_TREE.md)
- **Whitepaper DAO:** [`docs/WP-Mainet/ZION_Mainnet_Whitepaper_v3.0.5_CZ.md`](../../../../docs/WP-Mainet/ZION_Mainnet_Whitepaper_v3.0.5_CZ.md)
- **Kanonický whitepaper:** [`V3/docs/ZION_Mainnet_Whitepaper_v3.0.5_Canonical.md`](../../docs/ZION_Mainnet_Whitepaper_v3.0.5_Canonical.md)

---

## Hierarchie DAO v kostce

```
                        ⭐ CENTRUM ⭐
              Co-Admins: Maitreya Buddha + Sarah Issabela
                  (Supreme Authority — 50/50)
                           │
        ┌──────────────────┼──────────────────┐
        │                  │                  │
   ┌────▼────┐      ┌─────▼─────┐      ┌────▼────┐
   │ Koncil 9│      │Round Table│      │Sacred   │
   │  (Rada) │      │  (12 AI)  │      │Trinity  │
   │  2 + 7  │      │ Advisors  │      │ (Rama/  │
   └────┬────┘      └─────┬─────┘      │Sita/    │
        │                 │           │Hanuman) │
   ┌────┴────┐      ┌─────┴─────┐     └────────┘
   │Feminine │      │4 Circles │
   │  (3)    │      │Strategic │
   │Masculine│      │Operational│
   │  (4)    │      │Community │
   └─────────┘      │Wisdom   │
                     └─────────┘
```

---

## Základní parametry

| Parametr | Hodnota | Zdroj |
|----------|---------|-------|
| DAO Treasury | 1 500 000 000 ZION (genesis premine, sloty 7–8) | `types.rs` † |
| Multi-sig | 5 z 7 signatářů | `types.rs` |
| Denní limit výdajů | 100M ZION | `types.rs` |
| Práh návrhu | 1M ZION | `types.rs` |
| Hlasovací období | 7 dní | `types.rs` |
| Timelock | 48 hodin | `types.rs` |
| Quorum | 10 % oběžného množství | `types.rs` |
| Default API port | 8080 | `config.rs` |

† Kanonický premine je definován ve V31 `v3_compat.rs` `PREMINE_OUTPUTS` (DAO Treasury = sloty 7–8, 1B + 0.5B, time-lock do bloku 144 000). Legacy V3 konstanta `DAO_TREASURY_TOTAL = 4_000_000_000` v `types.rs` je překonaná — slot 6 (2.5B) byl přesměrován na L6 Issobella Orbital Station & Quantum Research Fund.

---

## 20-Year Transition Plan

| Fáze | Roky | Maitreya kontrola | DAO kontrola |
|------|------|-------------------|--------------|
| 1. Centralizovaná stabilita | 2025–2030 | 100 % | 0 % |
| 2. Hybridní governance | 2030–2037 | 70 % | 30 % |
| 3. Přechod | 2037–2045 | 25–50 % | 50–75 % |
| 4. Plné DAO | 2045+ | 0 % (honorary) | 100 % |

---

## Stav implementace (V3)

| Komponent | Stav |
|-----------|------|
| `zion-dao` crate | ✅ Implementováno |
| Proposal engine | ✅ 5 typů návrhů |
| Voting engine | ✅ Token-weighted (1 ZION = 1 hlas) |
| Treasury | ✅ 5-of-7 multi-sig, denní limit |
| Timelock | ✅ 48h delay |
| L1 scanner | ✅ Sledování DAO memos |
| Humanitarian modul | ✅ Alokace 5 % block reward |
| CLI integrace | ✅ `zion-cli dao` subcommandy |
| Docker service | ✅ `Dockerfile.dao` |
| Testy | ✅ Integrační testy |

---

*„ZION se stane prvním skutečně spirituálně-decentralizovaným blockchain, kde consciousness a komunita společně řídí budoucnost lidstva."* 🌟
