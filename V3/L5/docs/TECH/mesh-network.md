# L5 Mesh Network — LoRa / Meshtastic Specification

> **Off-grid communication for Terra Nova physical communities and inter-node coordination.**

---

## 1. Purpose

The L5 Mesh Network provides **reliable, low-power, long-range communication** that operates independently of commercial internet and cellular infrastructure. It serves:

- **Guest safety** (emergency alerts, location tracking)
- **Farm operations** (sensor data, irrigation control, livestock monitoring)
- **Community coordination** (meetings, announcements, work assignments)
- **Inter-node communication** (seed library logistics, visitor referrals, protocol sync)
- **Resilience** (continuity during internet outages, natural disasters)

---

## 2. Technology Stack

### 2.1 Primary: Meshtastic

| Parameter | Specification |
|-----------|---------------|
| **Protocol** | Meshtastic (open-source, LoRa-based mesh) |
| **Frequency** | EU868 (Europe), EU433 (fallback), ISM band |
| **Modulation** | LoRa (Semtech SX1262) |
| **Range** | 5–15 km line-of-sight, 1–3 km urban/forest |
| **Data rate** | 300 bps – 10 kbps (spreading factor adjustable) |
| **Power** | 0.1–2W (configurable), solar-friendly |
| **Battery life** | 2–7 days ( depends on duty cycle) |
| **Encryption** | AES-256 (default), optional channel-specific keys |

### 2.2 Hardware Options

| Device | Role | Price | Power | Best For |
|--------|------|-------|-------|----------|
| **LilyGo T-Beam 868** | Mobile node, GPS | EUR 25–35 | 0.5W | Guardians, vehicles |
| **LilyGo T-Beam + SMA** | Fixed repeater | EUR 30–40 | 1W | Hilltop, solar-powered |
| **RAK4631 + RAK19007** | Gateway + sensors | EUR 60–80 | 1–2W | Farm hub, Raspberry Pi HAT |
| **WisBlock Meshtastic** | Modular sensor node | EUR 80–120 | 0.5–2W | Soil, weather, livestock |
| **SenseCAP T1000-E** | Tracker (wearable) | EUR 40–60 | 0.1W | Guest safety, children |

### 2.3 Gateway / Internet Bridge

| Option | Role | Cost | Use Case |
|--------|------|------|----------|
| **Meshtastic MQTT gateway** | Bridge mesh ↔ internet | RPi Zero 2W + T-Beam | Push sensor data to cloud |
| **Custom L5 Agent gateway** | Bridge mesh ↔ ZION node | Same hardware + Rust daemon | Push to L2 DAO, L4 OASIS |
| **Starlink fallback** | Emergency internet | Existing Starlink | When mesh needs internet |

---

## 3. Network Topology

### 3.1 Single Community (Genesis Garden Example)

```
                    [Hilltop Repeater]
                         │
        ┌────────────────┼────────────────┐
        │                │                │
   [Farm Hub]      [Glamping]       [Workshop]
   (sensors)        (guest alerts)   (tool tracking)
        │                │                │
        └────────────────┼────────────────┘
                         │
              [Guardian Node + L5 Agent]
                         │
                   [Starlink / 4G]
```

### 3.2 Multi-Community (Genesis Garden ↔ Dharma Temple)

```
[Genesis Garden] ◄──────── LoRa link ────────► [Dharma Temple]
      │                                           │
   [Repeater]                                 [Repeater]
   (coastal)                                  (mountain)
      │                                           │
      └─────────── High-altitude repeater ────────┘
                    (mid-Atlantic, shared)
```

**Inter-node range:**
- Genesis Garden (Algarve, sea level) to hilltop repeater: 10–15 km
- Hilltop repeater to Dharma Temple (La Palma, 400m): Not possible directly (ocean)
- **Reality:** Inter-node communication is **not direct LoRa**. It uses:
  1. Internet bridge at each community
  2. Encrypted tunnel (WireGuard / Meshtastic MQTT)
  3. Local mesh for each community independently

**Future:** Satellite LoRa (Swarm / Lacuna / Astrocast) for true ocean-spanning mesh.

---

## 4. Use Cases and Message Types

### 4.1 Guest Safety

| Message Type | Priority | Encryption | Retention |
|--------------|----------|------------|-----------|
| Emergency (SOS) | Critical | Yes | 30 days |
| Location beacon | Normal | Yes (anonymized) | 7 days |
| Check-in request | Normal | Yes | 24 hours |
| Weather alert | High | No (broadcast) | 24 hours |

### 4.2 Farm Operations

| Message Type | Frequency | Payload |
|--------------|-----------|---------|
| Soil moisture | Every 15 min | `{sensor_id, moisture_pct, timestamp}` |
| Solar production | Every 5 min | `{panel_id, watts, timestamp}` |
| Irrigation trigger | Event | `{valve_id, duration_sec, triggered_by}` |
| Livestock position | Every 30 min | `{tag_id, gps_lat, gps_lon, timestamp}` |
| Gate status | Event | `{gate_id, open/closed, timestamp}` |

### 4.3 Community Coordination

| Message Type | Audience | Payload |
|--------------|----------|---------|
| Meeting reminder | All | `{time, location, circle_name}` |
| Work assignment | Sub-circle | `{task, assignee, deadline}` |
| Meal announcement | All | `{menu, time, dietary_notes}` |
| Visitor arrival | Hospitality | `{name, eta, accommodation}` |

### 4.4 Inter-Node

