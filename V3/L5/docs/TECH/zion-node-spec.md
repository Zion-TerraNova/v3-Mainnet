# L5 Guardian Node — Technical Specification

> **Hardware and software specification for ZION blockchain nodes operating at L5 community locations.**

---

## 1. Purpose

The L5 Guardian Node is a **ZION full node** deployed at a Terra Nova physical community. It:
- Validates blocks and participates in network consensus
- Earns block rewards (shared 90/10 with community treasury)
- Provides local RPC endpoint for community applications
- Serves as a LoRa/mesh network gateway (future)
- Stores an immutable, censorship-resistant record of community transactions

---

## 2. Hardware Specification

### 2.1 Minimum Specification (Phase 1)

| Component | Requirement | Example |
|-----------|-------------|---------|
| **CPU** | x86-64, 4 cores, 2.0 GHz+ | Intel N100, AMD Ryzen 3 4300U |
| **RAM** | 16 GB DDR4/DDR5 | Crucial CT16G4SFRA32A |
| **Storage** | 2 TB NVMe SSD, TLC, DRAM cache | Samsung 990 EVO, WD Black SN850X |
| **Network** | Ethernet 1 Gbps + WiFi 6 | Dual NIC preferred |
| **Power** | 12–19V DC, 15–25W average | DC-DC converter from solar battery |
| **Case** | Fanless or low-noise, IP54+ | Akasa Turing A50, Silverstone PT13 |
| **UPS** | 2–4 hour battery backup | Small LiFePO4 pack (12V 20Ah) |

### 2.2 Recommended Specification (Phase 2+)

| Component | Requirement | Example |
|-----------|-------------|---------|
| **CPU** | x86-64, 6–8 cores, 3.0 GHz+ | AMD Ryzen 5 5600U, Intel Core i5-1240P |
| **RAM** | 32 GB DDR4/DDR5 | |
| **Storage** | 4 TB NVMe SSD + 4 TB SATA SSD (backup) | Samsung 990 Pro + Samsung 870 QVO |
| **Network** | Ethernet 2.5 Gbps + WiFi 6E + 4G/5G modem | Quectel RM500Q (5G) |
| **Power** | 12–24V DC, 20–35W average | |
| **Redundancy** | Dual storage (RAID 1), dual power input, dual network | |

### 2.3 Power Budget Context

| Community type | Solar capacity | Node share of daily energy |
|---------------|----------------|---------------------------|
| Small farm (5 kWp) | ~20 kWh/day | 1–2% |
| Medium farm (15 kWp) | ~60 kWh/day | <1% |
| Hydro-solar hybrid | Variable | Negligible |

**Conclusion:** A ZION node is **not a significant energy burden** for any L5 community with basic solar. It consumes less than a refrigerator.

---

## 3. Software Specification

### 3.1 Operating System

| Option | Pros | Cons | Recommendation |
|--------|------|------|----------------|
| **Ubuntu Server LTS** | Widely supported, ZION tested | Higher resource usage | Default choice |
| **Debian Stable** | Lean, stable, same package base | Slightly older packages | Alternative |
| **Alpine Linux** | Very small (~130 MB), secure | Non-glibc, some Rust crates need work | Future optimization |
| **NixOS** | Reproducible, declarative | Steep learning curve | For tech-savvy Guardians |

**Current default:** Ubuntu Server 22.04 LTS or 24.04 LTS (when ZION CI validates)

### 3.2 ZION Node Software

| Component | Source | Version |
|-----------|--------|---------|
| **zion-core node** | `V3/L1/core` (this repo) | Latest `main` or tagged release |
| **Runtime** | Rust (rustc 1.78+) | See `V3/rust-toolchain.toml` |
| **State database** | LMDB or SQLite (configurable) | Embedded |
| **P2P** | Custom ZION protocol over TCP/UDP | Port 8333 default |
| **RPC** | JSON-RPC over HTTP | Port 8443 default |

### 3.3 Monitoring and Telemetry

| Tool | Purpose | Integration |
|------|---------|-------------|
| **Prometheus** | Metrics collection (blocks, peers, mempool) | Node exports `/metrics` |
| **Grafana** | Visualization | Pre-built ZION dashboard |
| **Victron VRM** | Solar/battery monitoring (if Victron used) | Optional |
| **Uptime Kuma** | External uptime checks | Alerts to Guardian |
| **Custom L5 agent** | Send node health to community dashboard | Rust agent, ZION gRPC |

---

## 4. Network Architecture

### 4.1 Connectivity Stack

```
Internet
    ├── Starlink / 4G / Fiber (primary)
    ├── 4G failover (dual-SIM modem)
    └── LoRa mesh (local only, no internet)

Router (OpenWrt / pfSense)
    ├── ZION Node (wired, static IP)
    ├── Community WiFi (VLAN-isolated)
    ├── Management LAN (Guardians only)
    └── LoRa Gateway (USB/Serial)
```

### 4.2 Port Requirements

| Port | Protocol | Purpose | Direction |
|------|----------|---------|-----------|
| 8333 | TCP/UDP | ZION P2P | Inbound + outbound |
| 8443 | TCP | ZION RPC | Local only (or VPN) |
| 8444 | TCP | Pool stratum (if pool co-located) | Local only |
| 9090 | TCP | Prometheus metrics | Local only |
| 22 | TCP | SSH (management) | Local + VPN only |

