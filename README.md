# 🚀 ZenithSW — Multi-Chain Smart Money Shadow Engine & TUI Dashboard

**ZenithSW** adalah bot *shadow trading* multi-chain berlatensi sub-detik (<150 ms) yang melacak dan mengeksekusi *copy-trade* cerdas mengikuti pergerakan **Smart Money Terverifikasi On-Chain** dari **Nansen AI (7D Realized PnL)**, **Direct On-Chain DEX Graph Indexer**, serta top trader **FOMO.family** lintas blockchain (**Solana SVM** dan **Robinhood / Arbitrum EVM**).

---

## 🏛️ Arsitektur Sistem (Decoupled Engine & Smart Money Aggregator)

```text
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 1. MULTI-SOURCE SMART MONEY HARVESTING & PROFILING                                    │
│ • Nansen AI API v2: Profiler 7D Realized PnL Terbesar & Real-Time DEX Trades          │
│ • Direct On-Chain DEX Indexer: Reverse-Lookup Early Buyers pada Exploding Pools        │
│ • FOMO.family Verified Scraper: Embedded Privy Wallet Resolver & Deep On-Chain Audit   │
│ • Realized PnL Priority Ranking: Mengutamakan Realized Profit Nyata di Atas Win Rate   │
└────────────────────────────────────────┬───────────────────────────────────────────────┘
                                         │ (SQLite WAL + In-Memory Caching)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 2. SUB-SECOND MULTI-CHAIN STREAM INGESTION (<150 ms)                                   │
├────────────────────────────────────────┬───────────────────────────────────────────────┤
│           SOLANA SVM DRIVER            │            ROBINHOOD / ARBITRUM EVM           │
│ • Helius Dedicated WebSocket & RPC     │ • Alchemy Dedicated Node & Asset Transfers    │
│ • Parsing Enhanced Txs & TokenBalances │ • Full Bi-directional Stream (BUY & SELL)     │
│ • Staggered Polling (Anti-Throttling)  │ • In-Memory Hash Cache Deduplication          │
└────────────────────────────────────────┴───────────────────────────────────────────────┘
                                         │ (Deteksi Sinyal Swap On-Chain)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 3. CONFLUENCE & RISK GATING MATRIX                                                     │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ • Anti-Stablecoin & Base Asset Blacklist: Blokir USDC, USDT, WETH, ARB, SOL, dll.      │
│ • Exact DexScreener Pair Matching: Mencegah salah deteksi Base Token vs Quote Pool     │
│ • Multi-Pool Liquidity Aggregation: Agregasi likuiditas Raydium, Meteora, PumpFun      │
│ • Hard Gating Anti-Pucuk: Max Price Delta <= +10.0% dari harga entry whale             │
│ • Liquidity Safety Floor: Effective Liquidity >= $5,000.0 (Tolak Pool Debu)          │
│ • Multi-Wallet Confluence: Tier-S/A Eksekusi Langsung; Tier-B Minimal 2 Whale Masuk    │
└────────────────────────────────────────┬───────────────────────────────────────────────┘
                                         │ (Lolos Evaluasi)
                                         ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│ 4. PURE PROPORTIONAL WHALE MIRROR EXECUTION & BAG TRACKING                             │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ • Entry: Buka posisi shadow otomatis saat Whale Beli ($5.0 fixed size)                 │
│ • Proportional Partial Exit: Jual X% posisi jika Whale jual X% saldo koinnya           │
│ • Full Exit Dump: Tutup 100% posisi saat Whale melepas seluruh koinnya (>=95%)         │
│ • Atomic State: Sync real-time via UNIX Domain Socket (/tmp/zenithsw.sock)             │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

---

## 📊 TUI Dashboard Layout (Ratatui 4-Tab Matrix)

* **Tab `[1] LIVE STREAM ⚡`**: Aliran real-time transaksi beli/jual target whale (termasuk deteksi `SOLD PARTIAL 🟡` vs `SOLD FULL DUMP 🔴`), **Smart Contract / Mint Address Koin**, dan keputusan evaluasi gating bot.
* **Tab `[2] TARGET WALLETS 🎯`**: Tabel pemantauan dompet target dengan penanda sumber yang akurat:
  * `[NANSEN 🧠]`: Smart Money institusional dengan 7D Realized PnL terverifikasi.
  * `[ONCHAIN 🌐]`: Whale aktif hasil temuan mandiri On-Chain DEX Graph Indexer.
  * `[FOMO ⚡]`: Top trader FOMO yang telah diaudit saldo kas & kepemilikan token on-chainnya.
  *(Diurutkan otomatis dari Realized PnL tertinggi).*
* **Tab `[3] SHADOW POSITIONS 💼`**: Posisi trading aktif dengan live floating PnL, smart contract mint, sisa porsi bag (`Bag Remaining %`), dan harga live.
* **Tab `[4] LATENCY & STATS 📊`**: Latensi RPC Helius & Alchemy, parameter aktif, serta audit log riwayat eksekusi lengkap beserta smart contract token.

---

## ⚙️ Parameter Konfigurasi Aktif (`.env`)

| Parameter | Nilai Aktif | Penjelasan Teknis |
| :--- | :--- | :--- |
| **`MIN_LIQUIDITY_USD`** | **`$5,000.0`** | Batas minimal likuiditas pool DEX efektif (agregasi multi-pool / Pump.fun virtual liq). |
| **`MAX_PRICE_DELTA_PCT`** | **`+10.0%`** | Batas toleransi lonjakan harga masuk bot vs harga entry whale (*anti-pucuk*). |
| **`POSITION_SIZE_USD`** | **`$5.0`** | Ukuran posisi trading per order shadow copy-trade. |
| **`NANSEN_API_KEY`** | `nsn_***` | API Key Nansen v2 untuk harvester Smart Money 7D PnL & DEX trades. |
| **`FOMO_AUTH_TOKEN`** | `Bearer ***` | Privy JWT Session Token untuk sinkronisasi trader FOMO. |
| **`HELIUS_RPC_URL`** | `https://mainnet.helius-rpc.com/...` | Dedicated RPC Solana SVM. |
| **`ARBITRUM_RPC_URL`** | `https://arb-mainnet.g.alchemy.com/...` | Dedicated Alchemy RPC Arbitrum / Robinhood EVM. |
| **`SHADOW_EXIT_ENABLED`** | **`true`** | Mode mirror murni (menjual parsial/penuh mengikuti persentase dump whale). |

