# Jurisdikce a soulad s předpisy

> **Verze:** 1.0 — 2026-07-09
> **Platí pro:** ZION v3 mainnet, token ZION, bridge, DAO

---

## 1. Decentralizovaná síť

ZION je **decentralizovaná** blockchain síť bez centrálního kontroléra.

- Neexistuje žádná **právnická osoba** "ZION"
- Neexistuje žádná **centrální autorita** ani nadace
- Tvůrce (Yose) je fyzická osoba, nikoliv korporace
- Administrátoři (Rama, Sita, Hanuman) jsou **správci**, ne ředitelé
- DAO je komunitní governance — rozhodnutí jsou **kolektivní**

ZION jako síť **nemá jurisdikci** — je to open-source software běžící
na počítačích po celém světě.

---

## 2. Odpovědnost uživatelů

Každý uživatel, miner, node operátor, a vývojář je **samostatně odpovědný**
za dodržování zákonů své jurisdikce:

| Oblast | Odpovědnost |
|--------|-------------|
| Daně | Uživatel hlásí zisky/ztráty podle lokálních daňových předpisů |
| KYC/AML | Uživatel dodržuje AML povinnosti (zejména při fiat konverzi) |
| Reporting | Uživatel reportuje držbu kryptoměn podle lokálních pravidel |
| Licence | Node operátor ověří, zda nepotřebuje finanční licenci |
| Export | Vývojář ověří exportní omezení na kryptografický software |

**ZION neposkytuje právní ani daňové poradenství.** Konzultujte
kvalifikovaného právníka a daňového poradce.

---

## 3. Status ZION tokenu

### 3.1 Ne cenný papír

ZION token **není cenný papír** podle definice:

- **Nebyla provedena** veřejná nabídka (ICO/IEO)
- **Neexistuje** očekávání zisku z úsilí třetí strany (tvůrce neinzeruje zisk)
- **Neexistuje** společný podnik (investoři nebyli shromážděni)
- **Hodnota** je určena volným trhem, ne tvůrci
- **Token má utilitu** — platby za TX fee, mining, governance, bridge

### 3.2 Utility token

ZION je **utility token** s následujícími funkcemi:

- **TX fee** — platba za transakce (min. 1 flower = 0,000001 ZION)
- **Mining reward** — odměna za těžbu bloků
- **Governance** — 1 ZION = 1 DAO hlas
- **Bridge** — lock/unlock pro cross-chain převody
- **Staking** — wZION staking na Base (12% APR)

### 3.3 Ne komodita

ZION **není komodita** v právním smyslu — je to digitální token
s utility funkcí na decentralizované síti.

---

## 4. Regulace kryptoměn

ZION operuje jako **decentralizovaná síť**. Regulace se vztahují na
**interakce s fiat** (burzy, OTC, payment processors), nikoliv na
síť samotnou.

### 4.1 Burzy

Centralizované burzy, které listují ZION, jsou **samostatně odpovědné**
za:
- KYC/AML procesy
- Licenci v dané jurisdikci
- Reporting regulátorům
- Compliance s MiCA (EU), BSA (US), FCA (UK), atd.

ZION **neodpovídá** za činnost burz.

### 4.2 DEX

Decentralizované burzy (DEX) umožňující ZION trading jsou
**autonomní smart kontrakty** — ZION nekontroluje ani neprovozuje
žádný DEX.

### 4.3 Bridge

Bridge je **cross-chain protokol** — umožňuje převod ZION ↔ wZION
mezi L1 a EVM chainy. Bridge validátoři jsou **jmenováni adminy**
ale **nejsou** finanční zprostředkovatelé (neuchovávají fiat,
neposkytují investiční služby).

---

## 5. AML / CFT

### 5.1 Síť

ZION síť **neimplementuje** AML/KYC na protokolové úrovni — je to
decentralizovaná, pseudonymní síť (podobně jako Bitcoin).

