# ZenithSW — Milestone Ledger

> Artefak status durable untuk melanjutkan pekerjaan lintas sesi.
> **Aturan:** checkbox hanya boleh dicentang jika bukti executable/RPC/reference tercatat di bagian milestone. Test fixture atau build tidak sama dengan kesiapan live trading.

## Status keselamatan global

- [x] Engine tetap `OFF` selama implementasi dan verifikasi terakhir.
- [x] Tidak ada signing live pada pekerjaan parser/feed terakhir.
- [x] Tidak ada transaksi live yang dikirim pada pekerjaan parser/feed terakhir.
- [x] Tidak ada perubahan `.env`/config untuk mengaktifkan live trading.
- [ ] Live canary belum diotorisasi dan belum dijalankan.

## Bukti validasi terakhir

Terakhir diverifikasi dengan toolchain eksplisit:

```text
RUSTUP_TOOLCHAIN=1.94.1-x86_64-unknown-linux-gnu
/home/node/.cargo/bin/cargo
```

Hasil terakhir (commit `7b9234b`, push `origin/main` OK, working tree bersih):

- `mise exec rust@1.94.1 -- cargo test --locked`: **76 lulus, 0 gagal**
- `cargo fmt --check`, `cargo check`, `cargo clippy -D warnings`, `cargo build --release --locked`, `git diff --check`: **lulus**
- Perubahan terakhir: fail-closed config, `execution_intents` + state machine + unique constraints, Robinhood receipt hash validation, Solana serialized-tx blockhash parser + tests, `ActivitySync`/`SignalSync` snapshot IPC, runtime harness Engine→IPC→DB, README refresh, `forgeguard gate` dicopot dari repo.

Warning compile yang tercatat bukan error build, antara lain unused variable/field pada modul existing. Warning baru pada reconnect feed perlu cleanup terpisah dan tidak boleh disembunyikan.

## Milestone A — Nitro parser dan feed

### A1. Signed transaction kind 4

- [x] RLP parser dasar dan validasi trailing/truncated payload.
- [x] Decode legacy/EIP-1559 structure.
- [x] Decode `chain_id`, `nonce`, `gas`, `to`, `value`, `data`.
- [x] Fixture signed EIP-1559.
- [x] Negative test malformed/empty payload.

Bukti: termasuk dalam full test suite terakhir, seluruh test lulus.

### A2. Unsigned EOA transaction kind 0

- [x] Decode fixed-width Nitro fields.
- [x] Validasi `u64`/`u128` overflow.
- [x] Validasi 12 high bytes alamat harus nol.
- [x] Fixture valid.
- [x] Test truncation dan overflow.

Bukti: test `decodes_unsigned_kind_zero_fixture`, negative tests, full suite lulus.

### A3. Contract transaction kind 1

- [x] Decode `gas_limit`, `max_fee_per_gas`, `to`, `value`, calldata.
- [x] Fail-closed untuk truncation, overflow, high address bytes, wrong kind.
- [x] Fixture valid dan negative tests.

Bukti: test `decodes_contract_kind_one_fixture` dan negative tests, full suite lulus.

### A4. Batch kind 3

- [x] Decode framing panjang `uint64` big-endian + payload.
- [x] Dukungan nested batch.
- [x] Batas nesting 16.
- [x] Batas leaf message 4.096.
- [x] Reject empty/truncated/overflow frame.
- [x] Fixture nested batch.

Bukti: test `decodes_nested_batch_kind_three_fixture`, `rejects_truncated_batch_frame`, `rejects_batch_nesting_over_depth_limit`; full suite lulus.

### A5. SignedCompressedTx/Brotli kind 7

- [ ] Temukan dan pin kontrak format production yang lengkap.
- [ ] Tentukan framing compressed payload dan hasil decode.
- [ ] Tambahkan decompression output limit dan anti-bomb guard.
- [ ] Tambahkan fixture resmi/interoperabel.
- [ ] Tambahkan negative tests.
- [ ] Integrasikan dengan parser signed transaction setelah format terbukti.

Status: **BLOCKED / belum selesai**.

Referensi yang sudah diperiksa menyatakan reference implementation Nitro masih menandai `L2MessageKind_SignedCompressedTx` sebagai unimplemented. Brotli pada dokumentasi gas/batch compression bukan bukti format kind 7.

### A6. Sequencer WebSocket feed

- [x] Bounded read-only reader untuk `ws://`/`wss://`.
- [x] Parse envelope JSON `version: 1`.
- [x] Text dan Binary UTF-8 frame.
- [x] Ping/Pong dan Close handling.
- [x] Dedup berdasarkan `sequenceNumber` dalam satu read session.
- [x] Reconnect/backoff dasar dan batas consecutive disconnect.
- [x] Reject URL non-WebSocket dan malformed input.
- [ ] Integrasikan reader ke production engine/dispatcher.
- [ ] Persist cursor/sequence untuk resume setelah restart.
- [ ] Gap detection dan controlled backfill.
- [ ] Runtime test terhadap endpoint/relay nyata.

Status: **bounded reader selesai; integrasi produksi belum selesai**.

Referensi:

- Public feed Arbitrum One: `wss://arb1-feed.arbitrum.io/feed`
- Local feed relay: WebSocket port `9642`
- Format: Arbitrum docs “How to read the sequencer feed”

## Milestone B — Robinhood execution

- [x] Chain ID gate `4663` / `0x1237`.
- [x] Sebagian route discovery dan dry-run quote.
- [x] Calldata construction untuk jalur yang sudah dibuktikan.
- [x] `eth_call` dan `eth_estimateGas` pada dry-run path.
- [x] EIP-1559 signing primitive.
- [ ] Permit2 domain/nonce/expiration/spender validation untuk EIP-712 signing.
- [x] Dry-run ERC-20 allowance preflight terhadap spender route yang benar.
- [x] Read-only ERC-20 allowance owner → Permit2 (V4 flow).
- [x] Read-only Permit2 allowance owner/token → Universal Router dengan amount/expiration/nonce validation.
- [ ] Durable intent sebelum signing.

Bukti Permit2 terbaru: `src/chains/robinhood.rs` menambah `permit2_allowance()` dengan selector resmi `0xcc53287f` dan `Permit2AllowanceSnapshot`; `src/execution/evm_executor.rs` mewajibkan dua lapisan allowance untuk V4 non-native pada dry-run path. Full suite **39 lulus, 0 gagal**, release build lulus, `git diff --check` bersih.
- [ ] Live `eth_sendRawTransaction` gate terpisah.
- [x] Read-only Permit2 allowance state `(amount, expiration, nonce)`.
- [x] Bounded receipt polling; timeout tetap `UNKNOWN`.
- [x] Struktur/domain/expiry validator Permit2 EIP-712 tanpa signing.
- [ ] Permit2 signature generation dan authorization terpisah.
- [ ] Approval transaction owner → Permit2.
- [x] Receipt success + Swap event presence validator.
- [x] BUY balance delta reconciliation validator berbasis integer.
- [x] Fill price integer `output/input` berskala 1e18.
- [x] Token balance read pada block tag tertentu.
- [ ] Token/native balance before/after RPC wiring ke live receipt.
- [ ] Actual fill amount dan fill price terhubung ke live receipt.

Bukti terbaru: `src/chains/robinhood.rs` menambah `EvmSwapReconciliation::validate_buy()`, `fill_price_x18()`, dan `token_balance_of_at()`. Validator mewajibkan input balance turun tepat sebesar actual input, output balance naik tepat sebesar actual output, dan output memenuhi minimum. Full suite **44 lulus, 0 gagal**, release build lulus, `git diff --check` bersih.
- [ ] Actual fill amount dan fill price dari receipt/balance RPC.
- [ ] Persist posisi `OPEN` hanya setelah semua reconciliation lulus.
- [ ] BUY live canary terotorisasi.
- [ ] SELL dan partial SELL end-to-end.
- [ ] Persist `PARTIAL_EXIT`/`CLOSED` setelah reconciliation.

