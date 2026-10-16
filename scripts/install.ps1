# =============================================================================
# Indent Language — Universal Installer for Windows
# =============================================================================
#
# One-command install (installs the latest release, e.g. 2.4):
#   irm https://indent.xytro.site/install.ps1 | iex
#
#   # ...or straight from the repo:
#   irm https://raw.githubusercontent.com/xytrolabs/indent/main/scripts/install.ps1 | iex
#
# Specific version (params work when you save-and-run rather than pipe):
#   irm https://indent.xytro.site/install.ps1 -OutFile install.ps1
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Version v2.4.0
#
# Local install (from a local cargo build):
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Local
#
# Build from source without a prebuilt binary:
#   powershell -ExecutionPolicy Bypass -File install.ps1 -BuildFromSource
#
# Requires PowerShell 5.1+ (ships with Windows 10/11). Works in Windows
# PowerShell and PowerShell 7+.
# =============================================================================

param(
    [Parameter(Mandatory = $false)]
    [string]$Repo = "xytrolabs/indent",
    [Parameter(Mandatory = $false)]
    [switch]$Local,
    [Parameter(Mandatory = $false)]
    [switch]$BuildFromSource,
    [Parameter(Mandatory = $false)]
    [string]$Version = "latest",
    [Parameter(Mandatory = $false)]
    [string]$Channel = "stable"
)

$ErrorActionPreference = "Stop"

if ($Channel -notin @("stable", "exp")) {
    Write-Host "Unknown channel: $Channel" -ForegroundColor Red
    Write-Host "  Use -Channel stable or -Channel exp."
    exit 2
}
$ProgressPreference = "SilentlyContinue"

# GitHub requires TLS 1.2+. Windows PowerShell 5.1 (the `powershell.exe` that
# ships with Windows) still negotiates SSL3/TLS 1.0 by default on many systems,
# which makes every download fail with a vague "Could not create SSL/TLS secure
# channel" error. Opt in to TLS 1.2 before any network call.
try {
    $tls12 = [System.Net.SecurityProtocolType]::Tls12
    [System.Net.ServicePointManager]::SecurityProtocol =
        [System.Net.ServicePointManager]::SecurityProtocol -bor $tls12
} catch {
    # Very old .NET without Tls12 — nothing we can do here; the download will
    # surface its own error if the connection cannot be secured.
}

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
    try { Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $rustupInit -TimeoutSec 180 -UserAgent "indent-installer" }
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
        foreach ($tool in @("air")) {
            $src = Join-Path $buildDir $tool
            if (Test-Path $src) { Copy-Item $src (Join-Path $BinDir $tool) -Force }
        }
        # The clone has the complete std/ tree; prefer it over the partial
        # four-file download below.
        $srcStd = Join-Path $buildDir "std"
        if (Test-Path $srcStd) {
            New-Item -ItemType Directory -Path $StdDir -Force | Out-Null
            Copy-Item (Join-Path $srcStd "*") $StdDir -Recurse -Force
            Write-Host "✓ Installed std/ from cloned repo" -ForegroundColor Green
            $script:stdInstalled = $true
        }
        $srcPkgs = Join-Path $buildDir "packages"
        if (Test-Path $srcPkgs) {
            New-Item -ItemType Directory -Path $PkgDir -Force | Out-Null
            Copy-Item (Join-Path $srcPkgs "*") $PkgDir -Recurse -Force
            Write-Host "✓ Installed packages/ from cloned repo" -ForegroundColor Green
        }
    } finally {
        Pop-Location
        Remove-Item -Recurse -Force $buildDir -ErrorAction SilentlyContinue
    }
}