### 5.2 Uživatelé

Uživatelé jsou odpovědni za:
- **Nepřijímání** prostředků z nelegální činnosti
- **Ověření** protistrany při velkých transakcích
- **Reporting** podezřelých transakcí (podle lokálních pravidel)

### 5.3 Sanctions screening

Node operátoři a pool operátoři by měli:
- **Ověřit**, zda nepřijímají prostředky ze sankcionovaných adres
- **Blokovat** interakce se sankcionovanými entitami (OFAC, EU sanctions)
- ZION **neobsahuje** vestavěný sanctions screening (decentralizovaná síť)

---

## 6. Daňové důsledky

### 6.1 Mining

- Mining reward je **příjem** (podle lokálních pravidel)
- Coinbase maturity: 100 bloků — reward je "přijat" při potvrzení
- Pool operátoři: PPLNS payout je příjem pro miner

### 6.2 Transakce

- Posílání ZION mezi vlastními adresami: obvykle **neudálost** (žádný daňový event)
- Posílání ZION třetí straně: může být **prodej** nebo **dar** (konzultujte poradce)
- Bridge lock/unlock: obvykle **neudálost** (stejný token, jiná reprezentace)

### 6.3 Staking

- wZION staking reward (12% APR na Base): **příjem** v okamžiku claim
- Daňové zacházení závisí na jurisdikci

**Konzultujte daňového poradce.** Tento dokument není daňovní rada.

---

## 7. Export kryptografie

ZION používá:
- **Ed25519** (L1 podpisy) — open-source, bez exportních omezení (RFC 8032)
- **BLAKE3** (hashing) — open-source, bez omezení
- **secp256k1** (EVM) — open-source, bez omezení
- **QUIC/TLS** (P2P) — standardní šifrování

Software **neobsahuje** vojenskou kryptografii ani omezené algoritmy.
MIT licence umožňuje volný export.

---

## 8. Ochrana spotřebitele

### 8.1 Transparentnost

ZION poskytuje **maximální transparentnost**:
- Veškerý zdrojový kód je **open-source** (MIT)
- Premine alokace jsou **veřejné** a neměnné (genesis blok)
- Fee split (89/5/5/1) je **v kódu**, ne změnitelný adminy
- Bezpečnostní incidenty jsou **veřejně zdokumentovány**
- GPG signatury tvůrce jsou **veřejné**

### 8.2 Návratnost

- Transakce jsou **nevratné** (po potvrzení) — to je vlastnost blockchainu
- **Žádný** admin, tvůrce, ani DAO nemůže vrátit transakci
- Emergency pause (2-of-3) **zastaví** nové bloky, ale nevrátí staré

### 8.3 Podpora

- **GitHub Issues** — technická podpora (komunita)
- **SECURITY.md** — bezpečnostní hlášení
- **Žádná** telefonní podpora, žádná garance odpovědi
- ZION je **community-driven** projekt

---

## 9. Budoucí regulace

Kryptoměnová regulace se vyvíjí (MiCA EU, SEC US, FCA UK, MAS Singapore).
ZION **bude adaptovat** na nové regulace prostřednictvím:

1. **DAO governance** — komunita může navrhnout compliance změny
2. **Admin multisig** — 3-of-3 může implementovat nové pravidla (s time-lock)
3. **Hard fork** — 3-of-3 + DAO 75% + 90d (pro zásadní změny)

ZION **neslibuje** compliance se všemi budoucími regulacemi — to je
odpovědnost každého uživatele a operátora.

---

## 10. Kontakt

| Kanál | Účel |
|-------|------|
| `security@zionterranova.com` | Bezpečnost |
| `yose@zionterranova.com` (GPG) | Právní otázky |
| GitHub Issues | Regulační diskuse |

---

*Tento dokument je informační, nikoliv právní rada. ZION je open-source
software. Konzultujte právníka pro vaši konkrétní situaci.*
