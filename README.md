# Universal Clipboard

A lightweight, cross-platform clipboard synchronization daemon written in Rust. Automatically syncs clipboard content between devices on the same local network — no cloud, no accounts, data never leaves your LAN.

> **Status**: MVP in active development. Core sync is functional; message authentication and transport encryption are in progress.

[中文文档](./docs/README.zh.md)

---

## Features

- **Zero-config discovery** — finds peers automatically via mDNS, no IP configuration needed
- **Explicit pairing** — public-key-based trust model; only syncs content from devices you have paired
- **Loop prevention** — three-layer deduplication (event ID, content hash, connection backoff) stops clipboard content from bouncing between devices
- **Cross-platform** — Linux, macOS, and Windows via the `arboard` clipboard library
- **Headless daemon** — no GUI, runs in the background

---

## Architecture

The project is a Cargo workspace with 7 crates:

```
uniclip-daemon        ← main binary
├── uniclip-core      ← data models, Blake3 content hashing, LRU dedup cache
├── uniclip-crypto    ← Ed25519 device identity management
├── uniclip-proto     ← bincode wire protocol
├── uniclip-store     ← JSON config and trust persistence
├── uniclip-discovery ← mDNS advertisement and browsing
└── uniclip-transport ← TCP peer connection lifecycle
```

### Sync Flow

```
Device A — user copies text
  ↓
[Watcher] polls clipboard every 250ms, detects change
  ↓
Creates ClipboardItem: event_id (UUID v4) + content_hash (Blake3)
  ↓
[PeerManager] broadcasts over TCP to all connected trusted peers
  ↓
Device B [Listener] receives → verify trust → dedup check → write clipboard
  ↓
Device B [Watcher] detects new content, but suppression cache hits → no re-broadcast
```

---

## Getting Started

### Build

```bash
git clone <repo>
cd Universal_Clipboard
cargo build --release
```

The binary is at `target/release/uniclip-daemon`.

### Pair Two Devices

On **Device A**, export its pairing info:

```bash
uniclip-daemon show-pairing 7878
# Prints something like:
# {"device_id":"xxxxxxxx-...","device_name":"my-laptop","pubkey_b64":"..."}
```

On **Device B**, import Device A:

```bash
uniclip-daemon pair 7879 '{"device_id":"...","device_name":"...","pubkey_b64":"..."}'
```

On **Device A**, import Device B (pairing is bidirectional):

```bash
uniclip-daemon pair 7878 '{"device_id":"...","device_name":"...","pubkey_b64":"..."}'
```

### Run the Daemon

```bash
# Device A
uniclip-daemon run 7878

# Device B
uniclip-daemon run 7879
```

Once both daemons are running, the devices discover each other via mDNS and connect automatically. Anything you copy is synced instantly.

### Skip mDNS — Specify a Peer Manually

```bash
uniclip-daemon run 7878 192.168.1.100:7879
```

---

## Configuration

Config files are stored in the platform's standard config directory:

| Platform | Path |
|----------|------|
| Linux    | `~/.config/uniclip/` |
| macOS    | `~/Library/Application Support/uniclip/` |
| Windows  | `%APPDATA%\uniclip\uniclip\` |

| File | Contents |
|------|----------|
| `config.json` | Device ID, listen port, trusted peer list |
| `identity.key` | Ed25519 private key seed (base64 — keep this safe) |

---

## Security

This is an **MVP build** with known security limitations:

| Limitation | Plan |
|------------|------|
| TCP transport is unencrypted | Noise Protocol integration planned |
| Peer identity verified by device_id only, no signature check | Ed25519 infrastructure is in place; signing is being wired in |
| Private key stored as plaintext base64 | OS keychain integration planned |

**Do not use on untrusted or public networks in the current state.**

---

## Protocol

### Service Discovery

- Service type: `_uniclip._tcp.local.`
- TXT records: `device_id`, `device_name`

### Wire Format

All messages are serialized with `bincode` inside a simple length-prefixed frame (max 2 MB):

```
[4 bytes: payload length, big-endian] [N bytes: bincode(WireMessage)]
```

Current message types:
- `Hello { version, device }` — connection handshake
- `ClipboardPush { item }` — clipboard content delivery

---

## Development

```bash
# Type-check all crates
cargo check --workspace

# Build
cargo build --workspace

# Format
cargo fmt --all

# Lint
cargo clippy --workspace
```

### Crate Dependency Graph

```
uniclip-daemon
  └─ uniclip-{core, crypto, proto, store, discovery, transport}

uniclip-transport
  └─ uniclip-{core, proto}

uniclip-store
  └─ uniclip-{core, crypto}

uniclip-proto
  └─ uniclip-core
```

---

## Roadmap

- [ ] Ed25519 message signing and verification
- [ ] Noise Protocol transport encryption
- [ ] Atomic config file writes
- [ ] OS keychain storage for private key
- [ ] Image and binary clipboard content support
- [ ] REST API (`uniclip-api` crate)
- [ ] Unit and integration tests

---

## License

TBD.
