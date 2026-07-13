# ZION v3.0.5-beta — Simplified Community CLI

**One binary. Everything you need.**

This release simplifies the ZION experience: instead of 8 separate binaries, you get **one `zion` binary** with an interactive menu that guides you through wallet creation, mining, and node operation.

> **⚠️ Mainnet Beta — mine and transact at your own risk**
> The network is live and producing blocks. Genesis chain is permanent. Official public launch: **31 December 2026**.

---

## What's new in v3.0.5?

- **Single binary** — no more choosing between 8 different packages
- **Interactive menu** — arrow-key navigation, no need to memorize commands
- **Guided setup** — wallet → node → pool → miner, step by step
- **Live dashboard** — see node status, miner status, wallet balance at a glance
- **Self-contained on Windows** — node, pool, and miner are embedded in the CLI (10 MB total)
- **GPU mining support** — Metal (macOS), OpenCL/CUDA (Linux), CPU fallback

---

## Download

### Which file should I download?

| Your system | File | Size |
|-------------|------|------|
| **Linux** (Ubuntu, Debian, etc.) | `zion-cli-linux-x86_64.tar.gz` | 2.3 MB |
| **macOS Apple Silicon** (M1/M2/M3/M4) | `zion-cli-macos-aarch64.tar.gz` | 2.1 MB |
| **macOS Intel** | `zion-cli-macos-x86_64.tar.gz` | 2.2 MB |
| **Windows 10/11** | `zion-cli-windows-x86_64.zip` | 4.7 MB |

> **Not sure which macOS you have?** Click Apple menu → "About This Mac". "Apple M1/M2/M3" = aarch64. "Intel" = x86_64.

> **Windows users:** The Windows binary is larger (4.7 MB) because it has the node, pool, and miner embedded inside — you only need one file.

---

## Quick Start — 3 steps to mining

### Step 1: Download and extract

**Linux / macOS:**
```bash
# Extract
tar xzf zion-cli-macos-aarch64.tar.gz   # or your platform file

# Make executable (if needed)
chmod +x zion

# Run the interactive menu
./zion menu
```

**Windows:**
```powershell
# Extract the zip (right-click → Extract All)
# Open PowerShell in the extracted folder
.\zion.exe menu
```

### Step 2: Create your wallet

When you run `./zion menu`, select **"🚀 Guided Setup"** — it will walk you through everything.

Or do it manually:
```bash
# Create a new wallet with 24-word recovery phrase
# ⚠️ WRITE DOWN the 24 words on paper — they are your only backup!
./zion wallet new --mnemonic --out my-wallet.json

# View your wallet address (starts with "zion1...")
./zion wallet info --wallet my-wallet.json
```

> **What is a wallet?** A file that holds your private keys. Share your address (starts with `zion1...`) to receive ZION. The 24-word mnemonic is your **private** backup — never share it, never put it online.

### Step 3: Start mining

```bash
# Start mining to the official pool
./zion mine start --wallet my-wallet.json

# Check status
./zion mine status

# Stop
./zion mine stop
```

Or just use the menu: `./zion menu` → "Mine" → "Start"

> **Pool vs Solo:** By default, the CLI mines to the official pool (`62.171.141.136:8444`). In pool mode, you earn a share of every block. Solo mode only pays when *you* find a block — which is rare. **Pool mode is recommended.**
>
> **GPU mining:** The CLI auto-detects your GPU. On macOS it uses Metal, on Linux it uses OpenCL or CUDA. You can force a backend with `--backend cpu|opencl|cuda|metal`.

---

## All Commands

```bash
./zion menu              # Interactive arrow-key menu (easiest)
./zion wallet new        # Create wallet
./zion wallet balance    # Check balance
./zion wallet send --to zion1... --amount 1.5  # Send ZION
./zion mine start        # Start mining
./zion mine stop         # Stop mining
./zion mine status       # Mining status
./zion node info         # Node info (chain height, peers)
./zion node peers        # List connected peers
./zion status            # Network health check
./zion doctor            # Diagnostics
./zion monitor           # Live TUI monitor
./zion version           # Version info
```

---

## Run a Full Node (optional)

A full node downloads the entire blockchain and verifies all transactions. Running one helps decentralize the network.

**Linux / macOS:**
```bash
./zion node start        # Starts a local node (if bundled or installed)
./zion node status       # Check if it's running
```

**Windows:**
The Windows CLI has the node embedded — just run:
```powershell
.\zion.exe node start
```

---

## Network Parameters

| Parameter | Value | What it means |
|-----------|-------|---------------|
| Genesis hash | `4f75a0df...` | Fingerprint of the first block — proves you are on the right chain |
| Consensus | PoW (Ekam Deeksha) | Proof-of-work: BLAKE3 + RandomNPU dual-algo |
| Block target | 60 seconds | New block every ~60 seconds |
| Total supply | 144 billion ZION | Maximum ZION that will ever exist |
| Decimals | 6 (1 ZION = 1,000,000 flowers) | Smallest unit: 0.000001 ZION (a "flower") |
| Pool | `62.171.141.136:8444` | Official mining pool |
| RPC (localhost) | `127.0.0.1:8443` | Local node RPC port |

---

## Glossary

| Term | Meaning |
|------|---------|
| **Blockchain** | Public ledger of all transactions, shared across computers |
| **Node** | Computer running ZION software that stores/verifies the chain |
| **Mining** | Using computer power to secure the network and earn ZION |
| **Wallet** | File holding your private keys — lets you send/receive ZION |
| **Mnemonic** | 24 words that restore your wallet — write down, never share |
| **Block** | Group of transactions added to the chain every ~60s |
| **Pool** | Group of miners working together — rewards split among all |
| **ZION** | The currency of this blockchain |
| **Ekam Deeksha** | ZION custom PoW algorithm (BLAKE3 + RandomNPU) |
| **Flowers** | Smallest unit: 1 ZION = 1,000,000 flowers |
| **Mainnet Beta** | Live network, may have bugs — mine at your own risk |

---

## Build from Source

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/Zion-TerraNova/v3-Mainnet.git
cd v3-Mainnet/V3
cargo build --release -p zion-public

# Binary: target/release/zion
```

---

## Verification

SHA256 checksums in `SHA256SUMS.txt`:

```bash
# Linux / macOS
shasum -a 256 zion-cli-macos-aarch64.tar.gz
# Compare with SHA256SUMS.txt

# Windows
Get-FileHash zion-cli-windows-x86_64.zip -Algorithm SHA256
```

---

## Documentation

| Resource | Link |
|----------|------|
| **README** | [README.md](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/README.md) |
| **CLI Guide** | [V3/docs/CLI_GUIDE.md](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/V3/docs/CLI_GUIDE.md) |
| **CLI Reference** | [V3/docs/CLI_REFERENCE.md](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/V3/docs/CLI_REFERENCE.md) |
| **Whitepaper** | [docs/whitepaper.md](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/docs/whitepaper.md) |
| **Website** | [zionterranova.com](https://www.zionterranova.com) |

---

## License

MIT — see [LICENSE](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/LICENSE).

---

<div align="center">

**ZION — Multichain Dharma Ecosystem**

*Built with care, secured by consensus.*

*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*

*Peace & One Love 4ever.*

</div>
