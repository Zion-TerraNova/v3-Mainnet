# ZION

<div align="center">

<img src="./docs/stargate/nebula.jpg" width="260" height="260" alt="ZION Stargate" style="border-radius: 50%; object-fit: cover; box-shadow: 0 0 50px rgba(0,180,255,0.25);" />

<br/>

## Terra Nova — 100 years of evoluZion

**A multichain Dharma ecosystem secured by proof-of-work consensus.**

[www.newerth.cz]
<br/>
[www.zionterranova.com](https://www.zionterranova.com)

<br/>

</div>

ZION is a multi-layer blockchain: L1 PoW core, L2 DeFi and cross-chain bridge, L3 WARP and Hiran AI, and L4 Oasis — a consciousness-mining spiritual MMORPG.

This repository contains the v3 mainnet codebase. It is currently in **Mainnet Beta**: live, producing blocks, and open for mining at your own risk.

---

## Enter the Oasis

| Portal | Path |
|---|---|
| **Mine** | Run a node or miner on the ZION L1. Start with [`V3/cli/README.md`](./V3/cli/README.md). |
| **Play** | Enter the L4 Oasis world — avatars, quests, guilds, and the Golden Egg. See [`V3/L4/oasis/README.md`](./V3/L4/oasis/README.md). |
| **Build** | Explore the codebase, contracts, RPC, and bridge docs in [`V3/docs/`](./V3/docs/) and [`docs/`](./docs/). |

---

## Network Status

> **Mainnet Beta — live at your own risk**

| Parameter | Value |
|---|---|
| Status | Mainnet Beta |
| Protocol | 3.0.4 |
| Genesis hash | `4f75a0dfe6dde3b167287d445aa1ade56577b0e9166c641ed288b4c20a79bd6e` |
| Official launch | 2026-12-31 |

All disclosed security issues have been remediated. See [Security](./SECURITY.md) and the [disclosure report](./docs/security/SECURITY_DISCLOSURE_2026-07.md).

---

## Begin Guide — Start from zero

> Never used a blockchain before? You're in the right place.
> This guide walks you through everything step by step.
> All you need is a computer with Linux, macOS, or Windows (WSL).

### What is ZION in one paragraph?

ZION is a **proof-of-work blockchain** (like Bitcoin, but with a different mining algorithm). It has its own currency called **ZION**. You can **mine** ZION with your CPU or GPU, **send** it to others, and eventually **play** in the Oasis game world to earn more. The network is live right now — you can join it today.

### Step 0 — Install Rust

ZION is written in Rust. You need the Rust toolchain to build it.

```bash
# Linux / macOS / WSL — install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Verify it works
rustc --version
cargo --version
```

> **Windows users:** Install [WSL2](https://learn.microsoft.com/en-us/windows/wsl/install) first, then run the commands above inside WSL. Native Windows builds are planned but not yet supported.

### Step 1 — Get the code

```bash
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet/V3
```

### Step 2 — Build everything

This compiles the node, CLI, and miner. It takes 5–15 minutes the first time.

```bash
# Build all binaries (node + CLI + miner + pool + bridge + DAO + oasis)
cargo build --release

# The key binaries you'll use:
#   target/release/zion          — the CLI (wallet, mining, node control)
#   target/release/zion-node     — the blockchain node
#   target/release/zion-miner    — standalone miner
```

> **Want GPU mining?** Add a feature flag:
> - NVIDIA CUDA: `cargo build --release --features gpu-cuda -p zion-miner`
> - AMD / generic OpenCL: `cargo build --release --features gpu-opencl -p zion-miner`
> - Apple Silicon Metal: `cargo build --release --features gpu-metal -p zion-miner`

### Step 3 — Create your wallet

Your wallet holds your ZION. It's a JSON file protected by a password you choose.

```bash
# Generate a new wallet with a 24-word recovery phrase (mnemonic)
# WRITE DOWN the 24 words on paper and keep them safe — they are your only backup!
./target/release/zion wallet new --mnemonic --out my-wallet.json

# Check your wallet address (this is where mining rewards go)
./target/release/zion wallet info --wallet my-wallet.json
```

> **What is a wallet address?** It's like a bank account number but public — it starts with `zion1...` and you can share it freely. The 24-word mnemonic is your **private** key — never share it with anyone.

### Step 4 — Run a node (optional but recommended)

A node connects to the ZION network, downloads the blockchain, and verifies transactions. Running one helps keep the network decentralized.

```bash
# Start the node (it will sync the blockchain from other peers)
./target/release/zion-node

# In another terminal, check if it's working:
./target/release/zion node status
```

> **What is syncing?** The node downloads all blocks from the genesis block to the current tip. This can take a while on first run. After that, it stays up to date automatically.

### Step 5 — Start mining

Mining is how new ZION is created. Your computer solves math puzzles (proof-of-work), and when it finds a solution, you earn a block reward.

```bash
# The easiest way — run the onboarding wizard
./target/release/zion config init

# Or start mining directly with your wallet
./target/release/zion mine start --wallet my-wallet.json

# Check mining status
./target/release/zion mine status

# Stop mining
./target/release/zion mine stop
```

> **CPU vs GPU:** Mining with a CPU works but is slow. A GPU (graphics card) is much faster. Run `zion mine bench --gpu` to test your GPU hashrate.
>
> **Pool vs Solo:** By default, the CLI mines to the official pool (`pool.zionterranova.com:8444`). In pool mode, you earn a share of every block the pool finds. In solo mode, you only earn when *you* find a block — which could take a long time. Pool mode is recommended for beginners.

### Step 6 — Check your balance and send ZION

```bash
# Check your balance
./target/release/zion wallet balance --wallet my-wallet.json

# Send ZION to someone
./target/release/zion wallet send --to zion1... --amount 1.5 --wallet my-wallet.json
```

### Interactive menu (easiest for beginners)

If you don't want to remember commands, just run:

```bash
./target/release/zion menu
```

This opens an interactive arrow-key menu with all options — wallet, node, mining, pool, and config.

### Glossary — key terms explained simply

| Term | What it means |
|------|--------------|
| **Blockchain** | A public ledger of all transactions, shared across many computers |
| **Node** | A computer running the ZION software that stores and verifies the blockchain |
| **Mining** | Using your computer's power to secure the network and earn ZION rewards |
| **Wallet** | A file that holds your private keys — it lets you send and receive ZION |
| **Mnemonic** | 24 words that can restore your wallet — write them down, never share them |
| **Block** | A group of transactions added to the chain every ~60 seconds |
| **Pool** | A group of miners working together — rewards are split among participants |
| **ZION** | The currency of this blockchain (ticker: ZION) |
| **Genesis block** | The very first block — the foundation of the entire chain |
| **Mainnet Beta** | The live network is running but may still have bugs — mine at your own risk |

### Need help?

- **Full documentation:** [README_FULL.md](./README_FULL.md)
- **CLI reference:** [`V3/cli/README.md`](./V3/cli/README.md) — every command explained
- **Node docs:** [`V3/docs/`](./V3/docs/) — architecture, constants, runbooks
- **Website:** [zionterranova.com](https://www.zionterranova.com)
- **Issues:** [GitHub Issues](https://github.com/Zion-TerraNova/v3-Mainnet/issues)

---

## Languages

English · [Čeština](./docs/lang/README.cs.md) · [Español](./docs/lang/README.es.md) · [Français](./docs/lang/README.fr.md) · [Português](./docs/lang/README.pt.md)

---

## Full Documentation

For a complete overview of architecture, features, history, and roadmap, see **[README_FULL.md](./README_FULL.md)**.

---

## License

This project is licensed under the [MIT License](./LICENSE).

<div align="center">

Built with care, secured by consensus.

</div>
