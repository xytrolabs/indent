param()
# AIR — Accessible Indent Registry — Package Manager for Indent

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfigDir = Join-Path $env:USERPROFILE ".config\indent"
$ConfigFile = Join-Path $ConfigDir "air.json"
$DefaultRegistryRepo = "XytroLabs-Indent/air"
$DefaultRegistryRef = "master"
$DefaultRegistryIndexPath = "index.txt"
$DefaultRegistryUrl = "https://raw.githubusercontent.com/$DefaultRegistryRepo/$DefaultRegistryRef/$DefaultRegistryIndexPath"
$LegacyRegistryUrls = @(
    "https://raw.githubusercontent.com/indent-lang/packages/main/index.txt",
    "https://raw.githubusercontent.com/XytroLabs-Indent/air/main/packages/index.txt",
    "https://raw.githubusercontent.com/XytroLabs-Indent/air/main/packages/index.txt",
    "https://raw.githubusercontent.com/xytro-labs/Indent.ind/main/packages/index.txt"
)

function Write-Usage {
    @"
Usage:
  air init
  air install [--local|--global] <name> [source]
  air uninstall [--local|--global] <name>
  air update [--local|--global] [name]
  air search <query>
  air list [--local|--global]
  air index
  air publish <name> <package-file> [description] [--no-push]

Registry:
  air registry show
  air registry set <url-or-path>
  air registry github <owner/repo> [ref] [index-path]
  air registry reset
"@
}

function Ensure-ConfigDir {
    New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
}

function Get-DefaultConfig {
    return @{
        Index = $DefaultRegistryUrl
        Repo = $DefaultRegistryRepo
        Ref = $DefaultRegistryRef
        IndexPath = $DefaultRegistryIndexPath
    }
}

function Is-LegacyRegistryUrl([string]$url) {
    if ([string]::IsNullOrWhiteSpace($url)) { return $false }
    return $LegacyRegistryUrls -contains $url
}

function Migrate-LegacyRegistry($cfg) {
    if (-not (Is-LegacyRegistryUrl $cfg.Index)) {
        return $cfg
    }

    if (-not [string]::IsNullOrWhiteSpace($cfg.Repo) -and $cfg.Repo -ne "indent-lang/packages" -and $cfg.Repo -ne "xytro-labs/indent" -and $cfg.Repo -ne "xytro-labs/Indent.ind") {
        return $cfg
    }

    $cfg.Index = $DefaultRegistryUrl
    $cfg.Repo = $DefaultRegistryRepo
    $cfg.Ref = $DefaultRegistryRef
    $cfg.IndexPath = $DefaultRegistryIndexPath
    Save-Config $cfg
    Write-Host "Migrated AIR registry to: $($cfg.Index)"
    return $cfg
}

function Load-Config {
    if (-not (Test-Path $ConfigFile)) {
        return Get-DefaultConfig
    }

    try {
        $json = Get-Content $ConfigFile -Raw | ConvertFrom-Json
        $cfg = @{
            Index = [string]($json.Index)
            Repo = [string]($json.Repo)
            Ref = if ([string]::IsNullOrWhiteSpace([string]$json.Ref)) { "main" } else { [string]$json.Ref }
            IndexPath = if ([string]::IsNullOrWhiteSpace([string]$json.IndexPath)) { "packages/index.txt" } else { [string]$json.IndexPath }
        }
        return Migrate-LegacyRegistry $cfg
    } catch {
        return Get-DefaultConfig
    }
}

function Save-Config($cfg) {
    Ensure-ConfigDir
    $cfg | ConvertTo-Json | Set-Content $ConfigFile
}

function Normalize-GitHubSlug([string]$input) {
    $slug = $input
    if ($slug -match '^https?://github.com/') {
        $slug = $slug -replace '^https?://github.com/', ''
    }
    $slug = $slug.TrimEnd('/')
    $slug = $slug -replace '\.git$', ''
    if ($slug -notmatch '^[^/]+/[^/]+$') {
        throw "Invalid GitHub repo. Use owner/repo or https://github.com/owner/repo(.git)"
    }
    return $slug
}

function Remote-UrlExists([string]$url) {
    try {
        Invoke-WebRequest -Uri $url -Method Get -TimeoutSec 12 | Out-Null
        return $true
    } catch {
        return $false
    }
}

function Resolve-RepoCloneUrl([string]$repoInput) {
    if (Test-Path $repoInput) {
        return (Resolve-Path $repoInput).Path
    }
    if ($repoInput -match '^https?://' -or $repoInput -match '^git@') {
        return $repoInput
    }
    return "https://github.com/$repoInput.git"
}

