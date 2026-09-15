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
