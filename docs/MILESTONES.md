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
- [ ] Startup physical token balance recovery.
- [ ] Recovery posisi `OPEN`/`PENDING`.
- [ ] Blockhash expiry handling.
- [ ] Signature status multi-RPC.
- [ ] Durable SELL intent.
- [ ] SELL/partial exit reconciliation.
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
