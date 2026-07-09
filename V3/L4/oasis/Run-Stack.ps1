# ZION OASIS — Full Stack Launcher (Windows)
# Starts: Rust backend + UE5 Editor + optional monitoring

param(
    [switch]$Docker,
    [switch]$NoUE5,
    [switch]$Monitoring,
    [string]$Map = "LV_MainMenu",
    [string]$EnginePath = "C:\Program Files\Epic Games\UE_5.7"
)

$ErrorActionPreference = "Stop"
$Root = "$PSScriptRoot"

Write-Host ""
Write-Host "    ZZZZZ  III  OOO  N   N" -ForegroundColor Cyan
Write-Host "       Z    I  O   O NN  N" -ForegroundColor Cyan
Write-Host "      Z     I  O   O N N N" -ForegroundColor Cyan
Write-Host "     Z      I  O   O N  NN" -ForegroundColor Cyan
Write-Host "    ZZZZZ  III  OOO  N   N" -ForegroundColor Cyan
Write-Host ""
Write-Host "         O A S I S  V3" -ForegroundColor Yellow
Write-Host ""

# ─── 1. Start Backend ─────────────────────────────────────────────────────
Write-Host "[1/3] Starting Rust backend..." -ForegroundColor Yellow

if ($Docker) {
    docker compose -f "$Root\docker-compose.yml" up -d
    Write-Host "    Backend running in Docker (localhost:8094)" -ForegroundColor Green
} else {
    $BackendExe = "$Root\..\..\target\release\zion-oasis.exe"
    if (-not (Test-Path $BackendExe)) {
        Write-Host "    Building backend first..." -ForegroundColor Gray
        & cargo build --manifest-path "$Root\..\..\Cargo.toml" --release -p zion-oasis
    }

    # Start backend in background
    $BackendProc = Start-Process -FilePath $BackendExe -WorkingDirectory $Root -PassThru -WindowStyle Hidden
    Write-Host "    Backend PID: $($BackendProc.Id) (localhost:8094)" -ForegroundColor Green
}

# ─── 2. Health Check ────────────────────────────────────────────────────
Write-Host "[2/3] Waiting for backend health..." -ForegroundColor Yellow
$MaxRetries = 30
$Retry = 0
$Healthy = $false

while ($Retry -lt $MaxRetries -and -not $Healthy) {
    Start-Sleep -Seconds 1
    try {
        $Response = Invoke-WebRequest -Uri "http://localhost:8094/health" -UseBasicParsing -TimeoutSec 2 -ErrorAction Stop
        if ($Response.StatusCode -eq 200) {
            $Healthy = $true
            Write-Host "    Backend healthy!" -ForegroundColor Green
        }
    } catch {
        $Retry++
        Write-Host "    Retry $Retry/$MaxRetries..." -ForegroundColor Gray
    }
}

if (-not $Healthy) {
    Write-Error "Backend failed to start. Check logs: cargo run -p zion-oasis"
    exit 1
}

# ─── 3. Start UE5 Editor ────────────────────────────────────────────────
if (-not $NoUE5) {
    Write-Host "[3/3] Launching UE5 Editor..." -ForegroundColor Yellow

    $EditorPath = "$EnginePath\Engine\Binaries\Win64\UnrealEditor.exe"
    $UProject = "$Root\ue5\ZionOasis.uproject"

    if (Test-Path $EditorPath) {
        Start-Process $EditorPath -ArgumentList "`"$UProject`"", "$Map"
        Write-Host "    Editor launched!" -ForegroundColor Green
    } else {
        Write-Warning "    UE5 Editor not found at $EditorPath"
        Write-Host "    Install UE 5.4 via Epic Games Launcher" -ForegroundColor Gray
    }
}

# ─── 4. Monitoring ──────────────────────────────────────────────────────
if ($Monitoring) {
    Write-Host "[4] Starting Prometheus..." -ForegroundColor Yellow
    docker compose -f "$Root\docker-compose.yml" up -d prometheus
    Write-Host "    Prometheus: http://localhost:9090" -ForegroundColor Green
}

Write-Host ""
Write-Host "=== Stack Running ===" -ForegroundColor Cyan
Write-Host "  REST API:     http://localhost:8094" -ForegroundColor White
Write-Host "  WebSocket:    ws://localhost:8095" -ForegroundColor White
Write-Host "  Metrics:      http://localhost:9101/metrics" -ForegroundColor White
Write-Host "  Prometheus:   http://localhost:9090 (if --Monitoring)" -ForegroundColor White
Write-Host ""
Write-Host "Press any key to stop backend..." -ForegroundColor Yellow
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")

# Cleanup
if ($BackendProc) {
    Stop-Process -Id $BackendProc.Id -Force -ErrorAction SilentlyContinue
    Write-Host "Backend stopped." -ForegroundColor Green
}
