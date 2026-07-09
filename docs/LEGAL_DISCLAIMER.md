# Právní disclaimer

> **Verze:** 1.0 — 2026-07-09
> **Platí pro:** ZION v3 mainnet, zdrojový kód, CLI, SDK, kontrakty, dokumentaci

---

## 1. Neinvestiční rada

ZION (**ZION TerraNova**) je **open-source softwarový projekt**, nikoliv
investiční produkt, cenný papír, ani finanční instrument.

- Tento repozitář, dokumentace, whitepaper a jakákoliv komunikace od
  tvůrců **nenabízí investiční radu**.
- ZION token není nabízen ani prodáván jako investice. Nebylo provedeno
  žádné ICO (Initial Coin Offering), IEO, ani veřejná nabídka tokenů.
- Premine (16,78B ZION) byl alokován na genesis bloku pro účely
  provozu sítě, komunity a humanitárních cílů — **nebyl prodán
  investorům**.
- Jakákoliv hodnota ZION na sekundárních trzích je výsledkem volného
  trhu a **není garantována** tvůrci ani administrátory.

**Nekupujte ZION s očekáváním zisku.** Hodnota může klesnout na nulu.

---

## 2. Žádná záruka

Software je poskytován pod **MIT licencí** ("tak jak je"), bez jakýchkoliv
záruk, ať výslovných nebo implicitních, včetně záruk obchodovatelnosti
nebo vhodnosti pro konkrétní účel.

Specificky **není garantováno**:

- Bezpečnost softwaru (může obsahovat chyby nebo zranitelnosti)
- Nepřetržitý provoz sítě (může dojít k výpadkům, forkům, reorgům)
- Dostupnost bridge, DAO, pool, ani jiných služeb
- Neměnnost protokolu (může dojít k hard forku po DAO schválení)
- Hodnota ZION tokenů
- Kompatibilita s budoucími verzemi

### 2.1 Mainnet Beta

ZION v3.0.4 aktuálně běží jako **Mainnet Beta**. To znamená:

- Síť je živá a produkuje bloky, ale **může obsahovat chyby**
- **Těžba je na vlastní nebezpečí** — odměny jsou reálné, ale síť neprošla plným bezpečnostním auditem
- Genesis blok a historie řetězce jsou trvalé — pokud síť projde bezpečnostním ověřením, **nebudou resetovány**
- Oficiální veřejný launch je plánován na **31. prosince 2026**
- Do té doby může dojít k hard forku, změnám parametrů, nebo dalším úpravám po DAO schválení

---

## 3. Rizika

Používání ZION sítě a softwaru nese **významná rizika**:

### 3.1 Technická rizika

- **Chyby v kódu** — smart kontrakty i L1 konsenzus mohou obsahovat chyby
- **Reorg** — blockchain může být reorganizován (max 10 bloků)
- **Fork** — síť se může rozdělit (hard fork, soft fork)
- **Ztráta klíčů** — ztráta soukromého klíče = ztráta přístupu k prostředkům
- **Bridge riziko** — cross-chain bridge jsou cílem útoků
- **51% útok** — PoW síť s nízkým hashrate je zranitelná

### 3.2 Ekonomická rizika

- **Volatilita** — hodnota ZION může kolísat
- **Likvidita** — trh může mít nízkou likviditu
- **Inflace** — mining emission zvyšuje supply (127,22B ZION za ~100 let)
- **Regulace** — vlády mohou zakázat nebo omezit používání

### 3.3 Operační rizika

- **Server výpadky** — nody, pool, bridge mohou přestat fungovat
- **Kompromitace** — viz bezpečnostní incidenty F1, F5, TeamViewer (2026)
- **Admin riziko** — 3 admini mohou udělat chybu (nejsou ale schopni mintovat)
- **DAO riziko** — komunita může schválit špatný návrh

### 3.4 Historické incidenty

ZION v3 již zažilo bezpečnostní incidenty:

| Incident | Datum | Dopad | Náprava |
|----------|-------|-------|---------|
| F1 (padělané signatury) | 2026-06-30 | 589M ZION | Rollback + hard reset |
| F5 (inflace) | 2026-07-02 | 100K ZION | Balance check + hard reset |
| TeamViewer kompromitace | 2026-07-03 | Všechny klíče | Kompletní hard reset |

Viz: [`docs/security/SECURITY_DISCLOSURE_2026-07.md`](./security/SECURITY_DISCLOSURE_2026-07.md)

---

## 4. Odpovědnost uživatele

**Používáte ZION na vlastní nebezpečí.**

Jako uživatel, miner, node operátor, nebo vývojář jste **samostatně
odpovědni** za:

- Zálohování soukromých klíčů (offline, paper, metal)
- Ověření integrity softwaru (GPG signatury, genesis hash)
- Dodržování lokálních zákonů (daně, KYC/AML, reporting)
- Zabezpečení vlastního hardwaru a sítě
- Porozumění rizikům před použitím

Tvůrci, administrátoři a přispěvatelé **neodpovídají** za žádné ztráty
přímé ani nepřímé způsobené používáním ZION.

---

## 5. Exemption — humanitární účel

ZION má **humanitární poslání** — 5 % každého bloku jde na Children
Future Fund. Tento humanitární aspekt **nemění právní status** projektu:

- ZION není registrovaná charita ani nadace
- Humanitární fondy jsou spravovány DAO governance (decentralizované)
- Darování na humanitární účely přes ZION není daňově uznatelné
  (konzultujte daňového poradce)

---

## 6. Kontakt

| Kanál | Účel |
|-------|------|
| `security@zionterranova.com` | Bezpečnostní hlášení |
| GitHub Issues | Technické otázky, bug reporty |
| `yose@zionterranova.com` (GPG) | Právní otázky, tvůrce |

---

*Používáním ZION softwaru, sítě, nebo tokenů potvrzujete, že jste si vědomi
všech rizik a souhlasíte s tímto disclaimerem.*
