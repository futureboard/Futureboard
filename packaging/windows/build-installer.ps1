# Build the Windows installer with Inno Setup from the built edition's
# release artifacts (Community Edition, target\community\release, by default;
# pass -SourceDir for another edition's target directory).
param(
    [string]$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path,
    [string]$Iscc = "",
    [string]$AppVersion = "",
    [string]$SourceDir = ""
)

$ErrorActionPreference = "Stop"

function Find-InnoSetupCompiler {
    $candidates = @(
        $Iscc,
        $env:INNO_SETUP_PATH,
        (Get-Command iscc -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
    )

    foreach ($version in 7, 6) {
        $candidates += @(
            (Join-Path $env:ProgramFiles "Inno Setup $version\ISCC.exe"),
            (Join-Path ${env:ProgramFiles(x86)} "Inno Setup $version\ISCC.exe")
        )
    }

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    return $null
}

if (-not $Iscc) {
    $Iscc = Find-InnoSetupCompiler
}

if (-not $AppVersion) {
    $versionJson = Join-Path $Root "version.json"
    if (-not (Test-Path $versionJson)) {
        throw "version.json not found: $versionJson"
    }
    $AppVersion = (Get-Content $versionJson -Raw | ConvertFrom-Json).version
}

if (-not $SourceDir) {
    $SourceDir = Join-Path $Root "target\community\release"
}

$iss = Join-Path $PSScriptRoot "installer.iss"
$exe = Join-Path $SourceDir "FutureboardNative.exe"

if (-not (Test-Path $exe)) {
    throw "Native binary not found: $exe (run: cargo build-ce --release)"
}

if (-not $Iscc) {
    throw "Inno Setup compiler not found. Install Inno Setup 6/7 or pass -Iscc."
}

Write-Host "Building installer version: $AppVersion (source: $SourceDir)"
& $Iscc $iss "/DMyAppVersion=$AppVersion" "/DMySourceDir=$SourceDir"
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$output = Join-Path $Root "target\installer\FutureboardStudioSetup.exe"
Write-Host "Built installer: $output"
