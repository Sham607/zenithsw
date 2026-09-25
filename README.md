# ZenithSW

ZenithSW adalah engine *shadow copy-trading* multi-chain dengan daemon engine terpisah dan dashboard TUI Ratatui. Fokus implementasi saat ini adalah Solana dan fondasi eksekusi read-only Robinhood Chain.

> **Status penting:** fondasi lokal, konfigurasi fail-closed, database, IPC, runtime harness, dan code gates tervalidasi. Solana memiliki jalur mirror execution yang lebih lengkap. Robinhood masih dalam validasi dry-run dan **belum siap untuk copy-trade live**. Belum ada canary on-chain dengan rekonsiliasi receipt, event, saldo, dan posisi.

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

TUI juga melakukan refresh database berkala. Status IPC harus dibedakan dari snapshot SQLite; data dashboard tidak boleh dianggap bukti transaksi live tanpa verifikasi on-chain. Posisi fisik wallet yang diadopsi saat startup (`ORPHAN_ADOPTED`) diproteksi otomatis dan dibedakan dari posisi simulasi melalui flag database `is_dry_run`.

## Chain yang didukung

### Solana

- **Arsitektur Dual-Channel & Multi-Endpoint Round-Robin RPC**:
  - *Kanal Publik & WebSocket* (`SOLANA_RPC_URL`, `SOLANA_WS_URL`): Mendukung multi-endpoint yang dipisahkan koma dengan rotasi *atomic round-robin* (`AtomicUsize`), auto-failover, pacing 2.500ms antardompet, dan jeda adaptif HTTP 429 backoff (1.500ms) untuk menangani polling rutin tanda tangan wallet (`getSignaturesForAddress`) dan saldo token (`getTokenAccountsByOwner`) tanpa membakar kuota eksekusi (0 credit Helius).
  - *Kanal Presisi Eksekusi* (`SOLANA_EXECUTION_RPC_URL`, misal Helius): Didedikasikan eksklusif untuk parsing terurai transaksi whale (`getTransaction` jsonParsed), fetch blockhash kritis (`getLatestBlockhash`), dan broadcast transaksi langsung (`sendTransaction`).
- **Parser Native SOL Stream**: Menghitung selisih saldo lamports Native SOL fisik (`meta.preBalances` vs `meta.postBalances`) dengan kompensasi netral fee transaksi pada fee payer, mencegah salah klasifikasi transaksi DEX multi-hop/Native SOL sebagai `TRANSFER`.
- Jupiter/PumpPortal digunakan oleh jalur executor yang tersedia.
- Konfirmasi transaksi dipolling sebelum posisi dibuka atau ditutup.
- Balance token direkonsiliasi dari RPC.
- Posisi satu token digabung dengan VWAP (multi-lot accumulation didukung hingga batas exposure cap).
- Exit parsial mengikuti proporsi penjualan whale saat mode shadow aktif.
- Transaction aggregator yang dipakai untuk BUY harus menyediakan serialized transaction; blockhash diparse dari transaction yang benar-benar akan ditandatangani, bukan dari RPC `getLatestBlockhash` yang diambil terpisah.

### Robinhood Chain

