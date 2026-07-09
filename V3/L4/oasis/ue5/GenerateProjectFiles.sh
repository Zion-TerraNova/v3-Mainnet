#!/usr/bin/env bash
# ZION OASIS UE5 — Generate Project Files (Linux/macOS)
# Requires: Unreal Engine 5.4 built from source or installed

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENGINE_PATH="${UE5_ENGINE:-/opt/UnrealEngine/5.4}"
UPROJECT="$SCRIPT_DIR/ZionOasis.uproject"

# Auto-detect engine path
if [[ "$OSTYPE" == "darwin"* ]]; then
    ENGINE_PATH="${UE5_ENGINE:-/Users/Shared/Epic Games/UE_5.4}"
    UBT="$ENGINE_PATH/Engine/Binaries/Mac/UnrealBuildTool.app/Contents/MacOS/UnrealBuildTool"
else
    UBT="$ENGINE_PATH/Engine/Binaries/Linux/UnrealBuildTool"
fi

if [[ ! -f "$UBT" ]]; then
    echo "ERROR: UnrealBuildTool not found at: $UBT"
    echo "Set UE5_ENGINE environment variable or install UE 5.4"
    exit 1
fi

echo "Generating project files for ZION OASIS..."
echo "Engine: $ENGINE_PATH"
echo "UProject: $UPROJECT"

"$UBT" -ProjectFiles -Project="$UPROJECT" -Game -Engine -Progress

echo "Success! Open ZionOasis.xcworkspace (macOS) or build with make (Linux)"
