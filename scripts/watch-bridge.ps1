param(
    [string]$BridgeDirectory = (Join-Path $env:LOCALAPPDATA 'LscBridge'),
    [int]$PollSeconds = 5
)
$ErrorActionPreference = 'Stop'
$mutex = New-Object System.Threading.Mutex($false, 'Local\StargateLscBridgeSupervisor')
if (-not $mutex.WaitOne(0)) { $mutex.Dispose(); exit }
try {
    $exe = Join-Path $BridgeDirectory 'lscbridge.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw "Bridge assente: $exe" }
    $log = Join-Path $BridgeDirectory 'supervisor.log'
    $failures = 0
    while ($true) {
        $running = Get-Process lscbridge -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $exe }
        if ($running) { Start-Sleep -Seconds $PollSeconds; continue }
        if ((Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 1MB) {
            Move-Item -LiteralPath $log -Destination "$log.previous" -Force
        }
        try {
            Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) avvio bridge"
            $child = Start-Process -FilePath $exe -ArgumentList 'run' -WorkingDirectory $BridgeDirectory -WindowStyle Hidden -PassThru
            $started = Get-Date
            $child.WaitForExit()
            Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) uscita bridge: $($child.ExitCode)"
            if (((Get-Date) - $started).TotalSeconds -ge 60) { $failures = 0 }
        } catch {
            Add-Content -LiteralPath $log -Value "$(Get-Date -Format o) errore avvio: $($_.Exception.Message)"
        }
        $failures = [Math]::Min($failures + 1, 6)
        Start-Sleep -Seconds ([Math]::Min(60, 2 * [Math]::Pow(2, $failures)))
    }
} finally {
    $mutex.ReleaseMutex()
    $mutex.Dispose()
}
