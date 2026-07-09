# ZION v3 — Genesis Blok

> **Genesis hash**: `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
> **Timestamp**: `1767225600` (2026-01-01 00:00:00 UTC)
> **Zdroj**: [`V3/L1/core/src/genesis.rs`](../V3/L1/core/src/genesis.rs)

---

## Přehled

Genesis blok ZION v3 (výška 0) je základní blok mainnet blockchainu. Byl
regenerován během **3.0.4 hard genesis resetu** (2026-07-06) po odhalení
a nápravě bezpečnostních zranitelností F1 a F5.

Genesis blok obsahuje:
- **14 premine outputů** v celkové hodnotě 16 780 000 000 ZION (11,65 % ze 144B supply)
- **Žádnou miningovou dotaci** (subsidy = 0 na výšce 0; premine je jediný coinbase)
- **13 account-model transakcí** + **1 UTXO transakci** (bridge vault)
- Vloženou **genesis zprávu** se signaturou tvůrce

### Hlavička bloku

| Pole | Hodnota |
|------|---------|
| Výška | 0 |
| Verze | 3 |
| Previous hash | `0000000000000000000000000000000000000000000000000000000000000000` |
| Timestamp | `1767225600` (2026-01-01 00:00:00 UTC) |
| Algoritmus | `deeksha_lite_v1` |
| Nonce | 0 |
| Template ID | 0 |
| Dotace (subsidy) | 0 ZION |
| Miner reward | 0 ZION |

### Hash

```
4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e
```

Tento hash je **deterministický** — je vypočítán z konstrukce genesis bloku
a ověřen testem `genesis_hash_is_deterministic` v `genesis.rs`. Všechny
nody se musí shodnout na této hodnotě. Jakýkoliv node, který vypočítá
odlišný hash, je na jiné větvi (forku).

---

## Genesis Zpráva

Genesis zpráva je vložena do tagu první premine transakce, v tradici
Bitcoinového `scriptSig` dědictví.

### Krátká forma (vložená do TX hashe)

```
ZION Mainet Launch v3 — For Sarah Issobel, Maitreya Buddha, Radha & Sita,
Meriam, Friends, Family, Freedom Humanity and all the children of this
world: ZION is yours. Build a better world where you reach for the Stars.
The Golden Age begins. Peace & One Love 4ever.
— Yose / Zion Creator
```

### Plná forma (s ASCII artem — Strom života)

Plná genesis zpráva obsahuje ASCII art Stromu života a logo ZION.
Je vložena při kompilaci přes `include_str!("GENESIS_MESSAGE.txt")`.

Zdroj: [`V3/L1/core/src/GENESIS_MESSAGE.txt`](../V3/L1/core/src/GENESIS_MESSAGE.txt)

```

⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⣀⢂⣁⣧⣖⡖⠠⢠⠀⠀⢤⡀⢀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢼⣶⡭⣛⠫⡞⠡⠀⡤⢦⠆⠨⠀⠀⢸⠋⠬⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡀⠀⠒⢈⠀⢭⣉⠂⡄⢠⠖⣸⠑⣆⡦⠊⢀⠀⡂⢉⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⠍⠚⣁⣀⡀⣤⣰⢶⢷⢼⣿⠏⡡⢠⢗⡙⣶⣞⠛⣍⣪⣼⡠⠠⢶⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⠀⠀⠀⢄⣎⡠⢠⠉⠋⠓⠉⠋⢨⠘⠚⢉⡄⠁⢾⡌⣗⢿⠛⠲⠛⠋⡝⠑⠀⠌⡤⠄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠘⠥⠄⡚⣜⢣⣴⡨⢁⡀⣈⡅⠀⣀⠀⠈⣄⣀⢿⣯⡔⢊⢺⣷⠆⣷⠶⠂⠀⠀⠀⢀⡀⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠘⢁⣨⡅⠨⣤⣭⣵⣿⢿⢏⠿⠯⡁⠹⣿⡯⡜⠫⢯⢿⡾⣻⡅⣠⣆⣄⣰⡐⠲⠼⢶⠒⠯⠅⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠂⢈⠙⡋⣟⡛⣷⠴⢼⠓⠋⣺⣴⣷⣷⢾⣿⡿⣡⣠⣸⠗⠻⠹⠿⣟⢥⠯⣿⠻⢅⢴⢎⠄⠀⡄⢠⣀⠀⡀⠀⢄⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⢘⠳⠋⣤⣶⡿⢜⣳⢦⢶⣌⣩⠶⢠⣤⣯⠷⠈⠬⡉⠎⠎⣀⡌⠟⣝⣿⠇⡚⠒⠔⢀⣴⣍⣾⢲⠋⠟⠈⠙⠑⠉⢀⠄⠀
⠀⠀⠀⡀⣽⠿⠻⡈⠱⢻⣽⡟⣶⣚⡻⢏⢹⡋⠁⣀⣂⣤⣴⠄⢤⣐⣴⡾⣶⠯⣄⣉⢓⡭⢍⡆⡀⣈⣿⣷⡷⠶⠒⢂⣠⣠⢶⣾⣳⣯⣵⡄
⠀⠀⠀⠰⠴⠀⢘⢉⣧⣥⣏⠳⢈⣫⠞⣿⣷⢤⣤⣿⣿⣾⣧⣾⣿⣿⣿⣗⣿⣿⣿⠋⣚⡃⠿⡭⠹⣷⣿⠾⡿⢤⣤⣜⢿⣯⡿⣷⠯⣽⣿⡾
⠀⠀⠀⠀⠀⠐⠞⠻⣿⢟⣿⢿⠷⠥⣼⣷⢷⣯⠟⠻⠙⢉⡿⣿⢻⣹⣿⣿⢉⢳⣿⣿⣯⡶⡄⡶⢦⣷⣶⣿⡬⢥⠨⣭⣹⠏⠁⡘⢫⠉⠈⠀
⠀⠀⠔⣼⢂⠬⢌⠧⢋⡛⢡⣮⡡⠈⠓⣃⢀⣒⣊⣽⠻⣛⠟⢿⢸⣯⣿⣓⣿⡟⣷⣟⣿⣿⣿⣿⣻⣷⣟⣒⡺⠏⢰⡿⠿⣶⣶⡻⠒⡿⠦⡀
⠀⢆⣀⣆⣸⣿⠋⡴⢲⡁⡋⠀⢴⣮⣷⠟⠫⠿⣿⢶⢅⢴⣇⣸⣷⣿⣿⣧⣾⣿⣿⣿⣿⣿⣿⣿⣿⢿⢿⣟⣲⢦⠦⢋⡀⢿⣾⣷⣶⣤⠋⠆
⠈⠘⠛⠼⠿⡝⣻⠛⠻⠀⠀⠐⠛⢹⣱⣟⣽⣯⣿⡟⡊⣿⣷⣖⢽⣿⣿⣿⢿⣿⠀⠀⠘⠋⠃⠁⠀⠀⠨⠟⠿⡷⣥⣉⠁⠘⠉⠊⠚⠚⠓⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⠋⠀⠀⠀⠀⠈⠋⠹⣎⢻⣿⠟⠀⠈⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠛⢳⡕⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⣿⣿⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣿⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣾⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢸⣹⡇⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠚⠛⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀



████████╗██╗ ██████╗███╗   ██╗
╚══███╔╝██║██╔═══██╗████╗  ██║
  ███╔╝ ██║██║   ██║██╔██╗ ██║
 ███╔╝  ██║██║   ██║██║╚██╗██║
███████╗██║╚██████╔╝██║ ╚████║
╚══════╝╚═╝ ╚═════╝ ╚═╝  ╚═══╝.  "Mainet Launch v3"


For Sarah Issobel, Maitreya Buddha, Radha & Sita, Meriam, Friends, Family, Freedom Humanity and all the children of this world: ZION is yours.
Build a better world where you reach for the Stars. The Golden Age begins.
Peace & One Love 4ever.

