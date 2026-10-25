# Installazione — Stargate Labs

[English](INSTALL-EN.md) · [README](../README.md)

## Requisiti

Windows, SignalRGB, account LSC Smart Connect e lampade già abbinate nell'app telefono. PC e lampade sulla stessa LAN, senza isolamento tra client. Il bridge non sostituisce l'abbinamento Wi-Fi delle lampade.

## Installazione dalla release

1. Scarica ed estrai `lsc-battletron-v1.0.0-windows-x64.zip`.
2. Se un vecchio bridge è attivo, chiudilo prima di sostituire l'eseguibile. Per un aggiornamento gestito, chiudi prima il processo PowerShell del supervisore e poi `lscbridge.exe`.
3. Apri PowerShell nella cartella estratta. Per installare file, plugin e avvio automatico:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

Per creare le regole firewall dedicate serve PowerShell come amministratore. Senza privilegi puoi usare `-NoFirewall`, ma le regole LAN devono essere già presenti o autorizzate separatamente. Il server di controllo resta su loopback, non esposto in LAN.

4. Accedi usando **LSC Smart Connect sul telefono**, non la fotocamera generica:

```powershell
& "$env:LOCALAPPDATA\LscBridge\lscbridge.exe" login --open
& "$env:LOCALAPPDATA\LscBridge\lscbridge.exe" devices
```

Scansiona il QR nella pagina aperta e conferma il login nell'app. Il QR è temporaneo: rigeneralo se scade.

5. Avvia il supervisore; ai login successivi partirà dall'avvio automatico:

```powershell
powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\LscBridge\watch-bridge.ps1"
```

6. Riavvia SignalRGB, apri il servizio LSC Battletron e aggiorna i dispositivi. Ogni luce autenticata viene annunciata come controller. Posiziona le quattro zone nel layout. Il pannello permette refresh e riconnessione globale/mirata.

## Diagnostica e protezione dei dati

- Nessuna luce: controlla che `lscbridge.exe` sia attivo, la LAN e le regole UDP 40001/6667. Per la riscoperta: `lscbridge reconnect`.
- Nuove lampade: prima abbinale in LSC, poi esegui di nuovo `lscbridge devices`.
- Colori/tempi: prova un colore statico noto prima di confrontare effetti spaziali. Non aumentare la frequenza per correggere una rete lenta.
- Log: `%APPDATA%\LscBridgeridge.log`; supervisore: `%LOCALAPPDATA%\LscBridge\supervisor.log`.
- Configurazione e cache contengono sessioni e chiavi: non pubblicarle. Backup valido recuperabile da `config.json.backup`; originale corrotto conservato separatamente.

## Compilazione e test

Installa Rust e Visual Studio Build Tools C++ per Windows, più Node.js. Dalla radice:

```powershell
cargo test --manifest-path bridge/Cargo.toml
cargo build --manifest-path bridge/Cargo.toml --release
node scripts/build-plugin.mjs
node plugin/test/frame.test.mjs
node tests/bridge-smoke.mjs
```

## Rimozione

Chiudi il supervisore e il bridge; rimuovi il collegamento `LSCBattletronBridge.lnk` dall'Avvio automatico e i due file `BattletronBridge.js/.qml` dalla cartella plugin. Conserva `%APPDATA%\LscBridge` per un ripristino. Le regole firewall dedicate possono essere rimosse separatamente. Non modificare gli altri plugin SignalRGB.