- Chain ID: `4663` (`0x1237`).
- RPC aktif dikonfigurasi melalui `ROBINHOOD_RPC_URL`; chain ID wajib `4663` (`0x1237`).
- Pool discovery memakai `PoolManager.Initialize` dan filter token.
- Indexer discovery baru dikarantina sebagai kandidat: `TIER-B`, `is_active=false`, dan source `ONCHAIN_INDEXER_CANDIDATE`; metriknya belum merupakan realized PnL.
- Cluster detective hanya mengembalikan kandidat `CLUSTER_FORENSIC_CANDIDATE` inactive dan tidak melakukan ingestion registry.
- Route probing read-only mencakup Uniswap V3 (direct dan two-hop melalui USDG), V4, serta deployment Pons yang sudah dikenal.
- Native-currency route mengenali `address(0)` secara eksplisit; token/WETH tidak boleh ditebak.
- V3 quote memakai QuoterV2 melalui `eth_call`; V4 quote memakai `quoteExactInputSingle` melalui `eth_call`.
- Route terbaik dipilih berdasarkan `amountOut`, lalu calldata dibuat untuk router venue yang sesuai.
- Stream Robinhood hanya menerima receipt sukses dengan transfer dan venue teridentifikasi; transfer-only/venue unknown ditolak.
- Router simulation memakai calldata yang sama dengan rencana transaksi: `eth_call` lalu `eth_estimateGas`.
- Dry-run tidak signing, broadcast, atau membuat posisi `OPEN`.
- `send_raw_transaction` masih diblokir sampai live gate/canary disetujui.
- `LIVE_ROBINHOOD_ENABLED` hanya gate tambahan; `DRY_RUN=false` tetap ditolak bila signer, broadcast, receipt, dan reconciliation belum lengkap.
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
- multi-lot accumulation didukung dengan weighted VWAP hingga batas `MAX_TOKEN_EXPOSURE` ($50);
- posisi live hanya boleh dipersist setelah bukti transaksi dan balance on-chain lengkap.

### Dual-Channel Whale Gems Radar
Sistem radar mendeteksi aset pantauan whale secara asimetris untuk mencegah polusi token *dust* dan *airdrop spam*:
- **Jalur Saldo Pasif (`StructureConfirmed`)**: Token terdaftar di DexScreener wajib memiliki total nilai saldo kepemilikan whale $\ge$ $100 USD (`ui_amount * price_usd >= 100.0`). Koin debu di bawah $100 USD dibuang dari proyeksi radar.
- **Jalur Transaksi Aktif (`HighPriorityRadar`)**: Transaksi beli baru (`BUY` / `DirectSwap`) oleh whale $\ge$ $100 USD dalam 24 jam terakhir langsung diproyeksikan ke baris prioritas radar TUI, mendukung token tahap pra-DEX / Pump.fun.
- **Deduplikasi State & Transaksi Bersih**: Saldo snapshot kepemilikan whale (`TokenHolding`) dikelola melalui *upsert* berbasis `(chain, token_address, wallet_address)`. Ini menjaga pembacaan token baru whale tanpa membengkakkan database secara eksponensial. Transaksi pembelian whale (`DirectSwap`) tetap dipertahankan penuh (*append-only*).
- **Pruning Otomatis**: Proyeksi radar yang tidak lagi memenuhi kualifikasi dibersihkan otomatis setiap siklus agar data TUI selalu relevan dan bersih.

### Strategi Exit Terbifurkasi (`SHADOW_EXIT_ENABLED`)

ZenithSW mengisolasi aturan keluar menjadi dua mode operasional:

1. **Mode Whale Mirror (`SHADOW_EXIT_ENABLED=true`)**:
   - Menyimak transaksi penjualan whale secara real-time.
   - `WHALE_FULL_DUMP`: Jika whale menjual $\ge$ `WHALE_FULL_DUMP_PCT` (default `95.0%`) dan whale tunggal $\rightarrow$ menutup 100% posisi.
   - `WHALE_PARTIAL_DUMP`: Menjual proporsional berbobot sesuai porsi penjualan whale.
   - Protokol 50-25-25: Take Profit 1 (`+100%`) menjual 50% untuk *free-roll* modal pokok; Take Profit 2 (`+300%`) menjual 25% *runner-lock*.
   - Proteksi Moonbag: `PEAK_DRAWDOWN_GUARD_EXIT` (dump jika jatuh dari harga puncak) dan `PROFIT_LOCK_FLOOR_EXIT` (mengunci keuntungan minimum hijau).
   - Pre-TP Trailing Stop Guard (pemicu `+35%`, drop `15%` dari peak $\rightarrow$ dump 100%) dan Fast-Lane Hard Stop Loss.

