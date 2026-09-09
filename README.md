# 🚀 ZenithSW — Multi-Chain Smart Money Shadow Engine & TUI Dashboard

**ZenithSW** adalah bot *shadow trading* multi-chain berlatensi sub-milidetik (<150 ms) yang melacak dan mengeksekusi *copy-trade* cerdas mengikuti pergerakan **Top 100 Trader `fomo.family`** dan dompet *Smart Money* on-chain lintas blockchain (**Solana SVM**, **Robinhood / Arbitrum EVM**, **Base EVM**).

---

## 🏛️ Arsitektur Sistem (Decoupled Engine & TUI)

```text
               ┌───────────────────────────────────────────────────────────┐
               │         1. LEADERBOARD & WALLET INTELLIGENCE              │
               │   • fomo.family Leaderboard Scraper (Top 100 Sync)        │
               │   • On-chain Resolver: Handle -> Solana & EVM Addresses   │
               │   • Tiering Classifier: Tier-S (Whale) vs Tier-A (Alpha)  │
               └─────────────────────────────┬─────────────────────────────┘
                                             │ (Watchlist Sinkron Setiap 60s)
                                             ▼
               ┌───────────────────────────────────────────────────────────┐
               │           2. REAL-TIME SUB-SECOND STREAM INGESTION        │
               ├─────────────────────────────┬─────────────────────────────┤
               │      SOLANA SVM DRIVER      │       EVM MULTI-CHAIN       │
               │ • Helius Dedicated WS/RPC   │ • Alchemy Dedicated WS/RPC  │
               │ • Logs / Enhanced Txs       │ • Arbitrum & Base Transfers │
               └──────────────────────┬──────┴─────────────────────────────┘
                                      │ (< 150 ms Event Parsing)
                                      ▼
               ┌───────────────────────────────────────────────────────────┐
               │           3. CONFLUENCE & SLIPPAGE GATING MATRIX          │
               ├───────────────────────────────────────────────────────────┤
               │ • Delta Price Guard: (Price_Now - Whale_Entry) <= +5.0%   │
               │ • Liquidity Safety Floor: Pool Liquidity >= $30,000       │
               │ • Confluence Rule: Tier-S Instant OR 2x Tier-A Confluence │
               │ • Shadow Exit Engine: Whale SELL -> Seketika Trigger SELL │
               └──────────────────────┬────────────────────────────────────┘
                                      │ (Lolos Evaluasi)
                                      ▼
               ┌───────────────────────────────────────────────────────────┐
               │          4. ATOMIC EXECUTION & ANTI-MEV ROUTING           │
               ├─────────────────────────────┬─────────────────────────────┤
               │ • Jupiter Swap API          │ • Uniswap / Aggregator API  │
               │ • Jito Anti-MEV Tip Bundle  │ • EIP-1559 Dynamic Priority │
               └───────────────────────────────────────────────────────────┘
```

---

## 📊 TUI Dashboard Layout (Ratatui 4-Tab Matrix)

* **Tab `[1] LIVE STREAM ⚡`**: Aliran real-time transaksi beli/jual target whale dan keputusan evaluasi gating bot.
* **Tab `[2] TARGET WALLETS 🎯`**: Tabel pantauan 100 trader teratas `fomo.family` (Rank, PnL 24h, Win Rate, Tier S/A/B, Status).
* **Tab `[3] SHADOW POSITIONS 💼`**: Posisi trading terbuka dengan tracking harga puncak (*peak*), PnL %, dan target whale yang diikuti.
* **Tab `[4] LATENCY & STATS 📊`**: Pemantau latensi sub-detik jaringan (Helius RPC, Alchemy Arbitrum/Base) dan log audit transaksi.

---

## ⚙️ Parameter Aktif (.env)

| Parameter | Nilai Default | Penjelasan Teknis |
| :--- | :--- | :--- |
| **`MAX_PRICE_DELTA_PCT`** | **`+5.0%`** | Batas maksimal lonjakan harga saat bot masuk dibanding harga beli whale. Jika $> +5.0\%$, order ditolak (*anti-pucuk*). |
| **`MIN_LIQUIDITY_USD`** | **`$30,000.0`** | Batas minimal likuiditas pool DEX untuk menghindari *rug pull* dan *excessive slippage*. |
| **`POSITION_SIZE_USD`** | **`$15.0`** | Ukuran posisi tetap (~0.11 SOL / 0.006 ETH) per order. |
| **`FOMO_SYNC_INTERVAL_SEC`**| **`60`** | Interval pembaruan otomatis daftar top leaderboard fomo. |
| **`STREAM_POLL_INTERVAL_MS`**| **`500`** | Latensi pengecekan transaksi target wallet (sub-detik). |
| **`SHADOW_EXIT_ENABLED`** | **`true`** | Menjual token seketika saat target whale mengeksekusi order *SELL*. |

---

## 🚀 Panduan Menjalankan Bot

### 1. Inisialisasi Konfigurasi
Salin template konfigurasi dan masukkan RPC key Anda:
```bash
cp .env.example .env
```

### 2. Jalankan Background Daemon (Engine)
Di terminal 1 (background):
```bash
make engine
```

### 3. Jalankan TUI Dashboard
Di terminal 2 (interaktif):
```bash
make start
```

### Shortcut Navigasi TUI:
* **`[1]`, `[2]`, `[3]`, `[4]`** atau **`[Tab]` / `[Shift+Tab]`**: Berpindah tab dashboard.
* **`[Q]` / `[Esc]`**: Keluar dari dashboard TUI.

---

## 🧪 Testing & Verifikasi
Jalankan seluruh rangkaian test suite integrasi:
```bash
make test
```
