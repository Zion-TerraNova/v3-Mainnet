# Zásady soukromí

> **Verze:** 1.0 — 2026-07-09
> **Platí pro:** ZION v3 mainnet, node software, CLI, SDK, pool, bridge, DAO

---

## 1. Princip

ZION je **decentralizovaná** blockchain síť. Neexistuje žádná centrální
databáze uživatelů, žádný KYC, žádná registrace.

**ZION nevybírá, neukládá, ani neprodává osobní údaje.**

---

## 2. Co je veřejné na blockchainu

Blockchain je **veřejný ledger** — následující data jsou viditelná všem:

| Data | Veřejné? | Poznámka |
|------|----------|----------|
| Adresy (`zion1...`) | **Ano** | Pseudonymní, ne přímo identifikující |
| Zůstatky | **Ano** | Viditelné pro každou adresu |
| Transakce (od, komu, částka, fee) | **Ano** | Včetně memo pole |
| Bloky (hash, výška, čas, miner) | **Ano** | Včetně coinbase adresy |
| Smart kontrakty (EVM) | **Ano** | Zdrojový kód + stav |
| Bridge operace (lock/unlock) | **Ano** | L1 TX + EVM TX |
| DAO návrhy a hlasy | **Ano** | Váha hlasu = zůstatek |

**Pozor:** Ačkoliv adresy jsou pseudonymní, **lze je spojit s identitou**
pokud:
- Zveřejníte adresu (např. na sociálních sítích)
- Pošlete z/na adresu centralizované burzy (KYC)
- Použijete adresu opakovaně (korelace)

---

## 3. Co ZION software nevybírá

| Data | Sbírá? | Poznámka |
|------|--------|----------|
| Jméno, email, telefon | **Ne** | Žádná registrace |
| IP adresa | **Ne** | P2P neukládá IP (mimo dočasné peer spojení) |
| Geolokace | **Ne** | Žádné sledování polohy |
| Osobní dokumenty (KYC) | **Ne** | Žádný KYC/AML proces |
| Platební údaje | **Ne** | Žádné fiat platby |
| Procházení historie | **Ne** | Žádné trackery |
| Cookies | **Ne** | Žádné cookies v node/CLI/SDK |

---

## 4. P2P síť

Při provozování node:

- **IP adresa** vašeho node je **viditelná** ostatním peerům (P2P port 8333)
- IP adresa peerů je **dočasně uložena** v peer manageru (pro rate limiting, ban)
- IP adresy **nejsou** odesílány žádnému centrálnímu serveru
- Peer spojení jsou **šifrována** (QUIC/TLS)

**Doporučení:** Pro ochranu soukromí použijte **VPN nebo Tor**.

---

## 5. RPC

- RPC je vázán na `127.0.0.1` (localhost) — **neveřejný**
- RPC neukládá IP adresy volajících (jen localhost)
- RPC audit log ukládá **typ operace a čas** (ne IP, ne identitu)

---

## 6. Pool

Pool (`pool.zionterranova.com:8444`) sbírá:

- **Miner adresu** (pro payout) — pseudonymní
- **Share data** (hash rate, čas) — pro PPLNS výpočet
- **Worker ID** (volitelné, nastaví miner) — neidentifikující

Pool **neukládá**: IP adresy (mimo dočasné spojení), email, jméno.

---

## 7. Website

Website (`zionterranova.com`) může používat:
- **Analytiku** (anonymní, bez cookies pokud není souhlas)
- **Maintenance page** (statická, žádné trackery)

Website **neukládá**: osobní údaje, platební informace, klíče.

**Nikdy nezadávejte** soukromý klíč na website. Wallet operace jsou
**lokální** (CLI/desktop app).

---

## 8. Bridge

Bridge operace jsou **veřejné** na obou řetězcích:
- L1: lock TX (adresa, částka, memo `BRIDGE:<chain>:<recipient>`)
- EVM: mint/burn TX (EVM adresa, částka)

**EVM adresa příjemce** je viditelná v memo poli na L1.

---

## 9. DAO

DAO hlasování je **veřejné**:
- Hlasující adresa je viditelná (v memo `DAO:vote:<id>:yes/no/abstain`)
- Váha hlasu = zůstatek na adrese (veřejný)

---

## 10. Vaše odpovědnost

- **Používejte unikátní adresy** pro každou transakci (zabraňuje korelaci)
- **Nedávejte** adresu na sociální sítě (pokud nechcete být identifikován)
- **Používejte VPN/Tor** pro provoz node (skryje IP)
- **Šifrujte** komunikaci (GPG pro email)

---

## 11. GDPR

ZION je **decentralizovaná** síť bez centrálního kontroléra. GDPR
"right to be forgotten" **nelze aplikovat** na blockchain data —
transakce jsou **neměnné**.

- Osobní údaje **nejsou** sbírány softwarem
- Pokud zveřejníte adresu spojenou s identitou, **nelze** ji smazat
- Za **anonymizaci** jste odpovědni vy (používejte nové adresy)

---

*ZION respektuje vaše soukromí. Síť je navržena tak, aby sbírala
minimum dat a žádné osobní údaje.*
