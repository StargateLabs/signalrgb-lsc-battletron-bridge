# Login QR, sincronizzazione e avvio del bridge con il plugin SignalRGB.
param([switch]$NoBrowser)
$ErrorActionPreference = 'Stop'
$exe = Join-Path $PSScriptRoot 'bridge\target\release\lscbridge.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw 'Compilare prima il bridge in release.' }
$configPath = Join-Path $env:APPDATA 'LscBridge\config.json'
$savedSession = $null
if (Test-Path -LiteralPath $configPath) {
    $savedSession = (Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json).session
}
if (-not ($savedSession.sid -and $savedSession.ecode)) {
    Write-Host 'Scansiona il QR con LSC Smart Connect sul telefono.'
    $loginArgs = @('login')
    if (-not $NoBrowser) { $loginArgs += '--open' }
    & $exe @loginArgs
    if ($LASTEXITCODE -ne 0) { throw 'Login non completato. Rilancia connect.ps1 per un nuovo QR.' }
}
& $exe devices
if ($LASTEXITCODE -ne 0) { throw 'Sincronizzazione dispositivi fallita.' }
& (Join-Path $PSScriptRoot 'install.ps1') -NoFirewall
if ($LASTEXITCODE -ne 0) { throw 'Installazione non completata.' }
Write-Host 'Avvia o riavvia SignalRGB. Le luci sono disponibili come controller LSC.'
if (Get-Process -Name lscbridge -ErrorAction SilentlyContinue | Where-Object { $_.Path -in @($exe, (Join-Path $env:LOCALAPPDATA 'LscBridge\lscbridge.exe')) }) {
    Write-Host 'Bridge già attivo; configurazione ricaricata dopo la sincronizzazione.'
    exit 0
}
& $exe run
