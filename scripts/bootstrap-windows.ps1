param(
    [string]$Repo = "idealkritarat/lclip",
    [string]$Branch = "master",
    [string]$InstallDir = "$env:LOCALAPPDATA\Programs\LCP"
)

$ErrorActionPreference = "Stop"
$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("lcp-install-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null

function Invoke-Download {
    param([string]$Uri, [string]$OutFile)

    $Name = Split-Path -Leaf $OutFile
    for ($Attempt = 1; $Attempt -le 3; $Attempt++) {
        try {
            Write-Host "Downloading $Name (attempt $Attempt/3)..."
            $PreviousProgressPreference = $ProgressPreference
            $ProgressPreference = "SilentlyContinue"
            try {
                Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -TimeoutSec 120
            } finally {
                $ProgressPreference = $PreviousProgressPreference
            }
            $Size = [math]::Round((Get-Item $OutFile).Length / 1MB, 1)
            Write-Host "Downloaded $Name ($Size MB)."
            return
        } catch {
            if ($Attempt -eq 3) {
                throw
            }
            Write-Host "Download failed; retrying in 2 seconds..."
            Start-Sleep -Seconds 2
        }
    }
}

function Install-FromRelease {
    try {
        Write-Host "Checking latest LCP release..."
        $Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    } catch {
        return $false
    }

    $Asset = $Release.assets | Where-Object { $_.name -eq "lcp-windows-x86_64.zip" } | Select-Object -First 1
    if (-not $Asset) {
        return $false
    }

    $Zip = Join-Path $Tmp "lcp-windows-x86_64.zip"
    $Sha = Join-Path $Tmp "lcp-windows-x86_64.zip.sha256"
    Write-Host "Found release $($Release.tag_name)."
    Invoke-Download $Asset.browser_download_url $Zip
    Invoke-Download ($Asset.browser_download_url + ".sha256") $Sha

    Write-Host "Verifying checksum..."
    $Expected = ((Get-Content $Sha -Raw).Trim() -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) {
        throw "Checksum mismatch for $Zip"
    }

    Write-Host "Extracting release archive..."
    Expand-Archive -Force -Path $Zip -DestinationPath $Tmp
    $InstallScript = Get-ChildItem -Path $Tmp -Recurse -Filter install-windows.ps1 | Select-Object -First 1
    if (-not $InstallScript) {
        throw "Release archive did not contain install-windows.ps1"
    }
    Write-Host "Installing LCP..."
    & $InstallScript.FullName -InstallDir $InstallDir -SkipBuild
    return $true
}

function Get-CargoPath {
    $Cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($Cargo) {
        return $Cargo.Source
    }
    $Fallback = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $Fallback) {
        return $Fallback
    }
    return $null
}

function Install-Rustup {
    $Cargo = Get-CargoPath
    if ($Cargo) {
        return
    }

    $RustupInit = Join-Path $Tmp "rustup-init.exe"
    Write-Host "Rust toolchain not found; installing rustup with the minimal profile..."
    Invoke-Download "https://win.rustup.rs/x86_64" $RustupInit
    & $RustupInit -y --profile minimal
}

function Install-FromSource {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        throw "git is required for source install."
    }
    Install-Rustup
    Write-Host "Cloning LCP source..."
    git clone --depth 1 --branch $Branch "https://github.com/$Repo.git" (Join-Path $Tmp "lclip")
    & (Join-Path $Tmp "lclip\scripts\install-windows.ps1") -InstallDir $InstallDir
}

try {
    if (-not (Install-FromRelease)) {
        Write-Host "No matching binary release found; building LCP from source."
        Install-FromSource
    }
} finally {
    Remove-Item -LiteralPath $Tmp -Recurse -Force -ErrorAction SilentlyContinue
}
