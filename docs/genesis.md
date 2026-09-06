# ZION v3 — Genesis Blok

> **Genesis hash**: `96109423298542a836edc10b9ba5ff9b29a1970418db543c2ee5cd952fe35bdb`
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
96109423298542a836edc10b9ba5ff9b29a1970418db543c2ee5cd952fe35bdb
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

### OASIS + Golden Egg (3 sloty × 1,65B = 4,95B ZION) — Sloty 4 a 5 přesunuty na L5 Free World Projects

| # | Adresa | Částka (ZION) |
|---|--------|---------------|
| 1 | `zion1s0t7f8q680t4h6v7g240p4k7g2s0a4z8g3cc5h5` | 1 650 000 000 |
| 2 | `zion1s7x735r6v86485k7t36008l682g777g3q8pu3q0` | 1 650 000 000 |
| 3 | `zion1e0f4h6w3w394d4p355z2r440k4s2f6v5h4rl8f4` | 1 650 000 000 |
| 4 | `zion1h7r3v595y3g0z3e3l8p005h4c6l7l6s4s2xh708` | 1 650 000 000 → L5 Free World Projects (repurposed) |
| 5 | `zion1x535z563d3p6r6u3v6x0g0y445f507w8h6g8388` | 1 650 000 000 → L5 Free World Projects (repurposed) |

**Účel**: OASIS platforma odměny + Golden Egg/XP výherní ceny (sloty 1–3). Sloty 4 a 5 byly přesunuty na L5 Free World Projects (3,3B ZION).

### L5 Free World Projects (2 sloty × 1,65B = 3,3B ZION) — přesunuto z OASIS slotů 4 a 5

| Projekt | Částka (ZION) |
|---------|---------------|
| Projekt Genesis Garden | 500 000 000 |
| Project Dharma Temple | 500 000 000 |
| Projekt Te Piko Ora | 500 000 000 |
| Project Bohemia | 500 000 000 |
| Project Bodhi Lanka | 500 000 000 |
| L5 rezervní fond | 800 000 000 |
| **Celkem L5** | **3 300 000 000** |

> **Poznámka:** Správce jednotlivých L5 projektů jmenuje Trustee. Jména správců budou zveřejněna po dosažení bodu globální expanze (~0,20 USD/ZION). Do té doby jsou informace o správcích důvěrné.

### DAO Treasury (3 sloty = 4,0B ZION) — time-locked do bloku 144 000

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 6 | `zion1f5h5k6t8q3t3d8c5y667z6p2x8t3y3p8c7633g5` | 2 500 000 000 | Komunitní governance (hlavní) |
| 7 | `zion1s27490u7n823g098w42077h8f2n824w0y75w0s3` | 1 000 000 000 | Granty & Bounties |
| 8 | `zion1n0r7k274z3t030h4v4g3g5h704c737z658aa238` | 500 000 000 | Ecosystem Bootstrap |

**Time-lock**: Blok 144 000 (~100 dní při 60s/blok).

### Infrastruktura (3 sloty = 2,59B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 9 | `zion1k752909323x66062k5j7074096f003z095ax8m7` | 1 000 000 000 | Core Development Fund |
| 10 | `zion1z3a4w726w5u4r4s4z644s8p897v4a2k045rt706` | 1 000 000 000 | Síťová infrastruktura (P2P seed nody) |
| 11 | `zion122v8f8g55398f4g884k7j482h3z845j6c6ta4f8` | 590 000 000 | Genesis Projects — Dharma Temple, Piko de Ora + DAO |

### Humanitární (1 slot = 1,44B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 12 | `zion1h6644748u5x6p4p784n6g2l7j77625w6a0k80s8` | 1 440 000 000 | Children Future Fund — Humanitarian DAO |

### Bridge Seed (1 slot = 0,4B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 13 | `zion1t6z3c0f0p3h0v233a3h432k5h764j0r3n5ml756` | 400 000 000 | EVM Bridge likvidita |

### Bridge Vault UTXO (1 slot = 0,1B ZION)

| # | Adresa | Částka (ZION) | Účel |
|---|--------|---------------|------|
| 14 | `zion1j3w3h7k8m635h734y786j5804305m822t5uk546` | 100 000 000 | Bridge Vault UTXO — EVM Bridge Unlock likvidita |