Bukti terbaru: `src/execution/evm_executor.rs` menambahkan dry-run allowance preflight dengan spender route aktual (`SwapRouter02` untuk V3, `Permit2` untuk V4 non-native); tidak ada approval/signing/broadcast yang dilakukan. Full suite **39 lulus, 0 gagal**, release build lulus, `git diff --check` bersih.

Status: **belum live-ready**. Tidak ada bukti tx hash + receipt + event + balance reconciliation.

## Milestone C — Solana execution/recovery

- [x] RPC pool dan single broadcast foundation.
- [x] BUY intent foundation.
- [x] Startup physical token balance recovery.
- [ ] Recovery posisi `OPEN`/`PENDING`.
- [ ] Blockhash expiry handling.
- [ ] Signature status multi-RPC.
- [x] Durable SELL intent.
- [x] SELL/partial exit reconciliation.
- [ ] Auto-TP, stop-loss, whale mirror exit durable lifecycle.

Status: **belum live-ready penuh**.

## Milestone D — Scanner dan data

### D1. Global Robinhood scanner

- [x] Bounded `eth_getLogs` tail scanner.
- [ ] Persistent block cursor.
- [ ] Restart resume.
- [ ] Gap detection/recovery.
- [ ] Controlled backfill.
- [ ] Dedup identity `(tx_hash, log_index)`.
- [ ] Receipt/event classification production.

Status: **sebagian selesai**.

### D2. Shared RPC abstraction

- [ ] Satu abstraction pool untuk stream.
- [ ] Satu abstraction pool untuk indexer.
- [ ] Satu abstraction pool untuk cluster detective.
- [ ] Satu abstraction pool untuk quote/executor.
- [ ] Satu abstraction pool untuk receipt poller/reconciliation.
- [ ] Health metrics endpoint/latency/cooldown seragam.

Status: **belum selesai; implementasi masih terpisah lintas modul/chain**.

### Canary preflight — 14 Sep 2026 UTC

- Robinhood target `0x1cdb289befdfac8af945a288bcdccc382cb34d32` verified on chain `4663`.
- Factory `getLaunchedToken(token)` verified `pairToken = 0x5fc5360d0400a0fd4f2af552add042d716f1d168` (USDG), curve `0xc3588da017090047d1a4e8bb0669b56c829f7140`.
- User-provided successful transaction verified: `0x4651a4754ad0ab0b5c858103107f57b3bf6abe7880a06c7e1bcf77d5cf4c6105`, status `0x1`, UniversalRouter/Pons V2 route.
- Requested canary `ETH → XL` is incompatible with this XL launch record; valid curve input is USDG. No Robinhood canary broadcast performed.
- Solana target mint and Jupiter quote were verified earlier, but signing/simulation was not completed. No Solana broadcast performed.
- Engine remained OFF; total canary broadcasts: `0`.
- Robinhood preflight: chain `4663`, wallet `0xf9afd772f0cb93ebda110625916cb4613d36c673`, balance `638810631942295 wei`; bounded executor dry-run with `300000000000000 wei` stopped at the existing `0.002 ETH` reserve guard. No Robinhood quote/simulation/signing/broadcast occurred through the executor.
- User-provided Rabby reference verified route `ETH → USDG → XL` through UniversalRouter/PoolManager/Pons hook. V4 multi-hop encoder compiles; fresh deployment execution call still requires valid balance and route-specific verification.
- Solana preflight: wallet `bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk`, target mint exists, Jupiter quote/build succeeded for about `9.825M lamports`, and `simulateTransaction` returned `err=null`, fee `5970`, units `54515`. No signing/broadcast occurred.
- Jupiter `/price/v2` endpoint tested for SOL/USD returned HTTP 404; no hardcoded fallback restored. Nominal `$1` conversion remains based on the successful SOL→USDC quote, not the failed price endpoint.

### Solana Live Canary Execution — 02 Okt 2026 UTC

- Wallet: `bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk`
- Saldo Awal: `0.359750 SOL` (~$50 USD)
- **Phase 1 BUY Canary**:
  - Intent ID: `SOL-CANARY-BUY-1790913585708`
  - Nominal: `0.007 SOL` -> USDC (`EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v`)
  - Route: Jupiter Swap API v1 (`JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4`)
  - Tx Signature: `2VLUBR4EPp5V3huERKvD7quyLWgd4GX4Gaz5BEh74qeGc6j5WPAHYZ8TfByHB3zgom6QvWhg7KqU1WZqdN2mTB53`
  - Status On-chain: `CONFIRMED` (slot 452496007, compute units consumed: 122,930)
  - Hasil Fisik: `0.848364 USDC` (`848364` raw units) diterima di ATA `6C11VozCS2T2os9jGmQY1ndXMKSwFgA3iWKrNkpRBxLo`
  - Status Intent DB: `RECONCILED`
- **Phase 2 SELL Canary**:
  - Intent ID: `SOL-CANARY-SELL-1790913735967`
  - Nominal: `848364` micro-USDC (100% exit) -> SOL
  - Route: Jupiter Swap API v1
  - Tx Signature: `3m8pptkvrH34eU7SVwYquCE372c6AVjRuzjvPtriWKPvonrrm6wuQcYKkq9844CmLQCoRBK9n232sETVKzmgTm3Y`
  - Status On-chain: `CONFIRMED`
  - Status Intent DB: `RECONCILED`
- **Phase 3 Post-Trade Settlement**:
  - Sisa USDC: `0.0000`
  - Saldo Akhir SOL: `0.351157 SOL` (Gas buffer guard `>0.04 SOL` terpenuhi)
  - Status Verifikasi: Jalur settlement live Solana mainnet (signing lokal, failover RPC, durable intents SQLite, on-chain confirmation, dan post-trade reconciliation) 100% sukses terbukti di mainnet.

### Solana Live Engine Activation — 02 Okt 2026 UTC

- Database Transisi: `data/trading_state_live.db` (Opsi A — database mandiri bersih, riwayat virtual terisolasi).
- Mode Eksekusi: `DRY_RUN=false` aktif di daemon (PID `1529452`).
- Parameter:
  - `POSITION_SIZE_USD=10.0`
  - `STOP_LOSS_PCT=35.0`
  - `TAKE_PROFIT_PCT=100.0`
  - `COPYTRADE_ALLOWED_WHALES="solstice,detweiler,lunarfang"`
- Proteksi RPC: `SolanaStreamListener` terkunci ke RPC publik nol-kuota; endpoint Helius dedikasi eksekusi transaksi.
- Rekonsiliasi Startup: Filter saldo token yatim (orphan) diperketat untuk mengabaikan debu/rug koin bernilai < $1.00 USD. Posisi aktif awal: 0. Saldo SOL: `0.3581 SOL`.

### D3. Robinhood market data

- [x] Chainlink `AggregatorV3Interface.latestRoundData()` read.
- [x] Feed decimals read tanpa hardcode.
- [x] Positive answer validation.
- [x] `updatedAt` validation.
- [x] Staleness/heartbeat validation.
- [x] Token `oraclePaused()` read dan boolean validation.
- [x] Token `uiMultiplier()` read dan non-zero validation.
- [x] Sequencer uptime validator read-only.
- [x] `tradingCapabilities` dan official `/rhj/assets` metadata adapter read-only.
- [ ] Native/USD conversion dari chain target.
- [ ] V3/V4 pool state dan liquidity validation penuh.

Bukti terbaru: `src/chains/robinhood.rs`; `ROBINHOOD_ASSETS_API`; parser chain deployment `4663`, multiplier, status, dan trading capabilities; test `parses_official_asset_metadata_for_robinhood_deployment` serta `rejects_asset_metadata_without_robinhood_deployment`; full suite **39 lulus, 0 gagal**, release build lulus, `git diff --check` bersih.

