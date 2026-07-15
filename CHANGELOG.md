# Changelog

All notable changes to ZION v3 are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [3.0.6-beta] — 2026-07-15 (Protocol + Pool + Docs Sync)

### Added
- **AuxPow B2b stream profit system** — RVN kawpow live on Edge pool, auto coin switching, revenue tracking
- **GPU-native Pearl PoUW pipeline** — Merkle proof reconstruction, E2E verification
- **PearlStratum protocol** — PRL (Pearl) external coin support
- **BridgeValidator contract tracking** — Base Mainnet 5/5 guardian multisig address

### Fixed
- **Pool CLI earnings** — switched from non-existent JSON-RPC to pool HTTP API
- **Edge environment** — added missing `ZION_POOL_AUXPOW_*` and stream-profit env vars
- **Protocol version** — synced live node, code, and docs to `zion-v3-node/3.0.6`
- **Backup node documentation** — noted offline status and seed peers

---

## [3.0.5-beta] — 2026-07-10 (Community CLI)

### Added
- **Simplified community CLI** — single `zion` binary replaces 8 separate binaries
- **Interactive arrow-key menu** — `zion menu` with live dashboard (node, miner, pool, wallet)
- **Guided Setup** — wallet → node → pool → miner workflow, step by step
- **Live monitor** — `zion monitor` shows local stack status + chain height
- **Doctor diagnostics** — `zion doctor` checks config, node, wallet, pool, miner, AI
- **Network status** — `zion status` pings node, pool, website, explorer, AI
- **Hiran AI chat** — `zion ai chat` / `zion ai ask` (OpenAI-compatible endpoint)
- **Config management** — `zion config set/get` with TOML config at `~/.zion/zion.toml`
- **Custom command input** — type any `zion` subcommand from the interactive menu
- **Shell completions** — `zion completions bash|zsh|fish|powershell`
- **Windows self-contained bundle** — node, pool, miner embedded in CLI binary
- **Begin Guide** — beginner-friendly guide added to all 5 Lite READMEs (EN, CS, ES, FR, PT)
- **Visual README redesign** — Stargate hero, shields.io badges, emoji portal, footer

### Fixed
- **Server addresses** — replaced decommissioned old Edge server IP with domain `pool.zionterranova.com` in all config defaults, menu prompts, and help text
- **Wallet send** — sender address now read from wallet file (was broken using `cfg.miner.wallet`)
- **Wallet address/balance** — fallback to `zion-wallet.json` when config `miner.wallet` is empty
- **Doctor on Linux** — searches for `zion-miner` (not `zion-miner.exe`)
- **Download URLs** — error messages now point to GitHub releases page
- **Unused variable warning** — `show_console` on Unix no longer warns

### Changed
- **CLI package** — renamed `V3/cli/` → `V3/community-cli/` (package `zion-public`)
- **Pool default** — `stratum.zionterranova.com:8444`
- **Seed peers** — `rpc.zionterranova.com:8333`
- **AI endpoint** — `http://ai.zionterranova.com:8080`

---

## [3.0.4-beta] — 2026-07-09 (Mainnet Beta)

### Added
- **Mainnet Beta launch** — network is live and producing blocks
- **Hard genesis reset** — new genesis hash `4f75a0df...`, all keys regenerated on air-gapped machine
- **DeFi contracts on Base Mainnet** — ZIONGovernance, ZIONTreasury (3-of-3 multisig), ZIONStaking (12% APR), ZIONFarm (1 wZION/s)
- **wZION ERC-20 token** — deployed on 6 EVM chains (Base, BSC, Polygon, Arbitrum, Optimism, Avalanche)
- **ZIONBridge** — 5/5 validator threshold bridge, all 7 contracts verified on Basescan
- **TX unification** — account-model `memo` field (L1 hard fork, height-gated), L2 watchers scan `account_transactions`
- **F4.7 max-tx-amount cap** — height-gated cap at `emission::TOTAL_SUPPLY` (144B ZION), prevents inflation attacks
- **F5 balance check** — account-model sender balance validation, rejects TX where `sender_balance < amount + fee`
- **F1 signature validation** — `validate_peer_block` now calls `verify_signature()` for non-coinbase account TX
- **RPC debug logging** — `ZION_RPC_DEBUG=1` env var controls verbose P2P/RPC logging (default: off)
- **Memory leak fix** — bounded block retention, peer cap, WebSocket channel cleanup, `MALLOC_ARENA_MAX=1`
- **Security hardening** — UFW firewall, AppArmor, SSH keys-only, all services on 127.0.0.1, private keys scrubbed
- **Security disclosures** — ZION-2026-001 through ZION-2026-005 (F1-F5, C1-C8) publicly disclosed
- **Ethics & Philosophy documentation** — 4 ZION books (Genesis, Quantum Revolution, Ekam Deeksha, Terra Nova)
- **ZION Codex Bodhisattva Vow** — 4 Great Vows, 8 Bodhisattvas, 8 Guardian pledges, 11 Sefirot validator vows
- **evoluZion V2** — PoW → Proof-of-Care 10-year hybrid roadmap
- **Legal documentation** — Legal Disclaimer, Terms of Use, Privacy Policy, Jurisdiction, Token Disclosure
- **Multilingual READMEs** — English, Čeština, Español, Français, Português
- **GPG-signed genesis** — creator statement signed with Ed25519 GPG key
- **USB backup audit** — SHA256 verified, GPG signatures verified, 19/19 addresses cross-checked with genesis.rs

