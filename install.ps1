$ErrorActionPreference = "Stop"

$Repo = "aloglu/pester"
$BinName = "pester.exe"
$Step = 0
$TotalSteps = 5

function Test-ColorOutput {
    if ($env:NO_COLOR -or $env:PESTER_INSTALL_NO_COLOR) {
        return $false
    }

    if ([Console]::IsOutputRedirected) {
        return $false
    }

    return $true
}

$UseColor = Test-ColorOutput

function Format-Text {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Text,
        [Parameter(Mandatory = $true)]
        [string] $Code
    )

    if (-not $UseColor) {
        return $Text
    }

    return "$([char]27)[$Code`m$Text$([char]27)[0m"
}

function Write-Heading {
    Write-Host (Format-Text "pester installer" "1")
}

function Write-Detail {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    Write-Host "  $(Format-Text $Message '2')"
}

function Write-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    $script:Step += 1
    Write-Host ""
    Write-Host "$(Format-Text "[$script:Step/$TotalSteps]" '34') $(Format-Text $Message '1')"
}

function Write-Ok {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    Write-Host "  $(Format-Text 'OK' '32') $Message"
}

function Test-Windows {
    if ($env:PESTER_INSTALL_OS) {
        return $env:PESTER_INSTALL_OS -eq "Windows"
    }

    return [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
}

function Get-TargetArchitecture {
    if ($env:PESTER_INSTALL_ARCH) {
        $Arch = $env:PESTER_INSTALL_ARCH
    } else {
        $Arch = $null
        $RuntimeInformation = "System.Runtime.InteropServices.RuntimeInformation" -as [type]
        if ($RuntimeInformation) {
            try {
                $RuntimeArchProperty = $RuntimeInformation.GetProperty("OSArchitecture")
                if ($null -ne $RuntimeArchProperty) {
                    $RuntimeArch = $RuntimeArchProperty.GetValue($null, $null)
                    if ($null -ne $RuntimeArch) {
                        $Arch = $RuntimeArch.ToString()
                    }
                }
            } catch {
                $Arch = $null
            }
        }

        if ([string]::IsNullOrWhiteSpace($Arch) -and $env:PROCESSOR_ARCHITEW6432) {
            $Arch = $env:PROCESSOR_ARCHITEW6432
        }
        if ([string]::IsNullOrWhiteSpace($Arch) -and $env:PROCESSOR_ARCHITECTURE) {
            $Arch = $env:PROCESSOR_ARCHITECTURE
        }
        if ([string]::IsNullOrWhiteSpace($Arch)) {
            throw "Could not detect processor architecture."
        }
    }

    switch -Regex ($Arch) {
        "^(X64|x86_64|amd64)$" { return "x86_64" }
        "^(Arm64|AArch64|arm64|aarch64)$" { return "aarch64" }
        default { throw "Unsupported architecture: $Arch" }
    }
}

function Invoke-Download {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Uri,
        [Parameter(Mandatory = $true)]
        [string] $OutFile
    )

    $Params = @{
        Uri = $Uri
        OutFile = $OutFile
    }

    if ((Get-Command Invoke-WebRequest).Parameters.ContainsKey("UseBasicParsing")) {
        $Params.UseBasicParsing = $true
    }

    Invoke-WebRequest @Params
}

function Test-PathEntryPresent {
    param(
        [AllowNull()]
        [string] $PathValue,
        [Parameter(Mandatory = $true)]
        [string] $Directory
    )

    if ([string]::IsNullOrWhiteSpace($PathValue)) {
        return $false
    }

    $TrimChars = [char[]] "\/"
    $NormalizedDirectory = $Directory.TrimEnd($TrimChars)
    foreach ($Entry in ($PathValue -split ";")) {
        if ([string]::IsNullOrWhiteSpace($Entry)) {
            continue
        }

        $ExpandedEntry = [Environment]::ExpandEnvironmentVariables($Entry)
        $NormalizedEntry = $ExpandedEntry.TrimEnd($TrimChars)
        if ($NormalizedEntry -ieq $NormalizedDirectory) {
            return $true
        }
    }

    return $false
}

function Add-UserPathEntry {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Directory
    )

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not (Test-PathEntryPresent -PathValue $UserPath -Directory $Directory)) {
        if ([string]::IsNullOrWhiteSpace($UserPath)) {
            $NewUserPath = $Directory
        } else {
            $NewUserPath = "$UserPath;$Directory"
        }
        [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
    }

    if (-not (Test-PathEntryPresent -PathValue $env:Path -Directory $Directory)) {
        if ([string]::IsNullOrWhiteSpace($env:Path)) {
            $env:Path = $Directory
        } else {
            $env:Path = "$env:Path;$Directory"
        }
    }
}

function Stop-InstalledPester {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Executable
    )

    if (-not (Test-Path -LiteralPath $Executable)) {
        return
    }

    & schtasks /End /TN pester 2>$null | Out-Null
    & schtasks /Delete /TN pester /F 2>$null | Out-Null

    $ExecutablePath = [System.IO.Path]::GetFullPath($Executable)
    try {
        Get-CimInstance Win32_Process -Filter "Name = 'pester.exe'" |
            Where-Object {
                $_.ExecutablePath -and
                    ([string]::Equals(
                        [System.IO.Path]::GetFullPath($_.ExecutablePath),
                        $ExecutablePath,
                        [System.StringComparison]::OrdinalIgnoreCase
                    ))
            } |
            ForEach-Object {
                Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
            }
    } catch {
        Write-Warning "Could not stop existing pester processes: $_"
    }

    Start-Sleep -Milliseconds 500
}

