# Installation — Stargate Labs

[Italiano](INSTALL-IT.md) · [README](../README.en.md)

## Requirements

Windows, SignalRGB, an LSC Smart Connect account and lights already paired in the phone app. PC and lights must share a reachable LAN without client isolation. The bridge does not replace Wi-Fi onboarding.

## Install the release

1. Download and extract `lsc-battletron-v1.0.0-windows-x64.zip`.
2. Stop an existing bridge before replacing the executable. For a supervised update, stop its PowerShell supervisor first, then `lscbridge.exe`.
3. Open PowerShell in the extracted directory and install files, plugin and startup entry:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

Dedicated firewall rule creation requires an Administrator PowerShell. Without elevation, use `-NoFirewall` only if LAN rules already exist or are separately authorized. The control server listens on loopback only.

4. Sign in using **LSC Smart Connect on your phone**, not the generic camera:

```powershell
& "$env:LOCALAPPDATA\LscBridge\lscbridge.exe" login --open
& "$env:LOCALAPPDATA\LscBridge\lscbridge.exe" devices
```

Scan the QR displayed in the opened page and confirm the login in the app. Generate a new QR if it expires.

5. Start the supervisor; later Windows sign-ins use the startup entry:

```powershell
powershell -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "$env:LOCALAPPDATA\LscBridge\watch-bridge.ps1"
```

6. Restart SignalRGB, open the LSC Battletron service and refresh devices. Authenticated lights are announced as controllers. Place the four zones in the layout. The panel provides refresh and global/per-device reconnect.

## Troubleshooting and data protection

- No lights: check the bridge process, LAN reachability and UDP 40001/6667 rules. Request rediscovery with `lscbridge reconnect`.
- New lights: pair them in LSC first, then rerun `lscbridge devices`.
- Color/timing: compare a known static color before spatial effects. Higher update rates do not repair a slow network.
- Logs: `%APPDATA%\LscBridgeridge.log`; supervisor: `%LOCALAPPDATA%\LscBridge\supervisor.log`.
- Configuration and cache contain sessions and keys: never publish them. A valid `config.json.backup` can recover a damaged primary; the corrupt file is preserved separately.

## Build and test

Install Rust and Visual Studio C++ Build Tools for Windows, plus Node.js. From the repository root:

```powershell
cargo test --manifest-path bridge/Cargo.toml
cargo build --manifest-path bridge/Cargo.toml --release
node scripts/build-plugin.mjs
node plugin/test/frame.test.mjs
node tests/bridge-smoke.mjs
```

## Uninstall

Stop the supervisor and bridge. Remove `LSCBattletronBridge.lnk` from Windows Startup and `BattletronBridge.js/.qml` from the plugin folder. Retain `%APPDATA%\LscBridge` for rollback. Remove dedicated firewall rules separately if desired. Leave unrelated SignalRGB plugins unchanged.
