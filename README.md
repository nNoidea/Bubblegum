# BubblegumGNOME

A unified package manager GUI for GNOME — manage all your packages across **DNF**, **Flatpak**, and **Cargo** from one place.

## Features

- **Unified package list** — view and search all installed packages from DNF, Flatpak, and Cargo in one grid
- **Fuzzy search** — instant filtering as you type
- **Repository management** — view and toggle repositories/remotes for each package manager
- **Package details** — select a package to see full info in a bottom panel
- **Uninstall flow** — safe confirmation dialog with PolicyKit support
- **Responsive UI** — built with GTK4 + Libadwaita, adapts to any window size

## Screenshots

> TODO

## Building

### Dependencies

- Rust 2024 edition
- GTK4, Libadwaita (1.6+)
- DNF, Flatpak, Cargo (for runtime package management)

### Build & Run

```bash
# Build release
make build

# Run
make run

# Or directly with Cargo
cargo run --release
```

## License

Apache 2.0 — see [LICENSE](LICENSE).
