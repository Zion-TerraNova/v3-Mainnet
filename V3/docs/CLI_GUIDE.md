# ZION CLI Guide

## Purpose

`zion` is the unified operator gateway for the current ZION stack.

It is not just a thin wrapper around one binary. It is the intended control surface across:

- L1 node, pool, miner, and wallet,
- L2 bridge and DAO,
- L3 AI Native, WARP, and NCL,
- deployment, monitoring, and explorer workflows.

## Current Design Position

For the present production era, treat the CLI as an orchestrator-first interface.

That means:

- the CLI should expose services, health, status, and deploy flows cleanly,
- AI Native is presently an orchestration layer before it is a heavy local inference runtime,
- remote model backends are optional integrations rather than hard assumptions,
- degraded mode is acceptable when the agent backend is unavailable, but service visibility must remain strong.

## Build And Run

From the V3 workspace root:

```bash
cargo run -p zion-cli -- --help
```

Or build the binary once:

```bash
cargo build -p zion-cli
./target/debug/zion --help
```

If you run the binary without a subcommand, it now opens an interactive arrow-key launcher:

```bash
zion
```

You can also open the launcher explicitly:

```bash
zion menu
```

The menu is intended as the operator-first entrypoint for routine flows. It now starts on a grouped operator dashboard so you choose between health checks, stack operations, L1, mining and wallet, L2, L3, and onboarding before dropping into narrower submenus.
It uses arrow keys plus Enter, then dispatches the same canonical commands that remain available in typed form.
After a command finishes, the launcher now waits and returns you back into the menu instead of dropping straight to the shell.

## Top-Level Commands

```text
menu         Open interactive arrow-key operator menu
version      Print release metadata and manual update guidance
update       Check for and install the latest published CLI artifact
onboard      First-time setup wizard
start        Start service(s): all | node | pool | miner | agent | ai-native | bridge | dao | website | redis | monitoring
stop         Stop service(s): all | node | pool | miner | agent | ai-native | bridge | dao | website | redis | monitoring
restart      Restart service(s): all | node | pool | miner | agent | ai-native | bridge | dao | website | redis | monitoring
status       Health check — all layers
doctor       Run preflight diagnostics for config, local tools, and endpoints
logs         Tail logs for a service
dashboard    Open web dashboard in browser
node         L1 core node commands
pool         L1 pool commands
mine         L1 miner commands
wallet       Wallet operations
agent        L3 Hiranyagarbha AI Native agent gateway
deploy       Server deployment
config       Config management
bridge       L2 bridge gateway
dao          L2 DAO governance
explorer     Block explorer TUI
monitor      Live stack monitor TUI (all layers)
warp         L3 Warp cross-chain relay
ncl          L3 NCL Neural Compute Layer
completions  Print shell completion script
```

## Command Groups

### L1

`zion node`

- `status`
- `peers`
- `blocks`
- `block`
- `tx`
- `mempool`
- `sync`
- `rpc`

`zion pool`

- `stats`
- `miners`
- `config`
- `earnings`

`zion mine`

- `start`
- `stop`
- `bench`
- `status`
- `dcr`

Mining notes:

- `zion mine start --backend opencl|metal|cuda` now forwards the selected backend explicitly to `zion-miner`,
- `zion mine start --algorithm deeksha_lite_v1|deeksha_lite_fire|cosmic_harmony_ekam_deeksha_v2` selects the PoW algorithm (defaults to config `miner.algorithm`),
- `zion mine bench --ekam` now invokes the miner's real Ekam benchmark mode,
- `zion mine start` now uses `miner.profile` from config unless you explicitly pass `--profile`,
- `zion mine start` now fails fast when `miner.wallet` is not a valid `zion1...` address,
- `zion mine start --profile dual` keeps the ZION wallet and BTC payout wallet separate via `miner.wallet` and `miner.btc_wallet`.

`zion wallet`

- `new`
- `address`
- `balance`
- `send`
- `tithe`

### L2

`zion bridge`

- `status`
- `pending`
- `history`
- `get`
- `chains`
- `transfer`

`zion dao`

- `status`
- `proposals`
- `proposal`
- `vote`
- `treasury`
- `params`

### L3

`zion agent`

- `start`
- `stop`
- `restart`
- `status`
- `chat`
- `ask`
- `logs`
- `config`
- `memory`
- `rag`
- `tasks`
- `warp`
- `ncl`
- `oasis`

