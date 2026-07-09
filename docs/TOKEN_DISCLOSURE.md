# Token disclosure — ZION

> **Verze:** 1.0 — 2026-07-09
> **Platí pro:** ZION token (L1 native + wZION ERC-20)

---

## 1. Shrnutí

| Parametr | Hodnota |
|----------|---------|
| Název | ZION |
| Ticker | ZION |
| Typ | Native L1 token (UTXO + account model) |
| Wrapped | wZION (ERC-20, 6 EVM chainů) |
| Decimals | 6 (1 ZION = 1 000 000 flowers) |
| Max supply | 144 000 000 000 ZION (144 miliard) |
| Premine | 16 780 000 000 ZION (11,65 %) |
| Mining emission | 127 220 000 000 ZION (88,35 %) |
| ICO/IEO | **Žádné** — nebyla provedena veřejná nabídka |
| Genesis | 2026-01-01 00:00:00 UTC |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |

---

## 2. Žádné ICO

ZION **nebylo nabízeno** prostřednictvím:

- Initial Coin Offering (ICO)
- Initial Exchange Offering (IEO)
- Security Token Offering (STO)
- Veřejné crowdsale
- Private sale / seed round
- Airdrop (kromě OASIS rewards — viz níže)

**Premine nebyl prodán investorům.** Byl alokován na genesis bloku
pro účely provozu sítě, komunity a humanitárních cílů.

---

## 3. Premine — transparentní alokace

Všech 16,78B ZION premine je **veřejně zdokumentováno** v genesis bloku
a v [`genesis.md`](./genesis.md). Žádná část není skrytá.

| Kategorie | Částka (ZION) | % | Lock |
|-----------|---------------|---|------|
| OASIS + Golden Egg (5 slotů) | 8 250 000 000 | 49,2 % | admin-locked |
| DAO Treasury (3 sloty) | 4 000 000 000 | 23,8 % | admin + time-locked (blok 144 000) |
| Infrastruktura (3 sloty) | 2 590 000 000 | 15,4 % | admin-locked |
| Humanitární (Children Future Fund) | 1 440 000 000 | 8,6 % | admin-locked |
| Bridge Seed | 400 000 000 | 2,4 % | admin-locked |
| Bridge Vault UTXO | 100 000 000 | 0,6 % | admin-locked |
| **Celkem** | **16 780 000 000** | **100 %** | |

### 3.1 Genesis Creator — 590M ZION

Jediná alokace pro tvůrce (Yose): **590 000 000 ZION** (3,5 % premine,
0,41 % total supply). Účel: **lifetime rent** — živobytí tvůrce.

- **Admin-locked** — nelze utratit bez 3-of-3 multisig + DAO vote
- **Žádný** vesting schedule, žádný cliff — ale admin-lock funguje jako vesting
- Tvůrce **nemůže** unilaterálně utratit

### 3.2 Žádné insider allocation

- **Žádné** tokeny pro investory (nejsou žádní investoři)
- **Žádné** tokeny pro advisory board (neexistuje)
- **Žádné** tokeny pro VC firmy (žádné VC funding)
- **Žádné** tokens pro burzy (žádné listing fee v tokenech)

---

## 4. Emission schedule

Mining emission (127,22B ZION) je rozložena přes **decaying schedule**:

| Dekáda | Block reward (ZION) | Kumulativní (mld) |
|--------|---------------------|-------------------|
| 1 | 5 400,067 | ~2,84 |
| 2 | 4 320,054 | ~5,11 |
| 3 | 3 456,043 | ~6,93 |
| 4 | 2 764,834 | ~8,38 |
| 5 | 2 211,867 | ~9,54 |
| 6 | 1 769,494 | ~10,47 |
| 7 | 1 415,595 | ~11,22 |
| 8 | 1 132,476 | ~11,81 |
| 9 | 905,981 | ~12,29 |
| 10+ (tail) | 724,785 (perpetual) | → 127,22 |

- Decay faktor: **4/5 (0,8)** za dekádu (5 256 000 bloků)
- Po 10 dekádách (~100 let): **perpetual tail emission** (724,785 ZION/blok)
- Tail emission zajišťuje **trvalou** miner incentivu

---

## 5. Fee split — konsenzově vynucené

Každý blok reward je **automaticky rozdělen**:

```
Block subsidy
    ├── 89 % → Miner (proof-of-work reward)
    ├── 5 %  → Humanitarian (Children Future Fund)
    ├── 5 %  → Issobella (komunita/L5)
    └── 1 %  → Burn (pool fee, deflační)
```

**Tohle je v kódu (`emission.rs`). Nikdo — ani admini, ani DAO — nemůže
změnit fee split.** Vyžaduje by hard fork (3-of-3 + DAO 75% + 90d).

---

## 6. wZION (ERC-20)

