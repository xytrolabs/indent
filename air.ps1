# AIR — Accessible Indent Registry — pip-like package manager for Indent
# Windows PowerShell version
param(
    [string]$Command,
    [string[]]$Args
)

$AIR_HOME = "$env:USERPROFILE\.local\share\indent"
$AIR_PACKAGES = "$AIR_HOME\air-packages"
$AIR_CACHE = "$AIR_HOME\air-cache"
$AIR_REGISTRY = if ($env:AIR_REGISTRY) { $env:AIR_REGISTRY } else { "https://raw.githubusercontent.com/xytrolabs/air/main" }
$AIR_INDEX = "$AIR_REGISTRY/packages/index.txt"
$global:INSTALLED_FILE = "$AIR_HOME\installed.txt"

function Write-Usage {
    @"
AIR — Indent Package Manager (pip for Indent)

Usage:
  air install <name>[@version]     Install a package
  air install <name> <url>         Install from URL or local path
  air uninstall <name>             Remove a package
  air update [name]                Update one or all packages
  air search <query>               Search registry for packages
  air list                         List installed packages
  air info <name>                  Show package details
  air init                         Initialize a new Indent project

Examples:
  air install colors               Install latest from registry
  air install mypkg ./mypkg.ind    Install from local file
  air uninstall colors             Remove package
  air update                       Update all installed packages
  air search json                  Find json-related packages
  air list                         Show what's installed
"@
}

function Initialize-Dirs {
    foreach ($d in @($AIR_HOME, $AIR_PACKAGES, $AIR_CACHE)) {
        if (!(Test-Path $d)) { New-Item -ItemType Directory -Path $d -Force | Out-Null }
    }
}

function Get-Index {
    Initialize-Dirs
    $cacheFile = "$AIR_CACHE\index-$($AIR_INDEX.GetHashCode()).txt"
    if ((Test-Path $cacheFile) -and ((Get-Date) - (Get-Item $cacheFile).LastWriteTime).TotalMinutes -lt 30) {
        return Get-Content $cacheFile -Raw
    }
    try {
        $data = Invoke-RestMethod -Uri $AIR_INDEX -TimeoutSec 15
        $data | Out-File $cacheFile -Encoding UTF8
        return $data
    } catch {
        Write-Host "⚠ Cannot reach registry: $AIR_INDEX" -ForegroundColor Yellow
        return $null
    }
}

function Install-Package {
    param($Name, $Source)
    Initialize-Dirs

    if (-not $Source) {
        $index = Get-Index
        if (-not $index) { exit 1 }
        $lines = $index -split "`n"
        foreach ($line in $lines) {
            if ($line -match "^[[:space:]]*#") { continue }
            $parts = $line -split '\|'
            if ($parts.Count -ge 2) {
                $pkgName = $parts[0].Trim()
                if ($pkgName -eq $Name) {
                    $Source = $parts[1].Trim()
                    if ($Source -notmatch "^https?://") {
                        $Source = "$AIR_REGISTRY/packages/$($Source -replace '^\./', '')"
                    }
                    break
                }
            }
        }
        if (-not $Source) {
            Write-Host "✗ Package '$Name' not found in registry" -ForegroundColor Red
            exit 1
        }
    }

    $target = "$AIR_PACKAGES\$Name.ind"
    Write-Host "📦 Installing $Name..."

    if ($Source -match "^https?://") {
        Invoke-WebRequest -Uri $Source -OutFile $target -TimeoutSec 60
    } else {
        if (Test-Path $Source) {
            Copy-Item $Source $target -Force
        } else {
            Write-Host "✗ Source not found: $Source" -ForegroundColor Red
            exit 1
        }
    }

    Write-Host "✅ Installed $Name → $target"
}

function Uninstall-Package {
    param($Name)
    $target = "$AIR_PACKAGES\$Name.ind"
    if (Test-Path $target) {
        Remove-Item $target -Force
        Write-Host "🗑 Removed $target"
    }
    Write-Host "✅ Uninstalled $Name"
}

function Search-Packages {
    param($Query)
    $index = Get-Index
    if (-not $index) { exit 1 }
    
    Write-Host "🔍 Packages matching '$Query':`n"
    $lines = $index -split "`n"
    $found = 0
    foreach ($line in $lines) {
        if ($line -match "^[[:space:]]*#" -or $line.Trim() -eq "") { continue }
        $parts = $line -split '\|'
        if ($parts.Count -ge 2) {
            $name = $parts[0].Trim()
            $desc = if ($parts.Count -ge 4) { $parts[3].Trim() } else { "" }
            if ("$name $desc".ToLower().Contains($Query.ToLower())) {
                Write-Host "  {0,-20} {1}" -f $name, $desc
                $found++
            }
        }
    }
    if ($found -eq 0) { Write-Host "  (no results)" }
    Write-Host ""
}

function List-Packages {
    Initialize-Dirs
    if (!(Test-Path $INSTALLED_FILE) -or (Get-Content $INSTALLED_FILE -Raw).Trim() -eq "") {
        Write-Host "No packages installed"
        return
    }
    Write-Host "📦 Installed packages:`n"
    foreach ($line in (Get-Content $INSTALLED_FILE)) {
        $parts = $line -split '\|'
        if ($parts.Count -ge 2) {
            Write-Host "  {0,-20} {1}" -f $parts[0], $parts[1]
        }
    }
    Write-Host ""
}

# ── Main ────────────────────────────────────────────────────────────
Initialize-Dirs

switch ($Command) {
    "install"   { Install-Package $Args[0] $Args[1] }
    "add"       { Install-Package $Args[0] $Args[1] }
    "uninstall" { Uninstall-Package $Args[0] }
    "remove"    { Uninstall-Package $Args[0] }
    "update"    { if ($Args[0]) { Install-Package $Args[0] } else { Write-Host "Update all not yet implemented" } }
    "search"    { Search-Packages $Args[0] }
    "find"      { Search-Packages $Args[0] }
    "list"      { List-Packages }
    "info"      { Write-Host "info: $($Args[0])" }
    "init"      { if (!(Test-Path "indent.toml")) { "[package]`nname = `"my-project`"`nversion = `"0.1.0`"" | Out-File indent.toml }; Write-Host "✅ Ready!" }
    "help"      { Write-Usage }
    default     { Write-Usage }
}
