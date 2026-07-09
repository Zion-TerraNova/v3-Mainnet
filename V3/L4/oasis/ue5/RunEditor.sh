#!/usr/bin/env bash
# ZION OASIS UE5 — Launch Unreal Editor (Linux/macOS)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENGINE_PATH="${UE5_ENGINE:-/opt/UnrealEngine/5.4}"
UPROJECT="$SCRIPT_DIR/ZionOasis.uproject"
MAP="${1:-LV_MainMenu}"

if [[ "$OSTYPE" == "darwin"* ]]; then
    ENGINE_PATH="${UE5_ENGINE:-/Users/Shared/Epic Games/UE_5.4}"
    EDITOR="$ENGINE_PATH/Engine/Binaries/Mac/UnrealEditor.app/Contents/MacOS/UnrealEditor"
else
    EDITOR="$ENGINE_PATH/Engine/Binaries/Linux/UnrealEditor"
fi

if [[ ! -f "$EDITOR" ]]; then
    echo "ERROR: Unreal Editor not found at: $EDITOR"
    exit 1
fi

echo "Launching ZION OASIS Editor..."
echo "Map: $MAP"

"$EDITOR" "$UPROJECT" "$MAP"