Status: **sebagian selesai; harga Robinhood belum boleh dianggap setara dengan Solana**.

### D4. Candidate qualification/promotion

- [x] Candidate DB foundation.
- [ ] Independent transaction audit.
- [ ] Realized PnL.
- [ ] Win rate.
- [ ] Holding time.
- [ ] Sniper/MEV filter.
- [ ] Promotion manifest.
- [ ] Explicit promotion ke `wallet_registry.db`.
- [ ] Auto-promotion: **tetap tidak diaktifkan tanpa otorisasi eksplisit**.

Status: **audit/promotion worker belum selesai**.

## Milestone E — Reliability, observability, cleanup

### E1. Failure-path tests

- [ ] RPC rotation.
- [ ] 429/5xx/timeout.
- [ ] Broadcast timeout.
- [ ] Duplicate broadcast.
- [ ] Nonce collision.
- [ ] Stale quote.
- [ ] Expired blockhash.
- [ ] Reorg/replacement.
- [ ] Missing Swap event.
- [ ] Balance mismatch.
- [ ] Restart pending intent.
- [ ] Global scanner gap.

Status: **belum lengkap**.

### E2. TUI observability

- [ ] Active RPC endpoint.
- [ ] Request/success/failure count.
- [ ] 429/5xx count.
- [ ] Actual latency.
- [ ] Last block/slot.
- [ ] Wallet yang sedang dipoll.
- [ ] Activity per chain.
- [ ] Pending intents.

Status: **belum lengkap**.

### E3. Source cleanup

- [ ] Resolve unused fields/variables yang relevan.
- [ ] Hapus legacy commented Alchemy code setelah memastikan tidak dipakai.
- [ ] Rapikan warning reconnect feed.
- [ ] Review seluruh diff untuk perubahan unrelated.

Status: **belum selesai**.

### E4. ForgeGuard

- [ ] Inisialisasi repository ForgeGuard dengan izin eksplisit.
- [ ] Register task milestone.
- [ ] Jalankan `forgeguard gate --changed --output compact`.
- [ ] Simpan evidence gate.

Status: **blocked by missing `.forgeguard/config.toml`; jangan menjalankan `forgeguard init` otomatis**.

## Prioritas sesi berikutnya (update 15 Sep 2026, hasil eksekusi WIB)

### Prioritas 1 — Full snapshot E2E Engine → IPC → TUI

- [x] Harness menjalankan proses engine nyata dengan DB/socket terisolasi dan `DRY_RUN=true`.
- [x] Verifikasi heartbeat nyata, reconnect sebelum heartbeat, dan mode live tetap `false`.
- [x] Payload diverifikasi untuk `WalletSync`, `TrackedTokensUpdate`, `PositionUpdate`, `TradeSync`, `ActivitySync`, dan `SignalSync`.
- [x] Seed Solana/Base membuktikan chain filtering di TUI handler; Tier-B tidak masuk `WalletSync`.
- [x] Snapshot zero-row replacement dan stale/error behavior tetap diuji oleh handler/unit contract yang relevan.

Bukti aktual: `mise exec rust@1.94.1 -- cargo test --test runtime_harness_test --locked` → **2 lulus, 0 gagal**; `tests/tui_handler_test.rs` → **4 lulus, 0 gagal**. Engine test selalu cleanup; engine produksi tetap OFF.

Status: **DONE — tested/runtime-verified; bukan bukti on-chain live.**

### Prioritas 2 — PumpPortal expiry fail-closed

- [x] PumpPortal BUY tidak lagi signing/broadcast ketika `lastValidBlockHeight` authoritative tidak tersedia.
- [x] Metadata blockhash transaksi unsigned tetap dipersist sebelum intent ditandai `FAILED`.

Bukti aktual: `src/execution/solana_executor.rs`; `mise exec rust@1.94.1 -- cargo check --locked` → lulus; full suite → lulus.

Status: **DONE — code/test verified; belum ada broadcast PumpPortal.**

### Prioritas 3 — Timeout/replacement reconciliation

- [x] Test state transition timeout ke `PENDING_RECONCILIATION`.
- [x] Test `PENDING_RECONCILIATION → CONFIRMED → RECONCILED`.
- [x] Test replacement signature/tx hash ditolak dan terminal state tidak dapat dimulai ulang.

Bukti aktual: `mise exec rust@1.94.1 -- cargo test --test m8_intent_test --locked` → **6 lulus, 0 gagal**.

Status: **DONE — lifecycle test verified; belum bukti receipt live.**

### Prioritas 4 — Dry-run authoritative Robinhood BUY

- [ ] Quote → unsigned → intent → `eth_call` → `estimateGas` selesai tanpa signing.
- [x] Runner resmi dijalankan; fail-closed sebelum signing/broadcast.

Bukti aktual: `./target/debug/robinhood-dry-run` → **exit 1**, berhenti pada saldo native: kebutuhan `2797886987018938 wei`, saldo `606854667974295 wei`. Tidak ada quote/simulasi/signing/broadcast pada percobaan ini.

Status: **BLOCKED — saldo native tidak mencukupi untuk preflight runner.**

### Status pause sesi — 15 Sep 2026 WIB

- Pekerjaan dihentikan atas kondisi user: belum ada dana dingin untuk top-up wallet Solana maupun Robinhood.
- Wallet Solana dan Robinhood **belum didanai untuk canary**; tidak ada signing atau broadcast yang dilakukan.
- Runner Robinhood dry-run terakhir fail-closed karena saldo native tidak mencukupi: kebutuhan `2797886987018938 wei`, saldo terdeteksi `606854667974295 wei`.
- Estimasi dana teknis yang direkomendasikan sebelum resume: Robinhood sekitar `0.005 ETH` dan Solana sekitar `0.02 SOL`; angka ini bukan otorisasi transaksi.
- Engine produksi tetap **OFF**; `.env`, database produksi, dan private key tidak diubah.
- Resume sesi berikutnya: mulai dari status commit ini, verifikasi saldo/read-only preflight, lalu ulangi hanya prioritas 4; jangan mengulang prioritas 1–3 kecuali ada regresi.

### Gate yang berada di tangan user (tidak bisa dikerjakan agent tanpa otorisasi):

- Otorisasi canary live eksplisit (token, nominal, arah, batas gas/slippage) — tanpa ini live on-chain TIDAK BISA dibuktikan.
- Keputusan strategi `TAKE_PROFIT_PCT` (parser default 100.0 vs `.env.example` 50.0) — tidak boleh disamarkan.
- Dana wallet dan key yang dipakai untuk canary.

Aturan sesi berikutnya:

1. Jangan mengklaim live-ready.
2. Kerjakan nomor 1–4 di atas berurutan; setiap item DONE harus punya bukti command/test, bukan hanya edit kode.
3. Setelah setiap slice: `cargo test --locked`, `cargo build --release --locked`, `git diff --check`.
4. Update checkbox hanya setelah bukti command/RPC/reference dicatat.
5. Engine tetap OFF sampai live canary diotorisasi terpisah.

## Format update milestone

Setiap pekerjaan selesai harus menambahkan:

- tanggal/waktu WIB atau UTC yang jelas,
- file/path yang berubah,
- checkbox yang berubah,
- command/check yang benar-benar dijalankan,
- hasil numerik,
- blocker dan risiko yang masih terbuka,
- status engine/signing/broadcast.

---

### Update 17 Sep 2026 WIB — Integrasi Mekanisme Pertahanan Blueprint Solana (`src/lab.js`) ke ZenithSW

#### Ringkasan Perubahan:
1. **Dynamic Token Quarantine (`TokenCooldownRegistry`):**
   - Modul baru `src/gating/cooldown.rs`, diintegrasikan ke `ConfluenceEngine` (`src/gating/confluence.rs`).
   - Mencegah spam RPC/transmisi pada token yang gagal dieksekusi dengan backoff eksponensial $15 \text{ menit} \times 2^n$ (maks. 4 jam).
   - Penolakan zero-cost sebelum menembak RPC/DexScreener.
