<p align="center"><img src="assets/hero.svg" alt="Stargate Labs — LSC Smart Connect / Battletron" width="100%"></p>

<p align="center"><b>STARGATE LABS · SIGNALRGB INTEGRATIONS</b><br>[Italiano](README.md) · [English](README.en.md) · [Installazione IT](docs/INSTALL-IT.md) · [Installation EN](docs/INSTALL-EN.md)</p>

<p align="center"><img src="assets/brand/signalrgb-logo.svg" alt="signalrgb" height="30"></p>

<p align="center"><a href="https://stargatelabs.github.io/signalrgb-lsc-battletron-bridge/"><b>Presentazione del progetto ↗</b></a> &nbsp; · &nbsp; <a href="https://github.com/StargateLabs/signalrgb-lsc-battletron-bridge/releases"><b>Download</b></a></p>

# LSC Smart Connect / Battletron

Un bridge locale per portare le luci Wi-Fi LSC Smart Connect e Battletron sul canvas di SignalRGB. Il protocollo LAN dell’app desktop **LSC Battletron 2.0.17** è stato ricostruito tramite reverse engineering e verificato con fixture indipendenti. Login QR dall’app LSC, streaming colori sulla LAN, gestione dei dispositivi in SignalRGB.

| Bridge | SignalRGB | Streaming |
|---|---|---|
| Rust · Windows | Standalone JS + QML controls | Up to 30 Hz · Ball DP24 capped at 10 Hz |

## Installazione

Scarica lo ZIP da [Releases](https://github.com/StargateLabs/signalrgb-lsc-battletron-bridge/releases), estrailo e segui la [guida in italiano](docs/INSTALL-IT.md). Le istruzioni spiegano anche come tornare alla versione precedente.

## Architettura e verifiche

**61 test Rust + 9 test di parità + 82 controlli plugin**, più E2E su eseguibile release. Ultimo collaudo locale: otto luci online, ciascuna con frame variabili in 40 campioni su 20 secondi. Backup configurazione con sincronizzazione su disco e recupero verificato; riavvio automatico collaudato in circa quattro secondi. Nessuna promessa di latenza ottica universale.

## Limiti dichiarati

Compatibilità verificata sul gruppo del laboratorio, non su tutto il catalogo Tuya/LSC. Le credenziali cloud servono a login e chiavi; il rilascio può richiedere OFF cloud separato. Il supervisore recupera un processo terminato, non un processo vivo bloccato. Le LotusLamp Bluetooth non fanno parte di questo rilascio.

## Documentazione e contributi

- [Technical notes / Note tecniche](docs/TECHNICAL-IT.md)
- [Changelog](CHANGELOG.md)
- [Third-party notices](THIRD_PARTY_NOTICES.md)
- [MIT license](LICENSE)
- [Stargate Labs](https://github.com/StargateLabs) · [Lab portfolio](https://stargatelabs.github.io/StargateLabs/)

Progetto indipendente, non affiliato a Razer, SignalRGB, Tuya o LSC. I marchi appartengono ai rispettivi titolari.

<sub>Observe. Decode. Build. Verify. — Created by Stargate Labs.</sub>