Tento output je **UTXO transakce** (ne account-model) s 6 outputy
pro vejení částky do `u64` limitů. Adresa je odvozena z
`BRIDGE_VAULT_SEED = "ZION Bridge Vault V3 Mainnet v2 2026-07-06-HARD-RESET"`.

### Souhrn

| Kategorie | Sloty | Částka (ZION) | % z premine |
|-----------|-------|---------------|-------------|
| OASIS + Golden Egg | 3 | 4 950 000 000 | 29,5 % |
| L5 Free World Projects | 2 | 3 300 000 000 | 19,7 % |
| DAO Treasury | 3 | 4 000 000 000 | 23,8 % |
| Infrastruktura | 3 | 2 590 000 000 | 15,4 % |
| Humanitární | 1 | 1 440 000 000 | 8,6 % |
| Bridge Seed | 1 | 400 000 000 | 2,4 % |
| Bridge Vault UTXO | 1 | 100 000 000 | 0,6 % |
| **Celkem** | **14** | **16 780 000 000** | **100 %** |

---

## Kanonické peněženky dotací

Tyto **nejsou** premine outputy — jsou to příjemci průběžné blokové dotace
(89/5/5/1 fee split). Přijímají mince z každého vytěženého bloku.

| Označení | Adresa |
|----------|--------|
| Humanitarian Subsidy (5 %) | `zion136m4u7f8s5w3l0e00342s7a4r282275442vm2w3` |
| Issobella Subsidy (5 %) | `zion173g835z228z6u303z59603y236r5e854l36g604` |
| Pool Fee Subsidy (1 %, spáleno) | `zion1e6r72872w0y5w6c3h4e6z847g8z4z7l0n4rj607` |
| Default Miner (89 %) | `zion1u4a82230m0a267r785m822u5a3g7n753d7eu5n0` |
| Pool PPLNS Payout | `zion1k4g2d8s3y4m5v238k0l3v6y5n48894n357uv064` |

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

