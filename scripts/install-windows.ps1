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

function Invoke-LcpCommand {
    param(
        [string[]]$Arguments,
        [int]$TimeoutSeconds = 20
    )

    $CommandName = "lcp " + ($Arguments -join " ")
    $StartInfo = New-Object System.Diagnostics.ProcessStartInfo
    $StartInfo.FileName = $Lcp
    $StartInfo.Arguments = $Arguments -join " "
    $StartInfo.UseShellExecute = $false
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true
    $StartInfo.CreateNoWindow = $true

    $Process = New-Object System.Diagnostics.Process
    $Process.StartInfo = $StartInfo
    try {
        [void]$Process.Start()
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            try {
                $Process.Kill()
            } catch {
            }
            throw "$CommandName timed out after $TimeoutSeconds seconds."
        }

        $Output = $Process.StandardOutput.ReadToEnd().Trim()
        $ErrorOutput = $Process.StandardError.ReadToEnd().Trim()
        if ($Output) {
            Write-Host $Output
        }
        if ($ErrorOutput) {
            Write-Host $ErrorOutput
        }
        return $Process.ExitCode
    } finally {
        if ($Process) {
            $Process.Dispose()
        }
    }
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

$Lcp = Join-Path $InstallDir "lcp.exe"
Write-Host "Registering daemon autostart..."
$InstallExitCode = Invoke-LcpCommand -Arguments @("daemon", "install") -TimeoutSeconds 20
if ($InstallExitCode -ne 0) {
    throw "lcp daemon install failed with exit code $InstallExitCode."
}

Write-Host "Starting daemon..."
$StartExitCode = Invoke-LcpCommand -Arguments @("daemon", "start") -TimeoutSeconds 30
if ($StartExitCode -ne 0) {
    Write-Host "Warning: lcp daemon start exited with code $StartExitCode."
    Write-Host "You can retry after install with: lcp daemon start"
}

Write-Host "LCP installed to $InstallDir"
Write-Host "Open a new terminal if 'lcp' is not on PATH in this one."
