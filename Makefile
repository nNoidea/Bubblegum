.PHONY: build devbuild run devrun

build:
	cargo build --release

devbuild:
	cargo build

run:
	cargo run --release

devrun:
	cargo run
