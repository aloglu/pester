param(
    [Parameter(Mandatory = $true)]
    [string] $Exe,
    [int] $TimeoutSeconds = 10
)

$ErrorActionPreference = "Stop"
$ResolvedExe = [System.IO.Path]::GetFullPath($Exe)

function Invoke-PesterCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    $Process = [System.Diagnostics.Process]::new()
    $Process.StartInfo.FileName = $ResolvedExe
    foreach ($Argument in $Arguments) {
        $Process.StartInfo.ArgumentList.Add($Argument)
    }
    $Process.StartInfo.UseShellExecute = $false
    $Process.StartInfo.RedirectStandardOutput = $false
    $Process.StartInfo.RedirectStandardError = $false
    $Process.StartInfo.CreateNoWindow = $true

    try {
        if (-not $Process.Start()) {
            throw "Could not start pester $($Arguments -join ' ')"
        }
        if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
            $Process.Kill()
            throw "Timed out running pester $($Arguments -join ' ')"
        }

        if ($Process.ExitCode -ne 0) {
            throw "pester $($Arguments -join ' ') failed with exit code $($Process.ExitCode)"
        }
    } finally {
        $Process.Dispose()
    }
}

Invoke-PesterCommand -Arguments @("system", "install")
try {
    Invoke-PesterCommand -Arguments @("system", "status")
} finally {
    Invoke-PesterCommand -Arguments @("system", "uninstall", "--yes")
}
