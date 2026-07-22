# ZION v3.0.6-beta — Zion Trinity · Triple Stream

**Mine ZION. Earn ZION. Grow ZION.**

> **⚠️ Mainnet Beta — mine and transact at your own risk**
> The network is live and producing blocks. Genesis chain is permanent. Official public launch: **31 December 2026**.

---

## What's new in v3.0.6 — Zion Trinity?

### Zion Trinity · Triple Stream Engine

The ZION v3.0.6 miner introduces our proprietary **Triple Stream** mining
engine — your GPU and CPU work together to maximize your ZION earnings.

- **No exchanges, no selling, no price dumps**
- **Every hash you compute grows your ZION position**
- **Every hash deepens the ZION liquidity pool**

### Zion Liquidity

Traditional mining: mine a coin → sell on exchange → dump price → exit.

**Zion Liquidity inverts this:** mine → earn ZION → hold ZION → liquidity
grows. The pool handles all conversions internally — you never touch an
exchange, and there is zero sell pressure on ZION.

### Zion Grow

The longer you mine, the more ZION you hold. Every hash compounds your
position. No trading, no timing the market — just mine and grow.

### Performance

Optimized GPU kernels for AMD RDNA (RX 5000/6000 series):

| GPU | Algorithm | Hashrate |
|-----|-----------|----------|
| RX 5600 XT | Deeksha Lite v1 | 34 KH/s (solo) / 17 KH/s (Triple Stream) |
| RX 5700 XT | Deeksha Lite v1 | 28-30 KH/s |

---

## Download

### Which file should I download?

| Your system | File | Size | SHA256 |
|-------------|------|------|--------|
| **Linux x86_64** (Ubuntu, Debian, etc.) | `zion-miner-linux-x86_64.tar.gz` | 3.4 MB | `e4bcc7f4…` |
| **Linux ARM64** (Raspberry Pi 4/5, Ampere, Graviton) | `zion-miner-linux-aarch64.tar.gz` | 2.6 MB | `d44b1aa2…` |
| **macOS Apple Silicon** (M1/M2/M3/M4) | `zion-miner-macos-aarch64.tar.gz` | 3.3 MB | `2f9e330c…` |
| **macOS Intel** (pre-2020 Macs) | `zion-miner-macos-x86_64.tar.gz` | 3.4 MB | `76e47559…` |
| **Windows x86_64** (10/11) | `zion-miner-windows-x86_64.tar.gz` | 3.6 MB | `2af84f14…` |

> **All 5 platforms are now available!** Full SHA256 checksums are in
> `SHA256SUMS.txt` (download alongside the binary).
>
> **GPU support:** Linux x86_64 and macOS builds include OpenCL/Metal GPU
> acceleration. Windows and Linux ARM64 builds are CPU-only (GPU support
> coming in a future release).

---

## Quick Start — 3 steps to mining

### Step 1: Download and extract

```bash
# Download (Linux x86_64 example — choose the right file for your platform)
wget https://github.com/Zion-TerraNova/v3-Mainnet/releases/download/v3.0.6-beta/zion-miner-linux-x86_64.tar.gz

# Verify SHA256
sha256sum zion-miner-linux-x86_64.tar.gz
# Should match SHA256SUMS.txt

# Extract
tar xzf zion-miner-linux-x86_64.tar.gz
chmod +x zion-miner
```

> **Windows:** Extract the `.tar.gz` with 7-Zip or `tar -xzf` (Windows 10+
> has built-in `tar`). The binary is `zion-miner.exe`.
>
> **macOS:** `tar xzf zion-miner-macos-aarch64.tar.gz && chmod +x zion-miner`
> (Apple Silicon) or `zion-miner-macos-x86_64.tar.gz` (Intel).

### Step 2: Create your wallet

If you already have a ZION wallet (from v3.0.5-beta), skip to Step 3.

```bash
# Download the community CLI to create a wallet
# (the v3.0.6 miner binary focuses on mining — wallet creation is in the CLI)
wget https://github.com/Zion-TerraNova/v3-Mainnet/releases/download/v3.0.5-beta/zion-cli-linux-x86_64.tar.gz
tar xzf zion-cli-linux-x86_64.tar.gz
chmod +x zion

# Create wallet with 24-word recovery phrase
# ⚠️ WRITE DOWN the 24 words on paper — they are your only backup!
./zion wallet new --mnemonic --out my-wallet.json

# View your address (starts with "zion1...")
./zion wallet info --wallet my-wallet.json
```

### Step 3: Start mining

```bash
# Start mining to the official pool
./zion-miner \
    --pool 62.171.141.136:8444 \
    --wallet zion1YOUR_WALLET_ADDRESS \
    --worker my-rig \
    --gpu opencl \
    --algorithm deeksha_lite_v1 \
    --profile pool

# The miner will display a live dashboard:
#   - ZION / Deeksha Lite v1 hashrate
#   - Accepted/rejected shares
#   - Pool height and uptime
```

> **Pool vs Solo:** By default, the miner connects to the official pool
> (`62.171.141.136:8444`). In pool mode, you earn a share of every block.
> Solo mode only pays when *you* find a block — which is rare. **Pool mode
> is recommended.**

