param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\LCP",
    [switch]$KeepFiles
)

$ErrorActionPreference = "Stop"

Write-Host "Stopping daemon if it is running..."
$ExistingDaemon = @(Get-Process -Name "lanclipd" -ErrorAction SilentlyContinue)
if ($ExistingDaemon.Count -gt 0) {
    $ExistingDaemon | Stop-Process -Force
    $ExistingDaemon | Wait-Process -Timeout 10 -ErrorAction SilentlyContinue
}

Write-Host "Removing daemon autostart..."
& reg.exe delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v LCP /f 2>$null | Out-Null
if (($LASTEXITCODE -ne 0) -and ($LASTEXITCODE -ne 1)) {
    throw "Failed to remove LCP autostart with reg.exe exit code $LASTEXITCODE."
}

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath) {
    $PathParts = $UserPath -split ";" | Where-Object { $_ -and ($_ -ne $InstallDir) }
    [Environment]::SetEnvironmentVariable("Path", ($PathParts -join ";"), "User")
}

if (-not $KeepFiles -and (Test-Path $InstallDir)) {
    Write-Host "Removing installed binaries from $InstallDir..."
    Remove-Item -LiteralPath $InstallDir -Recurse -Force
}

Write-Host "LCP autostart removed. Identity/config are left intact."
