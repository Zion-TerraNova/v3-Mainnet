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
| 11 | `zion1s2j5s2a6f5k740k4d8s2k3y8v0t8d4k0u6my2k0` | 590 000 000 | Genesis Projects — Dharma Temple, Piko de Ora + DAO |

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
| Admin-1 (protocol governance, emergency pause) | **Rama** | `zion1m300z2f424k4m0k6c4l0v6v6w8l6j855s7je6e4` | `0xf354ccae30d6e9787e23e987e893e825f312f5c9` |
| Admin-2 (treasury oversight, DAO guardian) | **Sita** | `zion1d7z398t0n5c7j874a5n8v4h0d5c8j754z78t7m6` | `0x07e720245cdabc33a265df5bcdc504897ddf0b01` |
| Admin-3 (bridge admin, EVM multisig) | **Hanuman** | `zion1a363k2y366f6w4z2n2q4h2y822f3s5w2w56y3y4` | `0x9ab8ee6b874578e431aeb45bf28f8ca6041e1de6` |

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
| 1 | `zion1v330m245u4j2v6z8t485c8f472f8u5z3a82q0y4` | Admin-1 (Rama → Maitreya Buddha) |
| 2 | `zion186r522w0l538v030r0m297w43426z4v094lu5e8` | Admin-2 (Sita → Sarah Issobela) |
| 3 | `zion1g40723c645s038p0w7t8h0d7r8d325j7x0gc8j0` | Admin-3 (Hanuman → Elizabeth) |
| 4 | `zion1r3m3g8q6y4u2f8r4y2w4c3f02335d8j7v5dy064` | Jmenován DAO |
| 5 | `zion1u53766x73897r0z0z854c4p2f7v773g3e0z27v7` | Jmenován DAO |
| 6 | `zion144r475y5u58508y7f0a8d4g5c3a593m5q23e3a2` | Jmenován DAO |
| 7 | `zion1d8t2e3e3l3a684l578d894w5k8x2h2k3z6e63m7` | Jmenován DAO |

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