2. **Mode Bot Otonom (`SHADOW_EXIT_ENABLED=false`)**:
   - Mengabaikan seluruh transaksi jual whale (whale murni sebagai sinyal entri).
   - Eksekusi 100% *All-or-Nothing* (tanpa parsial/moonbag, posisi selalu biner `OPEN` $\rightarrow$ `CLOSED`):
     - `BOT_FULL_TP_EXIT`: Profit mencapai `BOT_TAKE_PROFIT_PCT` $\rightarrow$ Jual 100% full posisi.
     - `BOT_HARD_SL_CUTLOSS`: Kerugian mencapai `-BOT_STOP_LOSS_PCT` $\rightarrow$ Cut-loss 100% seketika.
     - `BOT_TRAILING_STOP_EXIT`: Gain mencapai `BOT_TRAILING_STOP_TRIGGER_PCT` lalu drop $\ge$ `BOT_TRAILING_STOP_PCT` dari peak $\rightarrow$ Jual 100% kunci profit.

3. **Global Safety Guard (Berlaku di Kedua Mode)**:
   - `LIQUIDITY_DRAIN_EXIT`: Jika likuiditas pool anjlok di bawah ambang `LIQUIDITY_DRAIN_USD` (anti jebakan *rug-pull*), posisi langsung didump 100%.


Parameter runtime dibaca dari `.env`/secret manager melalui `Config::load()`. Gunakan `.env.example` sebagai referensi nama variabel. Jangan commit `.env`, private key, API key, atau database runtime.

Default konfigurasi aman yang diuji:

```text
DRY_RUN=true
SIM_INITIAL_BALANCE=100.0
LIVE_ROBINHOOD_ENABLED=false
ENABLE_SOLANA=true
ENABLE_ROBINHOOD=false
ENABLE_BASE=false
IPC_SOCKET_PATH=/tmp/zenithsw.sock
```

Loader fail-closed memvalidasi boolean, angka, URL, interval, path absolut, chain scope, konflik mode, market-cap range, dan path database duplikat. Nilai invalid tidak diganti fallback diam-diam.

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

## Sinkronisasi Engine–TUI

TUI hanya menampilkan engine online setelah menerima heartbeat aktual yang masih fresh. Heartbeat membawa sequence monoton, timestamp, mode live/dry-run, status mode exit (`shadow_exit_enabled`), equity engine, parameter runtime, wallet scope, dan RPC health counters.

Snapshot periodik berikut mengganti state lama TUI setelah chain filtering:

- `WalletSync`
- `TrackedTokensUpdate`
- `PositionUpdate`
- `TradeSync`
- `ActivitySync`
- `SignalSync`

`ActivityBroadcast` dan `SignalBroadcast` tetap digunakan untuk event latency rendah. Snapshot periodik mencegah state activity/signal tertinggal ketika event terlewat atau IPC reconnect. Runtime harness Engine → IPC → DB telah lulus pada DB/socket terisolasi dan `DRY_RUN=true`.

### Optimasi Sinkronisasi & Polling Adaptif
- **Adaptive Polling Cadence**: Saat koneksi IPC sehat dan streaming berjalan, TUI memperlambat polling SQLite lokal ke siklus rekonsiliasi ringan (10 detik) untuk mencegah lock contention dan I/O ganda. Jika IPC terputus atau stale, TUI secara otomatis beralih ke mode cepat (1500ms) sebagai mekanisme fail-safe.
- **Harmonisasi Ring Buffer**: Query database untuk log aktivitas dan sinyal disinkronkan ke kapasitas 100 entri, selaras dengan batas tampungan ring buffer IPC real-time.
- **Indikator Visual Dinamis Tab 5**: Dashboard menampilkan status mode keluar aktif (`WHALE MIRROR (Shadow Exit) 🐋` atau `BOT AUTONOMOUS (Clean Scalp) 🤖`) dengan parameter Stop Loss dan Take Profit yang tersinkronisasi live.

