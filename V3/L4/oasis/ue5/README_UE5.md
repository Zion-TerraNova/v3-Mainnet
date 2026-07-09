# ZION OASIS — Unreal Engine 5 Setup Guide

## Prerequisites

- **Unreal Engine 5.4** (installed via Epic Games Launcher or built from source)
- **Visual Studio 2022** (Windows) with "Game development with C++" workload
- **ZION OASIS Rust backend** (see below)
- **Git LFS** (for future large assets)

## Project Structure

```
ue5/
├── Config/                     # Engine / Game / Input .ini files
├── Content/
│   ├── Blueprints/             # BP definitions (manual creation required)
│   │   ├── Game/
│   │   ├── Player/
│   │   └── UI/
│   ├── DataTables/             # CSV imports for avatars & quests
│   ├── Input/                  # Enhanced Input JSON reference
│   └── Maps/                   # Level files (manual creation required)
├── Source/
│   └── ZionOasis/              # C++ game module
│       ├── Avatar/
│       ├── Blockchain/
│       ├── Consciousness/
│       ├── Game/
│       ├── GoldenEgg/
│       ├── Guild/
│       ├── Player/
│       ├── Territory/
│       ├── UI/
│       └── ZionOasis.Build.cs
├── ZionOasis.uproject
└── README_UE5.md               # This file
```

## Quick Start

### 1. Generate Project Files

**Windows (PowerShell):**
```powershell
.\GenerateProjectFiles.ps1
```

**Linux / macOS:**
```bash
chmod +x GenerateProjectFiles.sh
./GenerateProjectFiles.sh
```

### 2. Open in IDE & Build

- Open `ZionOasis.sln` in **Visual Studio 2022**
- Set **Solution Configuration** to `Development Editor`
- Build → Build Solution (`Ctrl+Shift+B`)

### 3. Create Required Blueprints

> Blueprints are `.uasset` binary files and must be created inside the Editor.

Create these Blueprints (right-click in Content Browser → Blueprint Class):

| Blueprint | Parent Class | Path |
|-----------|-------------|------|
| `BP_ZionOasisGameMode` | `ZionOasisGameMode` | `/Game/Blueprints/Game/` |
| `BP_ZionCharacter` | `ZionCharacter` | `/Game/Blueprints/Player/` |
| `BP_ZionPlayerController` | `ZionPlayerController` | `/Game/Blueprints/Player/` |
| `BP_ZionHUD` | `ZionHUD` | `/Game/Blueprints/UI/` |
| `BP_GoldenEggManager` | `GoldenEggManager` | `/Game/Blueprints/Game/` |
| `BP_TerritoryManager` | `TerritoryManager` | `/Game/Blueprints/Game/` |

### 4. Configure BP_ZionCharacter

In `BP_ZionCharacter` defaults:
- **Mesh**: Assign a skeletal mesh (MetaHuman or UE5 Mannequin)
- **CameraBoom**: Target Arm Length = `400`, Use Pawn Control Rotation = `true`
- **FollowCamera**: Use Pawn Control Rotation = `false`
- **Default Mapping Context**: Create `IMC_ZionDefault` (see Input setup)

### 5. Setup Enhanced Input

Create in Content Browser:

**Input Actions** (`Content/Input/`):
- `IA_Move` (Axis2D)
- `IA_Look` (Axis2D)
- `IA_Jump` (Digital)
- `IA_Meditate` (Digital)
- `IA_Interact` (Digital)
- `IA_Sprint` (Digital)
- `IA_ToggleMap` (Digital)
- `IA_ToggleQuestLog` (Digital)

**Input Mapping Context** (`IMC_ZionDefault`):
| Action | Key | Trigger |
|--------|-----|---------|
| IA_Move | W/A/S/D, Left Stick | |
| IA_Look | Mouse, Right Stick | |
| IA_Jump | Space, A/Cross | |
| IA_Meditate | E, X/Square | |
| IA_Interact | F, B/Circle | |
| IA_Sprint | Left Shift, Left Trigger | |

### 6. Create Game Level

Create two maps (`File → New Level`):

- **`LV_MainMenu`** — Simple level with camera, sky, login UI
- **`LV_World`** — Open world with PlayerStart, BP_GoldenEggManager, BP_TerritoryManager

Set in **Project Settings → Maps & Modes**:
- Default GameMode: `BP_ZionOasisGameMode`
- Editor Startup Map: `LV_World`
- Game Default Map: `LV_MainMenu`

### 7. Start Rust Backend

From repo root:
```bash
cargo run --manifest-path V3/Cargo.toml -p zion-oasis
```

Or with Docker:
```bash
cd V3/L4/oasis
docker compose up -d
```

Backend runs on:
- REST API: `http://localhost:8094`
- WebSocket: `ws://localhost:8095`
- Metrics: `http://localhost:9101`

### 8. Launch Editor

```powershell
.\RunEditor.ps1 -Map LV_MainMenu
```

## Data Import

### Avatar DataTable

Import `Content/DataTables/UE5_AvatarDataTable.csv`:
1. Content Browser → Right-click → `Miscellaneous → Data Table`
2. Row Structure: Create `FAvatarRow` (auto-detected from AvatarTypes.h)
3. Import the CSV

### Avatar Quest DataTable

Same process with `UE5_AvatarQuestTable.csv` and `FAvatarQuestRow`.

## C++ Module Overview

| Class | Purpose |
|-------|---------|
| `AZionOasisGameMode` | Server authority, wallet login, block-mined broadcast |
| `AZionCharacter` | Player pawn, Consciousness + Guild components |
| `AZionPlayerController` | Wallet connect/disconnect, interaction RPC |
| `UZionBlockchainBridge` | HTTP client to Rust backend (REST API) |
| `UConsciousnessComponent` | XP tracking, 9 levels, daily cap |
| `UGuildComponent` | Guild create/join/leave |
| `AGoldenEggManager` | 108-clue treasure hunt |
| `ATerritoryManager` | 8 genesis territories |
| `AZionHUD` | UMG widget stack manager |

## Troubleshooting

| Issue | Fix |
|-------|-----|
| "Cannot find module ZionOasis" | Regenerate project files, ensure `ZionOasis` is in `.uproject` Modules |
| Build fails on `CommonUI` / `CommonGame` | Enable plugins in Edit → Plugins → Common UI / Common Game |
| `GameplayAbilities` not found | Enable Gameplay Abilities plugin |
| Backend connection refused | Ensure `zion-oasis` is running on port 8094 |
| WebSocket not connecting | Check `ws_port` in config (default 8095) |

## Next Steps

1. Add MetaHuman character assets
2. Create UMG widgets (login, HUD, quest dialog)
3. Implement L1 blockchain listener for real-time block-mined XP
4. Add Niagara VFX for meditation / level-up
5. Populate 51 avatar NPCs with dialog trees

---

For Rust backend API docs, see `../src/server.rs` (line 1-100 endpoint table).