2. **Realizable Exit Quote Provider & TTL Cache:**
   - Helper `get_realizable_exit_quote` di `src/execution/solana_executor.rs`.
   - In-memory cache dengan TTL 60 detik (fallback aman hingga 180 detik jika Jupiter mengalami timeout/429).
   - Injeksi pemantauan di Task 3 `src/bin/engine.rs` saat posisi Solana mendekati $\ge +10\%$ atau $\le -20\%$.
3. **Pure Whale Mirror Dump (Zero Kompromi):**
   - Penegakan dumping 100% instan saat target whale menjual $\ge 95\%$ tanpa delay holding time.
4. **Attribution Tracking & Automated Wallet Demotion:**
   - Metode `check_and_demote_underperforming_whale` di `src/db.rs`.
   - Ditrigger otomatis setelah penutupan posisi di `trade_history`.
   - Menurunkan wallet ke `TIER-B` dan menonaktifkan (`is_active = 0`) jika membukukan 2 kali kerugian beruntun atau menderita drawdown tunggal $\ge -30\%$.

#### File yang Diubah:
- `src/gating/cooldown.rs` (baru)
- `src/gating/mod.rs`
- `src/gating/confluence.rs`
- `src/execution/solana_executor.rs`
- `src/db.rs`
- `src/bin/engine.rs`
- `tests/simulation_test.rs`

#### Bukti Pengujian & Kompilasi:
- `cargo check --locked` → Finished dev profile in 11.39s (0 error, clean).
- `cargo test --lib gating::cooldown` → **3 passed, 0 failed**.
- `cargo test --test simulation_test test_gating_token_quarantine_cooldown` → **1 passed, 0 failed**.
- `cargo test --test simulation_test test_wallet_demotion` → **2 passed, 0 failed**.
- `cargo test --locked` (Full Suite) → **204 passed, 0 failed, 0 ignored** across unit tests, IPC, runtime harness, and simulation harness.

Status: **DONE — tested/runtime-verified; zero regressions pada codebase.**

---

### Update 17 Sep 2026 WIB — Ekstraksi 100% Parameter Trading ke `.env`

#### Ringkasan Perubahan:
1. **Config Struct & Parser Hardening (`src/config.rs`):**
   - Menambahkan 9 variabel baru ke `Config`:
     - `TAKE_PROFIT_2_PCT` (default: `300.0`%)
     - `LIQUIDITY_DRAIN_USD` (default: `10000.0` USD)
     - `PEAK_DRAWDOWN_PCT` (default: `50.0`%)
     - `PROFIT_LOCK_FLOOR_PCT` (default: `20.0`%)
     - `WHALE_FULL_DUMP_PCT` (default: `95.0`%)
     - `QUARANTINE_BASE_MINUTES` (default: `15` min)
     - `QUARANTINE_MAX_MINUTES` (default: `240` min)
     - `DEMOTE_CONSECUTIVE_LOSSES` (default: `2` trades)
     - `DEMOTE_CATASTROPHIC_LOSS_PCT` (default: `30.0`%)
   - Validasi fail-closed: menolak angka <= 0, infinite/NaN, base > max quarantine, atau pct > 100.
2. **Dynamic In-Memory Wiring:**
   - `TokenCooldownRegistry` (`src/gating/cooldown.rs`) menerima `base_mins` dan `max_mins` dari Config.
   - `SmartWalletDb` (`src/db.rs`) mendukung `set_demote_thresholds` dinamis via atomic configuration.
   - `src/bin/engine.rs` mengikat seluruh threshold exit dan demotion langsung ke Config tanpa magic numbers.
3. **Environment Templates:**
   - `.env.example` dan `.env` diperbarui dengan deskripsi lengkap.

#### Bukti Pengujian & Runtime:
- `cargo check --locked` → Lulus dev profile (0 error, clean).
- `cargo test --locked` (Full Suite) → **204 passed, 0 failed, 0 ignored**.
- `cargo build --bin zenithsw-engine --release --locked` → Sukses.
- `systemctl --user restart zenithsw-engine.service` → Active (running), PID `1103797`.

Status: **DONE — 100% trading strategy parameters are customizable via `.env`.**

---

### Update 17 Sep 2026 WIB — Startup Physical Position Reconciliation & Orphan Token Recovery (Polymarket Pattern Adaptation)

#### Ringkasan Perubahan:
1. **Startup Physical SPL Token Scan (`src/execution/solana_executor.rs`):**
   - Menambahkan method `reconcile_startup_positions(&self) -> Result<(usize, usize)>`.
   - Mengambil seluruh saldo fisik SPL Token & Token-2022 di wallet on-chain via `getTokenAccountsByOwner`.
   - Memfilter blacklist asset (USDC, USDT, WSOL).
2. **Dual-State Reconciliation (On-Chain vs SQLite):**
   - **Ghost Position Cleanup:** Posisi yang berstatus `status != 'CLOSED'` di SQLite namun saldo on-chain bernilai `0` langsung ditutup dengan exit reason `ORPHAN_AUDIT_ONCHAIN_ZERO`.
   - **Balance Mismatch Sync:** Posisi aktif di SQLite yang jumlah tokennya berbeda dengan fisik on-chain disinkronisasikan ke saldo aktual.
   - **Orphan Token Adoption:** Token on-chain dengan saldo `uiAmount > 0` yang tidak tercatat di SQLite otomatis diadopsi sebagai posisi aktif baru (`status = 'OPEN'`, `target_whale_handle = 'ORPHAN_ADOPTED'`, default risk framework aktif).
3. **Engine Startup Pipeline Integration (`src/bin/engine.rs`):**
   - Diinjeksi tepat setelah pemulihan intent transaksi dan sebelum worker streaming dimulai.
   - Berjalan otomatis jika `ENABLE_SOLANA=true`.

#### Bukti Pengujian & Verifikasi:
- Unit test baru: `startup_reconciliation_logic_filters_blacklisted_and_zero_balances` di `src/execution/solana_executor.rs` → **Passed**.
- `cargo check --bin zenithsw-engine` → **Passed (0 warning, 0 error)**.
- `cargo test --test simulation_test` → **16 passed, 0 failed**.
- `cargo test --test dual_track_test` → **1 passed, 0 failed**.

Status: **DONE — Zero-guessing startup reconciliation active on Solana.**

## [2026-09-20] Eksperimen Mirror 1 Whale: wallet_registry.db di-rebuild dari bunker_wallet.db
- [x] Backup: data/wallet_registry_backup_20260920_115233.db (integrity_check=ok, 541 wallet lama terjaga)
- [x] Rebuild tabel wallets = 15 wallet bunker (satu-set identik, terverifikasi SQL), semua TIER-S / is_active=1 / execution_track=FAST_LANE / source=BUNKER_FORENSIC
- [x] Gate otoritas engine (logika engine.rs:441-458 + db.rs:940): 15/15 PASS
- [x] Fix 3 address terpotong di bunker+registry via match unik dari /tmp/unipcs_deep_scan.json (AgmLJBMD..., 5Yyz7b37..., C5Rvdk9w...)
- [x] Verifikasi cross-check bunker vs registry: set identik, semua address 44-char base58
- Catatan: engine TIDAK dijalankan otomatis; DRY_RUN=true (simulasi virtual). Rollback: copy backup ke wallet_registry.db

