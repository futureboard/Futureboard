# Stage a Windows release folder with the native exe (icon + manifest embedded via app.rc).
param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [string]$Bin = "",
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"

if (-not $Bin) {
    $Bin = Join-Path $Root "target\release\futureboard_native.exe"
}
if (-not $Out) {
    $Out = Join-Path $Root "packaging\native\out\win"
}

if (-not (Test-Path $Bin)) {
    throw "Native binary not found: $Bin"
}

$productName = "Futureboard Studio"
$stage = Join-Path $Out $productName
New-Item -ItemType Directory -Force -Path $stage | Out-Null

$destExe = Join-Path $stage "$productName.exe"
Copy-Item -Force $Bin $destExe

# Ship shared PNG for shortcuts / installers (optional).
$iconPng = Join-Path $Root "apps\shared\app.png"
if (Test-Path $iconPng) {
    Copy-Item -Force $iconPng (Join-Path $stage "app.png")
}

Write-Host "Bundled Windows app: $stage"
