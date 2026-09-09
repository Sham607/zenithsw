.PHONY: all build release test start engine stop restart reset clean help

all: release

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

engine:
	cargo run --release --bin zenithsw-engine

start: release
	@if ! pgrep -f "target/release/zenithsw-engine" > /dev/null; then \
		echo "🚀 Auto-launching ZenithSW Engine Daemon..."; \
		nohup ./target/release/zenithsw-engine > data/engine.log 2>&1 & \
		sleep 1; \
	fi
	@cargo run --release --bin zenithsw

stop:
	@echo "Stopping ZenithSW Engine Daemon..."
	@pkill -x zenithsw-engine || true
	@echo "[OK] Engine stopped."

restart: stop
	@sleep 1
	@echo "Starting ZenithSW Engine Daemon..."
	@./target/release/zenithsw-engine &
	@sleep 1
	@echo "[OK] Engine restarted."

reset:
	@echo "Resetting Smart Wallets Database and Cache..."
	@pkill -x zenithsw-engine || true
	@rm -rf data/*.db*
	@rm -f /tmp/zenithsw.sock
	@echo "[OK] State reset successfully."

clean:
	cargo clean
	@rm -f /tmp/zenithsw.sock

help:
	@echo "ZenithSW Commands:"
	@echo "  make start   - Auto-launches Engine + TUI Dashboard client (zenithsw)"
	@echo "  make engine  - Run the background shadow daemon directly (zenithsw-engine)"
	@echo "  make stop    - Stop the background engine daemon"
	@echo "  make restart - Restart the background engine daemon"
	@echo "  make test    - Run the entire test suite"
	@echo "  make reset   - Clean SQLite database and socket"
	@echo "  make release - Compile optimized release binaries"
