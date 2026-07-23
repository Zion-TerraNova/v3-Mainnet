# ZION v3.0.6-beta — Trinity

**Date:** 2026-07-21

**Mine ZION. Earn ZION. Grow ZION.**

> **⚠️ Mainnet Beta — mine and transact at your own risk**
> The network is live and producing blocks. Genesis chain is permanent. Official public launch: **31 December 2026**.

---

## What's new in v3.0.6 — Trinity?

### Trinity Mining Engine

The ZION v3.0.6 miner introduces our proprietary **Trinity** mining
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

---

## Download

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

## Quick Start

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

### Start mining

```bash
./zion-miner \
    --pool 62.171.141.136:8444 \
    --wallet zion1YOUR_WALLET_ADDRESS \
    --worker my-rig \
    --gpu opencl \
    --algorithm deeksha_lite_v1 \
    --profile pool
```

> **Pool vs Solo:** By default, the miner connects to the official pool
> (`62.171.141.136:8444`). In pool mode, you earn a share of every block.
> Solo mode only pays when *you* find a block — which is rare. **Pool mode
> is recommended.**

---

## Support

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Pool:** `62.171.141.136:8444`
- **RPC:** `rpc.zionterranova.com:8443`

---

## License

MIT — see [LICENSE](https://github.com/Zion-TerraNova/v3-Mainnet/blob/main/LICENSE)

> **Note:** The miner binary is released under MIT license. The Trinity
> engine and AuxPow source code are proprietary and not included in the
> public repository. The ZION blockchain core, pool, and community CLI remain
> fully open-source.