— Yose / Zion Creator | Hooray to the Egg ! Om Namo Hiranyagarbha & Ekam Deeksha ! Thx Kalki/AmmaBhagavan !
```

---

## Premine Alopace

Všech 14 premine outputů je **admin-locked** (vyžaduje 3-of-3 admin multisig
+ DAO hlasování pro odemčení). DAO Treasury outputy jsou navíc **time-locked**.

### OASIS + Golden Egg (5 slotů × 1,65B = 8,25B ZION)

| # | Adresa | Částka (ZION) |
|---|--------|---------------|
| 1 | `zion1n3t6v6w3m8g4v6q8g7h7j4j6f7s8q2m7g7un8u0` | 1 650 000 000 |
| 2 | `zion16854w6h7a800k6h8n052s0h4k2v625x0w0z2320` | 1 650 000 000 |
| 3 | `zion1j8s2d6s6f248j7z3m80676p6m074x2q5p5er3w2` | 1 650 000 000 |
| 4 | `zion155k300w6x726p4x0w473s704d5k35865r2q75z8` | 1 650 000 000 |
| 5 | `zion1y293r8c6l5p3u0y7j8q8366372t7y070n3rp5r8` | 1 650 000 000 |

**Účel**: OASIS platforma odměny + Golden Egg/XP výherní ceny.

### DAO Treasury (3 sloty = 4,0B ZION) — time-locked do bloku 144 000

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 6 | `zion1u5u7k43240d5l4d0x7q5m3c4a838z4k000cv3q0` | 2 500 000 000 | Komunitní governance (hlavní) |
| 7 | `zion1m8d235x268h8d887s036m8c3x7s356d3r37k6m6` | 1 000 000 000 | Granty & Bounties |
| 8 | `zion102s8k4k0w783d657j255z865e47054s342u87v3` | 500 000 000 | Ecosystem Bootstrap |

**Time-lock**: Blok 144 000 (~100 dní při 60s/blok).

### Infrastruktura (3 sloty = 2,59B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 9 | `zion1e8j5z6v8e4c6s5x7r0w7e2r673h8k3a6d4xx877` | 1 000 000 000 | Core Development Fund |
| 10 | `zion1f7z374q068r3p657m8z220v7y6k045q255xp2d3` | 1 000 000 000 | Síťová infrastruktura (P2P seed nody) |
| 11 | `zion1s2j5s2a6f5k740k4d8s2k3y8v0t8d4k0u6my2k0` | 590 000 000 | Genesis Creator — Lifetime Rent |

### Humanitární (1 slot = 1,44B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 12 | `zion10797m0k3u356f2l443r062d4e49665f6n20j6x0` | 1 440 000 000 | Children Future Fund — Humanitarian DAO |

### Bridge Seed (1 slot = 0,4B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 13 | `zion1p3y7w4z7d2m3j0f00657r354y4f3q5k6y8ca0g7` | 400 000 000 | EVM Bridge likvidita |

### Bridge Vault UTXO (1 slot = 0,1B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 14 | `zion1j53677g5k83030x3s2z2z644e7h07792q0u02t7` | 100 000 000 | Bridge Vault UTXO — EVM Bridge Unlock likvidita |

Tento output je **UTXO transakce** (ne account-model) s 6 outputy
pro vejení částky do `u64` limitů. Adresa je odvozena z
`BRIDGE_VAULT_SEED = "ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"`.

### Souhrn

| Kategorie | Sloty | Částka (ZION) | % z premine |
|-----------|-------|---------------|-------------|
| OASIS + Golden Egg | 5 | 8 250 000 000 | 49,2 % |
| DAO Treasury | 3 | 4 000 000 000 | 23,8 % |
| Infrastruktura | 3 | 2 590 000 000 | 15,4 % |
| Humanitární | 1 | 1 440 000 000 | 8,6 % |
| Bridge Seed | 1 | 400 000 000 | 2,4 % |
| Bridge Vault UTXO | 1 | 100 000 000 | 0,6 % |
| **Celkem** | **14** | **16 780 000 000** | **100 %** |

---

## Zamykací mechanismus

### Obouvrstvý zámek

Všechny premine outputy používají **obouvrstvý zámek**:

1. **Time-lock** (`unlock_height`): Bloková výška, která musí být dosažena.
   - DAO Treasury: blok 144 000 (~100 dní)
   - Všechny ostatní: bez time-locku (okamžitě po admin-odemčení)

2. **Admin-lock** (`admin_locked`): Vyžaduje 3-of-3 admin multisig + DAO hlasování.
   - Všech 14 outputů je admin-locked.
   - `admin_unlocked` closure kontroluje on-chain stav odemčení.

**Oba zámky musí být splněny.** Admin-locked adresa nemůže převést prostředky
ani po vypršení time-locku, dokud admin multisig + DAO hlasování ji neodemkne.

Viz: `is_premine_transfer_allowed()` v `genesis.rs`.

---

## Kanonické peněženky dotací

Tyto **nejsou** premine outputy — jsou to příjemci průběžné blokové dotace
(89/5/5/1 fee split). Přijímají mince z každého vytěženého bloku.

| Označení | Adresa |
|----------|--------|
| Humanitarian Subsidy (5 %) | `zion1e0u5q5s660k4m4a634p2c2v358r8g59564054z7` |
| Issobella Subsidy (5 %) | `zion1f7y7l5k678y0v408e8s654d2282346k375526t2` |
| Pool Fee Subsidy (1 %, spáleno) | `zion1062522x6a083x6r4d24303l5h20698z7j8qk433` |
| Default Miner (89 %) | `zion1d6m0h2r8m7k8k2d8n072y7j3j4m0254323vq0e3` |
| Pool PPLNS Payout | `zion1e4489793c5x2r0a0a4d8z7r4u5d6k0s4k3ht5m2` |

> Issobella, pool-fee, default-miner a pool-payout adresy jsou odvozeny
> deterministicky z UTF-8 labelů přes `crypto::canonical_address_for_label`
> (BLAKE3 → StdRng → Ed25519). Klíče jsou rekonstruovatelné z repa —
> dostatečné pro bootstrap / open custody. Operátoři vyžadující exkluzivní
> kontrolu by měli vygenerovat nové klíče a přepsat env vars.

---

## Ověření integrity genesis

Genesis hash je ověřen třemi deterministickými testy:

```
test genesis::tests::genesis_hash_is_deterministic ... ok
test genesis::tests::genesis_body_hash_is_deterministic ... ok
test launch::tests::frozen_genesis_hash_is_deterministic ... ok
```

Spuštění: `cargo test -p zion-core --lib genesis launch::tests::frozen`

Jakýkoliv node, který vypočítá odlišný genesis hash, je na forku a bude
sítí odmítnut.

---

## Signatura tvůrce

Genesis blok a tento dokument jsou podepsány tvůrcem ZION (**Yose**)
pomocí PGP/GPG (Ed25519). Signatura prokazuje autenticitu genesis bloku,
premine alokací a genesis zprávy.

### Klíč tvůrce

| Pole | Hodnota |
|------|---------|
| Jméno | Yose (Zion Creator) |
| Email | yose@zionterranova.com |
| Key ID | `9018F94ACE7C93CF549612E225557B7072678D25` |
| Algoritmus | EdDSA (Ed25519) |
| Subkey ID | `4AB36907442F7D5E34C6243B2331C8DF8E75E813` |
| Expirace | bez expirace |

### Ověření

```bash
# Import veřejného klíče tvůrce
gpg --import docs/CREATOR_PUBKEY.asc

