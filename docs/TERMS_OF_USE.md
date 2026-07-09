# Podmínky použití

> **Verze:** 1.0 — 2026-07-09
> **Platí pro:** ZION v3 mainnet software, CLI, SDK, node, pool, miner, bridge, DAO

---

## 1. Přijetí podmínek

Používáním ZION softwaru, provozováním node, miningem, nebo interakcí
s ZION sítí souhlasíte s těmito podmínkami. Pokud nesouhlasíte, **nepoužívejte
software ani síť**.

---

## 2. Open-source licence

ZION v3 je vydán pod **MIT licencí**. Viz [`LICENSE`](../LICENSE).

- Můžete: používat, kopírovat, modifikovat, distribuovat, sublicencovat, prodávat
- Musíte: zachovat copyright notice a licenci ve všech kopiích
- Software je "tak jak je" — bez záruk

Tato licence se vztahuje na **zdrojový kód**. Tokeny ZION, síťová účast
a DAO governance se řídí **konsenzem sítě**, ne softwarovou licencí.

---

## 3. Role a odpovědnosti

### 3.1 Uživatel (wallet)

- Jste výhradně odpovědni za **bezpečnost soukromých klíčů**
- Ztráta klíče = **nevratná ztráta** přístupu k prostředkům
- Žádný admin, tvůrce, ani DAO nemůže obnovit ztracené klíče
- Transakce jsou **nevratné** (po potvrzení v bloku)

### 3.2 Node operátor

- Jste odpovědni za **konfiguraci a zabezpečení** vlastního node
- Node musí běžet s **stejným genesis hashem** (`4f75a0df...`)
- Node na jiném forku bude sítí **odmítnut**
- RPC by mělo být vázáno na `127.0.0.1` (neveřejné)
- P2P port (8333) je veřejný — zodpovídáte za firewall
- Jste odpovědni za **dodržování lokálních zákonů**

### 3.3 Miner

- Mining je **otevřený všem** (žádné povolení není potřeba)
- Mining reward: 89 % miner + 5 % humanitarian + 5 % issobella + 1 % burn
- Coinbase maturity: **100 bloků** před utracením
- Pool operátoři jsou odpovědni za **vlastní pool konfiguraci**
- Pool fee (1 %) je **spalována** (deflační)

### 3.4 Bridge validátor

- Validátoři jsou **jmenováni adminy** (2-of-3 threshold pro rotaci)
- Threshold: **5/5** pro unlock operace
- Validátor je odpovědný za **zabezpečení EVM klíče**
- Kompromitace klíče → okamžitá rotace (7d time-lock)

### 3.5 DAO guardian

- Guardiani jsou **jmenováni DAO** (3 admini + 4 komunitní)
- Threshold: **5-of-7** pro treasury operace
- Guardian je odpovědný za **zabezpečení Ed25519 klíče**
- Daily spend limit: 50M ZION

### 3.6 Vývojář

- Příspěvky do repozitáře podléhají [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- Příspěvky jsou licencovány pod **MIT**
- Tvůrce si vyhrazuje právo **odmítnout** příspěvky
- Hard fork vyžaduje **3-of-3 admin + DAO 75% supermajority + 90d time-lock**

---

## 4. Zakázané činnosti

Používáním ZION souhlasíte, že **nebudete**:

- **Falšovat transakce** nebo signatury
- **Zneužívat zranitelnosti** (místo toho nahlaste přes SECURITY.md)
- **Provádět 51% útok** nebo jiné útoky na konsenzus
- **Prát špinavé peníze** (AML/KYC povinnosti platí)
- **Financovat terorismus** nebo nelegální aktivity
- **Spamovat** P2P síť nebo mempool
- **Zneužívat bridge** pro double-spend nebo arbitráž exploity
- **Zneužívat DAO** pro sybil útoky nebo manipulaci hlasování

Porušení může vést k **banu** (P2P), **freeze** (admin multisig), nebo
**právnímu postihu** (podle lokální jurisdikce).

---

## 5. DAO governance

DAO je **decentralizovaná** — rozhodnutí komunity jsou závazná pro síť.

- 1 ZION = 1 hlas
- Quorum: 15 % circulating supply
- Voting period: 14 dní
- Time-locks: 72h (parametry), 7d (treasury), 30d (admin rotace), 90d (hard fork)

**Tvůrci a admini nemohou zvrátit** DAO schválený návrh (kromě emergency pause
2-of-3, což vyžaduje 2 admina).

---

## 6. Změny podmínek

Tyto podmínky mohou být aktualizovány:
1. **DAO návrh** (TREASURY_SPEND nebo PARAMETER_CHANGE)
2. **Admin schválení** (3-of-3)
3. **Time-lock** (min. 72h)
4. **Veřejná konzultace** (GitHub Issues)

Změny budou oznámeny v repozitáři a na webu.

---

## 7. Omezení odpovědnosti

V maximálním rozsahu povoleném zákonem:

- Tvůrci, administrátoři, přispěvatelé a DAO guardiani **neodpovídají**
  za žádné ztráty způsobené používáním ZION
- Žádná ustanovení nevytvářejí **zaměstnanecký, partnerský, ani agenturní**
  vztah mezi uživateli a tvůrci
- ZION není **právnická osoba** — je to open-source software a decentralizovaná síť

---

*Používáním ZION potvrzujete, že jste si přečetli a porozuměli těmto podmínkám.*
