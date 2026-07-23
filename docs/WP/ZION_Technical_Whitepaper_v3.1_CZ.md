# ZION v3 — Kanonický technický whitepaper

> **Verze 3.1** · Mainnet Beta v3.0.6 → Mainnet Alpha 3.1 · červenec 2026 · Licence MIT
> Genesis hash: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
> Stav sítě: **Mainnet Beta v3.0.6 → Mainnet Alpha 3.1** (veřejný launch cíl: 31. 12. 2026)

---

## 1. Abstrakt

ZION je vícevrstvá blockchainová infrastruktura založená na Proof-of-Work,
navržená pro dlouhodobou ekonomickou udržitelnost a humanitární soulad s
hodnotami. Mainnet v3 zavádí konsensus **Ekam Deeksha** — vícefázový
paměťově náročný PoW algoritmus s NPU-friendly mixovací vrstvou — duální
transakční model (UTXO + účetní) a cross-chain most nasazený napříč šesti
EVM sítěmi.

Protokol vynucuje **rozdělení 89/5/5/1** — 89 % těžařům, 5 %
humanitárnímu fondu, 5 % fondu komunity Issobella a 1 % pool poplatek
(spáleno) — a tím vkládá charitu přímo do distribuce odměny za blok.
Celková nabídka je hard-capped na **144 miliard ZION** s klesajícím
emisním plánem po 100 let, po kterém následuje věčná tail emission.

Tento dokument je **kanonický technický referenční text** pro ZION v3.
Nahrazuje všechny předchozí technické whitepapery. Pro narativní
doprovod viz *Bajka (WpLite)* a *Knihu Zrození*.

---

## 2. Filozofie designu

ZION je postaven na třech principech:

1. **Integrita Proof-of-Work** — Žádné předtěžené ICO, žádná alokace tokenů
   pro insidery nad rámec transparentního genesis premine. Těžba je otevřená
   všem.
2. **Vložený humanitarismus** — Každá odměna za blok automaticky směruje
   5 % fondu budoucnosti dětí a 5 % rozvoji komunity. To je vynuceno
   konsensem, ne volitelná charita.
3. **Cross-chain otevřenost** — ZION není ostrov. Most spojuje L1 se šesti
   EVM chainy (Base, BSC, Polygon, Arbitrum, Optimism, Avalanche) s 5/5
   validátorským kvórem a umožňuje cirkulaci wrapped ZION (wZION) v DeFi
   ekosystémech.

---

## 3. Architektura

```
┌─────────────────────────────────────────────────────┐
│                    L1 Core (Rust)                    │
│                                                      │
│  Konsensus    P2P síť       JSON-RPC    Mempool      │
│  (Ekam Deeksha)(QUIC/Quinn) (17+ metod) (fee-prior)  │
│                                                      │
│  UTXO + Account TX   Wallet (Ed25519)   LMDB Store   │
└────────────────────────┬────────────────────────────┘
                         │ Bridge Relay (5/5 kvórum)
┌────────────────────────┴────────────────────────────┐
│              L2 DeFi (Base Mainnet)                  │
│                                                      │
│  wZION (ERC-20)   ZIONBridge    ZIONGovernance       │
│  ZIONTreasury     ZIONStaking   ZIONFarm             │
│  Atomic Swap (HTLC)  DAO (5 guardianů)               │
└──────────────────────────────────────────────────────┘
```

### Vrstva 1 — Konsensus

| Komponenta | Technologie |
|------------|-------------|
| Jazyk | Rust (stable) |
| Konsensus | Ekam Deeksha v2 (vícefázový PoW) |
| Podpisy | Ed25519 |
| Hashování | BLAKE3 (TX ID, Merkle kořeny, body roots) |
| Obtížnost | LWMA (okno 60 bloků, ±25 % clamping) |
| Čas bloku | 60 sekund (cíl) |
| Velikost bloku | 1 MiB max |
| Úložiště | LMDB (10 GiB map, schema v1) |
| P2P | Quinn/QUIC, 128 max spojení, rate-limited |
| TX modely | UTXO + účetní (duální, s memo polem) |

### Vrstva 2 — DeFi