# Ověření signatury genesis zprávy
gpg --verify docs/GENESIS_MESSAGE.txt.sig V3/L1/core/src/GENESIS_MESSAGE.txt

# Ověření tohoto dokumentu
gpg --verify docs/genesis.md.sig docs/genesis.md

# Ověření prohlášení tvůrce
gpg --verify docs/CREATOR_STATEMENT.txt
```

### Soubory v repozitáři

| Soubor | Popis |
|--------|-------|
| `docs/CREATOR_PUBKEY.asc` | Veřejný klíč tvůrce (PGP) |
| `docs/GENESIS_MESSAGE.txt.sig` | Detached signatura genesis zprávy |
| `docs/genesis.md.sig` | Detached signatura tohoto dokumentu |
| `docs/CREATOR_STATEMENT.txt` | Clearsigned prohlášení tvůrce |

> **Poznámka**: GPG privátní klíč je uložen na air-gapped stroji.
> Signatury byly vygenerovány 2026-07-09.

---

## Kontext 3.0.4 hard resetu

V3 genesis byl **regenerován** 2026-07-06 jako součást 3.0.4 hard genesis
resetu. Bylo to nutné kvůli:

1. **F1 exploit** — Padělané P2P account transakční signatury umožnily
   útočníkovi vytvářet falešné transakce. Náprava: vynucení ověření
   signatury u všech non-coinbase account transakcí.

2. **F5 exploit** — Nedostatečná validace zůstatku odesílatele umožnila
   neomezenou inflaci. Náprava: vynucení `sender_balance >= amount + fee`
   u všech account transakcí.

3. **Kompromitace serveru** — Přístup přes TeamViewer a exponované služby
   vyžadovaly kompletní přestavbu serveru se zpevněnou konfigurací.

Hard reset regeneroval všechny premine adresy, kanonické peněženky a
genesis hash. Předchozí genesis hash (`d28dc404...`) je **neplatný** a
patří kompromitovanému řetězci.

Viz: [`docs/security/SECURITY_DISCLOSURE_2026-07.md`](./security/SECURITY_DISCLOSURE_2026-07.md)

---

*— Yose / Zion Creator*
