PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
APPLICATIONS ?= $(DATADIR)/applications

.PHONY: all build devbuild run devrun test check coverage coverage-check coverage-html install uninstall bump

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

test:
	cargo test

check:
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
	@if command -v update-desktop-database > /dev/null 2>&1; then \
		update-desktop-database $(APPLICATIONS); \
	fi
	@echo "Bubblegum successfully installed to $(BINDIR)/bubblegum"

uninstall:
	rm -f $(BINDIR)/bubblegum
	rm -f $(APPLICATIONS)/com.github.Bubblegum.desktop
	@if command -v update-desktop-database > /dev/null 2>&1; then \
		update-desktop-database $(APPLICATIONS); \
	fi
	@echo "Bubblegum uninstalled"