`zion warp`

- `status`
- `chains`
- `chain`
- `pending`
- `get`
- `stats`
- `validators`

`zion ncl`

- `status`
- `submit`
- `job`
- `jobs`
- `workers`
- `leaderboard`
- `schedule`
- `price`

### Operations

`zion deploy`

- `server`
- `website`
- `update`
- `prune`
- `ssh`
- `status`

`zion monitor` provides the live TUI stack view.

`zion explorer` provides the TUI explorer view.

## Typical Flows

### Interactive launcher

```bash
zion
```

Use this when you want OpenClaw-style navigation instead of remembering subcommands. The launcher now starts from a grouped dashboard with higher-signal categories, then fans into the common operator surfaces: health checks, service lifecycle, node, pool, mining, wallet, agent, bridge, dao, warp, ncl, config, and TUI views.
The launcher also stays alive across commands, so routine operator work can happen inside one continuous session.
The current guided workflows now cover miner start, wallet send, deploy actions, common agent operations, and direct entry into CLI auto-update.

### Version and update surface

```bash
zion version
zion update --check
zion update --yes
```

Use `zion version` to print the active CLI version, release line, config path, and the update distinction between the local CLI binary and remote stack operations.
Use `zion update --check` to compare the current executable against the latest published platform artifact, and `zion update --yes` to download, checksum-verify, and replace the local binary.
Do not confuse it with `zion deploy update`, which refreshes remote containers on the configured server.

### Full-stack status check

```bash
zion status
zion doctor
zion node status
zion pool stats
zion agent status
```

### Service lifecycle

```bash
zion start ai-native
zion restart bridge
zion logs website
```

### Node and wallet operations

```bash
zion wallet new --set-default
zion wallet info --wallet zion-wallet.json
ZION_WALLET_PASSWORD='strong-passphrase' zion wallet reveal --wallet zion-wallet.json --password-env ZION_WALLET_PASSWORD
zion node peers
zion node block 6801
zion wallet balance
zion update --check
zion update --yes
```

### L2 and L3 checks

```bash
zion bridge status
zion dao treasury
zion warp stats
zion ncl workers
```

### Agent usage

```bash
zion agent status
zion agent ask "What is the current L3 state?"
zion agent rag query "bridge" 
```

### Guided deploy and agent flows

```bash
zion
```

From the grouped dashboard, choose `Stack operations` for the guided deploy workflow or `L3 agent, warp & NCL` for the guided agent workflow.
The same `Stack operations` group now also exposes `Version & release info` and `Auto update CLI`.

## Service Semantics

The top-level lifecycle commands map to compose service names, not human-friendly container guesses.

Supported lifecycle targets currently include:

- `core` or `node`
- `pool`
- `miner`
- `agent` or `ai-native`
- `bridge`
- `dao`
- `website`
- `redis`
- `monitoring`

`monitoring` expands to the monitoring service bundle rather than a single process.

## Configuration

The CLI uses a config file, by default:

```text
~/.zion/zion.toml
```

Useful commands:

```bash
zion config show
zion config path
zion config validate
zion doctor
zion config set node.rpc_host <LEGACY_TAILSCALE>
zion onboard
```

`zion doctor` is the one-shot preflight for publishable/operator-facing installs. It combines config validation, local miner binary discovery, node RPC reachability, and AI Native reachability in one place.

## Operator Notes

## Companion Docs

Use these together:

- `V3/docs/CLI_REFERENCE.md` for command examples,
- `V3/docs/CLI_TROUBLESHOOTING.md` for incident response,
- `V3/docs/CLI_DEPLOY_PLAYBOOK.md` for rollout and validation flow.

## Operator Notes

### AI Native posture

Right now, `zion agent` should be understood as an operator-facing orchestration gateway.

It can front a remote LLM backend when available, but the CLI should remain useful even when the agent is running in degraded or fallback mode.

### Documentation contract

Whenever a new CLI command or service target is added, update the following in the same change:

- `V3/docs/CLI_GUIDE.md`
- `V3/docs/CLI_FAQ.md`
- public docs mirror under `APP&WEB/website-v2.9/public/docs/`

That keeps the operator surface auditable and prevents the CLI from outgrowing its docs again.