| Parametr | Hodnota |
|----------|---------|
| Název | Wrapped ZION |
| Ticker | wZION |
| Standard | ERC-20 |
| Decimals | 18 (EVM standard) |
| Chainy | Base, BSC, Polygon, Arbitrum, Optimism, Avalanche |
| Adresa | `0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6` (deterministic, všechny chainy) |
| Peg | 1:1 (1 ZION L1 = 1 wZION EVM) |
| Minting | Pouze přes bridge (lock ZION L1 → mint wZION) |
| Burning | Pouze přes bridge (burn wZION → unlock ZION L1) |

wZION **není** samostatný token — je to **wrapped reprezentace** ZION
na EVM chainech. Supply wZION = ZION locked v bridge vault.

---

## 7. Inflace

### 7.1 Mining inflace

- Rok 1: ~2,84B ZION mintováno (1,97 % total supply)
- Rok 10: ~12,29B ZION mintováno (8,53 %)
- Rok 100: ~127,22B ZION mintováno (88,35 %)
- Po roce 100: tail emission ~724,785 ZION/blok (perpetual, klesající %)

### 7.2 Deflační mechanismy

- **Fee burn** — 1 % pool fee spalována (každý blok)
- **TX fee burn** — všechny transakční fee spalovány na `zion1burn...dead`
- **Bridge burn** — wZION burn na EVM při unlock (nepřidává L1 supply)

### 7.3 Net inflace

Mining inflace > fee burn → **net inflace** v čase, ale klesající
(decaying reward). Po ~100 letech je inflace ~0,5 %/rok (tail emission).

---

## 8. Bridge vault

| Parametr | Hodnota |
|----------|---------|
| L1 adresa | `zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7` |
| Seed | `ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET` |
| Počáteční UTXO | 100 000 000 ZION (6 outputů) |
| Odemčení | 5/5 validator quorum (ne admin klíče) |

Bridge vault je **keyless** — odemyká se bridge konsenzem (validator
signatury), ne admin klíči. Žádný jednotlivec nemůže pohnout s vault
prostředky.

---

## 9. DAO treasury

| Slot | Adresa | Částka | Lock |
|------|--------|--------|------|
| 6 | `zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0` | 2,5B | blok 144 000 + admin + DAO |
| 7 | `zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6` | 1,0B | blok 144 000 + admin + DAO |
| 8 | `zion102s8k4k0w783d657j255z865e47054s342u87v3` | 0,5B | blok 144 000 + admin + DAO |

DAO treasury (4B ZION) je **trojí zámek**:
1. Time-lock: blok 144 000 (~100 dní)
2. Admin multisig: 3-of-3
3. DAO vote: quorum 15 %, 14d
4. Time-lock: 7d po schválení

**Daily spend limit: 50M ZION** (konzervativní start).

---

## 10. OASIS rewards

OASIS (5 slotů × 1,65B = 8,25B ZION) je **komunitní reward systém**:

- **Golden Egg/XP** — výherní ceny pro OASIS hráče
- **Admin-locked** — nelze utratit bez 3-of-3 + DAO
- **Žádné** tokeny pro tvůrce z OASIS (tvůrce má jen 590M lifetime rent)
- Distribuce přes DAO governance

---

## 11. Audit a ověření

### 11.1 Zdrojový kód

- **Open-source** (MIT) — každý může auditovat
- Genesis hash je **deterministický** — ověřitelný testem
- Premine adresy jsou **v genesis.rs** — veřejné
- Fee split je **v emission.rs** — veřejný

### 11.2 On-chain ověření

- **Blockchain explorer** — každá adresa, zůstatek, TX je veřejný
- **Basescan** — EVM kontrakty ověřeny (7/7)
- **Bridge** — lock/unlock TX veřejné na obou řetězcích

### 11.3 GPG signatury

- Genesis zpráva, genesis.md, creator statement, admin/Gen Z statement
  — vše **GPG podepsáno** tvůrcem
- Veřejný klíč: `docs/CREATOR_PUBKEY.asc`

---

## 12. Rizika tokenu

| Riziko | Popis |
|--------|-------|
| Volatilita | Hodnota může kolísat / klesnout na nulu |
| Likvidita | Nízká likvidita na malých burzách |
| Regulace | Vlády mohou zakázat/restrict |
| Bridge riziko | Cross-chain bridge jsou zranitelné |
- Smart contract riziko | EVM kontrakty mohou mít chyby |
| Konsenzus riziko | 51% útok na PoW síť |
| Key riziko | Ztráta klíče = ztráta prostředků |

Viz: [`LEGAL_DISCLAIMER.md`](./LEGAL_DISCLAIMER.md) §3 Rizika

---

## 13. Kontakt

| Kanál | Účel |
|-------|------|
| GitHub Issues | Technické otázky |
| `security@zionterranova.com` | Bezpečnost |
| `yose@zionterranova.com` (GPG) | Token/právní otázky |

---

*Tento dokument je **transparentní disclosure** tokenu ZION. ZION není
investice. Žádná záruka hodnoty. Konzultujte poradce před nákupem.*
