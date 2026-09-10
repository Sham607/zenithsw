# 🚀 ZenithSW — Multi-Chain Smart Money Shadow Engine & TUI Dashboard

**ZenithSW** adalah bot *shadow copy-trading* multi-chain berlatensi sub-detik (<150 ms) yang melacak dan mengeksekusi *copy-trade* cerdas mengikuti pergerakan **Smart Money Terverifikasi On-Chain** lintas blockchain (**Solana SVM** dan **Robinhood / Arbitrum EVM**).

Dilengkapi dengan modul **Whale Cluster Detective** (pelacak dompet bayangan anak yang didanai whale), **Whale Gems Radar** (deteksi koin nano/micro-cap berkonsensus tinggi), serta **Strict Confluence Gating Matrix**.

---

## 🏛️ Arsitektur Sistem (Decoupled Engine & Smart Money Intelligence)

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 1. MULTI-SOURCE SMART MONEY INTELLIGENCE & WALLET FORENSICS                            │
│ • Whale Cluster Detective: Trace parent-to-child funding graph (Direct On-Chain RPC)   │
│ • Direct On-Chain DEX Graph Indexer: Reverse-lookup top profitable early buyers        │
│ • Nansen AI API v2: Profiler 7D Realized PnL Terbesar & Real-Time DEX Trades          │
│ • One-Shot Snapshot Ingestion: Imun dari restriksi API (100% on-chain live monitoring) │
│ • Realized PnL Priority: Mengutamakan cuan terealisasi nyata di atas win rate semu     │
└────────────────────────────────────────┬───────────────────────────────────────────────┘
                                         │ (SQLite WAL + Thread-Safe Database)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 2. SUB-SECOND MULTI-CHAIN STREAM INGESTION (<150 ms)                                   │
├────────────────────────────────────────┬───────────────────────────────────────────────┤
│           SOLANA SVM DRIVER            │            ROBINHOOD / ARBITRUM EVM           │
│ • Helius Dedicated WebSocket & RPC     │ • Alchemy Dedicated Node & Asset Transfers    │
│ • Parsing Enhanced Txs & TokenBalances │ • Full Bi-directional Stream (BUY & SELL)     │
│ • Zero Rate-Limit Paced Stream (5 RPS) │ • In-Memory Hash Cache Deduplication          │
└────────────────────────────────────────┴───────────────────────────────────────────────┘
                                         │ (Deteksi Sinyal Transaksi Smart Money)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 3. STRICT CONFLUENCE & RISK GATING MATRIX                                              │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ • Sweet Spot Market Cap Window: $50,000.0 s/d $20,000,000.0 (Asymmetric Upside Focus)  │
│ • Minimum Whale Order Size: >= $250.0 (Saring transaksi debu/spam/lottery degen)      │
│ • Max Price Impact Guard: <= 15.0% (Toleransi kedalaman pool koin nano/micro-cap)      │
│ • Max Price Entry Delta: <= +10.0% dari harga eksekusi whale                           │
│ • Anti-Stablecoin & Gas Blacklist: Blokir mutlak USDC, USDT, WETH, ETH, SOL, ARB, dll. │
│ • Same-Whale Deduplication: Tolak akumulasi berulang dari 1 whale yang sama           │
│ • Multi-Whale DCA Consolidation: Gabungkan entri dari whale berbeda via VWAP (1 pos)   │
└────────────────────────────────────────┬───────────────────────────────────────────────┘
                                         │ (Lolos Evaluasi Confluence)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 4. PURE PROPORTIONAL WHALE MIRROR EXECUTION & RADAR                                    │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ • Entry: Buka posisi shadow otomatis saat Whale Beli ($5.0 / calibrated position size) │
│ • Proportional Partial Exit: Jual X% posisi jika Whale jual X% saldo koinnya           │
│ • Full Exit Dump: Tutup 100% posisi saat Whale melepas seluruh koinnya (>=95%)         │
│ • Auto Take-Profit Moonbag (+100% -> TP 50% & Lock Floor +25%) + Emergency SL (-20%)   │
│ • Atomic State: Sync real-time via UNIX Domain Socket (/tmp/zenithsw.sock)             │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 🧬 Whale Cluster Detective (Pelacak Dompet Terafiliasi)