| Message Type | Frequency | Payload |
|--------------|-----------|---------|
| Seed exchange request | Weekly | `{variety, quantity, preferred_date}` |
| Visitor referral | Event | `{name, dates, origin_community}` |
| Protocol version check | Monthly | `{node_version, l5_agent_version}` |
| Treasury sync | Monthly | `{balance_eur, balance_zion, last_tx_date}` |

---

## 5. Message Format

### 5.1 Standard Packet

```protobuf
message L5MeshPacket {
  string packet_id = 1;           // UUID
  string community_id = 2;        // "genesis-garden", "dharma-temple"
  string sender_node_id = 3;      // Hardware node ID
  int64 timestamp = 4;            // Unix epoch (ms)
  string msg_type = 5;            // "emergency", "sensor", "coordination", "inter-node"
  bytes payload = 6;              // JSON or protobuf payload
  bytes signature = 7;            // Ed25519 signature of sender
  int32 ttl = 8;                  // Time-to-live (hops remaining)
}
```

### 5.2 Example: Soil Moisture

```json
{
  "packet_id": "550e8400-e29b-41d4-a716-446655440000",
  "community_id": "genesis-garden",
  "sender_node_id": "tbeam-farm-03",
  "timestamp": 1750000000000,
  "msg_type": "sensor",
  "payload": {
    "sensor_type": "soil_moisture",
    "location": "bed-7-tomatoes",
    "value": 42.5,
    "unit": "pct",
    "battery_voltage": 3.85
  }
}
```

---

## 6. Security

### 6.1 Threat Model

| Threat | Impact | Mitigation |
|--------|--------|------------|
| Eavesdropping | Medium | AES-256 default encryption |
| Spoofing (fake node) | High | Ed25519 signatures on all packets |
| Denial of service (spam) | Medium | Rate limiting, TTL, reputation scoring |
| Physical theft of node | Medium | Tamper-evident cases, remote disable |
| Jamming | High | Frequency hopping (future), directional antennas |

### 6.2 Key Management

```
Community mesh master key (HSM or encrypted flash)
    ├── Channel key 1: Emergency (all nodes)
    ├── Channel key 2: Farm operations (farm nodes only)
    ├── Channel key 3: Guest safety (guest devices + hospitality)
    ├── Channel key 4: Inter-node (gateway nodes only)
    └── Channel key 5: Admin (Guardian nodes only)
```

**Key rotation:** Every 90 days, or immediately after node compromise.

---

## 7. Deployment Guide

### 7.1 Phase 1: Basic Coverage (Community Perimeter)

**Hardware:** 2–3 T-Beam units
**Setup time:** 2–4 hours
**Coverage:** 2–5 hectares

```bash
# 1. Flash Meshtastic firmware
pip install meshtastic
meshtastic --flash --device /dev/ttyUSB0 --firmware meshtastic_firmware_2.3.x.bin

# 2. Configure
meshtastic --set lora.region EU868
meshtastic --set lora.modem_preset LONG_FAST
meshtastic --set device.role CLIENT
meshtastic --set bluetooth.enabled false  # Save power

# 3. Set channel key (from Guardian ceremony)
meshtastic --ch-set psk base64:YOUR_BASE64_KEY --ch-index 0

# 4. Verify
meshtastic --nodes
```

### 7.2 Phase 2: Extended Coverage (Hilltop Repeater)

**Hardware:** T-Beam + 5dBi omni antenna + solar panel (20W) + 12V battery (20Ah)
**Setup time:** 1 day
**Coverage:** 10–15 km radius

**Installation:**
- Mount antenna 5+ meters above ground
- Point antenna toward community center
- Solar panel south-facing (north-facing in southern hemisphere)
- Battery in waterproof box

### 7.3 Phase 3: Sensor Integration

**Hardware:** RAK4631 + BME280 (temp/humidity/pressure) + soil moisture sensor + solar
**Setup time:** 2–3 hours per node
**Density:** 1 sensor node per 0.5 hectares

---

## 8. Maintenance

| Task | Frequency | Time | Responsible |
|------|-----------|------|-------------|
| Battery voltage check | Weekly | 5 min | Tech Guardian |
| Antenna inspection | Monthly | 15 min | Tech Guardian |
| Firmware update | Quarterly | 30 min per node | Tech Guardian |
| Key rotation | Quarterly | 1 hour | Finance + Tech Guardian |
| Full network test | Quarterly | 1 hour | All Guardians |
| Solar panel cleaning | Monthly | 30 min | Operations Guardian |

---

## 9. Future Roadmap

| Feature | Target | Description |
|---------|--------|-------------|
| **Satellite LoRa uplink** | 2028 | Swarm / Lacuna for true ocean-spanning mesh |
| **Directional antennas** | 2027 | Yagi / panel antennas for 30+ km links |
| **Voice over mesh** | 2027 | Codec2 digital voice (Meshtastic experimental) |
| **Offline maps** | 2027 | Community topography + node positions |
| **Cryptocurrency over mesh** | 2028 | ZION lightning-style payments via mesh |
| **Drone relay** | 2029 | Solar drone as airborne mesh repeater |

---

## 10. Reference Links

| Resource | URL |
|----------|-----|
| Meshtastic docs | https://meshtastic.org/docs/ |
| LilyGo T-Beam | https://github.com/LilyGo/LoRa-Series |
| RAK WisBlock | https://docs.rakwireless.com/Product-Categories/WisBlock/ |
| ZION L5 docs | `V3/L5/docs/README.md` |
| Guardian node spec | `V3/L5/docs/TECH/zion-node-spec.md` |

---

> *"When the internet dies, the mesh lives. When the power grid fails, the solar nodes whisper. When the world forgets how to listen, we still hear each other."*

*V3/L5/TECH · Mesh Network Spec · 2026*
