# Note tecniche / Reverse engineering

[English](TECHNICAL-EN.md)

Protocollo locale u32 big-endian + JSON, controllo TCP/UDP su 127.0.0.1:21444. Discovery/negoziazione UDP 40001 e beacon UDP 6667. Stream LAN autenticato AES-GCM e payload DP61/DP24; le Ball semplici usano DP24 a massimo 10 Hz. Frame recenti, deduplicazione e scarto dei colori in coda da oltre 100 ms; ciò non misura la latenza ottica.

Salvataggio atomico con `sync_all`, backup e recupero del JSON corrotto. Supervisore a istanza singola con attesa 4–60 secondi e log ruotato. Non rileva hang di un processo vivo. Lo spegnimento DP20 cloud, quando richiesto, è separato dallo stream e dipende dalla disponibilità cloud.

Test: `cargo test --manifest-path bridge/Cargo.toml`, `node plugin/test/frame.test.mjs`, `node tests/bridge-smoke.mjs`. Fixture sintetiche; nessuna DLL o app originale redistribuita.
