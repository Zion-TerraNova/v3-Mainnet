# ZION OASIS — Full Build Script (Windows)
# Builds Rust backend + generates UE5 project files

param(
    [switch]$Docker,
    [switch]$UE5Only,
    [switch]$BackendOnly
)

$ErrorActionPreference = "Stop"
$Root = "$PSScriptRoot"
$V3Root = "$Root\..\.."

Write-Host "=== ZION OASIS Build ===" -ForegroundColor Cyan
Write-Host ""

# ─── 1. Rust Backend ──────────────────────────────────────────────────────
if (-not $UE5Only) {
    Write-Host "[1/3] Building Rust backend (zion-oasis)..." -ForegroundColor Yellow

    if ($Docker) {
        Write-Host "    Docker build..." -ForegroundColor Gray
        docker compose -f "$Root\docker-compose.yml" build
    } else {
        Write-Host "    cargo build --release -p zion-oasis" -ForegroundColor Gray
        & cargo build --manifest-path "$V3Root\Cargo.toml" --release -p zion-oasis

        if ($LASTEXITCODE -ne 0) {
            Write-Error "Rust build failed!"
            exit 1
        }
    }

    Write-Host "    OK" -ForegroundColor Green
}

# ─── 2. UE5 Project Files ─────────────────────────────────────────────────
if (-not $BackendOnly) {
    Write-Host "[2/3] Generating UE5 project files..." -ForegroundColor Yellow

    $UProject = "$Root\ue5\ZionOasis.uproject"
    $EnginePath = "C:\Program Files\Epic Games\UE_5.7"
    $UBT = "$EnginePath\Engine\Binaries\DotNET\UnrealBuildTool\UnrealBuildTool.exe"

    if (Test-Path $UBT) {
        & $UBT -ProjectFiles -Project="$UProject" -Game -Engine -Progress
        Write-Host "    OK" -ForegroundColor Green
    } else {
        Write-Warning "    UE5 not found at $EnginePath — skip project generation"
    }
}

# ─── 3. Verify ────────────────────────────────────────────────────────────
Write-Host "[3/3] Verifying build artifacts..." -ForegroundColor Yellow

$BackendExe = "$V3Root\target\release\zion-oasis.exe"
if (Test-Path $BackendExe) {
    Write-Host "    Backend: $BackendExe" -ForegroundColor Green
} else {
    Write-Warning "    Backend binary not found"
}

$Sln = "$Root\ue5\ZionOasis.sln"
if (Test-Path $Sln) {
    Write-Host "    VS Solution: $Sln" -ForegroundColor Green
} else {
    Write-Warning "    VS Solution not found — run GenerateProjectFiles.ps1"
}

Write-Host ""
Write-Host "=== Build Complete ===" -ForegroundColor Cyan
Write-Host ""
Write-Host "Next steps:" -ForegroundColor White
Write-Host "  1. Start backend: cargo run -p zion-oasis" -ForegroundColor Gray
Write-Host "  2. Open ZionOasis.sln in Visual Studio 2022" -ForegroundColor Gray
Write-Host "  3. Build → Development Editor" -ForegroundColor Gray
Write-Host "  4. See ue5\README_UE5.md for Blueprint setup" -ForegroundColor Gray
