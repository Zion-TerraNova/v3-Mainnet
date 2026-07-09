# ZION CLI

<div align="center">

**Wallet, node, and miner gateway for the ZION network**

[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

</div>

---

## Overview

`zion` is the command-line interface for interacting with the ZION L1 blockchain.
It provides wallet management, node inspection, mining control, and pool monitoring —
all from a single binary.

## Installation

### Build from source

```bash
cd V3
cargo build -p zion-cli --release
# Binary: target/release/zion
```

### Shell completions

```bash
# Bash
eval "$(zion completions bash)"

# Zsh
eval "$(zion completions zsh)"

# Fish
zion completions fish | source

# PowerShell
zion completions powershell | Out-String | Invoke-Expression
```

## Commands

### Wallet

```bash
# Generate a new wallet (mnemonic-backed, 24 words)
zion wallet new --mnemonic --out zion-wallet.json

# Generate with encryption (password from env var)
export ZION_WALLET_PASSWORD="your-password"
zion wallet new --mnemonic --password-env ZION_WALLET_PASSWORD

# Import from mnemonic
zion wallet import-mnemonic --mnemonic "word1 word2 ... word24" --out zion-wallet.json

# Import from raw secret key (32-byte hex)
zion wallet import-secret-key --secret-key-hex <64hex> --out zion-wallet.json

# Show wallet info
zion wallet info --wallet zion-wallet.json

# Reveal decrypted secrets (requires password env var)
zion wallet reveal --wallet zion-wallet.json --password-env ZION_WALLET_PASSWORD

# Show configured wallet address
zion wallet address

# Check balance
zion wallet balance
zion wallet balance --address zion1...

# Send ZION
zion wallet send --to zion1... --amount 1.5 --wallet zion-wallet.json
zion wallet send --to zion1... --amount 1.5 --memo "payment" --wallet zion-wallet.json
```

### Node

```bash
# Node status (height, peers, mempool)
zion node status

# List connected peers
zion node peers

# Last N blocks
zion node blocks 10

# Block by height or hash
zion node block 12345
zion node block <hash>

# Transaction lookup
zion node tx <txid>

# Mempool info
zion node mempool

# Force peer sync
zion node sync

# Raw JSON-RPC call
zion node rpc getChainInfo '{}'
zion node rpc getBalance '{"address":"zion1..."}'

# Export chain snapshot
zion node snapshot --output snapshot.json

# WebSocket subscriptions
zion node websocket subscribe new_blocks
zion node websocket listen
```

### Mining

```bash
# Start mining (pool mode, uses config defaults)
zion mine start

# Start with options
zion mine start --pool pool.zionterranova.com:8444 --wallet zion1... --backend cuda --profile pool

# CPU benchmark
zion mine bench

# GPU benchmark
zion mine bench --gpu --backend cuda
zion mine bench --gpu --backend metal --work-size 256 --secs 10

# Ekam Deeksha benchmark
zion mine bench --ekam --backend cuda --secs 30

# Mining status
zion mine status

# Stop mining
zion mine stop

# DCR stealth worker
zion mine dcr status
zion mine dcr start
zion mine dcr stop
```

### Pool

```bash
# Pool stats
zion pool stats

# Active workers
zion pool miners

# Pool config
zion pool config

# PPLNS earnings
zion pool earnings
zion pool earnings --address zion1...
```

### Config

```bash
# Show config
zion config show

# Config file path
zion config path

# Set a value
zion config set miner.wallet zion1...
zion config set pool.host pool.zionterranova.com
zion config set miner.backend cuda

# Validate config
zion config validate

# Run onboarding wizard
zion config init
```

### Other

```bash
# Interactive menu (arrow-key navigation)
zion menu
# Or just: zion

# Version info
zion version

# Check for CLI updates
zion update --check

# Apply CLI update
zion update --yes

# Health check
zion status

# Preflight diagnostics
zion doctor
```

## Configuration

Config file: `~/.zion/zion.toml`

```toml
[node]
rpc_host = "127.0.0.1"
rpc_port = 8443
p2p_port = 8333
websocket_port = 8445

[pool]
host = "pool.zionterranova.com"
port = 8444

[miner]
wallet = "zion1..."
btc_wallet = ""
threads = "auto"
backend = "auto"        # auto | cpu | gpu | metal | opencl | cuda
profile = "pool"        # pool | solo | benchmark | dual
algorithm = "deeksha_lite_v1"  # deeksha_lite_v1 | deeksha_lite_fire | cosmic_harmony_ekam_deeksha_v2

[cli]
auto_update_check = true
```

## Security

- **Never store private keys in config files.** Use wallet files with encryption.
- **Use `--password-env` for wallet encryption/decryption.** The password is read
  from an environment variable, never from a command-line argument.
- **Wallet files use AES-256-GCM + PBKDF2-SHA256** (210,000 iterations) for encryption.

## License

MIT