---

## 🛡️ Aturan Eksekusi, Filter & Safeguards

1. **Realized PnL Priority (Bukan Win Rate Semu):**
   * Smart money diurutkan dan dikualifikasikan berdasarkan total keuntungan nyata (`pnl_24h_usd` / `7D Realized PnL`):
     * **Tier-S (👑):** Realized PnL $\ge \$50,000$ atau kas on-chain $\ge 10\text{ SOL} / 2\text{ ETH}$.
     * **Tier-A (⚡):** Realized PnL $\ge \$10,000$ atau kas on-chain $\ge 2\text{ SOL} / 0.5\text{ ETH}$.
     * **Tier-B (🟡):** Watchlist pasif — hanya dieksekusi jika terjadi konfluensi $\ge 2$ whale membeli koin yang sama dalam 5 menit.
2. **Anti-Stablecoin & Quote Asset Blacklist:**
   * Bot memiliki proteksi mutlak yang secara otomatis menolak pembelian aset gas atau stablecoin: `USDC`, `USDT`, `USD₮0`, `DAI`, `USDe`, `USDS`, `PYUSD`, `BUSD`, `EURC`, `WETH`, `ETH`, `WSOL`, `SOL`, `WBTC`, `ARB`, dll.
3. **Exact DexScreener Pair Matching:**
   * Mencegah kekeliruan pembacaan simbol (seperti pool pancake USDT terbaca WERC). Bot memverifikasi kontrak berada di sisi `baseToken`.
4. **Multi-Pool Liquidity Aggregation & Bonding Curve:**
   * Memindai seluruh pool aktif koin tersebut dan menjumlahkan likuiditas efektif di seluruh DEX (Raydium, Orca, Meteora, PumpSwap).
   * Menghitung virtual liquidity untuk token yang masih berada di bonding curve Pump.fun ($\approx 50\%\text{ Market Cap}$).
5. **Pure Whale Trigger & Bag Tracking:**
   * Bot tidak menggunakan SL statis. Posisi ditutup secara proporsional mengikuti transaksi whale:
     $$\text{Sell Ratio (\%)} = \left( \frac{\text{Token Amount Sold}}{\text{Whale Balance Before}} \right) \times 100\%$$

---

## 🛠️ Skrip Harvester & Forensik (`scripts/`)

* **`nansen_7d_smart_money_harvester.py`**: Memindai Top Smart Money 7D PnL tertinggi via Nansen API v2, melakukan audit RPC on-chain, dan menginjeksinya ke database target wallet.
* **`screen_and_verify_fomo_top150.py`**: Mengambil 100 trader FOMO lintas timeframe dan memverifikasi kepemilikan token & transaksi on-chain.
* **`audit_fomo_wallets.py`**: Skrip forensik RPC node untuk mengecek saldo kas dan histori tanda tangan transaksi dompet.

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
* **Menjalankan Test Suite (7/7 Pass):** `make test`

### 3. Shortcut Navigasi TUI:
* **`[1]` / `[2]` / `[3]` / `[4]`**: Berpindah tab dashboard secara instan.
* **`[Tab]` / `[Shift+Tab]`**: Rotasi tab berikutnya / sebelumnya.
* **`[Q]` / `[Esc]`**: Keluar dari dashboard visual (engine tetap jalan di background).
