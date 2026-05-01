#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$Repo   = 'dpkay-io/gitreg'
$Target = 'x86_64-pc-windows-msvc'
$Url    = "https://github.com/$Repo/releases/download/latest/gitreg-latest-$Target.zip"

if ($env:LOCALAPPDATA) {
    $InstallDir = Join-Path $env:LOCALAPPDATA 'Programs\gitreg'
} else {
    $InstallDir = Join-Path $env:USERPROFILE '.local\bin'
}

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.IO.Path]::GetRandomFileName())

try {
    New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null
    $Archive = Join-Path $TmpDir 'gitreg.zip'

    Write-Host "Downloading gitreg for $Target ..."
    Invoke-WebRequest -Uri $Url -OutFile $Archive -UseBasicParsing

    $ExtractDir = Join-Path $TmpDir 'extracted'
    Expand-Archive -Path $Archive -DestinationPath $ExtractDir -Force

    $ExeSrc = Get-ChildItem -Path $ExtractDir -Filter 'gitreg.exe' -Recurse |
              Select-Object -First 1
    if (-not $ExeSrc) { throw "gitreg.exe not found in downloaded archive" }

    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }
    $ExeDest = Join-Path $InstallDir 'gitreg.exe'
    Copy-Item -Path $ExeSrc.FullName -Destination $ExeDest -Force
    Write-Host "Installed gitreg.exe to $InstallDir"

    $UserPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
    $UserPathParts = $UserPath -split ';' | Where-Object { $_ -ne '' }
    if ($InstallDir -notin $UserPathParts) {
        $NewUserPath = ($UserPathParts + $InstallDir) -join ';'
        [Environment]::SetEnvironmentVariable('PATH', $NewUserPath, 'User')
        Write-Host "Added $InstallDir to your user PATH (takes effect in new sessions)."
    }
    if ($env:PATH -notlike "*$InstallDir*") {
        $env:PATH = "$InstallDir;$env:PATH"
    }

    & "$ExeDest" init
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "Note: Shell auto-tracking (the git() shim) requires Git Bash or WSL."
        Write-Host "      gitreg is installed and ready — run 'gitreg scan ~' to populate the registry."
    }
} finally {
    if (Test-Path $TmpDir) {
        Remove-Item -Path $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ""
Write-Host "Done. Open a new terminal so the updated PATH takes effect."
