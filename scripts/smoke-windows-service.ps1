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

    $ArgumentPayload = [string]::Join("`n", $Arguments)

    $Job = Start-Job -ScriptBlock {
        param(
            [string] $Exe,
            [string] $ArgumentPayload
        )

        $ErrorActionPreference = "Stop"
        if ([string]::IsNullOrEmpty($ArgumentPayload)) {
            $Arguments = @()
        } else {
            $Arguments = $ArgumentPayload -split "`n"
        }

        $Output = & $Exe @Arguments 2>&1
        if ($LASTEXITCODE -ne 0) {
            foreach ($Line in $Output) {
                Write-Output $Line
            }
            throw "pester $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
        }
        foreach ($Line in $Output) {
            Write-Output $Line
        }
    } -ArgumentList $ResolvedExe, $ArgumentPayload

    try {
        if (-not (Wait-Job -Job $Job -Timeout $TimeoutSeconds)) {
            Stop-Job -Job $Job
            throw "Timed out running pester $($Arguments -join ' ')"
        }
        Receive-Job -Job $Job
    } finally {
        Remove-Job -Job $Job -Force
    }
}

Invoke-PesterCommand -Arguments @("system", "install")
try {
    Invoke-PesterCommand -Arguments @("system", "status")
} finally {
    Invoke-PesterCommand -Arguments @("system", "uninstall")
}
