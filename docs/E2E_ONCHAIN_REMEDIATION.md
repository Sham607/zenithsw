# Remediasi Transaksi On-Chain End-to-End (Solana)

Sumber: audit 03 Okt 2026. Status: `[ ]` belum, `[x]` selesai + bukti test, `[~]` sebagian/butuh keputusan Bos.

## Bukti awal (sebelum patch)
- `trading_state_live.db`: 7 intent BUY → 5 `FAILED` bersignature, 2 `UNKNOWN`; posisi 0.
- 5 signature `null` di `getSignatureStatuses(searchTransactionHistory)` (publicnode + api.mainnet); saldo wallet tetap 358128530 lamports → tx tidak pernah landing.
- Jupiter `prioritizationFeeLamports:"auto"` → 160 lamports (1926 µLamports/CU) pada probe 03 Okt; level `veryHigh` max 100k → 13852 lamports. [Likely] penyebab drop.

## TODO
- [x] T1 Simulasi nyata `simulateTransaction` sebelum sign; gagal → `FAILED`, tanpa broadcast.
- [x] T2 Broadcast + rebroadcast tx bertanda tangan sama sampai confirmed / `lastValidBlockHeight` terlewati; `maxRetries=0`.
- [x] T3 Priority fee eksplisit (`SOLANA_PRIORITY_FEE_MAX_LAMPORTS`, level veryHigh) pengganti `"auto"`.
- [x] T4 Wajib `lastValidBlockHeight` dari Jupiter sebelum sign (fail-closed).
- [x] T5 Signature dipersist saat `SIGNED` (sebelum broadcast) → crash window tetap bisa direkonsiliasi.
- [x] T6 Resolver intent kanonik (startup + runtime): `FAILED` hanya jika err on-chain atau block height > lvbh; tanpa bukti → tetap `PENDING_RECONCILIATION`.
- [x] T7 Guard 1 intent aktif per (chain, mint) untuk BUY & SELL (anti double-buy/double-sell).
- [x] T8 Gas buffer guard fail-closed (error RPC saldo = tolak trade); `get_native_balance` tidak lagi `Ok(0.0)` diam-diam.
- [x] T9 Pembacaan saldo token fail-closed (respons malformed = error), jumlahkan semua token account mint.
- [x] T10 Rekonsiliasi BUY dari meta transaksi (`pre/postTokenBalances`) → token delta aktual; harga entry = SOL input ExactIn × SOL/USD ÷ token delta.
- [x] T11 Rekonsiliasi SELL dari meta transaksi (lamport delta + fee) → harga exit & PnL aktual.
- [x] T12 SELL: error saldo tidak lagi `unwrap_or(0)`; saldo 0 terbukti → tutup posisi tanpa PnL fiktif.
- [x] T13 SELL: create/update intent wajib sukses (`?`), hapus `let _ =`.
- [x] T14 Hapus rute PumpPortal (BUY & SELL) + config orphan `SOLANA_PUMPFUN_*`.
- [x] T15 Test negatif: guard per-mint, resolver expiry, parser fill meta, saldo malformed.
- [x] T16 `cargo test --locked`, `cargo build --release --locked`, `git diff --check`.

## Bukti verifikasi (03 Okt 2026, engine OFF)
- `cargo test --locked`: semua suite lulus (lib 196, intent 9, simulation 19, dst; 0 failed).
- `cargo clippy --locked --all-targets`: 0 warning. `cargo build --release --locked`: OK. `git diff --check`: bersih. rustfmt bersih.
- Test baru: `blockhash_expiry_requires_both_heights`, `signature_status_parser_is_fail_closed`, `simulation_parser_rejects_program_error_and_malformed`, `token_balance_parser_sums_accounts_and_rejects_malformed`, `swap_fill_parser_uses_owner_deltas_only`, `signature_is_extracted_from_signed_transaction`, `exclusive_intent_blocks_second_active_intent_per_mint`, `stale_pending_intents_expose_signature_and_expiry`.
- Probe read-only (tanpa sign/broadcast): Jupiter swap dengan fee `veryHigh` max 100k → `prioritizationFeeLamports` 99999, `lastValidBlockHeight` ada; `simulateTransaction` publicnode `err=None units=120273`.
- Startup reconciliation kini fail-closed (RPC error membatalkan, bukan menutup semua posisi sebagai ghost); ghost ditutup tanpa PnL fiktif.

## Belum dilakukan (butuh izin Bos)
- [ ] R1 Deploy binary release baru + start service.
- [x] R2 Live canary kecil pasca-patch (bukti landing + fill tx_meta).
- [ ] R3 Commit.

## Butuh keputusan Bos (tidak diubah)
- [~] D1 `.env` slippage BUY 350 bps (catatan lama 1%).
- [~] D2 Multi-lot whale sama diizinkan di kode (catatan lama: diblokir).
- [~] D3 Deskripsi unit systemd masih "(Dry Run)".
- [~] D4 Baris `SOLANA_PUMPFUN_*` di `.env` jadi tidak terpakai (aman, diabaikan loader).
- [~] D5 Root cause `GAS BUFFER 0.0000 SOL` via Helius portfolio belum diverifikasi (butuh izin kuota Helius).

## Sisa risiko terbuka
- Loop kirim BUY/SELL berjalan inline di task stream/monitor: satu swap dapat menahan task hingga ±120 detik (sebelumnya 30 detik).
- Fill fallback `balance_delta` dipakai bila `getTransaction` gagal 5×; masih bukti on-chain tapi bisa tercampur aktivitas lain di wallet yang sama.
- SELL yang landing terlambat (setelah timeout) ditandai `CONFIRMED` tetapi posisi DB tidak otomatis dipotong; startup reconciliation menyelaraskan `amount_native`, PnL porsi itu tidak tercatat.
- Live canary pasca-patch belum dijalankan.
