# =============================================================================
# Indent Language — Universal Installer for Windows
# =============================================================================
#
# One-command install:
#   powershell -c "irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex"
#
# Specific version:
#   powershell -c "`$v='v2.1.0'; irm ... | iex"
#
# Local install:
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Local
# =============================================================================

param(
    [Parameter(Mandatory = $false)]
    [string]$Repo = "xytrolabs/indent",
    [Parameter(Mandatory = $false)]
    [switch]$Local,
    [Parameter(Mandatory = $false)]
    [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

Write-Host ""
Write-Host "⚡ Indent Installer for Windows — Xytro Labs" -ForegroundColor Cyan
Write-Host ""

# ---- detect arch ----
$arch = $env:PROCESSOR_ARCHITECTURE
if ($arch -eq "AMD64") {
    $target = "x86_64-pc-windows-msvc"
} elseif ($arch -eq "ARM64") {
    $target = "aarch64-pc-windows-msvc"
} else {
    throw "Unsupported architecture: $arch"
}
Write-Host "  Platform: $target" -ForegroundColor Green

# ---- install directories ----
$IndentHome = Join-Path $env:USERPROFILE ".local\share\indent"
$BinDir = Join-Path $IndentHome "bin"
$StdDir = Join-Path $IndentHome "std"
$PkgDir = Join-Path $IndentHome "packages"
$LauncherDir = Join-Path $env:USERPROFILE ".local\bin"

New-Item -ItemType Directory -Path $BinDir -Force | Out-Null
New-Item -ItemType Directory -Path $StdDir -Force | Out-Null
New-Item -ItemType Directory -Path $PkgDir -Force | Out-Null
New-Item -ItemType Directory -Path $LauncherDir -Force | Out-Null

# ---- install binary ----
if ($Local) {
    Write-Host "→ Installing from local build..."
    $ScriptRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Get-Location }
    $LocalBin = Join-Path $ScriptRoot "..\indent-native\target\release\indent.exe"
    if (-not (Test-Path $LocalBin)) {
        throw "Local binary not found: $LocalBin`n  Build first: cd indent-native && cargo build --release"
    }
    Copy-Item $LocalBin (Join-Path $BinDir "indent.exe") -Force
    Write-Host "✓ Installed from local build" -ForegroundColor Green
} else {
    Write-Host "→ Downloading Indent $Version..."
    if ($Version -eq "latest") {
        $api = "https://api.github.com/repos/$Repo/releases/latest"
    } else {
        $api = "https://api.github.com/repos/$Repo/releases/tags/$Version"
    }
    $release = Invoke-RestMethod -Uri $api -TimeoutSec 60
    $asset = $release.assets | Where-Object { $_.name -like "*-$target.zip" } | Select-Object -First 1

    if (-not $asset) {
        Write-Host "No release asset found for: $target" -ForegroundColor Red
        Write-Host "Available assets:"
        $release.assets | ForEach-Object { Write-Host "  $($_.name)" }
        throw "Installation failed"
    }

    $tmp = Join-Path $env:TEMP "indent-install-$([guid]::NewGuid())"
    New-Item -ItemType Directory -Path $tmp | Out-Null

    $zipPath = Join-Path $tmp "indent.zip"
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -TimeoutSec 120
    Expand-Archive -Path $zipPath -DestinationPath $tmp -Force

    $exe = Get-ChildItem -Path $tmp -Recurse -Filter "indent.exe" | Select-Object -First 1
    if (-not $exe) { throw "Archive does not contain indent.exe" }

    Copy-Item $exe.FullName (Join-Path $BinDir "indent.exe") -Force
    Write-Host "✓ Downloaded indent" -ForegroundColor Green

    # Companion tools
    foreach ($tool in @("air", "indentpkg")) {
        $toolFile = Get-ChildItem -Path $tmp -Recurse -Filter "$tool.*" | Select-Object -First 1
        if ($toolFile) {
            $ext = [System.IO.Path]::GetExtension($toolFile.Name)
            $destName = if ($ext -eq ".ps1") { "$tool.ps1" } else { "$tool.exe" }
            Copy-Item $toolFile.FullName (Join-Path $BinDir $destName) -Force
            Write-Host "✓ Installed $tool" -ForegroundColor Green
        }
    }

    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}

# ---- download standard library ----
Write-Host "→ Installing standard library..."
$StdBase = "https://raw.githubusercontent.com/$Repo/main/std"
foreach ($file in @("io.ind", "math.ind", "strings.ind", "testing.ind")) {
    try {
        Invoke-WebRequest -Uri "$StdBase/$file" -OutFile (Join-Path $StdDir $file) -TimeoutSec 30
    } catch { Write-Host "  (skipping $file - not yet in repo)" }
}
Write-Host "✓ Standard library installed" -ForegroundColor Green

# ---- create launcher ----
@"
@echo off
setlocal
rem Include site-packages, packages, and std in INDENT_PATH (like Python's sys.path)
set "INDENT_PATH=$PkgDir;$StdDir;$IndentHome\site-packages;%INDENT_PATH%"
"$BinDir\indent.exe" %*
endlocal
"@ | Set-Content -Path (Join-Path $LauncherDir "indent.cmd")

@"
@echo off
setlocal
set "INDENT_PATH=$PkgDir;$StdDir;$IndentHome\site-packages;%INDENT_PATH%"
"$BinDir\indent.exe" --debug %*
endlocal
"@ | Set-Content -Path (Join-Path $LauncherDir "indent-debug.cmd")

Write-Host "✓ Created launcher: $LauncherDir\indent.cmd" -ForegroundColor Green

# ---- auto-configure registry ----
$ConfigDir = Join-Path $env:USERPROFILE ".config\indent"
New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
@"
INDENTPKG_INDEX=https://raw.githubusercontent.com/$Repo/main/packages/index.txt
AIR_REGISTRY_REPO=$Repo
AIR_REGISTRY_REF=main
AIR_REGISTRY_INDEX_PATH=packages/index.txt
"@ | Set-Content -Path (Join-Path $ConfigDir "air.env")

# ---- PATH check ----
Write-Host ""
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Gray
Write-Host "✓ Indent installed successfully!" -ForegroundColor Green
Write-Host ""
Write-Host "  Binary:    $BinDir\indent.exe"
Write-Host "  Launcher:  $LauncherDir\indent.cmd"
Write-Host "  Stdlib:    $StdDir"
Write-Host ""

$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$LauncherDir*") {
    Write-Host "⚠  Add to your user PATH:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  Run this in an admin PowerShell:" -ForegroundColor Cyan
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$LauncherDir', 'User')"
    Write-Host ""
    $response = Read-Host "  Add to PATH now? [Y/n]"
    if ($response -eq "" -or $response -match "^[Yy]") {
        [Environment]::SetEnvironmentVariable("Path", $userPath + ";$LauncherDir", "User")
        $env:Path = $env:Path + ";$LauncherDir"
        Write-Host "✓ PATH configured" -ForegroundColor Green
    }
} else {
    Write-Host "✓ PATH already configured" -ForegroundColor Green
}

Write-Host ""
Write-Host "  Try it:   indent --version"
Write-Host "  Format:   indent fmt myfile.ind"
Write-Host "  Help:     indent --help"
Write-Host "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Gray
Write-Host ""

# Configure VS Code settings so .ind files are recognized in any folder.
$vscodeSettingsPaths = @(
    (Join-Path $env:APPDATA "Code\User\settings.json"),
    (Join-Path $env:APPDATA "VSCodium\User\settings.json")
)

foreach ($settingsPath in $vscodeSettingsPaths) {
    $settingsDir = Split-Path -Parent $settingsPath
    New-Item -ItemType Directory -Path $settingsDir -Force | Out-Null

    if (Test-Path $settingsPath) {
        $raw = Get-Content -Path $settingsPath -Raw
        if ([string]::IsNullOrWhiteSpace($raw)) {
            $settings = [PSCustomObject]@{}
        } else {
            $settings = $raw | ConvertFrom-Json
        }
    } else {
        $settings = [PSCustomObject]@{}
    }

    if (-not ($settings.PSObject.Properties.Name -contains "files.associations")) {
        $settings | Add-Member -NotePropertyName "files.associations" -NotePropertyValue ([PSCustomObject]@{})
    }

    $settings."files.associations" | Add-Member -NotePropertyName "*.ind" -NotePropertyValue "indent" -Force

    if (-not ($settings.PSObject.Properties.Name -contains "workbench.iconTheme")) {
        $settings | Add-Member -NotePropertyName "workbench.iconTheme" -NotePropertyValue "indent-seti-icons"
    }

    $settings | ConvertTo-Json -Depth 30 | Set-Content -Path $settingsPath
}

Write-Host "Configured VS Code settings for .ind recognition and Indent icon theme defaults."