## [2026-09-20] Fix confluence palsu (XMR) + identifikasi MM bot
- [x] Diagnosa: solana_stream.rs:449 kirim address sbg handle; confluence COUNT(DISTINCT wallet_handle) -> 1 wallet = 2 "whale"; sinyal XMR (Wrapped Monero, liq $251k) lolos palsu
- [x] Regression test confluence_dedupe_test; fix handle resolution + gate dedupe by wallet_address; full suite 21 suites OK
- [x] Backfill 1.610 baris wallet_activities (handle=address) ke handle resmi; live verify pasca-restart: 0 baris rusak baru, confluence jujur
- [x] Forensik MM bot GpMZbSM2...: wallet EOA 1.274,6 SOL (~$138k), router Jupiter, 1.491 tx hari ini (709 BUY $39.7k / 782 SELL $40.1k, 318 token) = pola market maker/MEV; kontaminasi eksperimen: 2 posisi shadow (XMR + LMT) terekam

## [2026-09-20] Demote 6+1 wallet TIDAK LAYAK + wipe riwayat trading
- [x] 7 wallet (4 proxy + insider + burst drained, + Raydium vault authority) ditandai TIDAK_LAYAK di backup registry (7 insert) & live registry (is_active=0)
- [x] Wipe total trading_state.db: 2.707 aktivitas, 48 posisi, 177 sinyal, 83 trade history -> 0; snapshot aman: trading_state_snapshot_20260920_134516.db
- [x] Engine restart 20:48 WIB: "Tracing on-chain funding clusters for 8 Tier-S wallets"; aktivitas pertama dari sniper cluster (handle resmi terverifikasi); 0 sinyal/posisi sampai verifikasi
- Catatan: eksperimen bersih dimulai; motor: main, backbone, 4 sniper Bruce, 2 heavy feeder

## [2026-09-21] Implementasi Conditional Pre-TP Trailing Stop Guard (Capital Preservation)
- [x] Diagnosis dead config: parameter `TRAILING_STOP_PCT` di `.env` sebelumnya tidak tersambung ke loop evaluasi posisi di `src/bin/engine.rs`.
- [x] Implementasi Pre-TP Trailing Stop: `TRAILING_STOP_TRIGGER_PCT` (default 35.0%) dan `TRAILING_STOP_PCT` (15.0%) disambungkan ke `engine.rs` untuk posisi `OPEN` sebelum Tier 1 TP.
- [x] Sinkronisasi `Config`: struct, loader `parse_value`, validasi fail-closed, `.env.example`, dan mock constructor test (`simulation_test.rs`, `dual_track_test.rs`, `tui_handler_test.rs`).
- [x] Integration test deterministik: `test_pre_tp_conditional_trailing_stop_logic` lulus (17/17 tests passing di `simulation_test`).
- [x] Full regression suite: 220 tests PASS (180 unit + 40 integration across 8 test shards).
- [x] Deploy release binary & restart daemon: service PID 41557 aktif.
- [x] Eksekusi live: EMBER terkunci profit +31.37% (+$1.57 USD) via `TRAILING_STOP_EXIT`; TRENDS diputus di -17.03% (-$0.85 USD) sebelum terseret Hard SL -45%. Total realized PnL naik ke +$14.12 USD.

## [2026-09-21] Implementasi Dual-Channel RPC Architecture (Helius Quota Preserver)
- [x] Diagnosis pemborosan kuota: round-robin polling wallet membakar ~432.000 RPC request/hari, mengancam kuota 1.000.000 credits Helius habis dalam 2,5 hari.
- [x] Arsitektur Dual-Channel diimplementasikan:
  - Kanal Publik Nol-Biaya (`SOLANA_RPC_URLS` & `SOLANA_WS_URL`): `https://solana-rpc.publicnode.com`, `https://api.mainnet-beta.solana.com`, `wss://solana-rpc.publicnode.com` menangani polling rutin `getSignaturesForAddress`, radar token scan, dan WebSocket logsSubscribe.
  - Kanal Presisi Helius (`SOLANA_EXECUTION_RPC_URL`): Didedikasikan eksklusif untuk parsing terurai `getTransaction` (saat ada event transaksi whale), `getLatestBlockhash`, dan `sendTransaction` broadcast.
- [x] Fallback fail-closed: Jika kanal eksekusi Helius offline/habis kuota, engine otomatis failover ke pool RPC publik tanpa crash.
- [x] Full test suite: 220/220 unit & integration tests PASS.
- [x] Binary release dikompilasi (`zenithsw-engine` & `zenithsw`); engine dimatikan sementara sesuai arahan user.
- [x] Integrasi 2 mode `SHADOW_EXIT_ENABLED`: Sambungkan `config_stream.shadow_exit_enabled` di `src/bin/engine.rs:595` agar switch `true` (mirror dump whale) / `false` (independent bot exit) di `.env` berfungsi fisik. Test 220/220 PASS.

## [2026-09-22] Bifurkasi Exit Strategy: Parameter & Logika Terpisah (Whale Mirror vs Bot Autonomous 100%)
- [x] Bifurkasi Arsitektur Exit:
  - Mode `SHADOW_EXIT_ENABLED=true`: Protokol multi-tier (50% TP1 free-roll, 25% TP2 runner lock, moonbag peak drawdown, profit-lock floor, whale mirror partial & full dump).
  - Mode `SHADOW_EXIT_ENABLED=false`: Mode bot otonom (All-or-Nothing / clean scalp 100% full exit tanpa sisa moonbag) saat TP, SL, atau Trailing Stop terpicu.
- [x] Penambahan 4 Parameter Khusus Bot di `Config`, `.env.example`, dan `.env`:
  - `BOT_TAKE_PROFIT_PCT` (default: 50.0%) -> Dump 100% via `BOT_FULL_TP_EXIT`.
  - `BOT_STOP_LOSS_PCT` (default: 20.0%) -> Cut loss 100% via `BOT_HARD_SL_CUTLOSS`.
  - `BOT_TRAILING_STOP_TRIGGER_PCT` (default: 30.0%) -> Ambang pemicu trailing stop bot.
  - `BOT_TRAILING_STOP_PCT` (default: 10.0%) -> Dump 100% via `BOT_TRAILING_STOP_EXIT`.
- [x] Validasi fail-closed: finite, > 0.0, percentages <= 100.0.
- [x] Sinkronisasi TUI / Heartbeat IPC: Menyiarkan parameter aktif yang relevan berdasarkan mode yang sedang dipilih.
- [x] Test suite: `test_bot_autonomous_exit_full_tp_and_sl_logic` lulus; full regression suite: 222 passed (181 unit + 41 integration across all shards), 0 failed.
- [x] Kompilasi release binary: `zenithsw` & `zenithsw-engine` sukses.

## [2026-09-24] Integrasi Filter Dual-Channel Whale Gems Radar (Pembersihan Debu & Deteksi Transaksi Riil)
- [x] Masalah: Radar menampilkan 47 proyeksi token yang didominasi debu saldo sisa (dust) bernilai perak dan spam token airdrop karena sistem meloloskan seluruh koin dengan `amount > 0`.
- [x] Desain Logika Asimetris (Opsi B + A):
  - Opsi A (Filter Debu Saldo): Token dengan harga pasar DexScreener wajib memiliki total nilai kepemilikan whale minimal $100 USD (`ui_amount * price_usd >= 100.0`). Koin di bawah $100 USD dibuang dari proyeksi.
  - Opsi B (Jalur Transaksi Beli Aktif): Transaksi beli baru (`BUY` / `DirectSwap`) oleh whale minimal $100 USD dalam 24 jam terakhir ditarik dari `wallet_activities` dan diproyeksikan langsung sebagai `HighPriorityRadar`.
  - Pruning otomatis: Proyeksi kadaluarsa (>30 menit) dibersihkan dari `radar_projections` agar token yang tereliminasi tidak menetap selama 24 jam.
