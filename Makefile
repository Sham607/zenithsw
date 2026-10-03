CARGO ?= cargo

.PHONY: all build release test start start-live tui engine stop restart logs reset clean help

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
	@sed -i 's/^DRY_RUN=.*/DRY_RUN=true/' .env
	@sed -i 's|^TRADING_DB_PATH=.*|TRADING_DB_PATH=data/trading_state.db|' .env
	@echo "🟢 Menjalankan ZenithSW Engine Daemon (DRY-RUN)..."
	@systemctl --user restart zenithsw-engine.service
	@sleep 1
	@systemctl --user is-active --quiet zenithsw-engine.service && echo "[OK] Engine aktif DRY-RUN (PID: $$(systemctl --user show --property MainPID --value zenithsw-engine.service))." || (echo "[ERROR] Engine gagal start." && exit 1)

start-live: release
	@sed -i 's/^DRY_RUN=.*/DRY_RUN=false/' .env
	@sed -i 's|^TRADING_DB_PATH=.*|TRADING_DB_PATH=data/trading_state_live.db|' .env
	@echo "🔴 Menjalankan ZenithSW Engine Daemon (LIVE MODE)..."
	@systemctl --user restart zenithsw-engine.service
	@sleep 1
	@systemctl --user is-active --quiet zenithsw-engine.service && echo "[OK] Engine aktif LIVE (PID: $$(systemctl --user show --property MainPID --value zenithsw-engine.service))." || (echo "[ERROR] Engine gagal start." && exit 1)

tui: release
	@$(CARGO) run --release --bin zenithsw

stop:
	@echo "Stopping ZenithSW Engine Daemon..."
	@systemctl --user stop zenithsw-engine.service 2>/dev/null || true
	@pkill -x zenithsw-engine || true
	@echo "[OK] Engine stopped."

restart:
	@echo "Restarting ZenithSW Engine Daemon..."
	@systemctl --user restart zenithsw-engine.service
	@sleep 1
	@systemctl --user is-active --quiet zenithsw-engine.service && echo "[OK] Engine restarted (PID: $$(systemctl --user show --property MainPID --value zenithsw-engine.service))." || (echo "[ERROR] Engine gagal start." && exit 1)

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
	@echo "  make start      - Jalankan Engine Daemon (DRY-RUN)"
	@echo "  make start-live - Jalankan Engine Daemon (LIVE MODE)"
	@echo "  make tui        - Buka TUI Dashboard client (zenithsw)"
	@echo "  make stop       - Hentikan Engine Daemon"
	@echo "  make restart    - Restart Engine Daemon"
	@echo "  make logs       - Stream systemd logs engine"
	@echo "  make test       - Jalankan unit & integration test"
	@echo "  make reset      - Bersihkan database SQLite dan socket"
	@echo "  make release    - Kompilasi biner release"
