param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\LCP",
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BundledLcp = Join-Path $RepoRoot "lcp.exe"
$BundledDaemon = Join-Path $RepoRoot "lanclipd.exe"

function Get-CargoPath {
    $Cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($Cargo) {
        return $Cargo.Source
    }
    $Fallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $Fallback) {
        return $Fallback
    }
    throw "cargo was not found on PATH or at $Fallback"
}

if ((Test-Path $BundledLcp) -and (Test-Path $BundledDaemon)) {
    $LcpSource = $BundledLcp
    $DaemonSource = $BundledDaemon
} else {
    if (-not $SkipBuild) {
        Push-Location $RepoRoot
        try {
            $CargoProfile = if ($Configuration -eq "debug") { "dev" } else { "release" }
            & (Get-CargoPath) build --workspace --profile $CargoProfile
        } finally {
            Pop-Location
        }
    }

    $TargetDir = Join-Path $RepoRoot "target\$Configuration"
    $LcpSource = Join-Path $TargetDir "lcp.exe"
    $DaemonSource = Join-Path $TargetDir "lanclipd.exe"
}

if (-not (Test-Path $LcpSource) -or -not (Test-Path $DaemonSource)) {
    throw "Missing LCP binaries. Re-run without -SkipBuild, or run this script from an extracted release bundle."
}

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Force $LcpSource $InstallDir
Copy-Item -Force $DaemonSource $InstallDir

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$PathParts = @()
if ($UserPath) {
    $PathParts = $UserPath -split ";" | Where-Object { $_ }
}
if ($PathParts -notcontains $InstallDir) {
    $NewPath = (@($PathParts) + $InstallDir) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$InstallDir;$env:Path"
}

$Lcp = Join-Path $InstallDir "lcp.exe"
& $Lcp daemon install
& $Lcp daemon start

Write-Host "LCP installed to $InstallDir"
Write-Host "Open a new terminal if 'lcp' is not on PATH in this one."
