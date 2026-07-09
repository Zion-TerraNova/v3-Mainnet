# ZION OASIS UE5 — Launch Unreal Editor
# Usage: .\RunEditor.ps1 [-EnginePath "C:\\Program Files\\Epic Games\\UE_5.7"] [-Map LV_World]

param(
    [string]$EnginePath = "C:\Program Files\Epic Games\UE_5.7",
    [string]$Map = "LV_MainMenu"
)

$EditorPath = "$EnginePath\Engine\Binaries\Win64\UnrealEditor.exe"
$UProjectPath = "$PSScriptRoot\ZionOasis.uproject"

if (-not (Test-Path $EditorPath)) {
    Write-Error "Unreal Editor not found at: $EditorPath"
    Write-Host "Please install UE 5.7 via Epic Games Launcher or specify -EnginePath"
    exit 1
}

Write-Host "Launching ZION OASIS Editor..." -ForegroundColor Cyan
Write-Host "Map: $Map" -ForegroundColor Gray

& $EditorPath "$UProjectPath" $Map
