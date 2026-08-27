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
    Write-Host "Using bundled LCP binaries."
    $LcpSource = $BundledLcp
    $DaemonSource = $BundledDaemon
} else {
    if (-not $SkipBuild) {
        Write-Host "Building LCP from source..."
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

Write-Host "Stopping existing daemon if it is running..."
$ExistingDaemon = @(Get-Process -Name "lanclipd" -ErrorAction SilentlyContinue)
if ($ExistingDaemon.Count -gt 0) {
    $ExistingDaemon | Stop-Process -Force
    $ExistingDaemon | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
}

Write-Host "Copying binaries to $InstallDir..."
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -Force $LcpSource $InstallDir
Copy-Item -Force $DaemonSource $InstallDir

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$PathParts = @()
if ($UserPath) {
    $PathParts = $UserPath -split ";" | Where-Object { $_ }
}
if ($PathParts -notcontains $InstallDir) {
    Write-Host "Adding LCP to user PATH..."
    $NewPath = (@($PathParts) + $InstallDir) -join ";"
    [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    $env:Path = "$InstallDir;$env:Path"
}

$Daemon = Join-Path $InstallDir "lanclipd.exe"
Write-Host "Registering daemon autostart..."
$RunKey = "HKCU\Software\Microsoft\Windows\CurrentVersion\Run"
$DaemonRunValue = "`"$Daemon`""
& reg.exe add $RunKey /v LCP /t REG_SZ /d $DaemonRunValue /f | Out-Host
if ($LASTEXITCODE -ne 0) {
    throw "Failed to register LCP autostart with reg.exe exit code $LASTEXITCODE."
}
Write-Host "Autostart installed for the current user."

Write-Host "Starting daemon..."
try {
    Start-Process -FilePath $Daemon -WindowStyle Hidden | Out-Null
    Write-Host "lanclipd launch requested."
} catch {
    Write-Host "Warning: could not start lanclipd now: $($_.Exception.Message)"
    Write-Host "You can start it later by opening a new terminal and running: lanclipd"
}

Write-Host "LCP installed to $InstallDir"
Write-Host "Open a new terminal if 'lcp' is not on PATH in this one."
