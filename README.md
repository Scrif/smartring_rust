# SmartRing - Rust

A self-contained CLI for discovering, connecting to, and reading data from Colmi-family smart rings (R02, R06, R10, and compatible OEM variants) over Bluetooth LE.

The only other mature client for these rings is [colmi_r02_client](https://github.com/tahnok/colmi_r02_client) — a Python library that requires a runtime and isn't easily redistributable. 

Initial featureset in this project draws heavily from [colmi_r02_client](https://github.com/tahnok/colmi_r02_client), but is built entirely in Rust and compiles to a single binary with no runtime dependencies beyond system Bluetooth.

---

## Features

### Available now

| Command | Description |
|---|---|
| `scan` | Discover nearby Colmi-compatible rings by name prefix |
| `info` | Print firmware version, hardware model, and battery level |
| `reboot` | Reboot the ring |
| `set-time` | Sync the ring's clock to the current UTC time |
| `get-heart-rate-log` | Fetch timestamped heart rate readings for a given date |
| `get-steps` | Fetch sport-detail (step count, calories, distance) for a given date |
| `raw` | Send an arbitrary packet by hex and print the reply |

Global flags available on all device commands: `--address`, `--name`, `--debug`, `--record`.

### Planned

- `get-real-time` — on-demand heart rate and SpO2 readings (streaming)
- `sync` — pull all data into a local SQLite database, incrementally
- `get-heart-rate-log-settings` / `set-heart-rate-log-settings`
- `completions` — shell completion scripts for bash, zsh, and fish
- `--config` — default device address stored in `~/.config/smartring/config.toml`

---

## Installation

### Prerequisites

#### Global Requirements
- Rust toolchain (stable) — install via [rustup](https://rustup.rs)

#### OS Requirements
- **macOS**: Bluetooth permission granted to Terminal / your shell
- **Linux**: `libdbus-1-dev` and `pkg-config` (`sudo apt install libdbus-1-dev pkg-config`)
- **Windows**: WinRT Bluetooth — no extra packages needed

### Build from source

```sh
git clone https://github.com/austinandrews/smartringmanager
cd smartringmanager/smartring_rust
cargo build --release
# binary is at target/release/smartring
```

To install system-wide:

```sh
cargo install --path smartring-cli
```

---

## Usage

All device commands require either `--address` (preferred on Linux/Windows) or `--name` (required on macOS, since Core Bluetooth assigns per-session UUIDs instead of hardware MACs).

### Find your ring

```sh
smartring scan
# NAME             | ADDRESS
# R02_A1B2         | AA:BB:CC:DD:EE:FF

smartring scan --all   # show every visible BLE device
```

### Device info and maintenance

```sh
smartring --address AA:BB:CC:DD:EE:FF info
smartring --name R02_A1B2 info

smartring --address AA:BB:CC:DD:EE:FF set-time        # sync to current UTC
smartring --address AA:BB:CC:DD:EE:FF reboot
```

### Read health data

```sh
# Heart rate log for a specific date
smartring --address AA:BB:CC:DD:EE:FF get-heart-rate-log --date 2026-05-12

# Step data (defaults to today)
smartring --address AA:BB:CC:DD:EE:FF get-steps
smartring --address AA:BB:CC:DD:EE:FF get-steps --date 2026-05-11
smartring --address AA:BB:CC:DD:EE:FF get-steps --json
smartring --address AA:BB:CC:DD:EE:FF get-steps --csv | tee steps.csv
```

### Diagnostics

```sh
# Verbose BLE packet logging to stderr
smartring --debug --address AA:BB:CC:DD:EE:FF info

# Capture all received packets to a binary file
smartring --record --address AA:BB:CC:DD:EE:FF get-heart-rate-log --date 2026-05-12

# Send a raw command (battery request = command 0x03)
smartring --address AA:BB:CC:DD:EE:FF raw --command 3 --replies 1
```

---

## Compatibility

This project has only been tested against the Colmi R10. Currently, the following device name prefixes are recognised by `scan`:

R01, R02, R03, R04, R05, R06, R07, R09, R10, COLMI, VK-5098, MERLIN, Hello Ring, RING1, boAtring, TR-R02, SE, EVOLVEO, GL-SR2, Blaupunkt, KSIX RING

---

## Project layout

```
smartring_rust/
├── smartring-core/   # Protocol logic and BLE client (library crate)
└── smartring-cli/    # CLI frontend (binary crate)
```

`smartring-core` has no CLI or I/O concerns and can be reused as a library by a future TUI or GUI frontend.

---

## License

MIT
