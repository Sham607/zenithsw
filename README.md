# 🚀 ZenithSW — Multi-Chain Smart Money Shadow Engine & TUI Dashboard

**ZenithSW** adalah bot *shadow trading* multi-chain berlatensi sub-detik (<150 ms) yang melacak dan mengeksekusi *copy-trade* cerdas mengikuti pergerakan **Top Trader `fomo.family`** serta penemuan mandiri dompet *Smart Money* on-chain (**Direct On-Chain DEX Graph Indexer**) lintas blockchain (**Solana SVM**, **Robinhood / Arbitrum EVM**, **Base EVM**).

---

## 🏛️ Arsitektur Sistem (Decoupled Engine & Direct On-Chain Indexer)

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 1. SMART MONEY DISCOVERY & INTELLIGENCE                                          │
│ • FOMO Backend Direct API: Sinkronisasi Top Leaderboard & PnL Trader             │
│ • Direct On-Chain DEX Indexer: Reverse-Lookup Early Buyers pada Exploding Pools   │
│ • Anti-Scam & Reputation Scorer: Filter Dev/Creator Trap, Usia Dompet, Multi-Win │
└────────────────────────────────────────┬─────────────────────────────────────────┘
                                         │ (SQLite WAL + In-Memory Caching)
                                         ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 2. SUB-SECOND MULTI-CHAIN STREAM INGESTION (<150 ms)                             │
├────────────────────────────────────────┬─────────────────────────────────────────┤
│           SOLANA SVM DRIVER            │             EVM MULTI-CHAIN             │
│ • Helius Dedicated WebSocket & RPC     │ • Alchemy Dedicated WebSocket & RPC     │
│ • Parsing Enhanced Txs & TokenBalances │ • Arbitrum & Base Asset Transfers       │
│ • Staggered Polling (Anti-Throttling)  │ • In-Memory Signature Deduplication     │
└────────────────────────────────────────┴─────────────────────────────────────────┘
                                         │ (Deteksi Swap Beli / Jual)
                                         ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 3. CONFLUENCE & SLIPPAGE GATING MATRIX                                           │
├──────────────────────────────────────────────────────────────────────────────────┤
│ • Hard Gating Anti-Pucuk: Max Price Delta <= +5.0% dari harga entry whale        │
│ • Liquidity Safety Floor: DEX Pool Liquidity >= $30,000.0 (Tolak Koin Kosong)    │
│ • Strict Tiering: Tier-S (Rank 1-3) & Tier-A (Rank 4-15) Eksekusi Langsung      │
│ • Tier-B Watchlist: Pemantauan Dinamis untuk Promosi ke Top 15                   │
└────────────────────────────────────────┬─────────────────────────────────────────┘
                                         │ (Lolos Evaluasi)
                                         ▼
┌──────────────────────────────────────────────────────────────────────────────────┐
│ 4. PURE PROPORTIONAL WHALE MIRROR EXECUTION & BAG TRACKING                       │
├──────────────────────────────────────────────────────────────────────────────────┤
│ • Entry: Buka posisi shadow otomatis saat Whale Beli ($15.0 fixed size)          │
│ • Proportional Partial Exit: Jual X% posisi jika Whale jual X% saldo koinnya     │
│ • Full Exit Dump: Tutup 100% posisi saat Whale membuang seluruh koinnya          │
│ • Atomic State: Sync real-time via UNIX Domain Socket (/tmp/zenithsw.sock)       │
└──────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📊 TUI Dashboard Layout (Ratatui 4-Tab Matrix)

* **Tab `[1] LIVE STREAM ⚡`**: Aliran real-time transaksi beli/jual target whale (termasuk deteksi `SOLD 50% PARTIAL 🟡` vs `SOLD 100% FULL DUMP 🔴`) dan keputusan evaluasi gating bot.
* **Tab `[2] TARGET WALLETS 🎯`**: Tabel pemantauan dompet target dengan penanda asal `[ONCHAIN 🌐]` vs `[FOMO ⚡]` (Rank, Realized PnL, Win Rate %, Total Trades, Skor Reputasi, dan Status Eksekusi).
* **Tab `[3] SHADOW POSITIONS 💼`**: Posisi trading terbuka dengan tracking harga realtime, sisa porsi bag (`Bag Remaining %`), floating PnL, dan Realized PnL dari penjualan parsial.
* **Tab `[4] LATENCY & STATS 📊`**: Pemantau latensi sub-detik jaringan (Helius RPC, Alchemy Arbitrum/Base), status Direct On-Chain Indexer, dan audit log riwayat eksekusi.

