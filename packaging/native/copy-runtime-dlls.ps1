# Copy native runtime cdylib artifacts next to an exe or staged app folder.
param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$Out = "",
    [switch]$AllowMissing,
    [switch]$IncludeDebugSymbols
)

$ErrorActionPreference = "Stop"

if (-not $Out) {
    $Out = Join-Path $Root "target\$Profile"
}

$profileDir = Join-Path $Root "target\$Profile"
$depsDir = Join-Path $profileDir "deps"

if (-not (Test-Path $depsDir)) {
    if ($AllowMissing) {
        Write-Warning "Runtime deps directory not found: $depsDir"
        return
    }
    throw "Runtime deps directory not found: $depsDir"
}

New-Item -ItemType Directory -Force -Path $Out | Out-Null

$runtimeDlls = @(
    "gpui.dll",
    "SphereMidiService.dll"
)

$copied = @()
$missing = @()

foreach ($dllName in $runtimeDlls) {
    $source = Join-Path $depsDir $dllName
    if (-not (Test-Path $source)) {
        $source = Join-Path $profileDir $dllName
    }

    if (-not (Test-Path $source)) {
        $missing += $dllName
        continue
    }

    Copy-Item -Force $source (Join-Path $Out $dllName)
    $copied += $dllName

    if ($IncludeDebugSymbols) {
        $pdbName = [System.IO.Path]::ChangeExtension($dllName, ".pdb")
        $pdbSource = Join-Path $depsDir $pdbName
        if (-not (Test-Path $pdbSource)) {
            $pdbSource = Join-Path $profileDir $pdbName
        }
        if (Test-Path $pdbSource) {
            Copy-Item -Force $pdbSource (Join-Path $Out $pdbName)
        }
    }
}

if ($missing.Count -gt 0 -and -not $AllowMissing) {
    throw "Runtime DLL(s) not found: $($missing -join ', '). Build them with packaging/native/build-gui-runtime.ps1 first."
}

if ($missing.Count -gt 0) {
    Write-Warning "Runtime DLL(s) not found: $($missing -join ', ')"
}

if ($copied.Count -gt 0) {
    Write-Host "Copied runtime DLL(s) to $Out: $($copied -join ', ')"
} else {
    Write-Host "No runtime DLLs copied."
}