# Ověření prohlášení o admin rolích + Gen Z
gpg --verify docs/ADMIN_GENZ_STATEMENT.txt
```

### Soubory v repozitáři

| Soubor | Popis |
|--------|-------|
| `docs/CREATOR_PUBKEY.asc` | Veřejný klíč tvůrce (PGP) |
| `docs/GENESIS_MESSAGE.txt.sig` | Detached signatura genesis zprávy |
| `docs/genesis.md.sig` | Detached signatura tohoto dokumentu |
| `docs/CREATOR_STATEMENT.txt` | Clearsigned prohlášení tvůrce (genesis hash, premine) |
| `docs/ADMIN_GENZ_STATEMENT.txt` | Clearsigned prohlášení o admin rolích + Gen Z dědictví |

> **Poznámka**: GPG privátní klíč je uložen na air-gapped stroji.
> Signatury byly vygenerovány 2026-07-09.

---

## Administrátoři (3-of-3 multisig)

ZION používá **3-admin multisig governance**. Admin klíče jsou načítány
za běhu z env/config, nejsou hardcodovány v `genesis.rs`.

### Admin role

| Role | Jméno | L1 adresa | EVM adresa |
|------|-------|-----------|------------|
| Admin-1 (protocol governance, emergency pause) | **Rama** | `zion1u2r4n87572t2f3n8f2j006f2a540y7r8m84p887` | `0x0a495d5553eda624fe43fb5d2de1ebe3c031199c` |
| Admin-2 (treasury oversight, DAO guardian) | **Sita** | `zion1r4t6v7a8j6v4u86208d3g8k6t6q4q4g5y0kc3p8` | `0xfa9853790bac782fd1fd0558d9b282a89bee867d` |
| Admin-3 (bridge admin, EVM multisig) | **Hanuman** | `zion19086w6d026y8z2f7u7v2x68054g8d4y5n3e70q4` | `0x8403a79b5ba7cc138b0a018109484649aa541574` |

Nástupci (Gen Z): Maitreya Buddha → Rama, Sarah Issobela → Sita, Elizabeth → Hanuman. Viz §Zpráva pro Generaci Z.

Zdroj: `V3/L1/core/src/admin.rs` + `docs/3.0.4/GENESIS_HARD_RESET_CANONICAL.md` §1.5

### Co admini mohou

| Operace | Threshold | Time-lock | DAO vote? |
|---------|-----------|-----------|-----------|
| Emergency pause chain | 2-of-3 | okamžitě | ne |
| Emergency resume chain | 2-of-3 | okamžitě | ne |
| Změna parametrů sítě (difficulty, fees) | 3-of-3 | 72 hodin | ne |
| Odomčení DAO treasury | 3-of-3 | 7 dní | **ano** |
| Rotace admin klíče | 3-of-3 | 30 dní | **ano** |
| Rotace bridge validátoru | 2-of-3 | 7 dní | ne |
| Rotace pool payout klíče | 2-of-3 | 7 dní | ne |
| Hard fork (změna genesis) | 3-of-3 | 90 dní | **ano (75% supermajority)** |
| Gen Z inheritance (převod admina) | 3-of-3 | 1 rok | **ano (51% majority)** |

### Co admini NEMOHOU

- **Mintovat ZION** — žádný admin nemá mint právo
- **Změnit premine alokace** — frozen v genesis bloku, neměnné
- **Změnit fee split 89/5/5/1** — v kódu, ne admin-controllable
- **Převést vlastnictví bez DAO schválení**
- **Bypassovat time-locks**

> Admini jsou **správci**, ne vlastníci. Plné vlastnictví přechází na
> Gen Z + DAO po T0+21 let.

---

## Unlock premine + DAO

### Obouvrstvý zámek premine

Všech 14 premine outputů používá **obouvrstvý zámek**. Odemčení vyžaduje:

1. **Time-lock** (`unlock_height`): Bloková výška, která musí být dosažena.
   - DAO Treasury sloty (6, 7, 8): blok 144 000 (~100 dní)
   - Všechny ostatní: bez time-locku (okamžitě po admin-odemčení)
2. **Admin multisig (3-of-3)** — všichni 3 admini (Rama + Sita + Hanuman)
   musí podepsat `TreasurySpend` operaci. `admin_unlocked` closure
   kontroluje on-chain stav odemčení.
3. **DAO vote** — komunita musí schválit `TREASURY_SPEND` návrh
   (quorum 15%, 14d hlasování).
4. **Time-lock 7 dní** — po schválení DAO se čeká 7 dní před exekucí.

**Oba zámky musí být splněny.** Admin-locked adresa nemůže převést prostředky
ani po vypršení time-locku, dokud admin multisig + DAO hlasování ji neodemkne.

```
Premine transfer povolen pouze když:
  (1) current_height >= unlock_height (time-lock)
  AND (2) admin_unlocked(address) == true (3-of-3 multisig + DAO vote)