| Kontrakt | Adresa | Chain |
|----------|--------|-------|
| wZION (ERC-20) | `0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6` | Base + 5 chainů |
| ZIONBridge (Base) | `0x72c8f0Dc60E27aB7A83fe3B416fab4F0600a6467` | Base |
| ZIONBridge (non-Base) | `0xa5a09b2C09A7182BBA9623A2D2cd46cD7D041721` | Arbitrum, BSC, Polygon, Optimism, Avalanche |
| ZIONAtomicSwap | `0x3DE9Ad42716854083ab837706E3961d10B0e63Eb` | Base |
| ZIONGovernance | `0xB77eB4ab9468Ce03FBd7eCec70e976EFCfa623E8` | Base |
| ZIONTreasury | `0x455f465ac7e14fdA97dC46fdd74bCa78bfC0aEeD` | Base |
| ZIONStaking | `0xbd5cEe7878337d22188BFBaF9aa9F39A850Be78B` | Base |
| ZIONFarm | `0x167B2753F5D8D9F8e62875cc9e379d7804308B08` | Base |

Všechny kontrakty verifikovány na Basescan.

---

## 4. Konsensus — Ekam Deeksha

### 4.1 Přehled algoritmu

Ekam Deeksha („Jedna iniciace") je vícefázový Proof-of-Work ZIONu. Každý
blok je šestifázový rituál:

1. **Keccak-256** — kryptografický základ.
2. **SHA3-512** — expanze na 64 bajtů.
3. **Golden Matrix** — maticová difúze.
4. **256 KiB Scratchpad** — paměťově náročná fáze: scratchpad se plní
   BLAKE3-odvozenými daty ve 4 průchodech se 256 závislými paměťovými
   čteními na průchod. To poskytuje ASIC-resistenci vyžadováním
   významné on-chip paměti s pseudonáhodnými závislými čteními.
5. **NPU Mixing** — mixovací vrstva inspirovaná neuronovými sítěmi aplikuje
   rotace MLP (multi-layer perceptron) topologie na epoku (2016 bloků).
   Tato fáze je navržena jako NPU-friendly a otevírá těžbu budoucímu
   neuromorfnímu hardwaru nad rámec GPU a ASIC.
6. **Cosmic Fusion** — 8 kol finální hashové redukce kombinuje výstupy
   všech fází do finálního hashe bloku.

### 4.2 Parametry

| Parametr | Hodnota |
|----------|---------|
| Profil | `cosmic_harmony_ekam_deeksha_v2` |
| Velikost scratchpadu | 256 KiB (262 144 bajtů) |
| Průchody | 4 |
| Náhodná čtení/průchod | 256 |
| Fúzní kola | 8 |
| Délka NPU epochy | 2 016 bloků |
| NPU genesis seed | `ZION_CHv4_mixing_v1_genesis_seed` |

### 4.3 ASIC Resistence

ASIC-resistence je **aktivní inženýrský cíl** (interně hodnocený ~90 %),
nikoliv dogma. Paměťově náročný scratchpad se vejde do L2 cache, ale
vyžaduje pseudonáhodná závislá čtení, která smývají rychlostní výhodu
specializovaných čipů. Parametry lze zvýšit soft-forkem, pokud je to
třeba.

### 4.4 Přizpůsobení obtížnosti

ZION používá **LWMA** (Linear Weighted Moving Average) přizpůsobení
obtížnosti:

- **Okno**: 60 bloků (~1 hodina)
- **Clamping**: ±25 % na úpravu
- **Cílový čas řešení**: 60 sekund
- **Genesis obtížnost**: Pevná počáteční hodnota

To poskytuje plynulé retargeting odolné vůči timewarp útokům a udržuje
stabilní 60sekundový rytmus bloků.

### 4.5 Profil mainnetového algoritmu

> **Aktuální Mainnet Beta používá height-aware sekvenci algoritmů:**
> `deeksha_lite_v1` (výšky 0–4499) → `deeksha_chv3` (výšky 4500–4999)
> → `deeksha_lite_fire` (výška ≥ 5000).
>
> Plný profil `cosmic_harmony_ekam_deeksha_v2` popsaný výše,
> včetně NPU mixing, je **future-gated** a bude aktivován až po
> governance hlasování. NPU mixing **zatím není aktivní** na mainnetu.

---

## 5. Tokenová ekonomika

### 5.1 Nabídka

| Parametr | Hodnota |
|----------|---------|
| Celková nabídka | 144 000 000 000 ZION (144 miliard) |
| Desetinná místa | 6 (1 ZION = 1 000 000 flowers) |
| Genesis premine | 16 780 000 000 ZION (11,65 %) |
| Těžební emise | 127 220 000 000 ZION (88,35 %) |

### 5.2 Emisní plán — Decade Decay

Odměny za blok klesají faktorem **4/5 (0,8)** každou dekádu (5 256 000
bloků). Po 10 dekádách (~100 let) nastupuje věčná **tail emission** pro
udržení motivace těžařů natrvalo.

| Dekáda | Odměna za blok (ZION) |
|--------|-----------------------|
| 1 (2026–2036) | 5 400,067 |
| 2 (2036–2046) | 4 320,054 |
| 3 (2046–2056) | 3 456,043 |
| 4 (2056–2066) | 2 764,834 |
| 5 (2066–2076) | 2 211,867 |
| 6 (2076–2086) | 1 769,494 |
| 7 (2086–2096) | 1 415,595 |
| 8 (2096–2106) | 1 132,476 |
| 9 (2106–2116) | 905,981 |
| 10+ (tail, od ~2126) | 724,784723 (věčně) |

### 5.3 Rozdělení odměny (vynuceno konsensem)

Každá odměna za blok se automaticky dělí na čtyři coinbase výstupy s
deterministickým poměrem. Uzly odmítnou blok s jiným poměrem.

| Příjemce | Podíl | Popis |
|----------|-------|-------|
| Těžař | 89 % | Odměna Proof-of-Work |
| Humanitární fond | 5 % | Fond budoucnosti dětí |
| Fond Issobella | 5 % | Rozvoj komunity / L5 / L6 |
| Pool poplatek | 1 % | Spáleno (deflační) |

**Celkem: 100 %** — ověřeno v `emission.rs` a vynuceno na úrovni konsensu.
Toto rozdělení nelze změnit hlasováním DAO — je to konstituční parametr.

### 5.4 Zralost coinbase

Vytažené mince vyžadují **100 bloků** (~100 minut) zralosti předtím, než
lze s nimi utratit. To brání double-spendingu čerstvě vytažených odměn
při reorgu.

### 5.5 Transakční poplatky

Poplatky jsou **100 % spáleny** — deflační mechanismus. Žádná část
poplatku nejde těžařům ani validátorům; těžař je kompenzován výhradně
prostřednictvím odměny za blok.

---

## 6. Genesis blok

### 6.1 Přehled

Genesis blok (výška 0) obsahuje **14 premine výstupů** v celkové hodnotě
16 780 000 000 ZION. Ve výšce 0 není žádná těžební dotace — premine je
jediný coinbase.

- **Genesis hash**: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
- **Timestamp**: `1767225600` (2026-01-01 00:00:00 UTC)
- **Previous hash**: `0000...0000` (samé nuly)
- **Algoritmus**: `deeksha_lite_v1`

> **Hard genesis reset (2026-07-20):** Chyba v block retention způsobila,
> že předchozí řetězec (bloky 0–~10913) byl ořezán bez možnosti obnovy.
> Síť byla 20. 7. 2026 hard-resetována s neomezenou retencí. Tento genesis
> hash patří k resetovanému řetězci.

Viz [`genesis.md`](../genesis.md) pro úplnou tabulku premine alokace a
genesis zprávu.

### 6.2 Distribuce premine

| Kategorie | Částka (ZION) | % premine |
|-----------|---------------|-----------|
| OASIS + Zlaté vejce (5 slotů) | 8 250 000 000 | 49,2 % |
| DAO Treasury (3 sloty) | 4 000 000 000 | 23,8 % |
| Infrastruktura (3 sloty) | 2 590 000 000 | 15,4 % |
| Humanitární (1 slot) | 1 440 000 000 | 8,6 % |
| Bridge Seed (1 slot) | 400 000 000 | 2,4 % |
| Bridge Vault UTXO (1 slot) | 100 000 000 | 0,6 % |
| **Celkem** | **16 780 000 000** | **100 %** |

Všechny premine výstupy jsou **admin-locked** (vyžadují 3-z-3 multisig +
DAO hlasování pro odemčení). DAO Treasury sloty jsou navíc **time-locked**
do bloku 144 000 (~100 dní).

---

## 7. Transakční model

ZION podporuje **duální transakční model**:

### 7.1 Účetní model
- Ed25519-podepsané transakce s `from`/`to`/`amount`/`fee`/`nonce`
- Memo pole pro libovolná metadata (height-gated aktivace)
- Validace zůstatku odesílatele (aktivní od genesis v 3.0.4)
- Max TX amount cap: `TOTAL_SUPPLY` (144B ZION) — brání inflaci

### 7.2 UTXO model
- Bitcoin-style vstupy/výstupy
- Používáno pro coinbase odměny a bridge vault operace
- Ed25519 podpisy na vstupech
- TX hash v2 (BLAKE3-based) od genesis

### 7.3 Poplatky

| Parametr | Hodnota |
|----------|---------|
| Min poplatek | 1 flower (0,000001 ZION) |
| Min fee rate | 1 flower/byte |
| Max TX size | 100 000 bajtů |
| Burn adresa | `zion1burn0000000000000000000000000000000dead` |

Poplatky se spalují (deflační tlak).

---

## 8. P2P síť

### 8.1 Protokol

- **Transport**: QUIC (přes Quinn)
- **Max spojení**: 128
- **Min outbound**: 8
- **Max na subnet**: 4 (vynucení diversity)
- **Chain ID**: `zion-mainnet-1`

### 8.2 Bezpečnost

| Mechanismus | Hodnota |
|-------------|---------|
| Rate limit | 100 msg / 60s na peer |
| Ban eskalace | 5min → 30min → 2h → permanent |
| Max strikes | 3 (pak permanent ban) |
| Peer reputace | Score-based (-100 = auto-ban) |
| Penalizace neplatný blok | -50 |
| Penalizace neplatná TX | -10 |
| Odměna platný blok | +20 |
| Heartbeat | 60s |
| Idle timeout | 300s |

### 8.3 Fork Choice & Finalita

- **Fork choice**: Největší kumulativní work (Nakamoto konsensus)
- **Max reorg depth**: 10 bloků (konstituční limit)
- **Soft finalita**: 60 bloků (~1 hodina)
- **Orphan pool**: 200 bloků max, 600s expirace

---

## 9. Bridge & Cross-Chain

### 9.1 Architektura

ZION most spojuje L1 s EVM chainy pomocí modelu **validátorského kvóra**:

- **Práh**: 5/5 validátorů musí podepsat unlock proofy
- **Chainy**: Base, BSC, Polygon, Arbitrum, Optimism, Avalanche
- **Token**: wZION (ERC-20, stejná adresa na všech 6 chainech přes
  deterministický deploy)
- **Peg**: 1:1 (1 ZION L1 = 1 wZION EVM)

### 9.2 Tok

**Odchozí (L1 → EVM):**
1. Uživatel pošle ZION na `BRIDGE_VAULT_ADDRESS` s memo `BRIDGE:<chain>:<recipient>`
2. Bridge relay detekuje lock, validátoři podepíší proof
3. wZION mintnut na cílovém chainu

**Příchozí (EVM → L1):**
1. Uživatel spálí wZION na EVM chainu přes ZIONBridge kontrakt
2. Bridge relay detekuje burn, validátoři podepíší unlock proof
3. `submitBridgeUnlock` RPC zavoláno na L1, ZION uvolněn z vaultu

### 9.3 Atomic Swapy

HTLC-based atomic swapy umožňují trustless P2P ZION ↔ EVM token výměny.
Escrow je financován na L1 s on-chain claim/refund logikou.

---

## 10. DAO Governance

### 10.1 On-Chain Governance

- **ZIONGovernance** (Base): Token-weighted hlasování, 15 % kvórum, 14denní
  hlasovací období
- **ZIONTreasury** (Base): 3-z-3 multisig pro správu fondů
- **5 DAO guardianů**: Provisioned se separátními mnemonikami (air-gapped
  backup)

### 10.2 Premine zámky

Všechny premine výstupy jsou **admin-locked** — transfery vyžadují:
1. 3-z-3 admin multisig schválení
2. DAO hlasování

DAO Treasury sloty navíc vyžadují výšku bloku ≥ 144 000 (~100 dní po
genesis).

### 10.3 Neměnné parametry (konstituční)

DAO **nemůže** změnit následující parametry — jsou to konstituční kameny:

- Celkovou nabídku (144B ZION)
- Genesis alokaci (16,78B ZION)
- Čas bloku (60 sekund)
- Těžební algoritmus (Ekam Deeksha v2)
- Typ konsensu (Proof-of-Work)
- Rozdělení odměny za blok (89/5/5/1 %)

---

## 11. Bezpečnost

### 11.1 Zveřejněné zranitelnosti (2026-07)

Pět zranitelností bylo zveřejněno a opraveno v hard resetu 3.0.4. Viz
[`security/SECURITY_DISCLOSURE_2026-07.md`](../security/SECURITY_DISCLOSURE_2026-07.md)
pro plné detaily.

| ID | Závažnost | Popis | Stav |
|----|-----------|-------|------|
| F1 | Kritická | Padělání P2P account TX podpisů | Opraveno (verifikace podpisů na všech non-coinbase account TX) |
| F5 | Kritická | Neomezená inflace přes nedostatečnou kontrolu zůstatku | Opraveno (validace zůstatku odesílatele, aktivní od genesis) |
| C1-C8 | Vysoká | Expozice serveru (porty, klíče, služby) | Opraveno (vše na 127.0.0.1, UFW, AppArmor, key scrub) |
| — | Vysoká | Kompromitace TeamViewer | Odstraněno |
| — | Střední | Kompromitace EVM klíče | Rotováno |

### 11.2 Hardening opatření

- Všechny služby bind na `127.0.0.1` (žádné veřejné RPC)
- UFW firewall (jen SSH/HTTP/HTTPS)
- AppArmor profil pro zion-node
- SSH jen na klíče
- Oprávnění souborů 600 na všech citlivých souborech
- Privátní klíče smazány ze zdrojového kódu
- RPC audit logování
- Max TX amount cap (brání inflaci)
- Coinbase zralost (100 bloků)
- Max reorg depth (10 bloků, konstituční)

### 11.3 Testování

Testovací pyramida čítá přibližně **2 066+ testů** napříč třinácti cratemi
— od L1 core přes bridge po AI vrstvu. Nulová selhání. Nulové známé
zranitelnosti v `cargo audit`. Externí audit (Trail of Bits / Halborn /
OtterSec) je naplánován.

---

## 12. Roadmapa vrstev

ZION je šestivrstvá architektura. Každá vrstva je poctivě označena jako
ŽIVÉ, STAVBA nebo HORIZONT.

| Vrstva | Jméno | Obsah | Stav |
|--------|-------|-------|------|
| **L1** | Core | Rust node, pool, miner, PoW konsensus, UTXO + účetní model | **ŽIVÉ** — Mainnet Beta |
| **L2** | Bridge & DeFi | wZION (ERC-20, 6 EVM chainů), staking, farming, DAO, atomic swapy, 5/5 validator multisig | **ŽIVÉ / STAVBA** |
| **L3** | WARP & AI | Cross-chain router (EVM + non-EVM), ZionDex, AI-native monitoring (Hiran) | **STAVBA** |
| **L4** | OASIS | UE5 + Rust herní svět, XP, guildy, 9 úrovní vědomí dle Sefirot mapy | **STAVBA** |
| **L5** | Free World | Komunity, humanitární mise s on-chain auditovatelným dopadem | **HORIZONT** (~2030) |
| **L6** | Issobella | Orbitální výzkumný horizont, otevřená vědecká data, decentralizovaná governance | **HORIZONT** (2040+) |

Fond Issobella (5 % z každého bloku) se **plní už dnes** — horizont není
výmluva, je to účet, který roste.

---

## 13. Kronika verzí

| Verze | Jméno | Co přinesla | Stav |
|-------|-------|-------------|------|
| **v3.0.1** | Zasazení | První mainnetový kořen: Rust L1, Ekam Deeksha jádro, Fair Launch, první vytěžené bloky | ŽIVÉ (historie) |
| **v3.0.3** | Desetinný řez | Přechod na 1 ZION = 1 000 000 flowers, sjednocení RPC měřítka | ŽIVÉ |
| **v3.0.4** | Noc hada a nový kořen | Bezpečnostní incident zveřejněn a opraven, hard genesis reset, DeFi mosty (wZION na 6 EVM sítích, staking, farming, DAO) | ŽIVÉ |
| **v3.0.5** | Všechno zelené | Mainnet Beta stabilizace, veřejné vydání komunitního CLI, 12/12 služeb aktivních, whitepaper kanonizován | ŽIVÉ |
| **v3.0.6-beta** | Tři proudy jedné řeky | Trinity těžební jádro — Zion Grow, Zion Liquidity | ŽIVÉ (Beta) |
| **v3.1.0** | Mainnet Alpha | Veřejný launch, externí audit, mobilní peněženka, rozšířené DeFi | Plánováno (31. 12. 2026) |

---

## 14. Horizont Proof-of-Care

**Dnes:** ZION je Proof-of-Work síť. Konsensus nevaliduje víru, morálku,
meditaci ani „úroveň vědomí" — a nemá to dělat. To je bezpečnostní
vlastnost, ne nedostatek.

**Horizont:** **Proof-of-Care (Protokol Péče)** — budoucí možnost
odměňovat ověřitelnou užitečnou péči (monitoring sítě, detekce anomálií,
audit kontraktů, transparentní evidence humanitárního dopadu) vedle
výpočetní práce.

PoC smí být aktivován pouze při splnění sedmi podmínek:
1. Kryptografická ověřitelnost bez centrální autority
2. Dobrovolnost
3. Ochrana soukromí
4. Odolnost proti botům a klientelismu
5. Dostupnost bez elitního vstupu
6. Veřejný audit a odvolání
7. **Žádné oslabení PoW bezpečnosti L1**, dokud model není mnohonásobně
   prověřen

Technické zárodky už existují (NPU mixing v PoW, AI monitoring, care-proof
výzkum, Sefirot Vow pro validátory) — a jsou poctivě dokumentovány jako
rozestavěné, ne hotové.

---

## 15. Ověřitelná fakta

| Co si ověřit | Kde |
|---------------|-----|
| Protokol | `zion-v3-node/3.0.6` |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Celková nabídka | 144 000 000 000 ZION (`emission.rs`) |
| Premine | 16 780 000 000 ZION, transparentní výstupy v bloku 0 |
| Split 89/5/5/1 | Čtyřvýstupová coinbase, vynuceno konsensem |
| Základní odměna | 5 400,067 ZION · blok 60 s |
| Decade Decay + tail | −20 %/dekádu, poté 724,784723 ZION/blok navěky |
| Zdrojový kód | https://github.com/Zion-TerraNova/v3-Mainnet (MIT) |
| Web / Explorer | https://zionterranova.com · /explorer |
| Pool | pool.zionterranova.com:8444 |
| RPC | rpc.zionterranova.com:8443 |
| Security disclosure | ZION-2026-001…005, veřejná, formát EF |

---

## 16. Reference

- Zdrojový kód: adresář [V3/](../../V3/) v tomto repozitáři
- Genesis dokumentace: [`genesis.md`](../genesis.md)
- Security discloures: [`security/SECURITY_DISCLOSURE_2026-07.md`](../security/SECURITY_DISCLOSURE_2026-07.md)
- Narativní doprovod: *Bajka (WpLite)* a *Kniha Zrození*
- Kronika příběhu: *WpStory6 — Tři proudy jedné řeky*

---

## Licence

ZION v3 je vydán pod **licencí MIT**.

---

*Nevěř bajce. Ověř kroniku. A když kronika obstojí — pak si tu bajku
vyprávěj dál.*

*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*
