RUST_TOOLCHAIN ?= rust@1.94.1
CARGO = mise exec $(RUST_TOOLCHAIN) -- cargo

.PHONY: all build release test start engine stop restart reset clean help

all: release

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test --locked

engine:
	$(CARGO) run --release --bin zenithsw-engine

start: release
	@if systemctl --user is-active --quiet zenithsw-engine.service; then \
		echo "🟢 ZenithSW Engine Daemon is active (systemd)."; \
	elif ! pgrep -f "target/release/zenithsw-engine" > /dev/null; then \
		echo "🚀 Auto-launching ZenithSW Engine Daemon via systemd..."; \
		systemctl --user start zenithsw-engine.service || (nohup ./target/release/zenithsw-engine > data/engine.log 2>&1 &); \
		sleep 1; \
	fi
	@$(CARGO) run --release --bin zenithsw

stop:
	@echo "Stopping ZenithSW Engine Daemon..."
	@systemctl --user stop zenithsw-engine.service 2>/dev/null || true
	@pkill -x zenithsw-engine || true
	@echo "[OK] Engine stopped."

restart:
	@echo "Restarting ZenithSW Engine Daemon..."
	@systemctl --user restart zenithsw-engine.service 2>/dev/null || ./target/release/zenithsw-engine &
	@sleep 1
	@echo "[OK] Engine restarted."

logs:
	@journalctl --user -u zenithsw-engine.service -f

reset:
	@echo "Resetting Smart Wallets Database and Cache..."
	@pkill -x zenithsw-engine || true
	@rm -rf data/*.db*
	@rm -f /tmp/zenithsw.sock
	@echo "[OK] State reset successfully."

clean:
	$(CARGO) clean
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