Whale Tier-S sering kali memecah modal ke beberapa *burner/shadow wallet* untuk membeli koin nano-cap sebelum koin tersebut dipromosikan ke publik. 

Modul **`WhaleClusterDetective`** memindai histori transfer modal awal (*parent-to-child initial funding*) via RPC Alchemy & Helius:
* Mendeteksi transfer keluar $\ge 0.05\text{ ETH}$ atau $\ge \$500\text{ USDC}$ dari Top Whale ke alamat baru.
* Otomatis menginjeksi dompet anak ke tabel `wallets` dengan badge **`[DETECTIVE 🕵️]`** dan handle terafiliasi (contoh: `@unipcs_sub_53a8`).
* Memungkinkan bot mengeksekusi *Shadow Buy* di blok pertama ketika dompet rahasia ini mulai memborong koin nano-cap.

---

## 💎 Whale Gems Radar & Portfolio Harvester (Tab `[3]`)

Panel intelijen multi-whale yang memetakan konsensus kepemilikan koin dari Top Smart Wallets:

1. **Hierarki Kapitalisasi Pasar (Asymmetric Multiplier Focus):**
   * **`NANO_CAP 🚀🚀`** ($< \$1\text{M}$): Prioritas #1 (Potensi upside $20\times - 100\times+$).
   * **`MICRO_CAP 🚀`** ($\$1\text{M} - \$3\text{M}$): Prioritas #2 (Potensi upside $10\times - 50\times$).
   * **`LOW_CAP 💎`** ($\$3\text{M} - \$15\text{M}$): Prioritas #3 (Potensi upside $3\times - 10\times$).
   * **`SMALL_CAP`** ($\$15\text{M} - \$50\text{M}$) & **`MID_LARGE`** ($> \$50\text{M}$).
2. **Kondisi Pasar & Indikator Teknikal:**
   * **`ACCUMULATION_BOTTOM 🟢 (Ready Buy)`**: 24h Change $-30\%$ s/d $+25\%$, Pool Liquidity $\ge \$15\text{k}$.
   * **`LOCAL_TOP 🔴 (Wait Pullback)`**: 24h Change $> +100\%$ atau 6h Change $> +35\%$.
   * **`OVERSOLD_RECOVERY 🟡`**: 24h Change $< -30\%$.

---

## 📊 TUI Dashboard Layout (Ratatui 5-Tab Matrix)

* **Tab `[1] LIVE STREAM ⚡`**: Aliran real-time swap beli/jual target whale (termasuk deteksi `SOLD PARTIAL 🟡` vs `SOLD FULL DUMP 🔴`), **Smart Contract Mint Address**, dan keputusan evaluasi gating bot.
* **Tab `[2] TARGET WALLETS 🎯`**: Tabel pemantauan dompet target dengan badge sumber terverifikasi:
  * `[DETECTIVE 🕵️]`: Dompet bayangan anak yang terdeteksi didanai langsung oleh Top Whale Tier-S.
  * `[NANSEN 🧠]`: Smart Money institusional dengan 7D Realized PnL terverifikasi.
  * `[ONCHAIN 🌐]`: Whale aktif hasil temuan mandiri On-Chain DEX Graph Indexer.
  * `[FOMO ⚡]`: Top trader FOMO hasil verifikasi on-chain.
* **Tab `[3] WHALE GEMS RADAR 💎`**: Tabel konsensus multi-whale, klasifikasi MCap (Nano/Micro), pool liquidity, harga live, dan status akumulasi.
* **Tab `[4] SHADOW POSITIONS 💼`**: Posisi trading aktif dengan live floating PnL, smart contract mint, sisa porsi bag (`Bag Remaining %`), dan harga live.
* **Tab `[5] LATENCY & STATS 📊`**: Latensi RPC Helius & Alchemy, parameter aktif, serta audit log riwayat eksekusi lengkap beserta smart contract token.

---

## ⚙️ Parameter Konfigurasi Aktif