# Tracks whether the full std/ tree was installed from a release archive or a
# clone, so the fallback download below does not replace it with four files.
$stdInstalled = $false

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
        if ($Channel -eq "exp" -and $Version -eq "latest") {
            # Experimental builds are tagged v2.4.1-exp.1 and published as GitHub
            # prereleases, so they never become "latest". This endpoint lists
            # releases newest-first, and the tag shape picks the experimental ones.
            $api = "https://api.github.com/repos/$Repo/releases?per_page=30"
        } elseif ($Version -eq "latest") {
            $api = "https://api.github.com/repos/$Repo/releases/latest"
        } else {
            $api = "https://api.github.com/repos/$Repo/releases/tags/$Version"
        }
        # The GitHub API rejects requests without a User-Agent. Pass it with the
        # dedicated -UserAgent parameter rather than a "User-Agent" entry in
        # -Headers: Windows PowerShell 5.1 throws
        # "The 'User-Agent' header must be modified using the appropriate
        # property or method" for the latter, which made every install fail
        # before a single byte was downloaded.
        $apiHeaders = @{
            "Accept" = "application/vnd.github+json"
        }
        $release = Invoke-RestMethod -Uri $api -TimeoutSec 60 -Headers $apiHeaders -UserAgent "indent-installer"
        if ($Channel -eq "exp" -and $Version -eq "latest") {
            $release = $release | Where-Object { $_.tag_name -match '^v[0-9].*-' } | Select-Object -First 1
            if (-not $release) {
                Write-Host "No experimental build has been published yet in $Repo." -ForegroundColor Red
                Write-Host "  The experimental channel follows tagged prereleases such as v2.4.1-exp.1."
                Write-Host "        https://github.com/$Repo/releases"
                exit 1
            }
            Write-Host "  Experimental build: $($release.tag_name)" -ForegroundColor Yellow
        }
        # Windows releases are .zip archives containing indent.exe. Match by
        # exact target first, then fall back to any windows/arch .zip so naming
        # variants (pc-windows-msvc vs windows) both work.
        $asset = $release.assets | Where-Object { $_.name -like "*-$target.zip" } | Select-Object -First 1
        if (-not $asset) {
            $archTok = if ($target -like "x86_64*") { "x86_64" } elseif ($target -like "aarch64*") { "aarch64" } elseif ($target -like "i686*") { "i686" } else { $target }
            $asset = $release.assets | Where-Object {
                ($_.name -like "*.zip") -and ($_.name -like "*$archTok*") -and ($_.name -like "*windows*")
            } | Select-Object -First 1
        }
        # There is no native ARM64 Windows build yet, but Windows 11 on ARM runs
        # the x86_64 one through its built-in emulation. Falling back is far more
        # useful than telling an ARM user that nothing exists.
        if (-not $asset -and $target -like "aarch64-pc-windows-*") {
            $asset = $release.assets | Where-Object { $_.name -like "*x86_64*pc-windows*.zip" } | Select-Object -First 1
            if ($asset) {
                Write-Host "  (no native ARM64 Windows build yet - installing the x86_64 build," -ForegroundColor Yellow
                Write-Host "   which Windows 11 on ARM runs under emulation)" -ForegroundColor Yellow
            }
        }
        if ($asset) {
            $tmp = Join-Path $env:TEMP "indent-install-$([guid]::NewGuid())"
            New-Item -ItemType Directory -Path $tmp | Out-Null
            $zipPath = Join-Path $tmp "indent.zip"
            Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -TimeoutSec 120 -UserAgent "indent-installer"
            Expand-Archive -Path $zipPath -DestinationPath $tmp -Force
            $exe = Get-ChildItem -Path $tmp -Recurse -Filter "indent.exe" | Select-Object -First 1
            if ($exe) {
                Copy-Item $exe.FullName (Join-Path $BinDir "indent.exe") -Force
                # Record the channel where `indent` looks for it, so `--version`
                # and `--channel` report it and `--update` keeps following it.
                Set-Content -Path (Join-Path $IndentHome "channel") -Value $Channel
                Write-Host "✓ Downloaded indent" -ForegroundColor Green
                foreach ($tool in @("air")) {
                    # A release archive ships `air.ps1`, a bare `air` (the shell
                    # version) and `air.ind` (the package source). Matching "air.*"
                    # and guessing the extension could copy the plain-text `air.ind`
                    # into place as air.exe, so only accept a real executable form
                    # and keep its own name.
                    $toolFile = $null
                    foreach ($candidate in @("$tool.ps1", "$tool.exe", "$tool.cmd")) {
                        $found = Get-ChildItem -Path $tmp -Recurse -Filter $candidate | Select-Object -First 1
                        if ($found) { $toolFile = $found; break }
                    }
                    if ($toolFile) {
                        Copy-Item $toolFile.FullName (Join-Path $BinDir $toolFile.Name) -Force
                        Write-Host "✓ Installed $($toolFile.Name)" -ForegroundColor Green
                    } else {
                        Write-Host "  (no Windows $tool launcher in this archive; skipped)" -ForegroundColor Yellow
                    }
                }
                # Release archives are self-contained: install the complete
                # std/ (and packages/) they ship.
                $archiveStd = Get-ChildItem -Path $tmp -Recurse -Directory -Filter "std" | Select-Object -First 1
                if ($archiveStd) {
                    New-Item -ItemType Directory -Path $StdDir -Force | Out-Null
                    Copy-Item (Join-Path $archiveStd.FullName "*") $StdDir -Recurse -Force
                    Write-Host "✓ Installed std/ from release archive" -ForegroundColor Green
                    $stdInstalled = $true
                }
                $archivePkgs = Get-ChildItem -Path $tmp -Recurse -Directory -Filter "packages" | Select-Object -First 1
                if ($archivePkgs) {
                    New-Item -ItemType Directory -Path $PkgDir -Force | Out-Null
                    Copy-Item (Join-Path $archivePkgs.FullName "*") $PkgDir -Recurse -Force
                    Write-Host "✓ Installed packages/ from release archive" -ForegroundColor Green
                }
                Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
                $installed = $true
            }
        }
    } catch {
        Write-Host "  (no prebuilt release: $($_.Exception.Message))" -ForegroundColor Yellow
    }
    if (-not $installed) {
        # Never force a compile: some users have no toolchain and no way to get
        # one. Building from source is strictly opt-in via -BuildFromSource.
        if (-not $BuildFromSource) {
            Write-Host ""
            Write-Host "No pre-built binary is available for $target in $Repo." -ForegroundColor Red
            Write-Host ""
            Write-Host "  Nothing was compiled and nothing was changed."
            Write-Host ""
            Write-Host "  Indent installs a pre-built binary by default, so no compiler is needed."
            Write-Host "  To get one, either:"
            Write-Host "    - install a release that ships a $target build:"
            Write-Host "        https://github.com/$Repo/releases" -ForegroundColor Cyan
            Write-Host "    - or ask for a specific version that has one:"
            Write-Host "        -Version <tag>"
            Write-Host ""
            Write-Host "  If you do have Rust and want to compile it yourself:"
            Write-Host "        -BuildFromSource"
            Write-Host ""
            exit 1
        }
        Write-Host ""
        Write-Host "No prebuilt release found - building from source as requested..." -ForegroundColor Yellow
        Write-Host ""
        Install-FromSource
    }
}

