.PHONY: all build release test start engine clean reset help

all: build

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

engine:
	cargo run --release --bin zenithsw-engine

start:
	cargo run --release --bin zenithsw

reset:
	@echo "Resetting Smart Wallets Database and Cache..."
	@rm -rf data/*.db*
	@rm -f /tmp/zenithsw.sock
	@echo "[OK] State reset successfully."

clean:
	cargo clean
	@rm -f /tmp/zenithsw.sock

help:
	@echo "ZenithSW Commands:"
	@echo "  make start   - Run the TUI Dashboard client (zenithsw)"
	@echo "  make engine  - Run the background shadow daemon (zenithsw-engine)"
	@echo "  make test    - Run the entire test suite"
	@echo "  make reset   - Clean SQLite database and socket"
	@echo "  make release - Compile optimized release binaries"
