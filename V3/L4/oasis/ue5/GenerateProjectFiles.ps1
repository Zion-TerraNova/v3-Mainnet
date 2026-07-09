# ZION OASIS UE5 — Generate Visual Studio Project Files
# Requires: Unreal Engine 5.7 installed via Epic Games Launcher
# Usage: .\GenerateProjectFiles.ps1 [-EnginePath "C:\\Program Files\\Epic Games\\UE_5.4"]

param(
    [string]$EnginePath = "C:\Program Files\Epic Games\UE_5.7"
)

$UProjectPath = "$PSScriptRoot\ZionOasis.uproject"
$UBTPath = "$EnginePath\Engine\Binaries\DotNET\UnrealBuildTool\UnrealBuildTool.exe"

if (-not (Test-Path $UBTPath)) {
    Write-Error "UnrealBuildTool not found at: $UBTPath"
    Write-Host "Please install UE 5.4 via Epic Games Launcher or specify -EnginePath"
    exit 1
}

Write-Host "Generating Visual Studio project files for ZION OASIS..." -ForegroundColor Cyan
Write-Host "Engine: $EnginePath" -ForegroundColor Gray
Write-Host "UProject: $UProjectPath" -ForegroundColor Gray

& $UBTPath -ProjectFiles -Project="$UProjectPath" -Game -Engine -Progress

if ($LASTEXITCODE -eq 0) {
    Write-Host "Success! Open ZionOasis.sln in Visual Studio 2022" -ForegroundColor Green
} else {
    Write-Error "Project generation failed (exit code $LASTEXITCODE)"
}