| Parameter | Nilai Aktif | Penjelasan Teknis |
| :--- | :--- | :--- |
| **`MIN_MARKET_CAP_USD`** | **`$50,000.0`** | Batas bawah MCap (menolak token mati / illiquid pump.fun dead pools). |
| **`MAX_MARKET_CAP_USD`** | **`$20,000,000.0`** | Batas atas MCap (fokus pada koin mikro dengan potensi asymmetric upside). |
| **`WRAPPED_ASSET_BLACKLIST`** | **`cbBTC`, `WBTC`, `WETH` + exact contracts** | Mencegah BTC/gas wrapper masuk sebagai posisi copy-trade. |
| **`MARKET_CAP_FAIL_CLOSED`** | **`true` untuk EVM** | Market cap hilang/tidak valid ditolak dengan `REJECTED_MARKET_CAP_UNVERIFIED`. |
| **`MIN_WHALE_BUY_USD`** | **`$250.0`** | Batas minimal order whale (menyaring transaksi debu/spam). |
| **`MAX_PRICE_IMPACT_PCT`** | **`15.0%`** | Toleransi slippage kedalaman pool DEX nano/micro-cap. |
| **`MAX_PRICE_DELTA_PCT`** | **`+10.0%`** | Batas toleransi lonjakan harga masuk bot vs harga entry whale (*anti-pucuk*). |
| **`MIN_LIQUIDITY_USD`** | **`$5,000.0`** | Batas minimal likuiditas pool DEX efektif. |
| **`POSITION_SIZE_USD`** | **`$5.0`** | Ukuran posisi trading per order shadow copy-trade. |
| **`SHADOW_EXIT_ENABLED`** | **`true`** | Mode mirror murni (menjual parsial/penuh mengikuti persentase dump whale). |

---

## 🛠️ Skrip Harvester & Forensik (`scripts/`)

* **`whale_cluster_detective.py`**: Skrip forensik pelacak jejak pendanaan on-chain (parent-child transfer trace) untuk Top Tier-S Whales.
* **`one_shot_fomo_harvester.py`**: Skrip ekstraksi snapshot 100 top traders (7D, 30D, All-Time) ke SQLite tanpa memicu restriksi API.
* **`leaderboard_wallet_harvester.py`**: One-shot harvester Solana Tracker dan OKX Smart Money leaderboard; default **dry-run**, validasi address chain-aware, deduplikasi, dan ingest eksplisit dengan `--execute`. Credential hanya dari environment (`SOLANA_TRACKER_API_KEY`, `OKX_API_KEY`, `OKX_SECRET_KEY`, `OKX_PASSPHRASE`).
* Provider discovery baru tidak menggunakan Alchemy/Birdeye; live monitoring tetap mengikuti konfigurasi engine yang tersedia.

Contoh aman (default dry-run):
```bash
export SOLANA_TRACKER_API_KEY='[REDACTED]'
export OKX_API_KEY='[REDACTED]'
export OKX_SECRET_KEY='[REDACTED]'
export OKX_PASSPHRASE='[REDACTED]'
python3 scripts/leaderboard_wallet_harvester.py --limit 100
# Tambahkan --execute hanya setelah meninjau output kandidat.
```

OKX dipanggil per chain yang didukung adapter (Solana, Ethereum, Base; Arbitrum dapat ditolak API bila parameter tidak tersedia). Adapter tidak melakukan polling kontinu.

* **`nansen_7d_smart_money_harvester.py`**: Memindai Top Smart Money 7D PnL tertinggi via Nansen API v2.
* **`screen_fomo_bottom_vs_top.py`**: Pemindaian koin berklaster modal besar pada whale portofolio.

---

## 🚀 Panduan Menjalankan Bot

### 1. Menjalankan Dashboard TUI (All-in-One)
Perintah ini otomatis menyalakan background engine daemon dan membuka antarmuka TUI:
```bash
make start
```

### 2. Mengelola Engine Daemon
* **Menyalakan Daemon di Background:** `make engine`
* **Menghentikan Daemon:** `make stop`
* **Restart Daemon & Clear Socket:** `make restart`
* **Menjalankan Test Suite:** `make test`

### 3. Shortcut Navigasi TUI:
* **`[1]` / `[2]` / `[3]` / `[4]` / `[5]`**: Berpindah tab dashboard secara instan.
* **`[Tab]` / `[Shift+Tab]`**: Rotasi tab berikutnya / sebelumnya.
* **`[Q]` / `[Esc]`**: Keluar dari dashboard visual (engine tetap jalan di background).

## 🔐 Kebijakan Credential Provider

API key/secret/passphrase tidak pernah ditulis ke source code, SQLite, log, README, atau commit. Credential yang pernah dibagikan sebaiknya dirotasi setelah pengujian.
