# noa installer script for Windows (PowerShell)
#
# Usage (PowerShell as Administrator):
#   iwr -useb https://raw.githubusercontent.com/celestia-island/noa/dev/scripts/install.ps1 | iex
#
# Or downloaded:
#   .\install.ps1 [-FromSource] [-Version X.Y.Z] [-Prefix PATH]
#
# Options:
#   -FromSource    Build from source (requires Rust 1.85+)
#   -Version X.Y.Z Install a specific version (default: latest)
#   -Prefix PATH   Installation directory (default: $env:LOCALAPPDATA\noa\bin)

param(
    [switch]$FromSource,
    [string]$Version = "latest",
    [string]$Prefix = ""
)

$ErrorActionPreference = "Stop"

# --- Defaults ---
$GITHUB_REPO = "celestia-island/noa"
$BINARY = "noa.exe"
$SERVER_BINARY = "noa-server.exe"

if ($Prefix -eq "") {
    $Prefix = Join-Path $env:LOCALAPPDATA "noa\bin"
}

# --- Detect platform ---
function Get-Platform {
    $arch = $env:PROCESSOR_ARCHITECTURE.ToLower()
    if ($arch -eq "amd64") { $arch = "x86_64" }
    if ($arch -eq "arm64") { $arch = "aarch64" }
    return "windows-$arch"
}

# --- Install from prebuilt binary ---
function Install-FromRelease {
    $platform = Get-Platform
    Write-Host "→ Platform: $platform"

    if ($Version -eq "latest") {
        Write-Host "→ Fetching latest release version..."
        try {
            $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$GITHUB_REPO/releases/latest"
            $tag = $release.tag_name
        } catch {
            Write-Host "Failed to fetch latest version. Falling back to -FromSource." -ForegroundColor Yellow
            Install-FromSource
            return
        }
    } else {
        $tag = "v$($Version -replace '^v','')"
    }
    Write-Host "→ Installing noa $tag"

    switch ($platform) {
        "windows-x86_64"  { $asset_name = "noa-windows-x86_64.zip" }
        "windows-aarch64" { $asset_name = "noa-windows-aarch64.zip" }
        default           {
            Write-Host "Prebuilt binary not available for platform: $platform" -ForegroundColor Yellow
            Write-Host "Falling back to -FromSource..." -ForegroundColor Yellow
            Install-FromSource
            return
        }
    }

    $asset_url = "https://github.com/$GITHUB_REPO/releases/download/$tag/$asset_name"
    $tmpdir = Join-Path $env:TEMP "noa-install-$([System.Guid]::NewGuid())"
    $archive = Join-Path $tmpdir $asset_name
    New-Item -ItemType Directory -Force -Path $tmpdir | Out-Null

    Write-Host "→ Downloading $asset_name..."
    try {
        Invoke-WebRequest -Uri $asset_url -OutFile $archive
    } catch {
        Write-Host "Prebuilt binary not found. Falling back to -FromSource." -ForegroundColor Yellow
        Remove-Item -Recurse -Force $tmpdir -ErrorAction SilentlyContinue
        Install-FromSource
        return
    }

    Write-Host "→ Extracting..."
    Expand-Archive -Path $archive -DestinationPath $tmpdir -Force

    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    foreach ($bin in @($BINARY, $SERVER_BINARY)) {
        $src = Get-ChildItem -Path $tmpdir -Name $bin -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($src) {
            $src_path = Join-Path $tmpdir $src
            $dst_path = Join-Path $Prefix $bin
            Copy-Item -Path $src_path -Destination $dst_path -Force
            Write-Host "✓ Installed $dst_path"
        }
    }

    Remove-Item -Recurse -Force $tmpdir -ErrorAction SilentlyContinue
}

# --- Install from source ---
function Install-FromSource {
    Write-Host "→ Building from source (requires Rust 1.85+)..."

    if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
        Write-Host "Rust is not installed. Install it first: https://rustup.rs" -ForegroundColor Red
        exit 1
    }

    $tmpdir = Join-Path $env:TEMP "noa-build-$([System.Guid]::NewGuid())"
    $ref = $Version
    if ($ref -eq "latest") { $ref = "dev" }

    Write-Host "→ Cloning repository (branch: $ref)..."
    try {
        git clone --depth 1 --branch $ref "https://github.com/$GITHUB_REPO.git" $tmpdir 2>$null
    } catch {
        Write-Host "Branch '$ref' not found. Using default branch..."
        git clone --depth 1 "https://github.com/$GITHUB_REPO.git" $tmpdir
    }

    Write-Host "→ Building (release)..."
    Push-Location $tmpdir
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "Build failed"
        }
    } finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    foreach ($bin in @($BINARY, $SERVER_BINARY)) {
        $src = Join-Path $tmpdir "target\release\$bin"
        if (Test-Path $src) {
            $dst = Join-Path $Prefix $bin
            Copy-Item -Path $src -Destination $dst -Force
            Write-Host "✓ Installed $dst"
        }
    }

    Remove-Item -Recurse -Force $tmpdir -ErrorAction SilentlyContinue
}

# --- Verify PATH ---
function Verify-Path {
    $currentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($currentPath -notlike "*$Prefix*") {
        Write-Host ""
        Write-Host "⚠  $Prefix is not in your PATH." -ForegroundColor Yellow
        Write-Host "   Adding to User PATH..."
        $newPath = "$Prefix;$currentPath"
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "✓ Added to User PATH. Restart your terminal for changes to take effect."
    }
}

# --- Main ---
Write-Host ""
Write-Host "╔══════════════════════════════════╗" -ForegroundColor Cyan
Write-Host "║  noa installer  (v0.1.0-alpha)  ║" -ForegroundColor Cyan
Write-Host "╚══════════════════════════════════╝" -ForegroundColor Cyan
Write-Host ""

if ($FromSource) {
    Install-FromSource
} else {
    Install-FromRelease
}

Write-Host ""

if (Get-Command $BINARY -ErrorAction SilentlyContinue) {
    Write-Host "✓ noa installed successfully!" -ForegroundColor Green
} else {
    Verify-Path
    $bin_path = Join-Path $Prefix $BINARY
    if (Test-Path $bin_path) {
        Write-Host "Run: & '$bin_path' --help"
    }
}

# Refresh PATH in current session
$env:Path = "$Prefix;$env:Path"