> **GPU mining:** The miner auto-detects your GPU. On Linux it uses OpenCL
> (AMD) or CUDA (NVIDIA). You can force a backend with `--gpu opencl|cuda|cpu`.

---

## All Commands

```bash
# Mining
./zion-miner --pool 62.171.141.136:8444 --wallet zion1... --gpu opencl --algorithm deeksha_lite_v1 --profile pool
./zion-miner --profile benchmark --loops 3          # Benchmark mode
./zion-miner --profile pool --loops 999999          # Pool mode (default)

# Options
--pool <addr>          Pool address (default: 62.171.141.136:8444)
--wallet <addr>        Your ZION wallet address
--worker <name>        Worker name (default: local-gpu)
--gpu <backend>        GPU backend: opencl, cuda, cpu (default: opencl)
--algorithm <algo>     Algorithm: deeksha_lite_v1 (default)
--profile <mode>       Mining mode: pool, benchmark (default: pool)
--loops <n>            Number of mining loops (default: 999999)
```

---

## GPU Requirements

| GPU | Minimum | Recommended |
|-----|---------|-------------|
| **AMD** | RX 560 (4GB) | RX 5600 XT / 5700 XT (6GB+) |
| **NVIDIA** | GTX 1060 (6GB) | RTX 3060 (12GB) |
| **Apple Silicon** | M1 (8-core GPU) | M2/M3/M4 Pro+ |
| **RAM** | 8GB | 16GB+ |
| **OS** | Ubuntu 20.04+ / macOS 12+ / Win 10+ | Latest |
| **Driver** | AMDPRO 22.x+ / NVIDIA 525+ | Latest |

> **AMD users:** Install AMDPRO (ROCm) driver for best performance.
> OpenCL 2.0+ required.
>
> **macOS users:** Apple Metal is used for GPU acceleration on Apple Silicon.
> No driver installation needed.
>
> **Windows users:** This build is CPU-only. GPU support (OpenCL) will be
> added in a future release.

---

## What is Zion Trinity · Triple Stream?

**Zion Trinity** is the v3.0.6 release name. **Triple Stream** is ZION's
proprietary mining architecture that maximizes your ZION earnings by
utilizing your entire rig — GPU and CPU — simultaneously.

**You mine ZION. You earn ZION. That's all you need to know.**

Behind the scenes, the pool handles everything else: converting external
coin rewards to ZION, managing liquidity, and ensuring zero sell pressure
on the ZION price.

### Zion Grow

**Zion Grow** is the miner incentive program:

- **Mine continuously** → your ZION balance grows every block
- **No selling required** — the pool handles conversions
- **Compounding position** — the longer you mine, the more ZION you hold
- **Liquidity flywheel** — more miners → more liquidity → more stable price

### Zion Liquidity

**Zion Liquidity** is the liquidity-building mechanism:

- Every hash you compute deepens the ZION liquidity pool
- No exchange dumps — miners never sell ZION on the open market
- The pool converts all rewards to ZION internally
- Result: ZION price stability increases as the network grows

---

## Troubleshooting

### Miner hangs on startup

```bash
# Check GPU is detected
clinfo -l

# If no GPU detected, install AMDPRO driver:
# Ubuntu: sudo apt install rocm-opencl-dev
```

### Low hashrate

```bash
# Check GPU is being used
# The miner dashboard shows "GPU OPENCL" if GPU is active

# Try different work sizes
export ZION_GPU_WORK_SIZE=8192    # Default for 18 CU GPUs
export ZION_NONCE_COUNT=32768     # 4x work_size
```

### Connection refused

```bash
# Check pool is reachable
nc -zv 62.171.141.136 8444

# If connection fails, check your firewall or try again later
```

---

## Verification

```bash
# Verify SHA256 of the download
sha256sum zion-miner-linux-x86_64.tar.gz
# Compare with SHA256SUMS.txt

# Verify the binary runs
./zion-miner --version
# Should print: zion-miner 3.0.6
```

---

## Previous release

- [v3.0.5-beta — Simplified Community CLI](https://github.com/Zion-TerraNova/v3-Mainnet/releases/tag/v3.0.5-beta)
  (single `zion` binary with interactive menu, wallet, node, pool, miner)

---

## Support

- **Documentation:** [docs.zionterranova.com](https://docs.zionterranova.com)
- **Website:** [zionterranova.com](https://zionterranova.com)
- **Pool:** `62.171.141.136:8444`
- **RPC:** `rpc.zionterranova.com:8443`

---

## License

MIT — see [LICENSE](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/LICENSE)

> **Note:** The miner binary is released under MIT license. The Triple Stream
> engine and AuxPow source code are proprietary and not included in the
> public repository. The ZION blockchain core, pool, and community CLI remain
> fully open-source.

---

<div align="center">

**ZION Zion Trinity — Multichain Dharma Ecosystem**

*Built with care, secured by consensus.*

*Gate, Gate, Paragate, Parasamgate, Bodhi Svaha.*

*Peace & One Love 4ever.*

</div>