- [x] Eksekusi kode:
  - `src/radar.rs`: `OnChainTokenEvidence` ditambahkan `ui_amount: Option<f64>`, `RadarPolicy` ditambahkan `min_holding_value_usd: f64`, reduksi mengevaluasi saldo koin per dompet dan status DirectSwap.
  - `src/radar_rpc.rs`: Parser akun token Solana mengambil nilai desimal riil `uiAmount` dari response RPC.
  - `src/db.rs`: Penambahan `get_recent_buy_evidence` dan `prune_stale_radar_projections`.
  - `src/radar_runtime.rs`: Penggabungan bukti holding fisik dan transaksi beli `wallet_activities` 24 jam terakhir.
- [x] Verifikasi pengujian:
  - Unit tests baru: `holding_below_min_holding_value_usd_is_excluded` dan `holding_below_min_holding_value_with_direct_swap_is_preserved_as_high_priority` lulus.
  - Full regression suite: 223 passed (184 unit + 39 integration), 0 failed.
- [x] Verifikasi live daemon:
  - Binary release dikompilasi & daemon di-restart.
  - Hasil observasi radar riil: Proyeksi terpangkas dari 47 token sampah menjadi 11 token berkurasi tinggi (5 `HighPriorityRadar` dari transaksi beli whale riil: X7, EMBERCAT, LYNKS, DURIAL, CONVICTION; dan 6 `StructureConfirmed` bernilai saldo >= $100 USD: EMBER, FLAME, FINE, UCAT, FRIENDS).
- [x] Round-Robin Pool RPC Publik Nol-Biaya:
  - Diagnosa kegagalan endpoint: Mayoritas node gratis (publicnode, Chainstack free, Ankr free) memblokir `getTokenAccountsByOwner` (403 Forbidden / Plan Upgrade) karena ketiadaan indeks `--account-index spl-token-owner`.
  - Endpoint valid diverifikasi: `https://public.rpc.solanavibestation.com` dan `https://api.mainnet-beta.solana.com` lolos 100%.
  - `src/radar_rpc.rs`: Mengintegrasikan `ReadOnlyRpcClient` dengan cursor atomic round-robin ganda (50:50) failover, jeda adaptif status 429 backoff 1.500ms, serta pacing 2.500ms antar-dompet.
  - `.env`: Mengaktifkan rotasi `SOLANA_RPC_URL=https://api.mainnet-beta.solana.com,https://public.rpc.solanavibestation.com`.
  - Verifikasi: Test suite lulus, kompilasi release sukses, daemon aktif lancar tanpa mengorbankan kuota Helius.
- [x] Audit On-Chain & Purge Registry Whale Panutan:
  - Backup resmi dibuat: `data/wallet_registry.db.bak_20260924_purge`.
  - Evaluasi 20 dompet panutan via RPC Solana: 17 dompet terverifikasi aktif (transaksi hari ini / <4 hari), sedangkan 3 dompet terbukti mati suri >14 hari tanpa aktivitas.
  - Eliminasi 3 dompet mati suri dari `data/wallet_registry.db`:
    1. `bunker_veeman_feeder_01` (`42e6KaraNpvvHUrcUt33GcQYmxaPN6vp5HdT39xVYiJR` | 39 hari inaktif)
    2. `bunker_veeman_feeder_02` (`8NkMzBchjoxyABfX4kiZoAGtMBRog6iNp9KuwhucTHNd` | 18 hari inaktif)
    3. `bunker_detweiler_sat_02` (`9UXDr5sZwcE2duBKXkzqTbwKknfuycWqpRrV5QZmDWXg` | 14 hari inaktif)
  - Registry kini terkonsolidasi menjadi 17 dompet aktif (13 Tier-S + 4 Tier-A), beban RPC terpangkas 15%, dan daemon `zenithsw-engine` aktif berjalan normal.

## [2026-09-24] Penyelesaian Blindspot Parser Native SOL Stream (Eliminasi False Transfer pada Swap DEX)
- [x] Masalah: Transaksi swap DEX berbasis Native SOL (seperti pembelian token LinkedInu oleh `bunker_veeman_sat_02` senilai 45 SOL / $5.417 USD) salah diklasifikasikan sebagai `TRANSFER` alih-alih `BUY` karena parser `src/streams/solana_stream.rs` hanya membaca saldo SPL Wrapped SOL (`preTokenBalances` vs `postTokenBalances`) dan buta terhadap pengurangan saldo SOL fisik.
- [x] Solusi Arsitektur:
  - Implementasi fungsi `extract_native_sol_delta`: Membaca array `meta.preBalances` dan `meta.postBalances` berdasarkan indeks rekening dompet di `transaction.message.accountKeys`.
  - Kompensasi Fee Netral: Menghitung offset biaya transaksi (`meta.fee`) khusus untuk fee payer (indeks 0) sehingga biaya gas/priority fee murni tidak disalahartikan sebagai pembayaran swap.
  - Penyatuan Aliran Quote Asset: `delta_sol = native_sol_delta + delta_wsol`, mendukung multi-hop swap, direct Native SOL, maupun SPL Wrapped SOL.
- [x] Verifikasi Pengujian:
  - Unit tests baru: `extract_native_sol_delta_handles_fee_payer_and_offset`, `extract_native_sol_delta_returns_zero_for_unrelated_address`, dan `buy_with_native_sol_classified_as_buy` lulus.
  - Total pengujian: 229 passed (188 unit + 41 integration across all shards), 0 failed.
- [x] Verifikasi Database & Operasional:
  - Binary release dikompilasi (`cargo build --release`) dan daemon `zenithsw-engine.service` di-restart aktif.
  - Rekam jejak historis token LinkedInu (`id: 672` dan `id: 1041`) di `wallet_activities` dikoreksi menjadi `BUY`.

## [2026-09-25] Isolasi Total Kuota RPC: Dedicated Portfolio RPC & Penstabilan Equity TUI pada Mode Simulasi
- [x] Masalah: Tampilan saldo portofolio pada TUI berkedip dan drop ke `0.0` karena rate limit HTTP 429 (`Solana RPC 540/756 fail=0 429=216`) dari pemindaian token akun fisik ke node RPC publik gratisan tiap 15 detik, padahal engine berjalan dalam mode simulasi (`DRY_RUN=true`).
- [x] Solusi Arsitektur:
  - Penambahan parameter konfigurasi `SOLANA_PORTFOLIO_RPC_URL`: Mengalokasikan endpoint RPC sekunder Helius khusus untuk pemindaian akun dompet bot sendiri, menjaga kanal `SOLANA_EXECUTION_RPC_URL` tetap steril hanya untuk eksekusi transaksi trading riil.
  - Perutean Cerdas di `SolanaExecutor::post_rpc`: Permintaan baca saldo (`getBalance`, `getTokenAccountsByOwner`, `getTokenAccountBalance`, `getAccountInfo`) otomatis dialirkan ke `SOLANA_PORTFOLIO_RPC_URL` terlebih dahulu dengan fallback ke `rpc_urls`.
  - Penyeleksian Mode `DRY_RUN` di Heartbeat Worker (`src/bin/engine.rs`): Pada mode simulasi, pembacaan token fisik on-chain dimatikan (`Vec::new()`) dan total modal portofolio (`open_positions_usd`) dihitung murni dari posisi virtual terbuka di database (`active_positions`), mencegah eksekusi kueri token kosong sisa uji coba.
  - Proteksi Nilai Saldo SOL Transien: Menyimpan nilai saldo SOL valid terakhir jika terjadi kegagalan jaringan sementara agar tampilan antarmuka TUI tidak anjlok ke 0.
- [x] Verifikasi Pengujian & Kompilasi:
  - Test suite penuh: 229 passed (188 unit + 41 integration across all shards), 0 failed.
  - Binary release dikompilasi sukses (`cargo build --release`).
- [x] Verifikasi Operasional & IPC:
  - File `.env` dan `.env.example` diperbarui dengan parameter `SOLANA_PORTFOLIO_RPC_URL`.
  - Daemon `zenithsw-engine.service` di-restart dengan sukses (PID 251918).
  - Probe socket IPC `/tmp/zenithsw.sock` mengonfirmasi: `Solana RPC: 3/3 fail=0 429=0`, `Equity USD: 117.93` (stabil dari 6 posisi virtual aktif), dan error 429 tereliminasi sepenuhnya.