# ---- download standard library ----
if ($stdInstalled) {
    Write-Host "✓ Standard library installed from release archive" -ForegroundColor Green
} else {
    Write-Host "→ Installing standard library..."
    $StdBase = "https://raw.githubusercontent.com/$Repo/main/std"
    # Ask the API for the real file list: the old hard-coded four modules meant a
    # user who fell back to this path got a std/ that was mostly missing, which
    # shows up later as confusing "module not found" errors.
    $stdFiles = @()
    try {
        $listing = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/contents/std?ref=main" `
            -TimeoutSec 60 -Headers @{ "Accept" = "application/vnd.github+json" } -UserAgent "indent-installer"
        $stdFiles = @($listing | Where-Object { $_.name -like "*.ind" } | ForEach-Object { $_.name })
    } catch {
        Write-Host "  (could not list std/ - falling back to a short list)" -ForegroundColor Yellow
    }
    if ($stdFiles.Count -eq 0) {
        $stdFiles = @("io.ind", "math.ind", "strings.ind", "testing.ind")
    }
    $downloaded = 0
    foreach ($file in $stdFiles) {
        try {
            Invoke-WebRequest -Uri "$StdBase/$file" -OutFile (Join-Path $StdDir $file) -TimeoutSec 30 -UserAgent "indent-installer"
            $downloaded++
        } catch { Write-Host "  (skipping $file)" }
    }
    Write-Host "✓ Standard library installed ($downloaded modules)" -ForegroundColor Green
}