function Copy-InstalledBinary {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Source,
        [Parameter(Mandatory = $true)]
        [string] $Destination
    )

    for ($Attempt = 1; $Attempt -le 20; $Attempt++) {
        try {
            Copy-Item -Path $Source -Destination $Destination -Force
            return
        } catch {
            if ($Attempt -eq 20) {
                throw
            }

            Start-Sleep -Milliseconds 250
        }
    }
}

function Invoke-PesterSystemInstall {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Executable,
        [int] $TimeoutSeconds = 15
    )

    $StartInfo = New-Object System.Diagnostics.ProcessStartInfo
    $StartInfo.FileName = $Executable
    $StartInfo.Arguments = "system install"
    $StartInfo.UseShellExecute = $false
    $StartInfo.CreateNoWindow = $true
    $StartInfo.RedirectStandardOutput = $true
    $StartInfo.RedirectStandardError = $true

    $Process = New-Object System.Diagnostics.Process
    $Process.StartInfo = $StartInfo

    [void] $Process.Start()
    $Completed = $Process.WaitForExit($TimeoutSeconds * 1000)
    if (-not $Completed) {
        try {
            $Process.Kill()
        } catch {
        }
        return [pscustomobject] @{
            ExitCode = $null
            TimedOut = $true
            Lines = @("pester system install timed out after $TimeoutSeconds seconds; the background service may have completed setup before the installer stopped waiting.")
        }
    }

    $Output = @()
    $Output += $Process.StandardOutput.ReadToEnd() -split "`r?`n|`r" |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    $Output += $Process.StandardError.ReadToEnd() -split "`r?`n|`r" |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }

    return [pscustomobject] @{
        ExitCode = $Process.ExitCode
        TimedOut = $false
        Lines = $Output
    }
}

if (-not (Test-Windows)) {
    throw "Use install.sh on Linux and macOS."
}

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell Core on newer runtimes may not allow changing this; its defaults are sufficient.
}

$TargetArch = Get-TargetArchitecture
$Artifact = "pester-windows-$TargetArch.zip"

if ($env:PESTER_INSTALL_DRY_RUN -eq "1") {
    Write-Output $Artifact
    return
}

if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
    throw "LOCALAPPDATA is not set."
}

$BaseUrl = "https://github.com/$Repo/releases/latest/download"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\pester"
$InstalledExe = Join-Path $InstallDir $BinName

New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    Write-Heading
    Write-Detail "Target: Windows $TargetArch"
    Write-Detail "Artifact: $Artifact"

    Write-Step "Downloading release files"
    Invoke-Download "$BaseUrl/$Artifact" (Join-Path $TempDir $Artifact)
    Invoke-Download "$BaseUrl/checksums.txt" (Join-Path $TempDir "checksums.txt")
    Write-Ok "Downloaded $Artifact"

    Write-Step "Verifying checksum"
    $ExpectedLine = Get-Content (Join-Path $TempDir "checksums.txt") |
        Where-Object {
            $Parts = $_ -split "\s+"
            $Parts.Count -ge 2 -and $Parts[-1] -eq $Artifact
        } |
        Select-Object -First 1
    if (-not $ExpectedLine) {
        throw "Checksum entry not found for $Artifact"
    }
    $Expected = ($ExpectedLine -split "\s+")[0].ToLowerInvariant()
    $Actual = (Get-FileHash (Join-Path $TempDir $Artifact) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) {
        throw "Checksum verification failed"
    }
    Write-Ok "Checksum verified"

    Write-Step "Installing binary"
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Stop-InstalledPester $InstalledExe
    Expand-Archive -Path (Join-Path $TempDir $Artifact) -DestinationPath $TempDir -Force
    Copy-InstalledBinary (Join-Path $TempDir $BinName) $InstalledExe

    Add-UserPathEntry $InstallDir
    Write-Ok "Installed to $InstalledExe"

    Write-Step "Starting background service"
    $InstallResult = Invoke-PesterSystemInstall $InstalledExe
    foreach ($Line in $InstallResult.Lines) {
        Write-Detail ($Line.ToString())
    }
    if ($InstallResult.TimedOut) {
        Write-Ok "Background service setup command stopped waiting"
    } elseif ($InstallResult.ExitCode -ne 0) {
        throw "pester system install failed with exit code $($InstallResult.ExitCode)"
    } else {
        Write-Ok "Background service installed and started"
    }

    Write-Step "Finishing setup"
    Write-Ok "pester is ready"
    Write-Host ""
    Write-Host (Format-Text "Next steps:" "1")
    Write-Detail "pester add winddown --time 22:00 --every 5m --title `"Wind down`" --message `"No exciting stuff now.`""
    Write-Detail "pester system status"
}
finally {
    Remove-Item -Path $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
