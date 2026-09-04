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

# ---- detect OS (this installer targets Windows) ----
function Test-Windows {
    if ($IsWindows -eq $true) { return $true }
    if ($env:OS -like "*Windows*") { return $true }
    return [System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)
}
if (-not (Test-Windows)) {
    Write-Host ""
    Write-Host "⚠  This is the Windows installer (scripts/install.ps1)." -ForegroundColor Yellow
    Write-Host "    On Linux / macOS use the shell installer instead:" -ForegroundColor Yellow
    Write-Host "      scripts/install.sh" -ForegroundColor Cyan
    Write-Host ""
    exit 1
}

# ---- detect arch ----
$arch = ""
try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch {}
if ($arch -eq "") { $arch = $env:PROCESSOR_ARCHITECTURE }
switch ($arch) {
    "X64"   { $target = "x86_64-pc-windows-msvc" }
    "AMD64" { $target = "x86_64-pc-windows-msvc" }
    "Arm64" { $target = "aarch64-pc-windows-msvc" }
    "ARM64" { $target = "aarch64-pc-windows-msvc" }
    "X86"   { $target = "i686-pc-windows-msvc" }
    default { throw "Unsupported architecture: '$arch'" }
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

function Add-PathDir {
    param([string]$dir)
    if ($dir -and (Test-Path $dir) -and ($env:Path -notlike "*$dir*")) {
        $env:Path = "$dir;" + $env:Path
    }
}
function Add-ToUserPath {
    param([string]$dir)
    if (-not $dir) { return }
    $p = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $p) { $p = "" }
    if ($p -notlike "*$dir*") {
        [Environment]::SetEnvironmentVariable("Path", $p.TrimEnd(';') + ";" + $dir, "User")
    }
}
function Ensure-Git {
    if (Get-Command git -ErrorAction SilentlyContinue) { return $true }
    Write-Host "→ Git not found - installing Git for Windows via winget..." -ForegroundColor Yellow
    try { winget install --id Git.Git -e --accept-source-agreements --accept-package-agreements --silent | Out-Null } catch {}
    if (Get-Command git -ErrorAction SilentlyContinue) { Write-Host "✓ Git ready"; return $true }
    $gitCmd = Join-Path $env:USERPROFILE "AppData\Local\Programs\Git\cmd"
    Add-PathDir $gitCmd
    if (Get-Command git -ErrorAction SilentlyContinue) { Write-Host "✓ Git ready"; return $true }
    Write-Host "✗ Git is required to build from source (install from https://git-scm.com)." -ForegroundColor Red
    return $false
}
function Ensure-Rust {
    if (Get-Command cargo -ErrorAction SilentlyContinue) { Write-Host "✓ Rust (cargo) found"; return $true }
    Write-Host "→ Rust not found - installing via rustup (this downloads a few hundred MB)..." -ForegroundColor Yellow
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    $rustupInit = Join-Path $env:TEMP "rustup-init.exe"
    try { Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit -TimeoutSec 180 }
    catch {
        Write-Host "✗ Could not download rustup (https://win.rustup.rs). Install Rust manually from https://rustup.rs" -ForegroundColor Red
        return $false
    }
    try {
        & $rustupInit -y --profile minimal --default-toolchain stable --default-host x86_64-pc-windows-msvc | Out-Null
    } finally {
        Remove-Item -Force $rustupInit -ErrorAction SilentlyContinue
    }
    Add-PathDir $cargoBin
    Add-ToUserPath $cargoBin
    if (Get-Command cargo -ErrorAction SilentlyContinue) { Write-Host "✓ Rust installed"; return $true }
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { Write-Host "✓ Rust installed"; return $true }
    Write-Host "✗ Rust install did not complete. Open a new terminal and re-run, or install from https://rustup.rs" -ForegroundColor Red
    return $false
}
function Ensure-Msvc {
    # Building with the msvc target needs the MSVC linker (VS C++ Build Tools).
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
        if ($vs) { Write-Host "✓ MSVC Build Tools found"; return }
    }
    Write-Host "→ MSVC C++ Build Tools not detected. Installing VS Build Tools (C++ workload) - this is large..." -ForegroundColor Yellow
    try {
        winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-source-agreements --accept-package-agreements --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" | Out-Null
        Write-Host "✓ Installed VS Build Tools (reopen the terminal to use it)" -ForegroundColor Green
    } catch {
        Write-Host "  (could not install Build Tools automatically)" -ForegroundColor Yellow
    }
    Write-Host "  If linking fails, install 'Desktop development with C++' from https://visualstudio.microsoft.com/downloads/ and retry." -ForegroundColor Yellow
}

function Install-FromSource {
    Write-Host ""
    Write-Host "→ Checking build dependencies..."
    if (-not (Ensure-Git)) { throw "Git is required to build from source" }
    if (-not (Ensure-Rust)) { throw "Rust is required to build from source" }
    Ensure-Msvc
    $buildDir = Join-Path $env:TEMP ("indent-src-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $buildDir | Out-Null
    Write-Host "→ Cloning $Repo ..."
    git clone --depth 1 "https://github.com/$Repo.git" $buildDir | Out-Null
    Push-Location (Join-Path $buildDir "indent-native")
    try {
        Write-Host "→ Building indent (release). This can take a few minutes..."
        cargo build --release
        $bin = Join-Path $buildDir "indent-native\target\release\indent.exe"
        if (-not (Test-Path $bin)) { throw "Build finished but no indent.exe was produced" }
        Copy-Item $bin (Join-Path $BinDir "indent.exe") -Force
        Write-Host "✓ Built and installed indent from source" -ForegroundColor Green
        foreach ($tool in @("air", "indentpkg")) {
            $src = Join-Path $buildDir $tool
            if (Test-Path $src) { Copy-Item $src (Join-Path $BinDir $tool) -Force }
        }
    } finally {
        Pop-Location
        Remove-Item -Recurse -Force $buildDir -ErrorAction SilentlyContinue
    }
}

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
    Write-Host "→ Fetching Indent $Version..."
    $installed = $false
    try {
        if ($Version -eq "latest") {
            $api = "https://api.github.com/repos/$Repo/releases/latest"
        } else {
            $api = "https://api.github.com/repos/$Repo/releases/tags/$Version"
        }
        $release = Invoke-RestMethod -Uri $api -TimeoutSec 60
        $asset = $release.assets | Where-Object { $_.name -like "*-$target.zip" } | Select-Object -First 1
        if ($asset) {
            $tmp = Join-Path $env:TEMP "indent-install-$([guid]::NewGuid())"
            New-Item -ItemType Directory -Path $tmp | Out-Null
            $zipPath = Join-Path $tmp "indent.zip"
            Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -TimeoutSec 120
            Expand-Archive -Path $zipPath -DestinationPath $tmp -Force
            $exe = Get-ChildItem -Path $tmp -Recurse -Filter "indent.exe" | Select-Object -First 1
            if ($exe) {
                Copy-Item $exe.FullName (Join-Path $BinDir "indent.exe") -Force
                Write-Host "✓ Downloaded indent" -ForegroundColor Green
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
                $installed = $true
            }
        }
    } catch {
        Write-Host "  (no prebuilt release: $($_.Exception.Message))" -ForegroundColor Yellow
    }
    if (-not $installed) {
        Write-Host ""
        Write-Host "No prebuilt release found for $Repo - building from source instead..." -ForegroundColor Yellow
        Write-Host ""
        Install-FromSource
    }
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
