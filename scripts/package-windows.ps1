param(
    [string]$OutDir = "dist",
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$OutPath = Join-Path $RepoRoot $OutDir
$Stage = Join-Path $OutPath "lcp-windows-x86_64"

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

if (-not $SkipBuild) {
    Push-Location $RepoRoot
    try {
        $CargoProfile = if ($Configuration -eq "debug") { "dev" } else { "release" }
        & (Get-CargoPath) build --workspace --profile $CargoProfile
    } finally {
        Pop-Location
    }
}

if (Test-Path $Stage) {
    Remove-Item -LiteralPath $Stage -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $Stage | Out-Null

$TargetDir = Join-Path $RepoRoot "target\$Configuration"
Copy-Item -Force (Join-Path $TargetDir "lcp.exe") $Stage
Copy-Item -Force (Join-Path $TargetDir "lanclipd.exe") $Stage
Copy-Item -Force (Join-Path $RepoRoot "README.md") $Stage
Copy-Item -Force (Join-Path $RepoRoot "LICENSE") $Stage
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "scripts") | Out-Null
Copy-Item -Force (Join-Path $RepoRoot "scripts\install-windows.ps1") (Join-Path $Stage "scripts")
Copy-Item -Force (Join-Path $RepoRoot "scripts\uninstall-windows.ps1") (Join-Path $Stage "scripts")

$Zip = Join-Path $OutPath "lcp-windows-x86_64.zip"
if (Test-Path $Zip) {
    Remove-Item -LiteralPath $Zip -Force
}
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Zip
Get-FileHash -Algorithm SHA256 $Zip |
    ForEach-Object { "$($_.Hash.ToLowerInvariant())  $(Split-Path $_.Path -Leaf)" } |
    Set-Content -NoNewline -Encoding ascii "$Zip.sha256"

Write-Host "Wrote $Zip"
Write-Host "Wrote $Zip.sha256"