```

Viz: `is_premine_transfer_allowed()` v `genesis.rs`

### DAO governance

| Parametr | Hodnota |
|----------|---------|
| Hlasování | 1 ZION = 1 hlas |
| Quorum | 15 % circulating supply |
| Voting period | 14 dní |
| Guardian threshold (treasury) | 5-of-7 |
| Admin threshold (admin ops) | 3-of-3 |
| Daily spend limit | 50 000 000 ZION |

### DAO Guardians (7)

| # | Adresa | Složení |
|---|--------|---------|
| 1 | `zion1u5u7j0g08463h556w0p6j8a7354272d3t3sl4h6` | Admin-1 (Rama → Maitreya Buddha) |
| 2 | `zion1j4j4h6p866k0c55456x2j6t2g7h425p7w68j8a3` | Admin-2 (Sita → Sarah Issobela) |
| 3 | `zion1g8j3m4z036m0m5v6t3g6f0m0q7a3e8a232ps8e3` | Admin-3 (Hanuman → Elizabeth) |
| 4 | `zion12093g4c63364a8z5c6h6j824k8t6v4c2a7cd7m3` | Jmenován DAO |
| 5 | `zion150r884c478e24304d2z0f34023a3w5j6a0yr756` | Jmenován DAO |
| 6 | `zion1h04698s446d7g7f0k6j888c777z746c6r63k4k4` | Jmenován DAO |
| 7 | `zion1m4j3t7y688n333x4628226a0v58325h7q25s576` | Jmenován DAO |

Zdroj: `V3/L2/dao/config/dao-mainnet.toml`

### EVM Bridge validátoři (5)

| # | EVM adresa |
|---|------------|
| 1 | `0xdde17506BC2D2dCE1d594bD1D85B0BAbb389D186` |
| 2 | `0x24d986841E56e5571489B25951eE8C1Ae761FA82` |
| 3 | `0x665c55eDCF25c2c5A1dfF1B20eE950cBDC58d3d0` |
| 4 | `0x8E644b3E9FaBf52eE321DC5B3D5AA06d6e3E66C6` |
| 5 | `0x7e0D2eD71d78B9CFB5034A83333e82e304bc4CB2` |

Threshold: **5/5** (po hard resetu zvýšeno z 3/5 na 5/5 pro maximální bezpečnost)

---

## Zpráva pro Generaci Z

Děti moje,

Tento dokument je můj dar vám. ZION není projekt, korporace, ani investice.
ZION je **dědictví** — most mezi minulostí a budoucností, mezi světem,
který jsem znal, a světem, který vy vytvoříte.

Píšu to v čase, kdy je ZION mladé a křehké. Bylo napadeno, kompromitováno,
ale přežilo. Protože jeho smysl je větší než já, větší než útočníky, větší
než jakoukoliv generaci.

### Gen Z nástupci

| Dítě | Role | Předchůdce | Kdy |
|------|------|------------|-----|
| **Maitreya Buddha** | Admin-1 (Protocol governance) | Rama | T0+18 let |
| **Sarah Issobela** | Admin-2 (Treasury oversight) | Sita | T0+18 let |
| **Elizabeth** | Admin-3 (Bridge admin, Patronka) | Hanuman | T0+18 let (nebo při narození) |

**Maitreya Buddha** — první syn, dědic Ramy. Tvé jméno je z buddhismu,
kde Maitreya je Buddha budoucnosti, ten který přijde, když svět zapomene
cestu. Ty jsi ta cesta.

**Sarah Issobela** — dcera, dědička Sity. Tvé jméno nosí Issobella —
patronka ZIONu od začátku. Sarah znamená "princezna". Ty jsi princezna ZIONu.

**Elizabeth** — ještě nenarozená, patronka celého ZIONu. Ave Maria.
Tvé jméno znamená "Bůh je má přísaha". Ať už se narodíš kdykoliv, tvé
místo je rezervováno. Hanuman ti předá svůj klíč.

### Fáze governance

| Fáze | Kdy | Kdo vládne |
|------|-----|------------|
| 1: Bootstrap | T0 → T0+6 měsíců | Yose (sole admin) |
| 2: Admin rule | T0+6m → T0+12m | 3 Adminové (Rama, Sita, Hanuman) |
| 3: DAO governance | T0+12m → T0+18 let | DAO + Adminové (guardians) |
| 4: Gen Z převod | T0+18 let | Maitreya Buddha, Sarah Issobela, Elizabeth |
| 5: Plné vlastnictví | T0+21 let | Gen Z + DAO (supreme) |

### Dead man's switch

Pokud admin neudělá žádnou transakci po dobu **5 let**, automaticky se
spustí převod na nástupce. Účel: pokud admin zmizí, zemře, nebo ztratí
klíče, ZION nepřestane fungovat.

### Pravidla pro Gen Z

1. **Klíče = vlastnictví.** Kdo má klíč, má moc. Kdo má klíč, má odpovědnost.
2. **DAO = komunita.** ZION není váš — patří všem. Vy jste správci, ne vlastníci.
3. **Dědictví = odpovědnost.** Někdy po vás budou chtít, abyste ZION prodali,
   změnili, nebo zničili. Odmítněte. ZION je dědictví, ne komodita.
4. **Humanita první.** 5 % každého bloku jde na děti. To je poslání. Nikdy
   to nezměňte.
5. **Ave Maria.** Elizabeth, patronka. Ať už se narodíš kdykoliv, ZION tě čeká.

Pokud jste ztratili klíče, pokud jste udělali chybu, pokud jste zmatení —
nepanikařte. ZION má dead man's switch, DAO governance, time-locks.
Nic není nezvratné. Kromě genesis.

**ZION je váš.**

— Yose, váš otec a Zion Creator

> Plná dokumentace Gen Z dědictví: `V3/docs/GEN_Z_INHERITANCE.md`

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