### Changed
- **Server migration** — old Edge server decommissioned, new Edge server deployed
- **Label rename** — "Genesis Creator" replaced with neutral label across 40 files
- **Canonical topology** — hardcoded seed peers moved to new server, Tailscale removed
- **L2 security patch** — claimant guard, threshold 5/5, reorg safety, key hygiene, escrow key zeroing, memo cap

### Security
- **F1 (forged P2P signatures)** — FIXED: `validate_peer_block` now verifies signatures
- **F5 (unlimited inflation)** — FIXED: sender balance validation + max-tx-amount cap
- **C1-C8 (server exposure)** — FIXED: UFW, AppArmor, 127.0.0.1 binding, SSH keys-only
- **TeamViewer compromise** — FIXED: removed, SSH keys-only access
- **EVM key compromise** — FIXED: all EVM keys rotated, multisig treasury

---

## [3.0.3] — 2026-06-27 (Decimal Fork)

### Added
- **Decimal fork** — migration from 1e12 (1 ZION = 1e12 flowers) to 1e6 (1 ZION = 1,000,000 flowers)
- **`scaled_amount()` RPC helper** — normalizes pre-migration amounts to 1e6 scale
- **LI.FI cross-chain DEX integration** — WidgetLight, 30+ DEX, 20+ bridges, 25+ chains
- **WARP D-04** — WARP carries native L1 ZION (wZION on EVM, ZION on non-EVM)
- **12 chain adapters** — 11 functional + TON watch-only, pure-Rust serializers (BCS, CBOR, TL-B)
- **WARP Lightning Network bridge** — BOLT11 parser + LND REST client
- **Stargate logo** — holographic cosmic portal, 28 rotating layers, 9 chevrons

### Changed
- **Protocol version** — bumped to 3.0.3
- **`FLOWERS_PER_ZION`** — changed from 1e12 to 1e6

---

## [3.0.2] — 2026-06-15

### Added
- **Fire algorithm optimization** — GPU mining performance improvements
- **Explorer upgrade** — block detail viewer, clickable rows, hash truncation
- **Dashboard enhancements** — Recent Blocks feed, Network Hashrate, Pool Metrics

---

## [3.0.1] — 2026-06-05

### Added
- **Genesis hard reset** — first genesis regeneration with new key material
- **Hiran v2.3** — AI agent training pipeline improvements
- **L2 big upgrade** — bridge, DAO, atomic-swap watchers
- **L3 big update** — WARP cross-chain protocol expansion
- **Metal GPU optimization** — macOS Metal framework support
- **Auto-updater** — CLI self-update mechanism

---

## [3.0.0] — 2026-05-20 (Initial V3 Mainnet)

### Added
- **V3 mainnet launch** — initial genesis block, PoW consensus
- **L1 Core** — Rust-based node with Ed25519, BLAKE3, LWMA difficulty, UTXO model
- **L2 Bridge** — ZION L1 ↔ EVM bridge with validator quorum
- **L2 DAO** — governance daemon with 5 guardians
- **L2 Atomic Swap** — HTLC atomic swap daemon
- **L3 WARP** — cross-chain protocol (initial 6 EVM chains)
- **L4 Oasis** — consciousness mining game (Rust backend + UE5 frontend)
- **L5 Free World** — community layer with sefirot governance
- **L6 Issobella** — guardian layer for humanitarian missions
- **CLI** — `zion` command-line interface for wallet, node, mining
- **Pool** — Stratum mining pool
- **Miner** — GPU/CPU miner (Ekam Deeksha dual-algo)
- **Dashboard** — web-based monitoring (Python stdlib, zero dependencies)
- **Website** — zionterranova.com (Next.js)

---

## Versioning Scheme

ZION uses a modified semantic versioning scheme:

| Component | Format | Example |
|-----------|--------|---------|
| Protocol | `MAJOR.MINOR.PATCH` | `3.0.6` |
| Release tag | `vMAJOR.MINOR.PATCH[-suffix]` | `v3.0.4-beta` |
| Suffix | `-beta`, `-rc`, `-stable` | `v3.1.0-rc1` |

- **MAJOR** — consensus-breaking changes (new genesis, hard fork)
- **MINOR** — new features, backward-compatible
- **PATCH** — bug fixes, security patches
- **-beta** — Mainnet Beta (pre-official launch)
- **-rc** — Release Candidate
- **-stable** — Official stable release

## Roadmap

| Version | Target | Status |
|---------|--------|--------|
| 3.0.5-beta | Community CLI | ✅ Live (2026-07-10) |
| 3.0.4-beta | Mainnet Beta | ✅ Live (2026-07-09) |
| 3.0.4-stable | Official Public Launch | 📅 2026-12-31 |
| 3.1.0 | Wallet SDK + Mobile App + TX History | 🔜 Q3 2026 |
| 3.2.0 | Proof-of-Care hybrid (NPU mining) | 🔜 2027 |
| 4.0.0 | Full Proof-of-Care consensus | 🔜 2028+ |

---

<div align="center">

**ZION is under active development.** The project evolves continuously with regular versioned releases.

*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*

</div>
