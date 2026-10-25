# Technical notes / Reverse engineering

[Italiano](TECHNICAL-IT.md)

Local protocol: big-endian u32 length plus JSON, TCP/UDP control on 127.0.0.1:21444. UDP 40001 discovery/negotiation and UDP 6667 beacons. Authenticated AES-GCM LAN stream and DP61/DP24 payloads; simple Ball devices use DP24 capped at 10 Hz. Latest colors, deduplication and dropping color packets queued for more than 100 ms do not establish optical latency.

Atomic replacement with `sync_all`, backup and damaged-JSON recovery. Single-instance supervisor with 4–60 second retry delay and rotated log; it does not detect live process hangs. Cloud DP20 shutdown, when requested, is separate from streaming and depends on cloud availability.

Tests: `cargo test --manifest-path bridge/Cargo.toml`, `node plugin/test/frame.test.mjs`, `node tests/bridge-smoke.mjs`. Synthetic fixtures; no original DLL or app redistribution.
