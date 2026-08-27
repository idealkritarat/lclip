param(
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\LCP",
    [switch]$KeepFiles
)

$ErrorActionPreference = "Stop"

$Lcp = Join-Path $InstallDir "lcp.exe"
if (Test-Path $Lcp) {
    & $Lcp daemon uninstall
    & $Lcp daemon stop
} else {
    reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v LCP /f 2>$null | Out-Null
}

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath) {
    $PathParts = $UserPath -split ";" | Where-Object { $_ -and ($_ -ne $InstallDir) }
    [Environment]::SetEnvironmentVariable("Path", ($PathParts -join ";"), "User")
}

if (-not $KeepFiles -and (Test-Path $InstallDir)) {
    Remove-Item -LiteralPath $InstallDir -Recurse -Force
}

Write-Host "LCP autostart removed. Identity/config are left intact."
