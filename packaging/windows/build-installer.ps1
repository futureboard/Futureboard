# Build the Windows installer with Inno Setup from target/release artifacts.
param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [string]$Iscc = "",
    [string]$AppVersion = ""
)

$ErrorActionPreference = "Stop"

if (-not $Iscc) {
    $Iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
}

if (-not $AppVersion) {
    $versionJson = Join-Path $Root "version.json"
    if (-not (Test-Path $versionJson)) {
        throw "version.json not found: $versionJson"
    }
    $AppVersion = (Get-Content $versionJson -Raw | ConvertFrom-Json).version
}

$iss = Join-Path $PSScriptRoot "installer.iss"
$exe = Join-Path $Root "target\release\FutureboardNative.exe"

if (-not (Test-Path $exe)) {
    throw "Native binary not found: $exe (run: cargo build --release -p futureboard_native)"
}

if (-not (Test-Path $Iscc)) {
    throw "Inno Setup compiler not found: $Iscc"
}

Write-Host "Building installer version: $AppVersion"
& $Iscc $iss "/DMyAppVersion=$AppVersion"
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$output = Join-Path $Root "target\installer\FutureboardStudioSetup.exe"
Write-Host "Built installer: $output"