## Durable execution intent

Execution intent memisahkan lifecycle niat transaksi dari lifecycle posisi:

```text
INTENT
→ QUOTED
→ SIMULATED
→ SIGNED
→ BROADCASTED
→ PENDING_RECONCILIATION
→ CONFIRMED
→ RECONCILED
```

Status ambigu diarahkan ke `UNKNOWN`; posisi `OPEN` tidak digunakan sebagai pengganti bukti rekonsiliasi intent. Jalur Robinhood BUY menyimpan quote block, calldata hash, expiry, dan simulation evidence sebelum signing. Jalur Solana aggregator memparse recent blockhash dari serialized transaction aktual sebelum metadata disimpan dan signing dilakukan.

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

Fondasi RPC, discovery, encoder, durable intent, dan test sudah tersedia. SELL on-chain dan live reconciliation Robinhood belum selesai. Jangan mengaktifkan Robinhood untuk copy-trade produksi berdasarkan test/build saja.

## Database

```text
data/trading_state.db   posisi, signal, activity, trade history, tracked token, execution intents
data/wallet_registry.db registry smart wallet
data/candidate_wallet_registry.db kandidat belum dipromosikan
```

Kedua database memakai SQLite WAL. Jangan menghapus atau mereset database tanpa backup dan persetujuan eksplisit. `make reset` bersifat destruktif terhadap state runtime dan tidak boleh dipakai sembarangan.

### Self-Maintenance & Health Guard Database
- **Indexing Kolom Timestamp**: Kolom `observed_at` pada tabel `radar_evidence` dan `radar_market_history` diproteksi indeks SQLite B-tree untuk mencegah degradasi performa (*full-table scan*) saat pembersihan data.
- **Background Maintenance Worker**: Dijalankan secara terisolasi oleh background task setiap 1 jam (`src/bin/engine.rs`).
- **Pruning Terjadwal**: Menghapus `radar_evidence` kadaluarsa (>24 jam) dengan menyisakan state snapshot token teranyar, memangkas `radar_market_history` (>7 hari), serta mengeksekusi `PRAGMA wal_checkpoint(PASSIVE)` dan `PRAGMA optimize`.
- **Zero Heartbeat Lock Contention**: Operasi `DELETE` dan optimasi I/O besar dipindahkan seluruhnya dari alur tulis streaming/radar ke background worker, menjaga IPC heartbeat TUI tetap lancar (`<10ms`) tanpa risiko *stale heartbeat*.

## Menjalankan

```bash
make release
make test
make engine
make start
make stop
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
mise exec rust@1.94.1 -- cargo fmt --all -- --check
mise exec rust@1.94.1 -- cargo check --locked
mise exec rust@1.94.1 -- cargo test --locked
mise exec rust@1.94.1 -- cargo clippy --all-targets -- -D warnings
mise exec rust@1.94.1 -- cargo build --release --locked
git diff --check
```

Verifikasi terbaru: **188 library tests & 41 integration tests lulus (total 229 test), 0 gagal**. Lulus test/build bukan bukti transaksi live. Engine dan socket tetap OFF setelah validasi.

## Credential dan keamanan

- Credential hanya dari environment atau secret manager.
- Jangan cetak credential ke log, README, source, atau commit.
- Credential yang pernah dikirim melalui chat harus dirotasi sebelum live.
- Jangan menonaktifkan verifikasi TLS untuk mengatasi masalah endpoint.
- Live broadcast memerlukan canary eksplisit: token, nominal, arah transaksi, batas gas/slippage, dan izin broadcast.
- Jika metadata quote, expiry, price, liquidity, receipt, event, atau saldo tidak dapat diverifikasi, engine harus berhenti fail-closed.
