# ZenithSW

ZenithSW adalah engine *shadow copy-trading* multi-chain dengan daemon engine terpisah dan dashboard TUI Ratatui. Fokus implementasi saat ini adalah Solana dan fondasi eksekusi read-only Robinhood Chain.

> **Status penting:** Solana memiliki jalur mirror execution yang lebih lengkap. Robinhood masih dalam tahap validasi dry-run dan **belum siap untuk copy-trade live**. `send_raw_transaction` tetap fail-closed.

## Arsitektur

```text
zenithsw-engine
  ├─ wallet/activity ingestion
  ├─ confluence gating
  ├─ Solana executor
  ├─ Robinhood RPC adapter + EVM dry-run executor
  ├─ SQLite WAL trading state
  └─ Unix socket IPC: /tmp/zenithsw.sock
          │
          ▼
      zenithsw TUI
```

Engine dan TUI berbagi:

- `TRADING_DB_PATH` untuk posisi, signal, activity, trade history, dan tracked token;
- `WALLET_DB_PATH` untuk registry wallet;
- `IPC_SOCKET_PATH` untuk heartbeat, activity, signal, position update, trade sync, dan log.

TUI juga melakukan refresh database berkala. Status IPC harus dibedakan dari snapshot SQLite; data dashboard tidak boleh dianggap bukti transaksi live tanpa verifikasi on-chain.

## Chain yang didukung

### Solana

- RPC/WebSocket dikonfigurasi melalui environment.
- Jupiter/PumpPortal digunakan oleh jalur executor yang tersedia.
- Konfirmasi transaksi dipolling sebelum posisi dibuka atau ditutup.
- Balance token direkonsiliasi dari RPC.
- Posisi satu token digabung dengan VWAP.
- Re-entry dari whale yang sama saat posisi masih terbuka ditolak.
- Exit parsial mengikuti proporsi penjualan whale.

### Robinhood Chain

- Chain ID: `4663` (`0x1237`).
- Venue: Uniswap V4 deployment Robinhood.
- RPC aktif dikonfigurasi melalui `ROBINHOOD_RPC_URL`.
- Pool discovery memakai `PoolManager.Initialize` dan filter token.
- Native-currency route mengenali `address(0)` sebagai currency native.
- V4 quote memakai `quoteExactInputSingle` melalui `eth_call`.
- Router simulation memakai calldata Universal Router yang sama dengan rencana transaksi.
- Dry-run tidak signing, broadcast, atau membuat posisi `OPEN`.
- `send_raw_transaction` masih diblokir sampai live gate/canary disetujui.
- SELL Robinhood masih fail-closed dan belum boleh mengubah posisi database.

Alamat kontrak Robinhood yang dikunci di source:

```text
PoolManager:    0x8366a39CC670B4001A1121B8F6A443A643e40951
PositionManager:0x58daec3116aae6D93017bAAea7749052E8a04fA7
V4Quoter:       0x8Dc178eFB8111BB0973Dd9d722ebeFF267c98F94
UniversalRouter:0x8876789976decbfcbbbe364623c63652db8c0904
Permit2:        0x000000000022D473030F116dDEE9F6B43aC78BA3
```

## Position dan gating

Aturan umum yang dipakai lintas executor:

- identitas aset adalah `chain + contract/mint address`, bukan ticker;
- asset blacklist diterapkan sebelum entry;
- harga, liquidity, market cap, amount, dan slippage harus valid serta finite;
- satu posisi aktif per token dan chain;
- averaging hanya dari whale berbeda;
- same-whale re-entry diblokir;
- partial exit dan full exit diproses secara proporsional;
- posisi live hanya boleh dipersist setelah bukti transaksi dan balance on-chain lengkap.

Parameter runtime dibaca dari `.env`/secret manager melalui `Config::load()`. Gunakan `.env.example` sebagai referensi nama variabel. Jangan commit `.env`, private key, API key, atau database runtime.

## Mode dry-run Robinhood

Runner dry-run eksplisit tersedia sebagai:

```bash
ROBINHOOD_POOL_INIT_BLOCK=0x3ad690f \
ROBINHOOD_DRY_RUN_TOKEN=0xfad40755de679b262337f8d964d75cac51f8c643 \
ROBINHOOD_DRY_RUN_SYMBOL=AC \
ROBINHOOD_TRADE_AMOUNT_WEI=1000000000000 \
cargo run --release --bin robinhood-dry-run
```

Pipeline read-only:

```text
chain validation
→ deployment validation
→ wallet/native balance
→ PoolKey discovery
→ V4 quote eth_call
→ Universal Router calldata
→ router eth_call
→ eth_estimateGas
→ selesai tanpa signing/broadcast/DB position
```

`ROBINHOOD_POOL_INIT_BLOCK` opsional, tetapi disarankan bila block Initialize sudah diketahui agar tidak melakukan scan log besar berulang. Nominal dry-run di atas hanya berlaku untuk proses tersebut dan bukan konfigurasi strategi permanen.

## Status eksekusi Robinhood

Sebelum disebut siap live, jalur berikut wajib lulus pada data on-chain aktual:

```text
PoolKey valid
→ quote stabil
→ balance/allowance/nonce/gas
→ router eth_call
→ eth_estimateGas
→ signing dengan key yang sudah dirotasi
→ broadcast canary
→ receipt status=1
→ Swap event terverifikasi
→ balance delta terverifikasi
→ persist posisi
```

Saat ini fondasi RPC, discovery, encoder, dan test sudah tersedia. BUY dry-run masih memerlukan verifikasi provider/quote yang stabil; SELL on-chain dan live reconciliation Robinhood belum selesai. Jangan mengaktifkan Robinhood untuk copy-trade produksi berdasarkan test/build saja.

## Database

```text
data/trading_state.db   posisi, signal, activity, trade history, tracked token
data/wallet_registry.db registry smart wallet
```

Kedua database memakai SQLite WAL. Jangan menghapus atau mereset database tanpa backup dan persetujuan eksplisit. `make reset` bersifat destruktif terhadap state runtime dan tidak boleh dipakai sembarangan.

## Menjalankan

```bash
# Build optimized binaries
make release

# Test suite
make test

# Jalankan engine daemon
make engine

# Jalankan engine + TUI
make start

# Hentikan engine
make stop

# Restart engine
make restart
```

Binary utama:

```text
zenithsw-engine       daemon engine
zenithsw              TUI dashboard
robinhood_probe       RPC/pool discovery probe
robinhood-dry-run     read-only Robinhood BUY simulation runner
```

## Verifikasi sebelum commit/release

```bash
cargo test --locked
cargo build --release --locked
git diff --check
```

Warnings compiler harus dipisahkan dari failure. Lulus test/build tidak sama dengan bukti transaksi live.

## Credential dan keamanan

- Credential hanya dari environment atau secret manager.
- Jangan cetak credential ke log, README, source, atau commit.
- Credential yang pernah dikirim melalui chat harus dirotasi sebelum live.
- Jangan menonaktifkan verifikasi TLS untuk mengatasi masalah endpoint.
- Live broadcast memerlukan canary eksplisit: token, nominal, arah transaksi, batas gas/slippage, dan izin broadcast.
