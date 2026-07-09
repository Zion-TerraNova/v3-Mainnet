#!/usr/bin/env bash
# ZION OASIS — Full Build Script (Linux/macOS)
# Builds Rust backend + generates UE5 project files

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
V3_ROOT="$SCRIPT_DIR/../.."

DOCKER=false
UE5_ONLY=false
BACKEND_ONLY=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --docker) DOCKER=true; shift ;;
        --ue5-only) UE5_ONLY=true; shift ;;
        --backend-only) BACKEND_ONLY=true; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo "=== ZION OASIS Build ==="
echo ""

# ─── 1. Rust Backend ──────────────────────────────────────────────────────
if [[ "$UE5_ONLY" == "false" ]]; then
    echo "[1/3] Building Rust backend (zion-oasis)..."

    if [[ "$DOCKER" == "true" ]]; then
        echo "    Docker build..."
        docker compose -f "$SCRIPT_DIR/docker-compose.yml" build
    else
        echo "    cargo build --release -p zion-oasis"
        cargo build --manifest-path "$V3_ROOT/Cargo.toml" --release -p zion-oasis
    fi

    echo "    OK"
fi

# ─── 2. UE5 Project Files ─────────────────────────────────────────────────
if [[ "$BACKEND_ONLY" == "false" ]]; then
    echo "[2/3] Generating UE5 project files..."

    UPROJECT="$SCRIPT_DIR/ue5/ZionOasis.uproject"
    ENGINE_PATH="${UE5_ENGINE:-/opt/UnrealEngine/5.4}"

    if [[ "$OSTYPE" == "darwin"* ]]; then
        ENGINE_PATH="${UE5_ENGINE:-/Users/Shared/Epic Games/UE_5.4}"
        UBT="$ENGINE_PATH/Engine/Binaries/Mac/UnrealBuildTool.app/Contents/MacOS/UnrealBuildTool"
    else
        UBT="$ENGINE_PATH/Engine/Binaries/Linux/UnrealBuildTool"
    fi

    if [[ -f "$UBT" ]]; then
        "$UBT" -ProjectFiles -Project="$UPROJECT" -Game -Engine -Progress
        echo "    OK"
    else
        echo "    WARNING: UE5 not found at $ENGINE_PATH — skip project generation"
    fi
fi

# ─── 3. Verify ────────────────────────────────────────────────────────────
echo "[3/3] Verifying build artifacts..."

if [[ -f "$V3_ROOT/target/release/zion-oasis" ]]; then
    echo "    Backend: $V3_ROOT/target/release/zion-oasis"
else
    echo "    WARNING: Backend binary not found"
fi

if [[ -f "$SCRIPT_DIR/ue5/ZionOasis.sln" ]] || [[ -d "$SCRIPT_DIR/ue5/ZionOasis.xcworkspace" ]]; then
    echo "    Project files: OK"
else
    echo "    WARNING: Project files not found"
fi

echo ""
echo "=== Build Complete ==="
echo ""
echo "Next steps:"
echo "  1. Start backend: cargo run -p zion-oasis"
echo "  2. Open project in IDE (VS/Rider/Xcode)"
echo "  3. Build Development Editor"
echo "  4. See ue5/README_UE5.md for Blueprint setup"