---

## ⚙️ Parameter Aktif (.env)

| Parameter | Nilai Default | Penjelasan Teknis |
| :--- | :--- | :--- |
| **`MAX_PRICE_DELTA_PCT`** | **`+5.0%`** | Batas maksimal lonjakan harga saat bot masuk dibanding harga beli whale. Jika $> +5.0\%$, order ditolak (*anti-pucuk*). |
| **`MIN_LIQUIDITY_USD`** | **`$30,000.0`** | Batas minimal likuiditas pool DEX untuk menghindari *rug pull* dan koin unindexed $0.00. |
| **`POSITION_SIZE_USD`** | **`$15.0`** | Ukuran posisi per order copy-trade (~0.11 SOL / 0.006 ETH). |
| **`FOMO_SYNC_INTERVAL_SEC`**| **`5`** | Interval pembaruan status live feed ke TUI. |
| **`STREAM_POLL_INTERVAL_MS`**| **`500`** | Latensi pengecekan transaksi target wallet (sub-detik). |
| **`SHADOW_EXIT_ENABLED`** | **`true`** | Mode mirror murni (menjual parsial atau penuh mengikuti proporsi sell whale). |

---

## 🛡️ Aturan Eksekusi & Bag Tracking

1. **Pure Whale Trigger:**
   * Bot tidak menggunakan Stop Loss statis atau Trailing Stop acak. Sinyal beli dan jual **100% diatur oleh pergerakan dompet Smart Money yang diikuti**.
2. **Deteksi Saldo Koin Whale (Pre/Post Balance):**
   * Bot menghitung persentase saldo koin yang dilepas oleh whale:
     $$\text{Sell Ratio (\%)} = \left( \frac{\text{Token Amount Sold}}{\text{Whale Balance Before}} \right) \times 100\%$$
   * Jika whale menjual **50%** $\rightarrow$ Bot menjual **50%** porsi shadow position (`PARTIAL_EXIT`).
   * Jika whale menjual **$\ge 95\%$** $\rightarrow$ Bot menutup **100%** posisi (`CLOSED`).
3. **Dual Failover & Graceful Offline Fallback:**
   * Jika sesi web FOMO kadaluwarsa (401/429), bot tetap berjalan 24/7 memantau blockchain secara on-chain via Helius RPC & SQLite cache lokal.
4. **Direct On-Chain DEX Graph Indexer:**
   * Memindai Raydium / Pump.fun secara mandiri untuk menemukan whale baru berkinerja tinggi (Win Rate $>65\%$, Multi-Token Wins, Anti-Creator check).

---

## 🚀 Panduan Menjalankan Bot

### 1. Inisialisasi Konfigurasi
Salin template konfigurasi dan masukkan RPC key Anda di file `.env`:
```bash
cp .env.example .env
```

### 2. Menjalankan Dashboard TUI (All-in-One)
Perintah ini akan otomatis menyalakan background engine daemon jika belum aktif, lalu membuka dashboard TUI:
```bash
make start
```

### 3. Mengelola Engine Daemon Terpisah
* **Menyalakan Daemon Manual:** `make engine`
* **Menghentikan Daemon:** `make stop`
* **Restart Daemon:** `make restart`
* **Menjalankan Test Suite:** `make test`

### Shortcut Navigasi TUI:
* **`[1]` / `[2]` / `[3]` / `[4]`**: Berpindah tab dashboard secara instan.
* **`[Tab]` / `[Shift+Tab]`**: Rotasi tab berikutnya / sebelumnya.
* **`[Q]` / `[Esc]`**: Keluar dari dashboard visual (engine tetap jalan di background).

---

## 🧪 Testing & Verifikasi
Jalankan seluruh rangkaian test suite integrasi (Gating, Proportional Exit, Database, Indexer Scoring):
```bash
cargo test
```
Semua test terverifikasi **7/7 PASS**.