# ---- create launcher ----
# Written as ASCII (not the host's default encoding) so cmd.exe parses it the
# same on every machine, and it fails *loudly* if the runtime is missing rather
# than doing nothing.
@"
@echo off
setlocal
set "INDENT_EXE=$BinDir\indent.exe"
if not exist "%INDENT_EXE%" (
    echo indent: the Indent runtime was not found at "%INDENT_EXE%" 1>&2
    echo         Re-run the installer, or set INDENT_EXE to the right path.
    exit /b 1
)
rem Include site-packages, packages, and std in INDENT_PATH (like Python's sys.path)
set "INDENT_PATH=$PkgDir;$StdDir;$IndentHome\site-packages;%INDENT_PATH%"
"%INDENT_EXE%" %*
endlocal
"@ | Set-Content -Path (Join-Path $LauncherDir "indent.cmd") -Encoding ASCII

@"
@echo off
setlocal
set "INDENT_EXE=$BinDir\indent.exe"
if not exist "%INDENT_EXE%" (
    echo indent: the Indent runtime was not found at "%INDENT_EXE%" 1>&2
    exit /b 1
)
set "INDENT_PATH=$PkgDir;$StdDir;$IndentHome\site-packages;%INDENT_PATH%"
"%INDENT_EXE%" --debug %*
endlocal
"@ | Set-Content -Path (Join-Path $LauncherDir "indent-debug.cmd") -Encoding ASCII

Write-Host "✓ Created launcher: $LauncherDir\indent.cmd" -ForegroundColor Green

# ---- self-check: make sure the runtime actually runs ----
# A copy that exists but cannot start (missing runtime DLLs, blocked by AV,
# wrong architecture) shows up to the user as "installed, but indent does
# nothing". Run it once here so the failure is reported loudly instead.
$exePath = Join-Path $BinDir "indent.exe"
$runtimeOk = $false
if (Test-Path $exePath) {
    try {
        $verOutput = & $exePath --version 2>&1 | Out-String
        if ($verOutput -match "(?i)indent\s+\d") {
            $runtimeOk = $true
        }
    } catch { }
}
if ($runtimeOk) {
    Write-Host "✓ Runtime works: $($verOutput.Trim())" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "⚠  indent.exe was installed but did not run." -ForegroundColor Yellow
    Write-Host "    Running 'indent' returns to the prompt with no output until this is fixed." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "    The usual cause is a missing 'Microsoft Visual C++ Redistributable'" -ForegroundColor Yellow
    Write-Host "    (the error is 'VCRUNTIME140.dll was not found'). This release needs it;" -ForegroundColor Yellow
    Write-Host "    newer releases bundle the runtime and do not." -ForegroundColor Yellow
    Write-Host ""

    # Try to fix it for the user: the redistributable is a normal winget package.
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "→ Installing the Visual C++ Redistributable automatically..." -ForegroundColor Cyan
        try {
            winget install --id Microsoft.VCRedist.2015+.x64 -e `
                --accept-source-agreements --accept-package-agreements --silent | Out-Null
        } catch { }
        $verOutput = ""
        try { $verOutput = & $exePath --version 2>&1 | Out-String } catch { }
        if ($verOutput -match "(?i)indent\s+\d") {
            $runtimeOk = $true
            Write-Host "✓ Fixed - runtime works now: $($verOutput.Trim())" -ForegroundColor Green
        }
    }

    if (-not $runtimeOk) {
        Write-Host "    Install it manually, then open a NEW terminal and run 'indent':" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "      winget install --id Microsoft.VCRedist.2015+.x64 -e" -ForegroundColor Cyan
        Write-Host "      # or download: https://aka.ms/vs/17/release/vc_redist.x64.exe" -ForegroundColor Cyan
        Write-Host ""
        Write-Host "    If it still does not run, please report the output of:" -ForegroundColor Yellow
        Write-Host "      & `"$exePath`" --version" -ForegroundColor Cyan
        Write-Host ""
    }
}

# Confirm the launcher resolves and runs (this is what `indent` on PATH uses).
$launcherPath = Join-Path $LauncherDir "indent.cmd"
if (Test-Path $launcherPath) {
    try {
        $launcherOut = cmd /c "`"$launcherPath`" --version" 2>&1 | Out-String
        if ($launcherOut -match "(?i)indent\s+\d") {
            Write-Host "✓ Launcher works: $launcherPath" -ForegroundColor Green
        } else {
            Write-Host "⚠  The launcher ran but produced no version output:" -ForegroundColor Yellow
            Write-Host "      $($launcherOut.Trim())" -ForegroundColor Yellow
        }
    } catch {
        Write-Host "⚠  Could not run the launcher: $($_.Exception.Message)" -ForegroundColor Yellow
    }
}

# ---- auto-configure registry ----
$ConfigDir = Join-Path $env:USERPROFILE ".config\indent"
New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
@"
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
if ($null -eq $userPath) { $userPath = "" }
if ($userPath -notlike "*$LauncherDir*") {
    Write-Host "⚠  Add to your user PATH:" -ForegroundColor Yellow
    Write-Host ""
    Write-Host "  No admin rights are needed - this is the user PATH:" -ForegroundColor Cyan
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$LauncherDir', 'User')"
    Write-Host ""
    $response = Read-Host "  Add to PATH now? [Y/n]"
    if ($response -eq "" -or $response -match "^[Yy]") {
        $newUserPath = if ([string]::IsNullOrWhiteSpace($userPath)) { $LauncherDir } else { $userPath.TrimEnd(';') + ";" + $LauncherDir }
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        $env:Path = $env:Path + ";$LauncherDir"
        Write-Host "✓ PATH configured" -ForegroundColor Green
        Write-Host "  Open a NEW terminal (not this one) before running 'indent'." -ForegroundColor Cyan
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
