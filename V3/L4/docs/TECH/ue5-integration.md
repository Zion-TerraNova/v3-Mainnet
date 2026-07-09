# UE5 Integration

> Unreal Engine 5.4 client for OASIS — architecture, blueprints, and blockchain bridge.

---

## Project Structure

```
V3/L4/oasis/ue5/
├── ZionOasis.uproject
├── Config/
│   ├── DefaultEngine.ini
│   ├── DefaultGame.ini
│   └── DefaultInput.ini
├── Content/
│   ├── Blueprints/
│   │   ├── Game/BP_ZionOasisGameMode.uasset
│   │   ├── Game/BP_GoldenEggManager.uasset
│   │   ├── Game/BP_TerritoryManager.uasset
│   │   ├── Player/BP_ZionCharacter.uasset
│   │   ├── Player/BP_ZionPlayerController.uasset
│   │   └── UI/BP_ZionHUD.uasset
│   ├── DataTables/
│   │   ├── UE5_AvatarDataTable.csv
│   │   └── UE5_AvatarQuestTable.csv
│   ├── Input/InputActions.json
│   └── Maps/
│       ├── LV_MainMenu.umap
│       └── LV_World.umap
└── Source/ZionOasis/
    ├── Avatar/AvatarTypes.h
    ├── Blockchain/ZionBlockchainBridge.cpp
    ├── Blockchain/ZionBlockchainBridge.h
    ├── Consciousness/ConsciousnessComponent.cpp
    ├── Consciousness/ConsciousnessComponent.h
    ├── Consciousness/ConsciousnessTypes.h
    ├── Game/ZionGameInstance.cpp
    ├── Game/ZionOasisGameMode.cpp
    ├── GoldenEgg/GoldenEggManager.cpp
    ├── Guild/GuildComponent.cpp
    ├── Guild/GuildTypes.h
    ├── Player/ZionCharacter.cpp
    ├── Player/ZionPlayerController.cpp
    ├── Territory/TerritoryManager.cpp
    └── UI/ZionHUD.cpp
```

---

## Blockchain Bridge (C++)

`ZionBlockchainBridge` is a singleton ActorComponent that handles all L1 communication:

### Methods

```cpp
// Verify wallet ownership
UFUNCTION(BlueprintCallable)
bool VerifyWallet(const FString& Address, const FString& Signature);

// Query ZION balance (flowers)
UFUNCTION(BlueprintCallable)
int64 GetBalance(const FString& Address);

// Request block-mined XP hook (server calls this on new block)
UFUNCTION(BlueprintCallable)
bool RequestXpHook(const FString& Address, int64 BlockHeight);

// Submit transaction memo (e.g. clue discovery)
UFUNCTION(BlueprintCallable)
FString SubmitMemo(const FString& Address, const FString& Memo);
```

### HTTP Backend

UE5 talks to `zion-oasis` Rust server (not directly to L1 node) to avoid CORS and simplify authentication:

```
UE5 Client → HTTP JSON → zion-oasis:8094 → JSON-RPC → zion-core:8443
```

---

## Blueprints

### BP_ZionOasisGameMode

- Handles player spawn, territory initialization, Golden Egg state
- Calls `TerritoryManager` on BeginPlay
- Caches `AvatarDataTable` and `QuestDataTable`

### BP_ZionCharacter (MetaHuman)

- Third-person character with consciousness aura particle system
- Avatar skin swaps based on equipped NFT
- Animation blueprint: idle, walk, run, meditate, combat

### BP_ZionPlayerController

- Input mapping: WASD + gamepad + Enhanced Input
- Wallet login flow (web3 modal → signature → server verify)
- HUD updates: XP bar, CL badge, guild banner, mini-map

### BP_ZionHUD

- UMG widgets:
  - `W_PlayerProfile` — avatar, name, CL, XP, streak
  - `W_QuestLog` — active quests, objectives, rewards
  - `W_GuildPanel` — members, territory, war status
  - `W_GoldenEggTracker` — clues found, keys, hint button
  - `W_Chat` — global + guild + territory channels

---

## Data Tables

### UE5_AvatarDataTable.csv

| ID | Name | Ray | CLRequired | Rarity | MeshPath | AbilityBP |
|----|------|-----|-----------|--------|----------|-----------|
| 00 | Krishna-Maitreya | All | 9 | Legendary | ... | BP_CosmicVision |
| 01 | Rama | Blue | 4 | Epic | ... | BP_DharmaShield |
| ... | ... | ... | ... | ... | ... | ... |

### UE5_AvatarQuestTable.csv

| QuestID | AvatarID | Step | Title | ObjectiveType | Target | RewardXP |
|---------|----------|------|-------|-------------|--------|----------|
| 0101 | 01 | 1 | The Exile Test | Choice | PowerVsDharma | 200 |
| ... | ... | ... | ... | ... | ... | ... |

---

## Maps

### LV_MainMenu

- Cinematic intro (EKAM Temple fly-through)
- Wallet login widget
- Server selection (mainnet / testnet / local)
- Settings (graphics, audio, controls)

### LV_World

- 8 territories as persistent level streaming
- Day/night cycle synced to real-world UTC
- Dynamic weather per territory (Himalayas = snow, Vrindavan = spring)
- Consciousness beacon VFX at guild-held territories
- Hidden EKAM Temple dimension (CL 9 + 3 keys unlock)

---

## Build & Run

### Windows

```powershell
# Generate VS project files
.\ue5\GenerateProjectFiles.ps1

# Build
.\ue5\Build-Oasis.ps1

# Launch editor
.\ue5\RunEditor.ps1
```

### macOS / Linux

```bash
# Generate project files
./ue5/GenerateProjectFiles.sh

# Build
./ue5/Build-Oasis.sh

# Launch editor
./ue5/RunEditor.sh
```

---

## Network Requirements

| Service | Port | Protocol | Note |
|---------|------|----------|------|
| zion-oasis backend | 8094 | HTTP | REST API |
| zion-oasis backend | 8095 | WebSocket | Real-time events |
| zion-core (local) | 8443 | JSON-RPC | Blockchain queries |

UE5 must have internet access for:
- MetaHuman identity service (optional, can use local assets)
- L1 node / pool (if running remote)
