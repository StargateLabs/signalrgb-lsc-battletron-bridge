# install.ps1 - installa lscbridge (bridge LSC Battletron -> SignalRGB)
# Uso: PowerShell (consigliato come Amministratore per le regole firewall)
param(
    [switch]$NoFirewall
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
$exeSrc = Join-Path $root "bridge\target\release\lscbridge.exe"
$pluginSrc = Join-Path $root "plugin\dist\BattletronBridge.js"
if (-not (Test-Path -LiteralPath $pluginSrc)) {
    & node (Join-Path $root "scripts\build-plugin.mjs")
    if ($LASTEXITCODE -ne 0) { throw "Build del plugin SignalRGB fallita" }
}

if (-not (Test-Path $exeSrc)) {
    Write-Error "Eseguibile non trovato: $exeSrc - compilare con 'cargo build --release' in bridge\"
    exit 1
}

$dest = Join-Path $env:LOCALAPPDATA "LscBridge"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$exeDest = Join-Path $dest "lscbridge.exe"
if (-not (Test-Path -LiteralPath $exeDest) -or (Get-FileHash -LiteralPath $exeSrc).Hash -ne (Get-FileHash -LiteralPath $exeDest).Hash) {
    Copy-Item $exeSrc $exeDest -Force
}
Write-Host "Bridge installato in $dest"

# plugin SignalRGB (Documents persistono tra versioni e fanno override)
if (Test-Path $pluginSrc) {
    $plugDir = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "WhirlwindFX\Plugins"
    New-Item -ItemType Directory -Force -Path $plugDir | Out-Null
    Copy-Item $pluginSrc (Join-Path $plugDir "BattletronBridge.js") -Force
    (Get-Item -LiteralPath (Join-Path $plugDir "BattletronBridge.js")).LastWriteTime = Get-Date
    Copy-Item (Join-Path $root "plugin\BattletronBridge.qml") (Join-Path $plugDir "BattletronBridge.qml") -Force
    $keyLightPlugin = Join-Path $root "plugin\Razer_Key_Light_Chroma.js"
    if (Test-Path -LiteralPath $keyLightPlugin) {
        Copy-Item -LiteralPath $keyLightPlugin -Destination (Join-Path $plugDir "Razer_Key_Light_Chroma.js") -Force
        (Get-Item -LiteralPath (Join-Path $plugDir "Razer_Key_Light_Chroma.js")).LastWriteTime = Get-Date
        Copy-Item -LiteralPath (Join-Path $root "plugin\Razer_Key_Light_Chroma.qml") -Destination (Join-Path $plugDir "Razer_Key_Light_Chroma.qml") -Force
    }
    Write-Host "Plugin installato in $plugDir"
    # Il plugin installato è standalone: nessun helper JS nella cartella Plugins.
}

# regole firewall: UDP discovery + TCP locale del bridge
if (-not $NoFirewall) {
    try {
        foreach ($port in @(40001, 6667)) {
            $ruleName = "LSC Bridge UDP $port"
            Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction Stop
            New-NetFirewallRule -DisplayName $ruleName -Direction Inbound -Protocol UDP -LocalPort $port -Program (Join-Path $dest "lscbridge.exe") -RemoteAddress LocalSubnet -Action Allow -ErrorAction Stop | Out-Null
        }
        # Il control server ascolta solo su loopback: nessuna apertura TCP sulla LAN.
        Get-NetFirewallRule -DisplayName "LSC Bridge TCP 21444" -ErrorAction SilentlyContinue | Remove-NetFirewallRule -ErrorAction Stop
        Write-Host "Regole firewall create"
    } catch {
        Write-Warning "Firewall non configurato (servono permessi admin): $($_.Exception.Message)"
    }
}

# avvio automatico al login
Copy-Item -LiteralPath (Join-Path $root 'scripts\watch-bridge.ps1') -Destination (Join-Path $dest 'watch-bridge.ps1') -Force
$startup = [Environment]::GetFolderPath("Startup")
$lnk = Join-Path $startup "LSCBattletronBridge.lnk"
$sh = New-Object -ComObject WScript.Shell
$sc = $sh.CreateShortcut($lnk)
$sc.TargetPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$sc.Arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + (Join-Path $dest 'watch-bridge.ps1') + '"'
$sc.WorkingDirectory = $dest
$sc.Description = "LSC Battletron bridge per SignalRGB"
$sc.WindowStyle = 7
$sc.Save()
Write-Host "Avvio automatico creato: $lnk"

Write-Host ""
Write-Host "Fatto. Prossimi passi:"
Write-Host "  1. lscbridge login     (QR con app LSC Smart Connect)"
Write-Host "  2. lscbridge devices   (sincronizza i dispositivi)"
Write-Host "  3. Avvia SignalRGB e abilita lo streaming"
exit 0