### 4.3 Security

| Layer | Measure |
|-------|---------|
| **Physical** | Locked case, tamper-evident seals, UPS-protected |
| **Network** | Firewall (nftables/iptables), VLAN isolation, no public RPC |
| **OS** | Automatic security updates, fail2ban, key-based SSH only |
| **Application** | ZION node runs as unprivileged user, sandboxed (future: systemd namespaces) |
| **Keys** | Node keys in TPM 2.0 or encrypted USB (YubiKey) |

---

## 5. Installation Procedure

### 5.1 Step-by-Step

```bash
# 1. Prepare hardware
#    - Assemble mini-PC, install NVMe, connect network
#    - Connect to solar battery via DC-DC converter (12V→19V)

# 2. Install OS
#    - Flash Ubuntu Server LTS to USB
#    - Install, enable automatic updates
#    - Create non-root user: zion-node

# 3. Install dependencies
sudo apt update
sudo apt install -y build-essential git curl pkg-config libssl-dev

# 4. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 5. Clone and build ZION
git clone https://github.com/Yose144/2.9.6.git
cd 2.9.6-main/V3
cargo build --release -p zion-core --bin node

# 6. Configure node
mkdir -p ~/.zion
# Copy example config, edit:
#   node_id = "[community-name]-guardian-01"
#   p2p_bind = "0.0.0.0:8333"
#   rpc_bind = "127.0.0.1:8443"
#   state_path = "/var/lib/zion/state.db"

# 7. Create systemd service
sudo tee /etc/systemd/system/zion-node.service << 'EOF'
[Unit]
Description=ZION Guardian Node
After=network.target

[Service]
Type=simple
User=zion-node
ExecStart=/home/zion-node/2.9.6-main/V3/target/release/node
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now zion-node

# 8. Verify
systemctl status zion-node
journalctl -u zion-node -f
```

### 5.2 Docker Alternative (Not Recommended for Production)

```bash
# For testing only — L5 nodes should run bare-metal for reliability
docker run -d \
  --name zion-node \
  -p 8333:8333 \
  -v /var/lib/zion:/data \
  zion-core:latest \
  --node-id [community-name] \
  --p2p-bind 0.0.0.0:8333 \
  --state-path /data/state.db
```

---

## 6. Maintenance

### 6.1 Daily (Automated)

| Check | Tool | Alert if |
|-------|------|----------|
| Node process running | systemd | Process down |
| Disk space > 80% | custom script | Disk full |
| Network connectivity | ping | No internet |
| Block height sync | ZION RPC | > 10 blocks behind |

### 6.2 Weekly (Guardian)

| Task | Time |
|------|------|
| Review Grafana dashboard | 5 min |
| Check ZION software updates | 10 min |
| Verify backup integrity | 10 min |
| Clean logs if > 1 GB | 5 min |

### 6.3 Monthly

| Task | Time |
|------|------|
| OS security updates | 30 min |
| Full state backup to external drive | 1 hour |
| Review revenue / treasury split | 15 min |
| Test failover (4G, UPS) | 30 min |

### 6.4 Quarterly

| Task | Time |
|------|------|
| Hardware inspection (dust, connections, fans) | 1 hour |
| Disaster recovery drill (restore from backup) | 2 hours |
| Capacity planning (disk, bandwidth) | 30 min |

---

## 7. Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|-------------|-----|
| Node won't start | Corrupt state DB | Restore from backup or resync |
| High CPU usage | Mempool overload | Restart node, check for spam |
| Disk full | Log bloat | Rotate logs, `journalctl --vacuum-size=100M` |
| No peers | Firewall / NAT | Check port 8333 forwarding |
| Slow sync | Low bandwidth / CPU throttling | Check power, network, `nice` priority |
| RPC timeout | State DB locked | Wait for compaction, restart |

---

## 8. Future Roadmap

| Feature | Target | Description |
|---------|--------|-------------|
| **Solar-aware scheduling** | 2027 | Node adjusts validation intensity based on battery SOC |
| **LoRa gateway integration** | 2027 | Node serves as Meshtastic gateway for community mesh |
| **Satellite failover** | 2028 | Starlink as primary, Iridium / Swarm as backup |
| **Light client mode** | 2028 | Low-resource mode for very small communities |
| **TPM 2.0 key storage** | 2028 | Hardware-protected node keys |
| **Inter-node channels** | 2029 | Payment channels between L5 nodes for fast settlement |

---

## 9. Reference Links

| Resource | URL |
|----------|-----|
| ZION Core source | `V3/L1/core/` |
| AGENTS.md (build commands) | `AGENTS.md` |
| Revenue constants | `V3/L1/cosmic-harmony/src/revenue.rs` |
| Pool PPLNS | `V3/L1/pool/src/pplns.rs` |
| Prometheus exporter (future) | `V3/L1/core/src/metrics.rs` |

---

> *"The node is not a server in a data center. It is a seed in the ground — small, quiet, but essential to the whole network."*

*V3/L5/TECH · Guardian Node Spec · 2026*
