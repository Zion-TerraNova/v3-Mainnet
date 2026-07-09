# ZION CLI Reference

## Scope

This document is the command-oriented companion to `V3/docs/CLI_GUIDE.md`.

Use it when you need practical examples for the existing CLI surface rather than the high-level positioning.

## Top-Level Runtime Control

### Interactive launcher

```bash
zion
zion menu
```

What this does:

- opens an arrow-key operator menu instead of requiring you to remember the whole command tree,
- starts from a grouped operator dashboard before drilling into narrower submenus,
- dispatches the same canonical typed commands under the hood,
- returns you to the menu after command completion instead of dumping you straight back to the shell,
- works best in a real interactive terminal,
- falls back to normal typed command usage for scripts and non-interactive environments.

### Version and release surface

```bash
zion version
zion update --check
zion update --yes
```

What this does:

- prints the active CLI binary version,
- shows the current release line and workspace track,
- shows the resolved config path when available,
- provides the local CLI update entrypoint,
- compares the local binary with the published platform artifact,
- can download and replace the local binary after checksum verification.

Important distinction:

- `zion update` targets the local CLI binary on the current machine,
- `zion deploy update` targets remote compose-managed containers on the configured server.

### Global health and visibility

```bash
zion status
zion doctor
zion logs node
zion logs ai-native
zion dashboard
```

What these do:

- `zion` opens the interactive launcher when no subcommand is provided,
- `zion version` prints release metadata plus manual update guidance,
- `zion update` checks or installs the latest published CLI binary for the current platform,
- `zion status` runs the broad stack health view,
- `zion doctor` runs the operator preflight across config sanity, local miner readiness, and endpoint reachability,
- `zion logs <service>` tails deploy-managed service logs,
- `zion dashboard` opens the web dashboard at the configured node host on port `3000`.

### Service lifecycle

Supported top-level lifecycle targets:

- `all`
- `node` or `core`
- `pool`
- `miner`
- `agent` or `ai-native`
- `bridge`
- `dao`
- `website`
- `redis`
- `monitoring`

Typical usage:

```bash
zion start ai-native
zion restart bridge
zion stop monitoring
```

`monitoring` expands into the monitoring service bundle instead of a single process.

If you pass an unsupported target, the CLI now fails locally with the supported target list instead of deferring the error to remote `docker compose`.

## L1 Commands

### `zion node`

Use for JSON-RPC-backed node inspection.

```bash
zion node status
zion node peers
zion node blocks
zion node block 6801
zion node tx 7d8c0e
zion node mempool
zion node sync
zion node rpc getChainInfo
```

Operational notes:

- `status` and `sync` are the first checks during runtime triage,
- `block` and `tx` are the fastest narrow inspection tools for chain incidents,
- `rpc` is the escape hatch when the wrapped subcommands are not enough.

### `zion pool`

Use for pool-side operational inspection.

```bash
zion pool stats
zion pool miners
zion pool config
zion pool earnings
```

Operational notes:

- `stats` is the first pool heartbeat check,
- `miners` helps spot worker churn or duplicate-share patterns,
- `earnings` is the operator-facing reward sanity check.

### `zion mine`

Use for miner runtime control and quick performance checks.

```bash
zion mine start
zion mine start --backend opencl
zion mine start --backend cuda --profile dual
zion mine status
zion mine bench
zion mine bench --gpu --backend metal --work-size 262144
zion mine bench --ekam --backend opencl --work-size 8192
zion mine dcr
zion mine stop
```

Operational notes:

- use `bench` before assuming a host is suitable for mining,
- `--ekam` now invokes the miner's real `--ekam-bench` path instead of only setting a benchmark profile,
- `--algorithm deeksha_lite_v1|deeksha_lite_fire|cosmic_harmony_ekam_deeksha_v2` is passed through to the miner (defaults to config `miner.algorithm`),
- `--backend opencl|cuda|metal` is now wired through to the miner instead of being reduced to a generic GPU flag,
- `mine start` now falls back to `miner.profile` from config when `--profile` is omitted,
- `mine start` now rejects malformed mining wallet addresses before launching the miner,
- use `status` before and after config changes,
- `mine start` now maps the ZION wallet to `ZION_MINER_ID` and keeps `miner.btc_wallet` separate for dual DCR payout flow,
- `dcr` belongs to miner diagnostics, not deployment.

### `zion wallet`

Use for local wallet and payment operations.