## [2026-09-25] Resolusi Bloat Database trading_state.db, Upsert TokenHolding Radar, & Self-Maintenance Otomatis
- [x] Masalah:
  - File `data/trading_state.db` membengkak hingga 706MB (+ WAL 209MB) akibat penumpukan 1.608.182 baris pada tabel `radar_evidence` (1.608.123 `TokenHolding`, 59 `DirectSwap`).
  - Polling radar tiap 120s menyedot ~5.812 saldo akun token dari 17 whale dan menyimpannya berulang kali dengan timestamp baru.
  - Query `DELETE` inline tanpa indeks pada kolom `observed_at` memicu full table scan dan lock contention SQLite, menahan thread IPC dan memicu status `[ENGINE: STALE HEARTBEAT 🔴]` di TUI.
- [x] Solusi Teknis & Arsitektur:
  - Pemisahan Karakter Data: Event transaksi `DirectSwap` (59 baris) dipertahankan utuh (*append-only*), sedangkan state saldo `TokenHolding` dialihkan ke model *upsert* berbasis `(chain, token_address, wallet_address)` (memperbarui saldo dan `observed_at` terbaru). Token baru lahir milik whale tetap tersimpan tanpa memicu ledakan baris.
  - Penghapusan Inline Delete: Operasi `DELETE` dihapus total dari alur tulis `insert_radar_evidence_batch` agar hot-path ingestion tidak tertahan lock database.
  - Indeks Database: Menambahkan indeks `idx_radar_evidence_observed_at` dan `idx_radar_market_observed_at` pada `src/db.rs`.
  - Worker Self-Maintenance Berkala (Task 5 di `src/bin/engine.rs`): Menjalankan pembersihan terjadwal setiap 1 jam via method `run_database_maintenance()`. Pruning menghapus evidence kadaluarsa (>24 jam) dengan tetap menjaga record snapshot teranyar untuk tiap holding token aktif, menghapus riwayat market kadaluarsa (>7 hari), serta mengeksekusi `PRAGMA wal_checkpoint(PASSIVE)` dan `PRAGMA optimize`.
  - Migrasi Data: Menjalankan skrip migrasi pemadatan dengan backup aman (`trading_state.db.bak_before_maintenance`), menyusutkan ukuran database dari >915MB menjadi 4.2MB (WAL 302KB).
- [x] Verifikasi Pengujian & Kompilasi:
  - Unit test `test_radar_evidence_upsert_and_maintenance` dibuat dan lulus.
  - Test suite penuh: 229 passed (188 unit + 41 integration across all shards), 0 failed.
  - Binary release dikompilasi sukses (`cargo build --release`).
- [x] Verifikasi Operasional & IPC:
  - Daemon `zenithsw-engine.service` diaktifkan kembali (PID 275002).
  - Probe socket IPC mengonfirmasi stream heartbeat beruntun tanpa delay (`heartbeat_seq: 6..79`), responsivitas sub-detik, dan tidak ada lagi fenomena stale heartbeat.

## [2026-09-27] Whitelist Filter Eksekusi Copytrade (COPYTRADE_ALLOWED_WHALES) & Prioritisasi Pemindaian Radar Market
- [x] Masalah & Kebutuhan Operasional:
  - User ingin membatasi eksekusi transaksi otomatis hanya pada klaster whale terpercaya tertentu (misal: `@bunker_solstice` dan `@bunker_detweiler`), sementara whale lain tetap ingin dipantau aktivitasnya di radar/TUI tanpa memicu pembelian aset otomatis (*monitor-only*).
  - Siklus pemindaian harga pasar DexScreener (`MAX_MARKET_TOKENS_PER_CYCLE = 25`) sebelumnya hanya mengurutkan token secara alfabetis, sehingga token yang baru saja dibeli whale (`DirectSwap`) berisiko terlewat dari siklus pemindaian cepat jika jumlah holding token whale melebihi batas siklus.
- [x] Solusi Teknis & Arsitektur:
  - Whitelist Eksekusi Whale (`src/config.rs` & `src/gating/confluence.rs`):
    - Penambahan variabel lingkungan `COPYTRADE_ALLOWED_WHALES` (daftar substring handle yang dipisahkan koma, case-insensitive).
    - Pemeriksaan gating Check B0 pada `ConfluenceEngine::evaluate_activity_with_mcap`: jika whitelist aktif dan handle dompet tidak cocok, sinyal ditolak secara fail-closed dengan alasan `REJECTED_COPYTRADE_DISABLED_MONITOR_ONLY (@handle)`.
  - Prioritisasi Pemindaian Radar Pasar (`src/radar_runtime.rs`):
    - Struktur data `TokenScanCandidate` mengagregasi bukti token berdasarkan: status `has_direct_swap`, jumlah dompet paus pemegang (`wallet_count`), dan timestamp observasi teranyar (`latest_observed_at`).
    - Pengurutan berbobot memastikan token yang sedang diakumulasi aktif selalu masuk prioritas utama kuota pemindaian DexScreener.
- [x] Verifikasi Pengujian & Kompilasi:
  - Unit test baru: `test_copytrade_allowed_whales_whitelist_gating` di `tests/simulation_test.rs` memvalidasi bahwa handle yang lolos dieksekusi normal dan handle di luar daftar ditolak dengan status monitor-only.
  - Penyesuaian mock konfigurasi di `tests/dual_track_test.rs`, `tests/simulation_test.rs`, dan `tests/tui_handler_test.rs`.
  - Full regression test suite: 228 passed (189 unit + 39 integration), 0 failed.
  - Kompilasi release binary sukses (`cargo build --release`).
- [x] Verifikasi Dokumentasi & Lingkungan:
  - `.env.example` diperbarui dengan parameter `COPYTRADE_ALLOWED_WHALES`.
  - `README.md` diperbarui dengan dokumentasi fitur whitelist eksekusi dan pemindaian berprioritas radar.

## [2026-09-27] Penyempurnaan Robustness Parser Multi-Pool DexScreener, Rotasi Antrean Radar, & Eliminasi Dead Token Starvation
- [x] Masalah:
  - Pada `parse_market_snapshot`, jika ada pool pertama yang mengembalikan data invalid atau parsial (misal `marketCap` null), operator `?` langsung membatalkan parsing seluruh response, mengabaikan pool valid lain yang lebih likuid.
  - Kandidat token saldo (`holding_candidates`) yang melebihi kuota pemindaian per siklus (`MAX_MARKET_TOKENS_PER_CYCLE = 50`) mengalami *queue starvation* (token di urutan bawah tidak pernah terpindai).
  - Token mati (HTTP 404 atau tidak memiliki pool DEX valid) terus dipindai berulang kali, memboroskan kuota siklus.
- [x] Solusi Arsitektur:
  - Error Handling Per-Pool: `parse_market_snapshot` di `src/radar.rs` menangani error parsing pool secara parsial (`match`), melompati pool cacat, dan memilih pool dengan likuiditas USD tertinggi yang valid.
  - Round-Robin Holding Cursor: Mengimplementasikan `holding_cursor: AtomicUsize` untuk merotasi antrean kandidat saldo secara adil di setiap siklus pemindaian pasar DexScreener.
  - Dead Token Tracking & 1-Hour Cooldown: Struktur `dead_tokens: Arc<RwLock<HashMap<String, Instant>>>` mencatat token 404 / tanpa pair dan mengabaikannya selama 3.600 detik (1 jam) agar tidak menyedot jatah pemindaian aset aktif.
  - In-Memory Market Cache (TTL 30 Menit): Menyimpan snapshot pasar DexScreener & GeckoTerminal OHLCV di RAM (`market_cache`) selama 1.800 detik agar data struktur pasar dan aksi harga tidak hilang saat rotasi antrean.
