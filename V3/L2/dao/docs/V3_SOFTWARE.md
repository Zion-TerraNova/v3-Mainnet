# 🛠️ V3 Software — `zion-dao`

> Technická dokumentace L2 DAO daemonu. Crate implementuje decentralizovanou governance včetně treasury managementu, voting engine, proposal systému a timelock ochrany.

---

## Přehled architektury

```
┌─────────────────────────────────────────┐
│           zion-dao daemon              │
│              (Axum HTTP API)            │
├─────────────────────────────────────────┤
│  API  │  Treasury │  Voting │ Executor │
├─────────────────────────────────────────┤
│  Proposal │  Timelock │  Quorum │ L1 Scan│
├─────────────────────────────────────────┤
│  Humanitarian │  Prizes │  Metrics     │
├─────────────────────────────────────────┤
│  SQLite (rusqlite) │ Config (TOML)     │
└─────────────────────────────────────────┘
```

---

## Moduly

### `types.rs` — Core DAO Types

**Konstanty:**

| Konstanta | Hodnota | Popis |
|-----------|---------|-------|
| `DAO_TREASURY_ADDRESSES` | 3 adresy | Treasury na L1 (4B ZION) |
| `FLOWERS_PER_ZION` | 10⁶ | Atomická přesnost (updated 3.0.3 fork) |
| `DAO_TREASURY_TOTAL` | 4×10²¹ flowers | Celkový treasury |
| `PROPOSAL_THRESHOLD` | 1M ZION (flowers) | Min. balance pro návrh |
| `VOTING_PERIOD_SECS` | 604 800 (7 dní) | Hlasovací období |
| `TIMELOCK_SECS` | 172 800 (48h) | Timelock delay |
| `QUORUM_PERCENT` | 10.0 | Min. % oběžného množství |
| `MULTISIG_THRESHOLD` | 5 | Signatáři nutní |
| `MULTISIG_TOTAL` | 7 | Celkem signatářů |
| `DAILY_SPEND_LIMIT` | 100M ZION (flowers) | Denní limit výdajů |

**DAO Memo Protokol:**

```
Format: "DAO:<action>:<data>"

DAO:vote:42:yes         → Hlasovat ANO na návrh 42
DAO:propose:treasury    → Vytvořit treasury návrh
DAO:execute:42          → Exekuovat schválený návrh 42
```

**Types:**
- `VoteChoice` — Yes / No / Abstain
- `Guardian` — Multi-sig signer (name, address, pubkey, active)
- `VoterSnapshot` — Snapshot balance v okamžiku návrhu

### `proposal.rs` — Proposal Engine

**Typy návrhů:**

| Typ | Popis | Parametry |
|-----|-------|-----------|
| `Parameter` | Změna DAO parametrů | parameter_name, current_value, proposed_value |
| `Treasury` | Výdaj z treasury | recipient, amount, purpose |
| `Emergency` | Emergency akce | action, justification |
| `Grant` | Financování projektu | recipient, amount, milestones, duration |
| `Humanitarian` | Humanitární alokace | category, amount, justification |

**Status návrhu:**
- `Draft` — Příprava
- `Active` — Hlasuje se
- `Passed` — Schváleno, čeká na timelock
- `Timelocked` — V timelocku
- `Executed` — Provedeno
- `Rejected` — Zamítnuto
- `Cancelled` — Zrušeno

### `voting.rs` — Voting Engine

**Princip:**
- Token-weighted: **1 ZION = 1 hlas**
- Snapshot: Zůstatek je zamčen v okamžiku vytvoření návrhu
- Jeden hlas na adresu a návrh
- Váha = balance v atomických jednotkách

```rust
pub struct Vote {
    pub proposal_id: u64,
    pub voter: String,           // L1 adresa
    pub choice: VoteChoice,      // Yes / No / Abstain
    pub weight: u64,             // Balance at snapshot
    pub tx_hash: Option<String>, // L1 TX hash (DAO memo)
    pub voted_at: DateTime<Utc>,
}
```

### `treasury.rs` — Treasury Management

**Multi-sig model:**
- 5 z 7 signatářů nutných pro jakýkoliv výdaj
- Denní limit: 100M ZION
- Všechny výdaje vyžadují schválený + timelockovaný návrh

**Treasury operace:**
- `Spend` — Odeslání ZION na adresu
- `HumanitarianGrant` — Alokace humanitárních fondů
- `Rebalance` — Interní přesun mezi treasury adresami

**Revenue inflows:**
- 25 % WARP bridge fees
- 25 % L2 bridge fees
- 100 % BTC buyback (cosmic-harmony)

### `timelock.rs` — Timelock Ochrana

- **48hodinové zpoždění** pro všechny treasury operace
- Zabraňuje náhlým rozhodnutím
- Poskytuje čas na community review a případné veto
- Emergency návrhy: 12h timelock

### `quorum.rs` — Quorum Engine

- **10 %** z oběžného množství musí hlasovat
- Výpočet z L1 snapshotu
- Pokud quorum není dosaženo, návrh je zamítnut

### `executor.rs` — Proposal Execution

- Automatická exekuce po timelocku
- Kontrola: návrh musí být `Passed` + timelock vypršel
- Multi-sig transakce pro treasury operace
- Audit log všech exekucí

### `humanitarian.rs` — Humanitární Modul

- Sleduje 5 % block reward (humanitární desátek)
- Automatická alokace: 60 % Issobela / 40 % Hanuman
- Kategorie: vzdělání, zdraví, potraviny, technologie
- Reporting: čtvrtletní transparentní reporty

