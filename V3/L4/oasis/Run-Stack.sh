#!/usr/bin/env bash
# ZION OASIS — Full Stack Launcher (Linux/macOS)

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKER=false
NO_UE5=false
MONITORING=false
MAP="LV_MainMenu"

while [[ $# -gt 0 ]]; do
    case $1 in
        --docker) DOCKER=true; shift ;;
        --no-ue5) NO_UE5=true; shift ;;
        --monitoring) MONITORING=true; shift ;;
        --map) MAP="$2"; shift 2 ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

echo ""
echo "    ZZZZZ  III  OOO  N   N"
echo "       Z    I  O   O NN  N"
echo "      Z     I  O   O N N N"
echo "     Z      I  O   O N  NN"
echo "    ZZZZZ  III  OOO  N   N"
echo ""
echo "         O A S I S  V3"
echo ""

# ─── 1. Start Backend ─────────────────────────────────────────────────────
echo "[1/3] Starting Rust backend..."

if [[ "$DOCKER" == "true" ]]; then
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" up -d
    echo "    Backend running in Docker (localhost:8094)"
else
    BACKEND_EXE="$SCRIPT_DIR/../../target/release/zion-oasis"
    if [[ ! -f "$BACKEND_EXE" ]]; then
        echo "    Building backend first..."
        cargo build --manifest-path "$SCRIPT_DIR/../../Cargo.toml" --release -p zion-oasis
    fi

    # Start backend in background
    cd "$SCRIPT_DIR"
    "$BACKEND_EXE" &
    BACKEND_PID=$!
    echo "    Backend PID: $BACKEND_PID (localhost:8094)"
fi

# ─── 2. Health Check ────────────────────────────────────────────────────
echo "[2/3] Waiting for backend health..."
MAX_RETRIES=30
RETRY=0
HEALTHY=false

while [[ $RETRY -lt $MAX_RETRIES && "$HEALTHY" == "false" ]]; do
    sleep 1
    if curl -sf http://localhost:8094/health >/dev/null 2>&1; then
        HEALTHY=true
        echo "    Backend healthy!"
    else
        RETRY=$((RETRY + 1))
        echo "    Retry $RETRY/$MAX_RETRIES..."
    fi
done

if [[ "$HEALTHY" == "false" ]]; then
    echo "ERROR: Backend failed to start."
    exit 1
fi

# ─── 3. Start UE5 Editor ────────────────────────────────────────────────
if [[ "$NO_UE5" == "false" ]]; then
    echo "[3/3] Launching UE5 Editor..."

    ENGINE_PATH="${UE5_ENGINE:-/opt/UnrealEngine/5.4}"
    if [[ "$OSTYPE" == "darwin"* ]]; then
        ENGINE_PATH="${UE5_ENGINE:-/Users/Shared/Epic Games/UE_5.4}"
        EDITOR="$ENGINE_PATH/Engine/Binaries/Mac/UnrealEditor.app/Contents/MacOS/UnrealEditor"
    else
        EDITOR="$ENGINE_PATH/Engine/Binaries/Linux/UnrealEditor"
    fi

    if [[ -f "$EDITOR" ]]; then
        "$EDITOR" "$SCRIPT_DIR/ue5/ZionOasis.uproject" "$MAP" &
        echo "    Editor launched!"
    else
        echo "    WARNING: UE5 Editor not found at $EDITOR"
    fi
fi

# ─── 4. Monitoring ──────────────────────────────────────────────────────
if [[ "$MONITORING" == "true" ]]; then
    echo "[4] Starting Prometheus..."
    docker compose -f "$SCRIPT_DIR/docker-compose.yml" up -d prometheus
    echo "    Prometheus: http://localhost:9090"
fi

echo ""
echo "=== Stack Running ==="
echo "  REST API:     http://localhost:8094"
echo "  WebSocket:    ws://localhost:8095"
echo "  Metrics:      http://localhost:9101/metrics"

if [[ "$MONITORING" == "true" ]]; then
    echo "  Prometheus:   http://localhost:9090"
fi

echo ""
echo "Press Enter to stop backend..."
read -r

# Cleanup
if [[ -n "$BACKEND_PID" ]]; then
    kill "$BACKEND_PID" 2>/dev/null || true
    echo "Backend stopped."
fi
