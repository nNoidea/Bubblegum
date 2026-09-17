# BubblegumGNOME

A unified package manager GUI for GNOME, manage all your packages across **DNF**, **Flatpak**, and **Cargo** from one place.

## Screenshots

![alt text](assets/image1.png)
![alt text](assets/image2.png)
![alt text](assets/image3.png)


## Features

- **Unified package list** — view and search all installed packages from DNF, Flatpak, and Cargo in one grid
- **Fuzzy search** — instant filtering as you type
- **Repository information** — view repositories/remotes for each package manager
- **Package details** — select a package to see full info in a bottom panel
- **Uninstall flow** — safe confirmation dialog with PolicyKit support
- **Responsive UI** — built with GTK4 + Libadwaita, adapts to any window size

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

GPL-3.0-or-later — see [LICENSE](LICENSE).