### `l1_scanner.rs` — L1 Blockchain Scanner

- Periodické skenování L1 pro DAO memos
- `DAO:vote:*` — zpracování hlasů
- `DAO:propose:*` — vytvoření návrhů
- `DAO:execute:*` — exekuce návrhů
- Sledování treasury balance na 3 DAO adresách

### `api.rs` — HTTP API (Axum)

**Endpointy:**

| Metoda | Cesta | Popis |
|--------|-------|-------|
| `GET` | `/health` | Healthcheck |
| `GET` | `/api/v1/proposals` | Seznam návrhů |
| `POST` | `/api/v1/proposals` | Vytvoření návrhu |
| `GET` | `/api/v1/proposals/:id` | Detail návrhu |
| `POST` | `/api/v1/proposals/:id/vote` | Hlasování |
| `POST` | `/api/v1/proposals/:id/execute` | Exekuce (po timelocku) |
| `GET` | `/api/v1/treasury` | Treasury balance |
| `GET` | `/api/v1/treasury/operations` | Historie operací |
| `GET` | `/api/v1/guardians` | Seznam guardianů |
| `GET` | `/api/v1/stats` | DAO statistiky |

### `metrics.rs` — Prometheus Metrics

- `zion_dao_proposals_total` — počet návrhů podle statusu
- `zion_dao_votes_total` — počet hlasů
- `zion_dao_treasury_balance` — zůstatek treasury (flowers)
- `zion_dao_humanitarian_allocated` — alokované humanitární fondy
- `zion_dao_l1_scans_total` — počet L1 scan cyklů
- `zion_dao_timelock_active` — aktivní timelocky

### `config.rs` — Konfigurace

```rust
pub struct DaoConfig {
    pub name: String,              // "zion-dao"
    pub bind: String,            // "0.0.0.0"
    pub port: u16,               // 8080
    pub db_path: String,         // "./dao.db"
    pub l1_rpc_url: String,      // "http://127.0.0.1:8443/jsonrpc"
    pub scan_interval_secs: u64, // 60
    pub api_key: String,
}
```

---

## Spuštění

### Lokálně

```bash
cargo run --manifest-path V3/Cargo.toml -p zion-dao
```

S custom konfigurací:
```bash
DAO_PORT=8080 DAO_L1_RPC=http://127.0.0.1:8443/jsonrpc \
  cargo run --manifest-path V3/Cargo.toml -p zion-dao
```

### Docker

```bash
docker compose -f V3/docker/docker-compose.yml up -d dao
```

Port: `8080`
Healthcheck: `GET /health`

### CLI

```bash
# Spuštění
cargo run --manifest-path V3/Cargo.toml -p zion-cli -- dao start

# Status
cargo run --manifest-path V3/Cargo.toml -p zion-cli -- dao status

# Nový návrh
cargo run --manifest-path V3/Cargo.toml -p zion-cli -- dao propose treasury
```

---

## Testy

### Integrační testy

```bash
cargo test --manifest-path V3/Cargo.toml -p zion-dao
```

| Test | Popis |
|------|-------|
| `test_parse_vote_memo` | Parsování DAO memo "DAO:vote:42:yes" |
| `test_parse_proposal_memo` | Parsování DAO memo "DAO:propose:treasury" |
| `test_constants` | Validace všech DAO konstant |
| `test_proposal_lifecycle` | Vytvoření → hlasování → exekuce |
| `test_treasury_multisig` | Multi-sig operace |
| `test_timelock_enforcement` | Timelock ochrana |

---

## Environment Variables

| Proměnná | Výchozí | Popis |
|------------|-----------|-------|
| `DAO_PORT` | `8080` | HTTP API port |
| `DAO_BIND` | `0.0.0.0` | Bind adres |
| `DAO_DB` | `./dao.db` | Cesta k SQLite |
| `DAO_L1_RPC` | `http://127.0.0.1:8443/jsonrpc` | L1 RPC URL |
| `DAO_API_KEY` | — | API klíč |

---

## Závislosti

- `tokio` — async runtime
- `axum` — HTTP web framework
- `rusqlite` — SQLite persistence
- `serde` + `serde_json` — serializace
- `chrono` — timestampy
- `uuid` — generování ID
- `tracing` — logging

---

## Relace k ostatním crate

| Crate | Vztah |
|-------|-------|
| `zion-core` (L1) | Block rewards, treasury addresses, DAO memos |
| `zion-bridge` (L2) | Bridge fee sharing (25 %) |
| `zion-warp` (L3) | Cross-chain fee sharing (25 %) |
| `zion-cosmic-harmony` | Revenue sharing (BTC buyback) |
| `zion-free-world` (L5) | Grant proposals, humanitarian fund |
| `zion-issobella` (L6) | Space mission proposals |
| `zion-cli` | Operator CLI |

---

## Multi-sig Guardian Seznam (Template)

| # | Jméno | Role | Adresa |
|---|-------|------|--------|
| 1 | Maitreya Buddha | Co-Admin | `zion1maitreya...` |
| 2 | Sarah Issabela | Co-Admin | `zion1sarah...` |
| 3 | Adam | Treasury Master | `zion1adam...` |
| 4 | Kryštof | Security | `zion1krystof...` |
| 5 | Anežka | Ethics | `zion1anezka...` |
| 6 | Max | Operations | `zion1max...` |
| 7 | Eliajah | Communications | `zion1eliajah...` |

---

*„ZION se stane prvním skutečně spirituálně-decentralizovaným blockchain."* 🌟
