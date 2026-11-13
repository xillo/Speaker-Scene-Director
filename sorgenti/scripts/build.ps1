# Speaker Scene Director - Windows release builder.
# Public builds never include local configuration or API keys.
# Use -Personal for a second portable ZIP with your existing Python settings.
param(
    [switch]$Personal,
    [string]$ConfigPath = "",
    [switch]$SkipTests
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'Rust Cargo is not installed. Install rustup from https://rustup.rs/, reopen PowerShell, then run BUILD.bat.'
    }
    Write-Host '[1/4] Building the Rust application...' -ForegroundColor Cyan
    if (-not $SkipTests) {
        & cargo test --release
        if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed. No package was created.' }
    }
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { throw 'Rust release build failed.' }

    $executable = Join-Path $root 'target\release\speaker-scene-director.exe'
    if (-not (Test-Path $executable)) { throw "Executable was not found: $executable" }

    $dist = Join-Path $root 'dist'
    $publicDir = Join-Path $dist 'public'
    New-Item -ItemType Directory -Force -Path $publicDir | Out-Null
    Copy-Item -LiteralPath $executable -Destination (Join-Path $publicDir 'SpeakerSceneDirector.exe') -Force
    foreach ($file in @('config.example.json', 'README.md', 'LICENSE')) {
        Copy-Item -LiteralPath (Join-Path $root $file) -Destination (Join-Path $publicDir $file) -Force
    }
    $publicZip = Join-Path $dist 'SpeakerSceneDirector-Public.zip'
    if (Test-Path $publicZip) { Remove-Item -LiteralPath $publicZip -Force }
    Compress-Archive -Path (Join-Path $publicDir '*') -DestinationPath $publicZip -CompressionLevel Optimal
    Write-Host "[2/4] Public package: $publicZip" -ForegroundColor Green

    if ($Personal) {
        Write-Host '[3/4] Preparing personal package from your existing configuration...' -ForegroundColor Cyan
        $sources = @()
        if ($ConfigPath) { $sources += $ConfigPath }
        $sources += @(
            'W:\XILLO_TS6_AutoDirector_v2\Vai\XILLO_AutoDirector\config.json',
            'W:\XILLO_AutoDirector\config.json',
            'W:\XILLO_TS6_AutoDirector_v2\config.json',
            'W:\XILLO_AutoDirector_v1.1\config.json',
            (Join-Path $root 'config.json')
        )
        $selected = $null
        foreach ($candidate in $sources) {
            if ($candidate -and (Test-Path -LiteralPath $candidate -PathType Leaf)) {
                $selected = (Resolve-Path -LiteralPath $candidate).Path
                break
            }
        }
        if (-not $selected) {
            throw 'No existing config.json was found. Public build succeeded. For your personal build use: .\scripts\build.ps1 -Personal -ConfigPath "W:\path\to\config.json"'
        }
        $raw = Get-Content -LiteralPath $selected -Raw -Encoding UTF8
        $configObject = $raw | ConvertFrom-Json
        if (-not $configObject.participants -or -not $configObject.default_scene -or -not $configObject.obs) {
            throw 'Your config.json is missing participants, default_scene, or obs fields.'
        }
        $personalDir = Join-Path $dist 'personal'
        New-Item -ItemType Directory -Force -Path $personalDir | Out-Null
        Copy-Item -LiteralPath $executable -Destination (Join-Path $personalDir 'SpeakerSceneDirector.exe') -Force
        Copy-Item -LiteralPath $selected -Destination (Join-Path $personalDir 'config.json') -Force
        Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $personalDir 'README.md') -Force
        Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $personalDir 'LICENSE') -Force
        $keyFile = Join-Path (Split-Path -Parent $selected) '.teamspeak_api_key'
        if (Test-Path -LiteralPath $keyFile -PathType Leaf) {
            Copy-Item -LiteralPath $keyFile -Destination (Join-Path $personalDir '.teamspeak_api_key') -Force
        }
        $vdoFile = Join-Path (Split-Path -Parent $selected) 'vdo_links.json'
        if (Test-Path -LiteralPath $vdoFile -PathType Leaf) {
            Copy-Item -LiteralPath $vdoFile -Destination (Join-Path $personalDir 'vdo_links.json') -Force
        }
        @'
PERSONAL BUILD - DO NOT PUBLISH OR UPLOAD THIS DIRECTORY OR ZIP.
Contains your existing local config.json and may contain a TeamSpeak API key.
The renamed TeamSpeak application may ask for authorization once.
The application keeps the same participant mappings and timing values from your existing config.json.
'@ | Set-Content -LiteralPath (Join-Path $personalDir 'PERSONAL_README.txt') -Encoding UTF8
        $personalZip = Join-Path $dist 'SpeakerSceneDirector-Personal-PRIVATE.zip'
        if (Test-Path $personalZip) { Remove-Item -LiteralPath $personalZip -Force }
        Compress-Archive -Path (Join-Path $personalDir '*') -DestinationPath $personalZip -CompressionLevel Optimal
        Write-Host "[4/4] PRIVATE package: $personalZip" -ForegroundColor Yellow
        Write-Host "Settings imported from: $selected" -ForegroundColor Green
        Write-Warning 'Never upload dist/personal or SpeakerSceneDirector-Personal-PRIVATE.zip to GitHub.'
    } else {
        Write-Host '[3/4] Personal package skipped. Pass -Personal to build both variants.' -ForegroundColor Gray
    }
    Write-Host 'Build completed successfully.' -ForegroundColor Green
} finally {
    Pop-Location
}
