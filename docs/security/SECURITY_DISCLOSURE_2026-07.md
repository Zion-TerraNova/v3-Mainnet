# ZION Network — Security Disclosure Bulletin

**Bulletin ID:** ZION-SEC-2026-07  
**Published:** 2026-07-06  
**Last Updated:** 2026-07-08  
**Author:** ZION Core Team  
**Status:** ACTIVE — hard reset complete, security patch 3.0.4 deployed

---

## Executive Summary

Between 2026-07-02 and 2026-07-03, the ZION V3 mainnet experienced a series of security incidents that compromised the integrity of the production Edge server, all cryptographic key material, and the EVM bridge deployment infrastructure. This bulletin discloses all known vulnerabilities, the timeline of events, remediation steps taken, and the decision to perform a full hard genesis reset.

**No external user funds were at risk.** ZION is pre-launch: the only participants are the core development team, and no tokens have been distributed to any third party. The chain had no external users, no exchange listings, and no liquidity on any DEX. All premine funds remain under sole developer control.

This disclosure follows the transparency principles established by the [Ethereum Foundation Public Disclosures](https://github.com/ethereum/public-disclosures/) program and the [Go Ethereum vulnerability disclosure policy](https://geth.ethereum.org/docs/developers/geth-developer/disclosures).

---

## Table of Contents

1. [Disclosure Policy](#1-disclosure-policy)
2. [Vulnerability Catalogue](#2-vulnerability-catalogue)
3. [Incident Timeline](#3-incident-timeline)
4. [Root Cause Analysis](#4-root-cause-analysis)
5. [Remediation & Hard Reset](#5-remediation--hard-reset)
6. [What We Are Publishing](#6-what-we-are-publishing)
7. [What We Are NOT Publishing](#7-what-we-are-not-publishing)
8. [Source Code Publication Plan](#8-source-code-publication-plan)
9. [Lessons Learned](#9-lessons-learned)
10. [Future Security Measures](#10-future-security-measures)
11. [Contact & Responsible Disclosure](#11-contact--responsible-disclosure)

---

## 1. Disclosure Policy

### Principles

ZION follows a **full disclosure after remediation** policy:

1. **Fix first, disclose second.** Vulnerabilities are patched and deployed before public details are released.
2. **90-day disclosure window.** Consistent with the Ethereum Foundation's policy, vulnerabilities are publicly disclosed within 90 days of the patch being deployed.
3. **Immediate disclosure for exploited vulnerabilities.** If a vulnerability was actively exploited (even by the development team accidentally), details are published as soon as the fix is confirmed deployed.
4. **No silent patches for consensus bugs.** Unlike Go Ethereum's policy of silent patches for network-health vulnerabilities, ZION discloses all consensus-level bugs fully. Rationale: ZION is pre-launch with a single-operator topology — there is no network of independent operators who might be at risk from premature disclosure.

### Scope

This disclosure covers:
- L1 consensus code (`V3/L1/core/src/`)
- L2 bridge, DAO, and atomic swap services (`V3/L2/`)
- L3 WARP cross-chain bridge (`V3/L3/warp/`)
- EVM smart contracts (wZION, ZIONBridge, ZIONGovernance, ZIONTreasury, ZIONStaking, ZIONFarm)
- Server infrastructure (Edge VPS)
- Cryptographic key material

---

## 2. Vulnerability Catalogue

A machine-readable JSON catalogue is available at [`vulnerabilities.json`](./vulnerabilities.json).

### ZION-2026-001: Forged Account Transaction via P2P (F1)

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-001 |
| **Severity** | HIGH (CVSS 8.1) |
| **Type** | Consensus — missing signature verification on P2P path |
| **Introduced** | v3.0.0 (account model introduction) |
| **Fixed** | v3.0.4 (commit `9341344d`) |
| **Deployed** | 2026-07-02 |
| **Exploited** | YES — forged account TX from external IP `109.81.30.165`, chain rolled back to height 22180 |

**Description:** The `validate_peer_block()` function in `V3/L1/core/src/peer_block_validation.rs` did not call `verify_signature()` for account-model transactions received via the P2P network. An attacker could forge a transaction from any account address without possessing the corresponding Ed25519 private key. UTXO transactions were not affected (they validate inputs via `validate_inputs_exist()`).

**Impact:** Attacker could spend any account's balance by forging P2P-propagated transactions. One forged transaction was observed from IP `109.81.30.165:57101`. The chain was rolled back to block 22180 and the fix deployed.

**Fix:** Added `verify_signature()` call to the account-model TX validation path in `validate_peer_block()`.

---

### ZION-2026-002: Account Model Balance Validation Bypass (F5)

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-002 |
| **Severity** | CRITICAL (CVSS 9.8) |
| **Type** | Consensus — missing balance check enables unlimited inflation |
| **Introduced** | v3.0.0 (account model introduction) |
| **Fixed** | v3.0.4 (commit `69d12c7`) |
| **Deployed** | 2026-07-02, activated at block 22394 |
| **Exploited** | YES — accidentally during escrow key rotation; 100,002 ZION created from empty address, subsequently burned |

**Description:** Neither the RPC submission path (`insert_transaction()`) nor the P2P validation path (`validate_peer_block()`) checked whether the sender had sufficient balance to cover `amount + fee` for account-model transactions. Any Ed25519 key holder could create ZION from nothing by submitting a transaction from a zero-balance address. This is a classic "account model without balance check" bug, analogous to early Ethereum consensus vulnerabilities.

The UTXO model is inherently safe — `validate_inputs_exist()` and `validate_value_conservation()` ensure conservation. The account model relied on application-layer balance display but **never enforced it at the consensus layer**.

**Impact:** Unlimited inflation. Any participant with an Ed25519 keypair could mint arbitrary ZION. One accidental exploitation occurred during escrow key rotation: 100,002 ZION created from a placeholder address `zion1s2g3...` (0 balance). The inflationary funds were burned to a provably-unspendable address `zion1n3570...` (derived from `[0xFF; 32]`, not a valid Ed25519 public key) in block 22362.

**Fix:** Added `account_balance_for()` helper to ChainState that computes confirmed balance minus pending mempool debits. Both RPC and P2P paths now reject transactions where `sender_balance < amount + fee`. Height-gated via `ZION_BALANCE_CHECK_HEIGHT` environment variable to avoid rejecting pre-fix blocks during IBD. 5 fuzz tests added (commit `a5472ec6`).

**Full technical report:** [`F5_SECURITY_INCIDENT_REPORT_2026-07-02.md`](../../F5_SECURITY_INCIDENT_REPORT_2026-07-02.md)

---

### ZION-2026-003: Edge Server Compromise via TeamViewer (Infrastructure)

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-003 |
| **Severity** | CRITICAL (CVSS 10.0) |
| **Type** | Infrastructure — remote access tool compromise |
| **Introduced** | N/A (operational) |
| **Fixed** | Hard genesis reset (in progress) |
| **Deployed** | Mitigation: 2026-07-02; Full remediation: pending (new server) |
| **Exploited** | YES — attacker had root access for ~47 minutes |

**Description:** The development machine (Windows PC) was compromised via TeamViewer. The attacker gained access to:

- SSH keys to the Edge production server (Hetzner VPS, `<LEGACY_EDGE>`)
- Root shell on Edge for approximately 47 minutes
- Pool payout signing key (Ed25519 secret key in plaintext in 3 shell scripts committed to git)
- Bridge escrow key
- EVM deployer/admin private key (`0xdde17506...`)
- DAO guardian mnemonics (5 of 5)
- Full source code repository

**Impact:** Complete compromise of all cryptographic key material. The attacker could:
- Drain any wallet whose secret key was stored on the server
- Forge bridge validator signatures
- Admin-control all 7 EVM smart contracts on 6 chains
- Execute DAO governance proposals
- Access and modify the running blockchain state

**Mitigation (immediate):**
1. Tailscale VPN disconnected (`tailscale down`)
2. UFW firewall locked to SSH/HTTP/HTTPS only
3. All services bound to `127.0.0.1` (no external exposure)
4. Private keys scrubbed from 5 files in git history
5. SSH changed to key-only authentication
6. AppArmor profile enforced for `zion-node`
7. 3 monitoring cron jobs installed
8. Website put in maintenance mode

**Full remediation:** Complete hard genesis reset with new keys on a fresh server. See Section 5.

---

### ZION-2026-004: Server Security Misconfiguration (C1-C8)

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-004 |
| **Severity** | HIGH (aggregate) |
| **Type** | Infrastructure — multiple server misconfigurations |
| **Introduced** | Initial Edge deployment |
| **Fixed** | Partially (2026-07-02); fully with new server |
| **Deployed** | Mitigations 2026-07-02 |
| **Exploited** | Unknown — server was accessible, but no direct exploitation evidence besides TeamViewer pivot |

**Description:** The Edge production server had 8 critical misconfigurations:

| # | Issue | Risk |
|---|-------|------|
| C1 | UFW allowed ports 8333/8334/8443-8447/8450/8452/8453/8455/8766-8768/8888/9102 from any IP | P2P, RPC, pool, L2 services exposed to internet |
| C2 | External P2P connection from `109.81.30.165` active | Attacker injected blocks (F1 exploit vector) |
| C3 | Pool payout secret key in plaintext in 3 git-tracked shell scripts | Key compromise |
| C4 | `edge-environment.sh` world-readable (mode 644) | Any server user could read secret keys |
| C5 | DB files world-readable (mode 644) | Chain state + bridge keys readable by any user |
| C6 | Tailscale ACL not configured | Any device on tailnet had full access |
| C7 | Stale cron job with `MEMO_V1_HEIGHT=24000` override | Could reset signature verification activation |
| C8 | Hardhat `.env` and Docker `.env` with EVM private keys | DeFi contract compromise |

**Impact:** The combination of these issues created the attack surface that enabled ZION-2026-003. Any single misconfiguration alone would have been high-risk; together they represent a systemic failure to follow production security baselines.

**Fix:** All mitigations applied 2026-07-02 (see [`SecurityFirst.md`](../../SecurityFirst.md)). Full remediation via new server deployment with hardened configuration from the start.

---

### ZION-2026-005: EVM Contract Admin Key Compromise

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-005 |
| **Severity** | HIGH (CVSS 8.5) |
| **Type** | Smart Contract — compromised deployer/admin key |
| **Introduced** | 2026-06-29 (DeFi deploy) |
| **Fixed** | Pending (validator revocation + contract abandonment) |
| **Deployed** | Pending |
| **Exploited** | No evidence of exploitation |

**Description:** The EVM deployer key (`0xdde17506...`) used to deploy all 7 smart contracts on 6 EVM chains was stored on the compromised development machine. This key has admin/owner role on:

- wZION token (`0x0c493763d107ab0ABb0aee1Ca3999292d8202bb6`) — 6 chains
- ZIONBridge (`0x72c8f0Dc...` Base, `0xa5a09b2C...` 5 others) — validator management, pause/unpause
- ZIONGovernance (`0xB77eB4ab9468Ce03FBd7eCec70e976EFCfa623E8`)
- ZIONTreasury (`0x455f465ac7e14fdA97dC46fdd74bCa78bfC0aEeD`)
- ZIONStaking (`0xbd5cEe7878337d22188BFBaF9aa9F39A850Be78B`)
- ZIONFarm (`0x167B2753F5D8D9F8e62875cc9e379d7804308B08`)

**Impact:** Attacker could mint wZION, add/remove bridge validators, pause contracts, withdraw staking rewards, or manipulate governance. No external users had funds in these contracts (pre-launch).

**Remediation plan:**
1. Revoke all 5 compromised validator addresses from ZIONBridge on all 6 chains (if admin key still works)
2. Pause all contracts (if possible)
3. Abandon compromised contracts
4. Deploy fresh contracts with new keys and multisig admin after L1 stabilization

---

### ZION-2026-006: Max Transaction Amount Cap (F4.7) — Defense-in-Depth

| Field | Value |
|-------|-------|
| **UID** | ZION-2026-006 |
| **Severity** | LOW (defense-in-depth, not directly exploitable) |
| **Type** | Consensus — missing sanity cap on transaction amount |
| **Introduced** | v3.0.0 |
| **Fixed** | v3.0.4 (commit `690b6dfe`) |
| **Deployed** | 2026-07-07, activated at height 1 |
| **Exploited** | No — defense-in-depth measure |

**Description:** Prior to F4.7, there was no upper bound on the `amount_zion` field in account-model transactions beyond the F5 balance check. While F5 prevents inflation from zero-balance addresses, F4.7 adds a hard cap equal to `emission::TOTAL_SUPPLY` (144 billion ZION) as a second layer of defense. Any transaction attempting to move more than the entire money supply is rejected outright, before the F5 balance check runs.

**Design decision:** The cap is set to `TOTAL_SUPPLY` (not 100M as initially proposed) to avoid colliding with legitimate premine-scale transfers (DAO treasury: 2.5B ZION, OASIS: 1.65B ZION). The cap is a supply-invariant: no legitimate transaction can exceed it, but inflationary garbage (e.g., `u64::MAX` or `u128::MAX`) is blocked.

**Exceptions:** `from == "genesis"` and `from == "coinbase"` are exempt (genesis premine allocation and block rewards).

**Height-gate:** `ZION_MAX_TX_AMOUNT_HEIGHT` environment variable (default `u64::MAX` = disabled). On the fresh chain post-hard-reset, activated at height 1.

**Validation paths:** Both RPC (`insert_transaction()`) and P2P (`validate_peer_block()`) enforce the cap — parity ensured.

**Tests:** 4 unit tests (`f4_7_rejects_tx_above_total_supply`, `f4_7_allows_premine_sized_tx`, `f4_7_boundary_exactly_total_supply_passes_cap`, `f4_7_disabled_by_default`). Live smoke test on production server (2026-07-08, height 81): TX with amount = TOTAL_SUPPLY + 1 rejected with `exceeds max allowed amount`; normal TX (1000 ZION) passed F4.7 and was rejected by F5 (insufficient balance).

**Fix files:** `V3/L1/cosmic-harmony/src/deeksha.rs`, `V3/L1/core/src/lib.rs`, `V3/L1/core/src/bin/node.rs`

---

## 3. Incident Timeline

| Date | Time (UTC) | Event |
|------|------------|-------|
| 2026-07-02 | ~18:00 | TeamViewer compromise detected. Attacker had root access to Edge server for ~47 minutes. |
| 2026-07-02 | 18:30 | SSH session terminated. Tailscale down. Emergency lockdown initiated. |
| 2026-07-02 | 19:00 | F1 vulnerability discovered — forged account TX from `109.81.30.165` in chain. |
| 2026-07-02 | 19:15 | Chain rollback to block 22180. F1 fix implemented (commit `9341344d`). |
| 2026-07-02 | 19:30 | Escrow key rotation initiated. F5 vulnerability discovered accidentally. |
| 2026-07-02 | 19:37 | F5 confirmed: 100,002 ZION created from zero-balance address. |
| 2026-07-02 | 19:48 | Inflationary 100,002 ZION burned to unspendable address (block 22362). |
| 2026-07-02 | 20:22 | F5 fix deployed. `ZION_BALANCE_CHECK_HEIGHT=22394`. Both nodes restarted. |
| 2026-07-02 | 20:30 | F5 active on mainnet (block 22395). |
| 2026-07-02 | 21:00 | Comprehensive server hardening (UFW, AppArmor, key scrub, service binding). |
| 2026-07-02 | 22:55 | L2 security patch deployed. Node binary swap. F5 fuzz tests pass. |
| 2026-07-03 | 00:00 | Decision: full hard genesis reset required. All key material considered compromised. |
| 2026-07-03 | 01:00 | `HARDRESETOFFICIAL.md` created — operational hard reset plan. |
| 2026-07-03 | - | Website put in maintenance mode (zionterranova.com). |
| 2026-07-06 | - | Phase 0-3 complete: all keys regenerated, genesis.rs updated, L2/L3 configs updated, bridge vault rotated, 49 stale addresses fixed across codebase. All tests pass (148 bridge, 25 DAO, 552 L1 core). |
| 2026-07-06 | - | This security disclosure published. |
| 2026-07-07 | - | Security patch 3.0.4 wave 1-2: dependency hardening (quinn-proto, crossbeam-epoch, anyhow, rand, indicatif, ratatui, lru, metal) + F4.7 max-tx-amount cap implemented. F4.7 activated on production server (`ZION_MAX_TX_AMOUNT_HEIGHT=1`). |
| 2026-07-08 | - | F4.7 smoke test on production (height 81): TX > TOTAL_SUPPLY rejected, normal TX passed F4.7 → rejected by F5. Cap confirmed working. |
| 2026-07-08 | - | Git history scrub: 87 secret occurrences (SSH keys + pool SKs) removed via `git filter-repo`. Force pushed to origin. |
| 2026-07-08 | - | `bincode 1.x` removed from dependency tree (heed `serde-bincode` feature disabled). `metal`/`paste` made macOS-only (target-gated). `cargo audit` clean (1 ignored: paste macOS-only). |

---

## 4. Root Cause Analysis

### Primary cause: TeamViewer on development machine

The root cause was the use of TeamViewer remote desktop software on the primary development machine (Windows PC). The attacker exploited TeamViewer to gain access to:
1. The local git repository (full source code)
2. SSH keys stored in `~/.ssh/` (providing root access to Edge server)
3. Various `.env` files containing EVM private keys

### Contributing factors

1. **Single-operator topology.** All services (L1 nodes, L2 bridge/DAO/swap, L3 WARP, pool) ran on a single VPS with a single SSH key. One compromised key = total compromise.

2. **Secrets in git history.** Pool payout signing keys were committed to shell scripts in the repository (3 files). Even after scrubbing, any clone of the repo retained the secret keys in git history.

3. **No defense in depth.** All services ran as root. No AppArmor confinement. No user isolation. No encrypted storage. DB files were world-readable (mode 644).

4. **Account model without balance validation.** ZION-2026-002 was a design flaw present since the account model was introduced in v3.0.0. The UTXO model has inherent value conservation; the account model was added as a convenience layer but skipped the fundamental `balance >= amount + fee` check.

5. **No firewall discipline.** UFW rules allowed public access to internal services (RPC, pool, L2 daemons) that should have been localhost-only.

---

## 5. Remediation & Hard Reset

### Why a full hard reset?

After the TeamViewer compromise, **all cryptographic material must be considered burned**:

- 14 premine wallet keypairs (8.25B + 4B + 2.59B + 1.44B + 0.5B ZION)
- 5 canonical subsidy wallet keypairs (block reward recipients)
- Bridge vault seed (100M ZION)
- Pool payout signing key
- Escrow key
- 5 EVM bridge validator keys
- 5 DAO guardian mnemonics
- EVM deployer/admin key
- SSH keys

A targeted key rotation would leave the risk of an attacker having retained a backup of any single key. The only safe option is a complete restart from Genesis block #0 with entirely new cryptographic material generated on a clean machine.

### What was done (Phases 0-3)

| Phase | Status | Description |
|-------|--------|-------------|
| 0 | DONE | Pre-flight: Tailscale down, machine audit, backup of current state |
| 1 | DONE | New key generation: 14 premine wallets, 5 canonical wallets, bridge vault seed, pool payout, EVM validators, DAO guardians, escrow, SSH keys |
| 2 | DONE | `genesis.rs` + `crypto.rs` + `fee.rs` updated with all new addresses and seeds |
| 3 | DONE | L2/L3 configs updated (3 TOML files, 3 Rust source files, 27 documentation files, scripts, dashboard) |

### What remains (Phases 4-10)

| Phase | Status | Description |
|-------|--------|-------------|
| 4 | ✅ DONE | EVM contract validator revocation / pause / abandonment |
| 5 | ✅ DONE | New server provisioning (`<ZION_SEED_PEER>`, hardened, Tailscale removed) |
| 6 | ✅ DONE | L1 hard reset: fresh genesis `4f75a0df...`, chain at height 80+ |
| 7 | ✅ DONE | Verification: genesis hash, block production, fee split, 7/7 services active |
| 8 | ✅ DONE | Documentation sync across all files |
| 9 | PENDING | Open-source publication (see Section 8) — after key rotation (Fáze 5) |
| 10 | FUTURE | Generational transfer: Issobella continuity protocol |

### Security patch 3.0.4 status (2026-07-08)

| Fáze | Status | Description |
|-------|--------|-------------|
| 1 | ✅ DONE | Dependency + code hardening (advisories, guardy, timeouty) |
| 2 | ✅ DONE | F4.7 Max TX amount cap — implementace |
| 3 | ✅ DONE | Push + rebuild + binary swap na nový server |
| 4 | ✅ DONE | F4.7 aktivace + smoke test (height 1, verified height 81) |
| 5 | ⏳ PENDING | Air-gapped key rotace (F4.1–F4.5) — requires owner on air-gapped machine |
| 6.1 | ✅ DONE | Git history scrub (87 secret occurrences removed) |
| 6.2 | ✅ DONE | Tailscale ACL (removed entirely — not needed for single-server) |
| 6.3 | ✅ DONE | Residual advisories (bincode removed, paste macOS-only) |
| 6.4 | ✅ DONE | Final security check (audit clean, cargo test green, disclosure updated) |

### Canonical runbook

The complete step-by-step procedure is at [`docs/3.0.4/GENESIS_HARD_RESET_CANONICAL.md`](../3.0.4/GENESIS_HARD_RESET_CANONICAL.md).

---

## 6. What We Are Publishing

In the spirit of radical transparency for a pre-launch project, ZION will publish:

### Immediate (with this bulletin)

1. **This disclosure document** — full vulnerability details, timeline, root cause analysis
2. **Machine-readable vulnerability catalogue** — [`vulnerabilities.json`](./vulnerabilities.json) (Go Ethereum format)
3. **F5 incident report** — [`F5_SECURITY_INCIDENT_REPORT_2026-07-02.md`](../../F5_SECURITY_INCIDENT_REPORT_2026-07-02.md)
4. **Server hardening audit** — [`SecurityFirst.md`](../../SecurityFirst.md)
5. **L2 security patch details** — [`PATCH_L2_SECURITY_2026-07-02.md`](../../PATCH_L2_SECURITY_2026-07-02.md)

### After hard reset completion

6. **Full L1 source code** — `V3/L1/core/src/` (Rust, ~15K LOC) — the consensus engine, genesis block, emission schedule, fee model, cryptographic primitives, RPC, P2P, validation
7. **Mining algorithms** — `V3/L1/cosmic-harmony/src/` — Cosmic Harmony v2 + DeekshaLite v1 (PoW hash functions)
8. **Pool server** — `V3/L1/pool/src/` — PPLNS payout, mining proxy, metrics
9. **L2 bridge** — `V3/L2/bridge/src/` — L1-to-EVM bridge relay (validator quorum, watcher, relayer)
10. **L3 WARP** — `V3/L3/warp/src/` — 12-chain cross-chain bridge (BCS, CBOR, TL-B serializers)
11. **EVM contracts** — `V3/L2/bridge/contracts/` — Solidity sources (wZION, ZIONBridge, Governance, Treasury, Staking, Farm)
12. **Diff of all security fixes** — git patches for F1, F5, L2 security hardening
13. **New genesis hash** — `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`
14. **Audit checklist** — reproducible verification steps for anyone to validate the new genesis

### Publication format

Following the Ethereum Foundation model:
- **GitHub repository** — public repo with tagged releases
- **Blog post** on project website — human-readable summary
- **SECURITY.md** in repo root — responsible disclosure instructions
- **vulnerabilities.json** — machine-readable, signed, versioned

---

## 7. What We Are NOT Publishing

The following will **never** be published:

1. **Private keys** — no secret key material, mnemonics, or key derivation seeds (current or historical)
2. **Encrypted key archives** — `/home/zionserver/zion-keys-2026-07-06-encrypted.tar.gz.aes`
3. **Attacker forensics** — IP addresses, session logs, and artifacts preserved for law enforcement (NCOZ)
4. **SSH credentials** — server access keys (rotated)
5. **Historical git objects containing secrets** — will be removed via BFG Repo-Cleaner before publication

### Git history scrub — ✅ COMPLETED (2026-07-08)

Git history was scrubbed using `git filter-repo --replace-text` on 2026-07-08. 87 secret occurrences were removed:

| What | Files | Method |
|------|-------|--------|
| Pool payout SK hex | `setup-edge.sh`, `launch-stack.sh`, `start-pool.sh` | `git filter-repo --replace-text` |
| EVM private keys | `hardhat/.env`, `V3/docker/.env` | `git filter-repo --replace-text` |
| Edge environment secrets | `edge-environment.sh` | `git filter-repo --replace-text` |
| DAO guardian mnemonics | Any file referencing 12/24-word seeds | `git filter-repo --replace-text` |
| SSH private keys | `*.pem`, `ssh-key-*`, `newzionssh.md` | `git filter-repo --replace-text` |

**Result:** 87 occurrences replaced across entire git history. Force pushed to origin. All collaborators must re-clone.

---

## 8. Source Code Publication Plan

### Why open source?

1. **Transparency.** After a security incident, trust must be rebuilt through verifiability. Anyone should be able to audit the consensus code, verify the genesis block, and confirm the fix for every vulnerability.

2. **Reproducibility.** The new genesis hash must be independently reproducible. Publishing `genesis.rs` allows anyone to `cargo build` and verify `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e`.

3. **Community.** ZION's vision (Proof-of-Care, humanitarian subsidy model, multi-chain WARP bridge) benefits from open collaboration. Security through obscurity has already failed; security through transparency is the path forward.

### Publication timeline

| Milestone | Target | What |
|-----------|--------|------|
| T+0 (this bulletin) | 2026-07-06 | Security disclosure + vulnerability catalogue |
| T+1 (hard reset done) | 2026-07-08* | Genesis verification data (hash, premine addresses, canonical wallets) |
| T+2 (verification) | 2026-07-10* | Full L1 source code + mining algorithms |
| T+3 (stabilization) | 2026-07-14* | L2 bridge, L3 WARP, EVM contracts, pool server |
| T+4 (documentation) | 2026-07-21* | Complete developer documentation, build instructions, architecture guide |

*Dates are targets, not commitments. Security takes precedence over schedule.

### License

Source code will be published under a dual license:
- **MIT** for core consensus and mining code
- **Apache 2.0** for tooling, scripts, and infrastructure

### What you can verify after publication

1. `cargo build --release -p zion-core --bin get-genesis-hash` produces the published genesis hash
2. All 14 premine addresses match published `PREMINE_OUTPUTS` in `genesis.rs`
3. Fee split (89/5/5/1) is enforced in `emission.rs`
4. F1 fix: `verify_signature()` is called for all account-model transactions in `validate_peer_block()`
5. F5 fix: `account_balance_for()` balance check is enforced in both `insert_transaction()` and `validate_peer_block()`
6. F4.7 fix: `max_tx_amount_active_at()` cap check rejects transactions where `amount_zion > emission::TOTAL_SUPPLY` (both RPC and P2P paths)
7. Bridge vault address is keyless (derived from seed, no corresponding private key)
8. No secret keys in any source file or git history (scrubbed via `git filter-repo`)

---

## 9. Lessons Learned

### 1. Never use remote desktop tools on development machines

TeamViewer, AnyDesk, VNC, and similar tools create an uncontrolled remote access vector. Development machines with access to production secrets must not run any remote access software. Use SSH with hardware keys or Tailscale SSH with ACLs.

### 2. Account models need explicit balance validation

This is consensus design 101. Every account-model blockchain (Ethereum, Solana, NEAR, Aptos) checks `sender_balance >= amount + fee` before accepting a transaction. ZION's hybrid UTXO/account model relied on the inherent safety of UTXO but failed to apply the same rigor to the account path. The fix was 15 lines of Rust.

### 3. Secrets must never touch git history

Even "temporary" commits with secret keys create permanent exposure once pushed. Use environment variables, encrypted config files, or secret management tools (Vault, SOPS). If a secret enters git, the entire history must be rewritten — costly and error-prone.

### 4. Single-operator topology is fragile

All eggs in one basket (one server, one SSH key, one admin). Any breach is total. Production deployments need at minimum:
- Separate machines for key generation (air-gapped) and runtime
- Multisig for administrative operations
- Separate user accounts per service (not root)

### 5. Defense in depth is not optional

Each misconfiguration (C1-C8) was individually survivable. Together, they created a cascade where one compromised SSH key led to total system compromise. Firewalls, user isolation, encrypted storage, AppArmor, and audit logging are all required — not aspirational.

### 6. Pre-launch is the right time to fail

The silver lining: these vulnerabilities were discovered and remediated before any external user had funds at risk. A hard genesis reset is feasible precisely because the network is in pre-launch. Post-launch, this same incident would have been catastrophic.

---

## 10. Future Security Measures

The following measures will be implemented as part of the hard reset and going forward:

### Infrastructure

- [ ] Fresh server with hardened base image (Ubuntu 24.04 LTS)
- [ ] UFW deny-all default, explicit allow only for P2P (8333) and SSH
- [ ] All RPC/L2/L3 services on localhost, exposed only via Tailscale
- [ ] Tailscale ACLs configured (node-level, port-level)
- [ ] AppArmor profiles for all ZION binaries
- [ ] Separate `zion` user (not root) for all services
- [ ] LUKS-encrypted data volume for chain state
- [ ] Automated security monitoring (failed SSH, UFW blocks, process anomalies)

### Key management

- [ ] Air-gapped key generation for all premine and canonical wallets
- [ ] Hardware security module (HSM) or hardware wallet for pool payout signing
- [ ] Multisig (3-of-5) for EVM contract admin operations
- [ ] No secret keys in git history (enforced by pre-commit hook)
- [ ] Encrypted key archive on offline media (USB + paper backup at separate location)

### Code quality

- [ ] Mandatory code review for all L1 consensus changes
- [ ] Fuzz testing for all transaction validation paths
- [ ] `cargo clippy` + `cargo fmt` enforced in CI
- [x] Max transaction amount cap (TOTAL_SUPPLY = 144B ZION) as additional inflation guard — F4.7, deployed 2026-07-07
- [ ] Consider UTXO-backed account model for v3.1.0 (eliminate account-model balance bugs by design)

### Process

- [ ] SECURITY.md in repo root with responsible disclosure instructions
- [ ] Bug bounty program (after launch, community-funded from DAO treasury)
- [ ] Regular security audits (quarterly internal, annual external)
- [ ] Incident response runbook

---

## 11. Contact & Responsible Disclosure

### Reporting vulnerabilities

If you discover a vulnerability in ZION, please report it responsibly:

- **Email:** security@zionterranova.com
- **PGP key:** (will be published with source code release)
- **GitHub Security Advisory:** (will be enabled on public repo)

Please do NOT:
- Open public GitHub issues for security vulnerabilities
- Exploit vulnerabilities on mainnet
- Share vulnerability details publicly before coordinated disclosure

### Acknowledgments

We believe transparency builds trust. This disclosure is modeled after the practices of:
- [Ethereum Foundation Public Disclosures](https://github.com/ethereum/public-disclosures/)
- [Go Ethereum Vulnerability Disclosure](https://geth.ethereum.org/docs/developers/geth-developer/disclosures)
- [Solidity Bug Disclosure](https://soliditylang.org/blog/category/security-alerts/)

---

## Appendix A: File Index

| Document | Path | Description |
|----------|------|-------------|
| This bulletin | `docs/security/SECURITY_DISCLOSURE_2026-07.md` | Main disclosure |
| Vulnerability catalogue | `docs/security/vulnerabilities.json` | Machine-readable (Geth format) |
| F5 incident report | `F5_SECURITY_INCIDENT_REPORT_2026-07-02.md` | Detailed F5 write-up |
| Server hardening | `SecurityFirst.md` | Full server audit + fixes |
| L2 security patch | `PATCH_L2_SECURITY_2026-07-02.md` | Bridge/DAO/swap hardening |
| Hard reset runbook | `docs/3.0.4/GENESIS_HARD_RESET_CANONICAL.md` | Canonical 10-phase procedure |
| Hard reset plan | `HARDRESETOFFICIAL.md` | Operational plan (status: EXECUTING) |

---

*This document will be updated as remediation progresses. Last revision: 2026-07-06.*
