# iptools

English · [简体中文](README.md)

A cross-platform terminal application for network management and diagnostics. Inspect adapters, configure IP settings, discover LAN devices, monitor traffic, and run common diagnostics from one interface.

[![CI](https://github.com/newcovid/iptools/actions/workflows/ci.yml/badge.svg)](https://github.com/newcovid/iptools/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/newcovid/iptools)](https://github.com/newcovid/iptools/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-2563eb)](#platform-support)
[![License](https://img.shields.io/github/license/newcovid/iptools)](LICENSE)

[Live demo](https://newcovid.github.io/iptools/) · [Download](https://github.com/newcovid/iptools/releases/latest) · [Report an issue](https://github.com/newcovid/iptools/issues)

![Interactive iptools WebAssembly demo](docs/assets/web-demo.png)

> The Web demo uses deterministic simulated data. It never reads your network, scans your LAN, or changes system configuration. Native and Web builds share the same UI, state machine, and interaction logic, and the demo works offline after its first load.

## Features

| Page | Capabilities |
|---|---|
| Dashboard | Host, active adapter, local addressing, DHCP, proxy, live/total traffic, and public connection data |
| Adapters | Physical/virtual adapters, IPv4/IPv6, MAC, SSID, link details, DHCP, and static IPv4 configuration |
| Scanner | CIDR-based ARP discovery with IP, MAC, vendor, and hostname results |
| Traffic | Per-interface rates, session totals, and totals since boot |
| Diagnostics | Ping, traceroute, port scan, public speed, link quality, and TCP/UDP LAN throughput |
| Settings | Language, scan concurrency, preset color themes, and remembered-parameter reset |

Highlights:

- Full keyboard and mouse support, including `Ctrl+R` history, inline completion, and clickable history entries;
- Chinese and English UI with Classic, Nord, Catppuccin Mocha, and Dracula themes;
- Single-file native releases with no additional runtime;
- Atomic configuration writes and automatic persistence of parameters, history, and UI position;
- Native Windows and Linux network backends with cancellable, supervised background work;
- DOM/Canvas WebAssembly demo with touch controls, offline PWA support, and same-origin assets only.

## Installation

Download the archive for your platform from [Releases](https://github.com/newcovid/iptools/releases/latest):

- Windows: run `iptools-*-windows-x86_64.exe`;
- Linux: extract `iptools-*-linux-x86_64.tar.gz`, then run `./iptools`.

Some operations require elevated permissions:

- Windows requires Administrator privileges to change IP configuration;
- Linux ARP and ICMP diagnostics require root or `CAP_NET_RAW`; the bundled `install.sh` can grant the minimum capability;
- Linux wireless details require `iw`, while network writes depend on PolicyKit, `sudo`, and an available `nmcli`, `netplan`, or `ip` backend.

### Build from source

Rust 1.97.0 is required and pinned by the repository. HTTP uses rustls, so OpenSSL development packages are not needed.

```bash
git clone https://github.com/newcovid/iptools.git
cd iptools
cargo build --release
```

The binary is written to `target/release/iptools` (`iptools.exe` on Windows).

## Usage

```text
iptools
iptools --config /path/to/config.json
iptools --demo
iptools --demo --scenario wifi-degraded
iptools --version
```

The default configuration file is `config.json` in the current directory. See [`config.example.json`](config.example.json) for all fields. The application-managed `session` section stores recent inputs and UI position and normally does not need manual editing.

### Default shortcuts

| Action | Key |
|---|---|
| Next / previous page | `Tab` / `Shift+Tab` |
| Navigate | Arrow keys or `W` `A` `S` `D` |
| Confirm / back | `Enter` / `Esc` |
| Edit | `E` |
| Start / stop | `Space` |
| Refresh | `R` |
| Input history | `Ctrl+R` |
| Toggle language | `Ctrl+L` |
| Help | `F1` |
| Quit | `Ctrl+C` / `Ctrl+Q` |

The footer shows the current context and effective bindings and is clickable. Native bindings can be remapped in `config.json`.

## Platform support

| Feature | Windows | Linux | Other Unix |
|---|:---:|:---:|:---:|
| Port scan and public/LAN speed | ✓ | ✓ | ✓ |
| Adapter enumeration and ARP scan | ✓ | ✓ `CAP_NET_RAW` | — |
| Ping, traceroute, and link quality | ✓ | ✓ `CAP_NET_RAW` | Limited |
| Wireless details | WLAN API | `iw` | — |
| IP configuration | WMI | `nmcli` / `netplan` / `ip` | — |

ARP discovery is limited to reachable devices on the same layer-2 network. Applying network settings may briefly interrupt connectivity; verify the adapter and values before confirming.

## Web demo

The [WebAssembly demo](https://newcovid.github.io/iptools/) includes `home-network`, `wifi-degraded`, and `multi-adapter`. It loads same-origin static assets only, has no telemetry, and turns adapter edits into simulated outcomes.

```text
?scenario=wifi-degraded&lang=en&renderer=canvas
```

Supported parameters are `scenario`, `lang`, and `renderer=dom|canvas`. URL parameters take precedence over browser-local settings.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
cargo check -p iptools-web --target wasm32-unknown-unknown
```

Run the Web demo locally with `trunk serve` from `crates/iptools-web`.

Documentation:

- [Architecture](docs/architecture.md)
- [Link quality guide (Chinese)](docs/link-quality-guide.md)
- [Changelog (Chinese)](CHANGELOG.md)
- [Contributing](CONTRIBUTING.md)
- [Security policy](SECURITY.md)

## License

[MIT](LICENSE) © newcovid
