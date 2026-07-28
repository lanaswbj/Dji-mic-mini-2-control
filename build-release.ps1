#Requires -Version 5
#
# Build DJI Mic Control for Windows release with all path-privacy safeties on.
#
# Produces two artifacts in Release\windows\ :
#     DJI Mic Control.exe                     - portable, self-contained binary
#     DJI Mic Control_<ver>_x64-setup.exe     - NSIS installer
# The installer registers the app in Add/Remove Programs and the Start menu
# (so it's searchable) and auto-provisions the WebView2 runtime on install; the
# portable exe assumes WebView2 is already present. For macOS/Linux, use
# build-release.sh.
#
# "Safeties on" means every absolute filesystem path Rust would otherwise bake
# into panic/backtrace strings (which `strip` does NOT remove) is remapped away,
# so a crash can never leak the build machine's home directory, username, or
# hostname. The build then re-scans the compiled binary and FAILS if anything
# slipped through.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $ScriptDir

# --- Path-privacy safety -----------------------------------------------------
# Strip the home dir and Cargo registry from embedded paths. CARGO_ENCODED_-
# RUSTFLAGS (0x1f-separated) tolerates spaces in paths. We emit both backslash
# and forward-slash prefix variants because rustc's embedded paths can use
# either separator on Windows; the post-build scan is the backstop.
Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue  # avoid cargo conflict
$UserHome  = $env:USERPROFILE
$CargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $UserHome '.cargo' }
$UserHomeF  = $UserHome  -replace '\\', '/'
$CargoHomeF = $CargoHome -replace '\\', '/'
$US = [string][char]0x1f
$remaps = @(
    "--remap-path-prefix=$CargoHome\=",
    "--remap-path-prefix=$CargoHomeF/=",
    "--remap-path-prefix=$UserHome\=/",
    "--remap-path-prefix=$UserHomeF/=/"
)
$env:CARGO_ENCODED_RUSTFLAGS = ($remaps -join $US)

foreach ($tool in 'cargo', 'npm') {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        throw "$tool not found in PATH."
    }
}

Write-Host '==> Building Windows release (portable exe + NSIS installer)'

# --- Build (Tauri runs the frontend `npm run build` itself) ------------------
# --bundles nsis produces the installer; the standalone exe is still emitted as
# a byproduct of the compile step, so we get both.
Push-Location gui
try {
    npm install --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'npm install failed' }
    npx tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
}

# --- Collect artifacts -------------------------------------------------------
$Out = Join-Path $ScriptDir 'Release\windows'
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

# Portable, self-contained exe (emitted by the compile step).
$Exe = Join-Path $ScriptDir 'target\release\djimic-gui.exe'
if (-not (Test-Path $Exe)) { throw "Expected binary not found: $Exe" }
Copy-Item $Exe (Join-Path $Out 'DJI Mic Control.exe')
Write-Host '    + DJI Mic Control.exe (portable)'

# NSIS installer: registers the app in Add/Remove Programs and the Start menu,
# and auto-provisions the WebView2 runtime on install.
#
# Matched by the version in tauri.conf.json, not by `-First 1`. Cargo never
# cleans this directory, so every installer this machine has ever produced is
# still in it — including ones from another version under another product name.
# `Select-Object -First 1` picked whichever came back first, and it put a
# nine-month-old 1.1.0 installer inside a 2.0.0 release, with the leak scan
# below dutifully verifying the wrong file. Anything other than exactly one
# match is now a build failure: guessing is what caused that.
$Version = (Get-Content (Join-Path $ScriptDir 'gui\src-tauri\tauri.conf.json') -Raw |
            ConvertFrom-Json).version
if (-not $Version) { throw 'Could not read version from gui\src-tauri\tauri.conf.json' }
$Setups = @(Get-ChildItem (Join-Path $ScriptDir 'target\release\bundle\nsis') `
                -Filter "*_${Version}_x64-setup.exe" -ErrorAction SilentlyContinue)
if ($Setups.Count -ne 1) {
    throw ("Expected exactly one NSIS installer for version $Version in " +
           "target\release\bundle\nsis, found $($Setups.Count): " +
           (($Setups | Select-Object -ExpandProperty Name) -join ', '))
}
$Setup = $Setups[0]
Copy-Item $Setup.FullName (Join-Path $Out $Setup.Name)
Write-Host "    + $($Setup.Name) (installer)"

# --- Verify no build-machine paths leaked into either shipped file -----------
$needles = @("$UserHome\", "$UserHomeF/", "$CargoHome\", "$CargoHomeF/",
             $env:USERNAME, [System.Net.Dns]::GetHostName()) |
           Where-Object { $_ } | Select-Object -Unique
function Assert-NoLeak([string]$path) {
    $bytes = [System.IO.File]::ReadAllBytes($path)
    $text  = [System.Text.Encoding]::GetEncoding(28591).GetString($bytes)  # Latin1
    $hit   = $needles | Where-Object { $text.Contains($_) }
    if ($hit) {
        Write-Error ("SECURITY: build-machine paths leaked into " +
                     "$([System.IO.Path]::GetFileName($path)): " + ($hit -join ', '))
        exit 1
    }
}
Assert-NoLeak $Exe
Assert-NoLeak $Setup.FullName
Write-Host '==> Leak scan clean: no home path, Cargo path, username, or hostname in the exe or installer.'

# --- Verify the logo/resources were embedded into the exe --------------------
# tauri-build embeds the Windows resource block (app icon + version info) at
# compile time, so a populated VersionInfo confirms the icon is in the exe.
$vi = (Get-Item $Exe).VersionInfo
if ($vi.ProductName) {
    Write-Host "==> Logo/resources embedded (ProductName: $($vi.ProductName))."
} else {
    Write-Warning 'exe has no embedded version resource - the app icon may be missing.'
}

Write-Host '==> Done. Artifacts in Release\windows:'
Get-ChildItem $Out | Select-Object -ExpandProperty Name
