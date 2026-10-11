PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPLICATIONS ?= $(DATADIR)/applications
ICONDIR ?= $(DATADIR)/icons/hicolor/512x512/apps

.PHONY: all build devbuild run devrun test check fmt fmt-check coverage coverage-check coverage-html install uninstall bump

all: build

# Bump the release version ([package] version is the SSOT) and refresh Cargo.lock.
bump:
	@if [ -z "$(NEW_VERSION)" ]; then echo "Usage: make bump NEW_VERSION=x.y.z" >&2; exit 1; fi
	@echo "$(NEW_VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "Invalid version '$(NEW_VERSION)' (expected X.Y.Z)" >&2; exit 1; }
	sed -i "0,/^version = \".*\"$$/s//version = \"$(NEW_VERSION)\"/" Cargo.toml
	@cargo check > /dev/null
	@echo "Bumped to $(NEW_VERSION) — commit Cargo.toml and Cargo.lock together"

build:
	cargo build --release

devbuild:
	cargo build

run:
	cargo run --release

devrun:
	cargo run

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

test:
	cargo fmt --check
	cargo test

check:
	cargo fmt --check
	cargo clippy -- -D warnings

coverage:
	cargo tarpaulin --out Stdout --out Html --output-dir target/tarpaulin

coverage-check:
	cargo tarpaulin --fail-under 35 --out Stdout

coverage-html:
	cargo tarpaulin --out Html --output-dir target/tarpaulin
	@echo "Coverage report generated at: target/tarpaulin/tarpaulin-report.html"

install: build
	install -d $(BINDIR)
	install -m 755 target/release/bubblegum $(BINDIR)/bubblegum
	install -d $(APPLICATIONS)
	install -m 644 data/com.github.Bubblegum.desktop $(APPLICATIONS)/com.github.Bubblegum.desktop
	install -d $(ICONDIR)
	install -m 644 data/com.github.Bubblegum.png $(ICONDIR)/com.github.Bubblegum.png
	@if command -v update-desktop-database > /dev/null 2>&1; then \
		update-desktop-database $(APPLICATIONS); \
	fi
	@if command -v gtk-update-icon-cache > /dev/null 2>&1; then \
		gtk-update-icon-cache -f -t $(DATADIR)/icons/hicolor 2>/dev/null || true; \
	fi
	@echo "Bubblegum successfully installed to $(BINDIR)/bubblegum"

uninstall:
	rm -f $(BINDIR)/bubblegum
	rm -f $(APPLICATIONS)/com.github.Bubblegum.desktop
	rm -f $(ICONDIR)/com.github.Bubblegum.png
	@if command -v update-desktop-database > /dev/null 2>&1; then \
		update-desktop-database $(APPLICATIONS); \
	fi
	@if command -v gtk-update-icon-cache > /dev/null 2>&1; then \
		gtk-update-icon-cache -f -t $(DATADIR)/icons/hicolor 2>/dev/null || true; \
	fi
	@echo "Bubblegum uninstalled"