function Get-IndentPkgCommand {
    $local = Join-Path $ScriptDir "indentpkg.ps1"
    if (Test-Path $local) { return $local }

    $cmd = Get-Command indentpkg.ps1 -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }

    throw "indentpkg.ps1 not found next to air.ps1 or in PATH"
}

function Invoke-IndentPkg([string[]]$pkgArgs) {
    $cfg = Load-Config
    $pkg = Get-IndentPkgCommand
    if (-not [string]::IsNullOrWhiteSpace($cfg.Index)) {
        $env:INDENTPKG_INDEX = $cfg.Index
    }
    & powershell -ExecutionPolicy Bypass -File $pkg @pkgArgs
}

function Update-IndexEntry([string]$indexFile, [string]$name, [string]$source, [string]$description) {
    $lines = @()
    if (Test-Path $indexFile) {
        foreach ($line in Get-Content $indexFile) {
            $trim = $line.Trim()
            if ($trim.StartsWith("#") -or [string]::IsNullOrWhiteSpace($trim)) {
                $lines += $line
                continue
            }
            $pkg = ($trim.Split('|')[0]).Trim()
            if ($pkg -eq $name) { continue }
            $lines += $line
        }
    }
    if ($lines.Count -eq 0) {
        $lines += "# name|source|description"
    }
    $lines += "$name|$source|$description"
    Set-Content -Path $indexFile -Value $lines
}

function Cmd-Registry([string[]]$argsIn) {
    if ($argsIn.Count -lt 1) { Write-Usage; exit 2 }
    $sub = $argsIn[0]
    $cfg = Load-Config

    switch ($sub) {
        "show" {
            Write-Host "Registry: $($cfg.Index)"
            if (-not [string]::IsNullOrWhiteSpace($cfg.Repo)) {
                Write-Host "Publish repo: $($cfg.Repo)"
                Write-Host "Publish ref: $($cfg.Ref)"
                Write-Host "Publish index path: $($cfg.IndexPath)"
            }
        }
        "set" {
            if ($argsIn.Count -ne 2) { Write-Usage; exit 2 }
            $value = $argsIn[1]
            if ($value -notmatch '^https?://' -and -not [System.IO.Path]::IsPathRooted($value)) {
                $value = Join-Path (Get-Location) $value
            }
            $cfg.Index = $value
            Save-Config $cfg
            Write-Host "Registry set to: $value"
        }
        "github" {
            if ($argsIn.Count -lt 2 -or $argsIn.Count -gt 4) { Write-Usage; exit 2 }
            $repo = Normalize-GitHubSlug $argsIn[1]
            $ref = if ($argsIn.Count -ge 3) { $argsIn[2] } else { "main" }
            $indexPath = if ($argsIn.Count -ge 4) { $argsIn[3] } else { "" }

            if ([string]::IsNullOrWhiteSpace($indexPath)) {
                $packagesCandidate = "https://raw.githubusercontent.com/$repo/$ref/packages/index.txt"
                $rootCandidate = "https://raw.githubusercontent.com/$repo/$ref/index.txt"
                if (Remote-UrlExists $packagesCandidate) {
                    $indexPath = "packages/index.txt"
                } elseif (Remote-UrlExists $rootCandidate) {
                    $indexPath = "index.txt"
                } else {
                    $indexPath = "packages/index.txt"
                }
            }

            $url = "https://raw.githubusercontent.com/$repo/$ref/$indexPath"
            $cfg.Repo = $repo
            $cfg.Ref = $ref
            $cfg.IndexPath = $indexPath
            $cfg.Index = $url
            Save-Config $cfg

            Write-Host "Registry set to: $url"
            Write-Host "GitHub registry URL: $url"
        }
        "reset" {
            $cfg = Get-DefaultConfig
            Save-Config $cfg
            Write-Host "Registry reset to: $($cfg.Index)"
        }
        default {
            Write-Usage
            exit 2
        }
    }
}