- [x] Verifikasi Pengujian & Kompilasi:
  - Unit test baru `market_parser_skips_broken_pool_and_keeps_valid_pool` lulus.
  - Full regression test suite lulus: 228 passed, 0 failed.

## [2026-10-01] Price Sanity Guard, Anti-Flash-Wick SL Debounce, & Verifikasi Likuiditas Realizable Jupiter
- [x] Masalah Operasional:
  - False Price Crash Glitch: DexScreener sewaktu-waktu mengalami tick anomali sesaat (>50% drop dalam 1 tick) yang dapat memicu trigger Stop Loss palsu dan cut-loss dini.
  - Flash Wick Whipsaw: Volatilitas sesaat pada Solana DEX sering kali menyentuh ambang batas SL (-35%) dalam hitungan milidetik sebelum memantul kembali, menutup posisi di harga terendah.
  - False Liquidity Drain: Likuiditas pool yang dilaporkan 0 atau turun drastis di DexScreener akibat keterlambatan indeks sering memicu *emergency dump*, padahal rute pool Jupiter riil masih sehat dan likuid.
- [x] Solusi Arsitektur & Safety Guards:
  - Downward Price Sanity Guard (`src/bin/engine.rs`):
    - Mendeteksi anomali penurunan harga instan >50% dalam 1 tick tunggal.
    - Menjalankan debounce 1.500ms dan melakukan verifikasi harga oracle segar tanpa cache (`SolanaStreamListener::fetch_token_price_fresh`). Jika harga pulih dan likuiditas sehat, tick glitch ditolak dan dibuang.
  - Anti-Flash-Wick SL Debounce & Re-Verification (`src/bin/engine.rs`):
    - Pemicu Stop Loss (baik `EMERGENCY_HARD_SL_CUTLOSS` pada mode shadow maupun `BOT_HARD_SL_CUTLOSS` pada mode bot) kini melewati jeda debounce 1.200ms untuk menyaring flash wick sesaat.
    - Mengambil harga live segar dari RPC/DexScreener membypass cache in-memory 3s TTL. Jika PnL terverifikasi pulih di atas ambang batas SL, aksi cut-loss dibatalkan otomatis.
    - Verifikasi rute likuiditas Jupiter (`get_fresh_exit_quote`): Memberikan peringatan dini jika *price impact* rute exit melebihi 30%.
  - Verifikasi Realizable Liquidity Drain (`src/bin/engine.rs`):
    - Saat likuiditas pool dilaporkan anjlok di bawah `LIQUIDITY_DRAIN_USD`, engine melakukan debounce 1.500ms dan re-fetch market.
    - Mengecek rute exit Jupiter (`get_fresh_exit_quote`): Jika rute masih aktif dengan *price impact* < 50% dan output SOL > 0, anomali DexScreener ditolak dan posisi tetap dipertahankan.
  - Fetcher Uncached & Max Liquidity Matcher (`src/streams/solana_stream.rs` & `src/execution/solana_executor.rs`):
    - Fungsi `fetch_token_price_fresh`: Bypass cache in-memory 3s TTL untuk verifikasi kritis.
    - Penelusuran fallback pencarian DexScreener kini mengevaluasi seluruh pair hasil pencarian dan memilih pair dengan likuiditas USD tertinggi (`max_liq`).
    - Fungsi `get_fresh_exit_quote`: Membaca quote realizable Jupiter dengan membypass cache internal 60s (`bypass_cache = true`).
  - Ekspansi Kuota Analisis Struktur Pasar (`src/radar_runtime.rs`):
    - Kuota prioritas pengumpulan GeckoTerminal OHLCV Tier-2 diperluas dari 5 token menjadi 15 token teratas (`priority_tokens.truncate(15)`).
- [x] Verifikasi Pengujian & Kompilasi:
  - `cargo fmt --all -- --check`: Lolos.
  - `cargo clippy --all-targets -- -D warnings`: Lolos (0 warning, 0 error).
  - Full test suite: 232 passed (190 unit + 42 integration across all suites), 0 failed.

## [2026-10-02] Durable SELL Intent State Machine & Rekonsiliasi Recovery Solana
- [x] Masalah & Celah Operasional:
  - Eksekusi alur SELL (`execute_partial_shadow_sell` & `execute_shadow_sell`) di `src/execution/solana_executor.rs` sebelumnya berjalan di memori tanpa mencatat intent ke database `execution_intents`.
  - Jika engine mati/crash saat transaksi jual sedang *in-flight* atau timeout, sistem kehilangan jejak status on-chain dan memicu desync saldo fisik terhadap database.
- [x] Solusi Arsitektur & State Machine:
  - Durable SELL Intent Tracking (`src/execution/solana_executor.rs`):
    - Sebelum eksekusi swap rute PumpPortal maupun Jupiter, engine mencatat intent unik (`SOL-SELL-<pos_id>-<route>-<timestamp>`) ke tabel `execution_intents` dengan status awal `QUOTED`.
    - Transisi terverifikasi: `QUOTED` -> `SIGNED` (setelah transaksi swap ditandatangani) -> `BROADCASTED` (dengan lampiran signature riil).
    - Jika terjadi fallback dari PumpPortal ke Jupiter, intent lama ditandai `FAILED` secara fail-closed dan intent baru dibuat untuk jalur Jupiter.
    - Status timeout konfirmasi (20 detik) diturunkan ke `PENDING_RECONCILIATION` dengan signature tersimpan.
    - Saat receipt terverifikasi on-chain: `CONFIRMED` -> `RECONCILED` sebelum porsi posisi ditutup di database.
  - Startup Recovery Handler (`src/bin/engine.rs`):
    - Handler startup engine membaca intent `SELL` gantung. Jika signature terkonfirmasi di on-chain RPC, intent ditransisikan ke `CONFIRMED` dan `RECONCILED`.
    - Scanner token fisik startup (`reconcile_startup_positions`) menyinkronkan saldo fisik aktual dan menutup ghost position jika token sudah ludes.
- [x] Verifikasi Pengujian & Kompilasi:
  - Unit/integration test baru: `execution_intent_solana_sell_lifecycle_and_timeout_reconciliation` di `tests/m8_intent_test.rs`.
  - `cargo fmt --all -- --check`: Lolos.
  - `cargo clippy --all-targets -- -D warnings`: Lolos (0 warning, 0 error).
  - Full test suite: **233 passed (190 unit + 43 integration across 8 shards), 0 failed**.
  - Binary release dikompilasi sukses: `cargo build --release --locked`.

## [2026-10-02] Audit On-Chain & Purge Registry Wallet Tidak Layak
- [x] Masalah & Temuan Audit:
  - `bunker_detweiler_sat_01` (`4X7FEZ283wQhRuyBjrgiNv76pnVasFXdRJqzFcG86HxY`): Status pasif/dormant total (0 signature di on-chain RPC sejak didaftarkan).
  - `bunker_solstice_feeder_01` (`ARu4n5mFdZogZAravu7CcizaojWnS6oqka37gdLT5SZn`): 100% dari 653 aktivitas merupakan event `TRANSFER` debu mikro token ($0.00000001 - $0.04 USD) dan perpindahan likuiditas, bukan transaksi `BUY` di DEX.
- [x] Tindakan Remediasi:
  - Backup snapshot registry: `data/wallet_registry.db.bak_20261002_093425`.
  - Hapus 2 entri wallet tidak layak dari `data/wallet_registry.db`.
  - Total wallet aktif terpangkas dari 18 menjadi 16 dompet.
  - Whitelist copytrade `COPYTRADE_ALLOWED_WHALES="solstice,detweiler,lunarfang"` kini terkonsentrasi eksklusif pada 3 sniper utama dengan riwayat PnL terbukti: `bunker_lunarfang_01`, `bunker_solstice_main_01`, dan `bunker_detweiler_main_01`.
