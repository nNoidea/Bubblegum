.PHONY: build devbuild run devrun test coverage coverage-check coverage-html

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

coverage:
	cargo tarpaulin --out Stdout --out Html --output-dir target/tarpaulin

coverage-check:
	cargo tarpaulin --fail-under 35 --out Stdout

coverage-html:
	cargo tarpaulin --out Html --output-dir target/tarpaulin
	@echo "Coverage report generated at: target/tarpaulin/tarpaulin-report.html"