function Cmd-Publish([string[]]$argsIn) {
    if ($argsIn.Count -lt 2) { Write-Usage; exit 2 }
    $name = $argsIn[0]
    $packageFile = $argsIn[1]
    $rest = if ($argsIn.Count -gt 2) { @($argsIn[2..($argsIn.Count - 1)]) } else { @() }

    if ($name -notmatch '^[a-zA-Z_][a-zA-Z0-9_]*$') { throw "Invalid package name: $name" }
    if (-not (Test-Path $packageFile)) { throw "Package file not found: $packageFile" }

    $description = "Published via air"
    if ($rest.Count -gt 0 -and $rest[0] -notmatch '^--') {
        $description = $rest[0]
        $rest = if ($rest.Count -gt 1) { @($rest[1..($rest.Count - 1)]) } else { @() }
    }

    $cfg = Load-Config
    $repo = $cfg.Repo
    $ref = if ([string]::IsNullOrWhiteSpace($cfg.Ref)) { "main" } else { $cfg.Ref }
    $indexPath = if ([string]::IsNullOrWhiteSpace($cfg.IndexPath)) { "packages/index.txt" } else { $cfg.IndexPath }
    $noPush = $false

    for ($i = 0; $i -lt $rest.Count; $i++) {
        switch ($rest[$i]) {
            "--registry-repo" { $i++; $repo = $rest[$i] }
            "--ref" { $i++; $ref = $rest[$i] }
            "--index-path" { $i++; $indexPath = $rest[$i] }
            "--no-push" { $noPush = $true }
            default { throw "Unknown option: $($rest[$i])" }
        }
    }

    if ([string]::IsNullOrWhiteSpace($repo)) {
        throw "No publish registry repo configured. Run: air registry github <owner/repo> [ref] [index-path]"
    }

    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        throw "git is required for air publish"
    }

    $cloneUrl = Resolve-RepoCloneUrl $repo
    $tmpDir = Join-Path $env:TEMP ("air-publish-" + [guid]::NewGuid().ToString())
    $registryDir = Join-Path $tmpDir "registry"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        & git clone --depth 1 --branch $ref $cloneUrl $registryDir | Out-Null

        $indexFile = Join-Path $registryDir $indexPath
        $indexDir = Split-Path -Parent $indexFile
        New-Item -ItemType Directory -Path $indexDir -Force | Out-Null
        if (-not (Test-Path $indexFile)) {
            Set-Content -Path $indexFile -Value "# name|source|description"
        }

        $destPkg = Join-Path $indexDir ("$name.ind")
        Copy-Item $packageFile $destPkg -Force

        Update-IndexEntry -indexFile $indexFile -name $name -source ("./$name.ind") -description $description

        Push-Location $registryDir
        & git add $indexPath ((Split-Path -Parent $indexPath) + "/$name.ind") | Out-Null
        $hasChanges = $true
        & git diff --cached --quiet
        if ($LASTEXITCODE -eq 0) { $hasChanges = $false }
        if (-not $hasChanges) {
            Write-Host "No publish changes detected for package '$name'."
            Pop-Location
            return
        }
        & git commit -m "air publish: $name" | Out-Null
        Pop-Location

        if ($noPush) {
            Write-Host "Published locally (no push) in: $registryDir"
            Write-Host "Run these commands to push:"
            Write-Host "  cd $registryDir"
            Write-Host "  git push origin $ref"
            return
        }

        Push-Location $registryDir
        & git push origin $ref
        Pop-Location

        Write-Host "Published '$name' to registry '$repo' on branch '$ref'."
    } catch {
        Write-Error $_
        if (Test-Path $registryDir) {
            Write-Host "Prepared publish state is at: $registryDir"
        }
        exit 1
    }
}

if ($args.Count -lt 1) {
    Write-Usage
    exit 2
}

$cmd = $args[0]
$rest = if ($args.Count -gt 1) { @($args[1..($args.Count - 1)]) } else { @() }

switch ($cmd) {
    "registry" { Cmd-Registry $rest }
    "publish" { Cmd-Publish $rest }
    "init" { Invoke-IndentPkg @("init") }
    "install" { Invoke-IndentPkg @("install") + $rest }
    "add" { Invoke-IndentPkg @("add") + $rest }
    "uninstall" { Invoke-IndentPkg @("uninstall") + $rest }
    "remove" { Invoke-IndentPkg @("remove") + $rest }
    "rm" { Invoke-IndentPkg @("rm") + $rest }
    "update" { Invoke-IndentPkg @("update") + $rest }
    "upgrade" { Invoke-IndentPkg @("upgrade") + $rest }
    "search" { Invoke-IndentPkg @("search") + $rest }
    "find" { Invoke-IndentPkg @("find") + $rest }
    "index" { Invoke-IndentPkg @("index") }
    "list" { Invoke-IndentPkg @("list") + $rest }
    "help" { Write-Usage }
    "--help" { Write-Usage }
    "-h" { Write-Usage }
    default {
        Write-Usage
        exit 2
    }
}
