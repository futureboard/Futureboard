# Build GUI/runtime cdylibs independently and copy them to target/<profile>.
# This keeps heavyweight runtime crates warm in Cargo's cache before rebuilding the app.
param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$Out = "",
    [switch]$IncludeDebugSymbols
)

$ErrorActionPreference = "Stop"

$targetDir = Join-Path $Root "target"
$gpuiManifest = Join-Path $Root "crates\gpui\Cargo.toml"

$profileArgs = @()
if ($Profile -eq "release") {
    $profileArgs += "--release"
}

Write-Host "Building GPUI runtime cdylib ($Profile)..."
& cargo build --manifest-path $gpuiManifest -p gpui @profileArgs --target-dir $targetDir
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Building MIDI runtime cdylib ($Profile)..."
& cargo build -p SphereMidiService @profileArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$copyArgs = @(
    "-File", (Join-Path $PSScriptRoot "copy-runtime-dlls.ps1"),
    "-Root", $Root,
    "-Profile", $Profile
)
if ($Out) {
    $copyArgs += @("-Out", $Out)
}
if ($IncludeDebugSymbols) {
    $copyArgs += "-IncludeDebugSymbols"
}

& pwsh @copyArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