```bash
zion wallet new --set-default
zion wallet new --mnemonic --words 24 --set-default
ZION_WALLET_PASSWORD='strong-passphrase' zion wallet new --mnemonic --password-env ZION_WALLET_PASSWORD
zion wallet import-mnemonic --mnemonic "abandon ..." --set-default
zion wallet import-secret-key --secret-key-hex deadbeef...
zion wallet info --wallet zion-wallet.json
zion wallet export --wallet zion-wallet.json
ZION_WALLET_PASSWORD='strong-passphrase' zion wallet reveal --wallet zion-wallet.json --password-env ZION_WALLET_PASSWORD
zion wallet address
zion wallet balance --address zion1example
zion wallet send --to zion1example --amount 1.25
zion wallet tithe
```

Operational notes:

- use `--set-default` when the generated/imported address should become the active miner payout destination,
- use `--password-env` when you want the wallet file to store encrypted secrets instead of plaintext JSON,
- `export` prints the stored wallet JSON as-is, which is useful for backup or external password-manager storage,
- `reveal` decrypts an encrypted wallet file back to structured JSON for backup/recovery workflows,
- check `balance` before any payout or tithe action,
- prefer explicit verification around address handling,
- treat send flows as operator actions that deserve manual confirmation.

## L2 Commands

### `zion bridge`

Use for bridge queue and transfer inspection.

```bash
zion bridge status
zion bridge pending
zion bridge history
zion bridge get bridge-op-42
zion bridge chains
zion bridge transfer base zion1example 10
```

Operational notes:

- `status` and `pending` are the first incident checks,
- `history` is useful when reconciling bridge progression,
- `transfer` should be treated as an operator action with explicit review.

### `zion dao`

Use for governance, treasury, and proposal visibility.

```bash
zion dao status
zion dao proposals
zion dao proposal 7
zion dao treasury
zion dao params
zion dao vote 7 yes
```

Operational notes:

- `treasury` and `params` are the fastest governance-state checks,
- `proposal` narrows into one object when `proposals` is too broad,
- `vote` should be treated as a deliberate operator action.

## L3 Commands

### `zion agent`

Use as the operator gateway to Hiranyagarbha.

```bash
zion agent status
zion agent config
zion agent memory
zion agent rag query "bridge"
zion agent ask "What is the current L3 state?"
zion agent tasks
zion agent warp
zion agent ncl
zion agent oasis
zion agent logs
```

Operational notes:

- `status` is the first health and mode check,
- `config` confirms backend and endpoint wiring,
- `memory` and `rag` are runtime introspection tools,
- `ask` and `chat` remain valid even in fallback mode when the service is healthy.

### `zion warp`

Use for relay visibility.

```bash
zion warp status
zion warp chains
zion warp chain base
zion warp pending
zion warp get warp-op-18
zion warp stats
zion warp validators
```

### `zion ncl`

Use for neural compute lane visibility.

```bash
zion ncl status
zion ncl jobs
zion ncl job ncl-job-22
zion ncl workers
zion ncl leaderboard
zion ncl schedule
zion ncl price
zion ncl submit ./job.json
```

Operational notes:

- `workers`, `leaderboard`, and `price` are the quick operator views,
- `submit` belongs to controlled task submission, not casual probing.

## Operations Commands

### `zion deploy`

Use for server-side deployment flows.

```bash
zion deploy status
zion deploy server
zion deploy website
zion deploy update
zion deploy prune
zion deploy ssh
```

Operational notes:

- `status` is the safe first step,
- `server` and `update` are the normal runtime-changing actions,
- `prune` is cleanup and should be used intentionally,
- `ssh` is for controlled direct access, not bypassing repeatable deploy flows by default.

### `zion config`

Use for effective config inspection and updates.

```bash
zion config show
zion config path
zion config validate
zion doctor
zion config set node.rpc_host <LEGACY_TAILSCALE>
zion config set node.rpc_port 8443
zion config set miner.btc_wallet bc1qexample
zion config init
```

Operational notes:

- `show` is the first config sanity check,
- `path` matters when the operator is unsure which file is active,
- `validate` checks backend/profile/URL/SSH key sanity before runtime,
- `doctor` is the wider preflight when you also need node reachability, local binary discovery, and agent reachability,
- `init` re-runs onboarding when the file is incomplete or stale.

### TUI and shell helpers

```bash
zion monitor
zion explorer
zion completions zsh
zion completions bash
```

Operational notes:

- `monitor` is the live multi-layer operator view,
- `explorer` is the terminal-first chain exploration surface,
- `completions` should be generated per shell and installed through the user's shell profile.

## Suggested Operator Habits

When something looks wrong, the shortest reliable path is usually:

1. `zion status`
2. `zion node status`
3. `zion agent status`
4. `zion logs <affected-service>`
5. the narrow command group for the failing layer

That sequence usually tells you whether the problem is global, layer-specific, or only one service.