$ErrorActionPreference = "Stop"

$Repo = "aloglu/pester"
$BinName = "pester.exe"

if ($IsMacOS -or $IsLinux) {
    throw "Use install.sh on Linux and macOS."
}

$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($Arch) {
    "X64" { $TargetArch = "x86_64" }
    "Arm64" { $TargetArch = "aarch64" }
    default { throw "Unsupported architecture: $Arch" }
}

$Artifact = "pester-windows-$TargetArch.zip"
$BaseUrl = "https://github.com/$Repo/releases/latest/download"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\Pester"

New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    Write-Host "Downloading $Artifact..."
    Invoke-WebRequest "$BaseUrl/$Artifact" -OutFile (Join-Path $TempDir $Artifact)
    Invoke-WebRequest "$BaseUrl/checksums.txt" -OutFile (Join-Path $TempDir "checksums.txt")

    $ExpectedLine = Get-Content (Join-Path $TempDir "checksums.txt") | Where-Object { $_ -match "  $([regex]::Escape($Artifact))$" } | Select-Object -First 1
    if (-not $ExpectedLine) {
        throw "Checksum entry not found for $Artifact"
    }
    $Expected = ($ExpectedLine -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash (Join-Path $TempDir $Artifact) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) {
        throw "Checksum verification failed"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Expand-Archive -Path (Join-Path $TempDir $Artifact) -DestinationPath $TempDir -Force
    Copy-Item -Path (Join-Path $TempDir $BinName) -Destination (Join-Path $InstallDir $BinName) -Force

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (($UserPath -split ";") -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
        $env:Path = "$env:Path;$InstallDir"
    }

    & (Join-Path $InstallDir $BinName) install

    Write-Host "Pester installed to $(Join-Path $InstallDir $BinName)"
    Write-Host ""
    Write-Host "Try:"
    Write-Host "  pester add winddown --time 22:00 --every 5m --title `"Wind down`" --message `"No exciting stuff now.`""
    Write-Host "  pester status"
}
finally {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
