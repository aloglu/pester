$ErrorActionPreference = "Stop"

$Repo = "aloglu/pester"
$LastWindowsVersion = "v0.1.8"
$BinName = "pester.exe"
$DaemonBinName = "pesterd.exe"
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

function Initialize-PesterNativeMethods {
    if ("PesterNativeMethods" -as [type]) {
        return
    }

    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class PesterNativeMethods
{
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr OpenEvent(UInt32 dwDesiredAccess, bool bInheritHandle, string lpName);

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    public static extern IntPtr OpenMutex(UInt32 dwDesiredAccess, bool bInheritHandle, string lpName);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool SetEvent(IntPtr hEvent);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern UInt32 WaitForSingleObject(IntPtr hHandle, UInt32 dwMilliseconds);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool ReleaseMutex(IntPtr hMutex);

    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool CloseHandle(IntPtr hObject);
}
"@
}

function Stop-PesterDaemonGracefully {
    try {
        Initialize-PesterNativeMethods

        $EventModifyState = [uint32] 0x0002
        $MutexModifyState = [uint32] 0x0001
        $SynchronizationSynchronize = [uint32] 0x00100000
        $WaitObject0 = [uint32] 0x00000000
        $WaitAbandoned = [uint32] 0x00000080
        $WaitTimeout = [uint32] 0x00000102

        $Event = [PesterNativeMethods]::OpenEvent($EventModifyState, $false, "Local\pester-daemon-stop")
        if ($Event -eq [IntPtr]::Zero) {
            return $false
        }

        try {
            if (-not [PesterNativeMethods]::SetEvent($Event)) {
                return $false
            }
        } finally {
            [void] [PesterNativeMethods]::CloseHandle($Event)
        }

        $MutexAccess = [uint32] ($SynchronizationSynchronize -bor $MutexModifyState)
        $Mutex = [PesterNativeMethods]::OpenMutex($MutexAccess, $false, "Local\pester-daemon")
        if ($Mutex -eq [IntPtr]::Zero) {
            return $true
        }

        try {
            $WaitResult = [PesterNativeMethods]::WaitForSingleObject($Mutex, 3000)
            if ($WaitResult -eq $WaitObject0 -or $WaitResult -eq $WaitAbandoned) {
                [void] [PesterNativeMethods]::ReleaseMutex($Mutex)
                return $true
            }
            if ($WaitResult -eq $WaitTimeout) {
                return $false
            }
            return $false
        } finally {
            [void] [PesterNativeMethods]::CloseHandle($Mutex)
        }
    } catch {
        return $false
    }
}

function Stop-InstalledPester {
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $Executables
    )

    $Targets = @()
    foreach ($Executable in $Executables) {
        if (Test-Path -LiteralPath $Executable) {
            $Targets += [System.IO.Path]::GetFullPath($Executable)
        }
    }

    if ($Targets.Count -eq 0) {
        return
    }

    [void] (Stop-PesterDaemonGracefully)

    $ProcessNames = $Targets |
        ForEach-Object { [System.IO.Path]::GetFileNameWithoutExtension($_) } |
        Sort-Object -Unique

    foreach ($ProcessName in $ProcessNames) {
        foreach ($Process in [System.Diagnostics.Process]::GetProcessesByName($ProcessName)) {
            try {
                $ProcessPath = $null
                try {
                    $ProcessPath = $Process.MainModule.FileName
                } catch {
                    continue
                }

                if ([string]::IsNullOrWhiteSpace($ProcessPath)) {
                    continue
                }

                $ProcessPath = [System.IO.Path]::GetFullPath($ProcessPath)
                $MatchesTarget = $false
                foreach ($Target in $Targets) {
                    if ([string]::Equals(
                        $ProcessPath,
                        $Target,
                        [System.StringComparison]::OrdinalIgnoreCase
                    )) {
                        $MatchesTarget = $true
                        break
                    }
                }

                if (-not $MatchesTarget) {
                    continue
                }

                if (-not $Process.HasExited) {
                    $Process.Kill()
                }
                if (-not $Process.WaitForExit(3000)) {
                    throw "Timed out waiting for $ProcessPath to exit."
                }
            } finally {
                $Process.Dispose()
            }
        }
    }
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

$BaseUrl = "https://github.com/$Repo/releases/download/$LastWindowsVersion"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\pester"
$InstalledExe = Join-Path $InstallDir $BinName
$InstalledDaemonExe = Join-Path $InstallDir $DaemonBinName

New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    Write-Heading
    Write-Detail "Target: Windows $TargetArch"
    Write-Detail "Windows support ended with $LastWindowsVersion; installing the last supported Windows release."
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
    Expand-Archive -Path (Join-Path $TempDir $Artifact) -DestinationPath $TempDir -Force
    $ExtractedExe = Join-Path $TempDir $BinName
    $ExtractedDaemonExe = Join-Path $TempDir $DaemonBinName
    if (-not (Test-Path -LiteralPath $ExtractedExe)) {
        throw "Release artifact is missing $BinName"
    }
    if (-not (Test-Path -LiteralPath $ExtractedDaemonExe)) {
        throw "Release artifact is missing $DaemonBinName"
    }

    Stop-InstalledPester @($InstalledExe, $InstalledDaemonExe)
    Copy-InstalledBinary $ExtractedExe $InstalledExe
    Copy-InstalledBinary $ExtractedDaemonExe $InstalledDaemonExe

    Add-UserPathEntry $InstallDir
    Write-Ok "Installed to $InstallDir"

    Write-Step "Starting background service"
    & $InstalledExe system install
    if ($LASTEXITCODE -ne 0) {
        throw "pester system install failed with exit code $LASTEXITCODE"
    }
    Write-Ok "Background service installed and started"

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
