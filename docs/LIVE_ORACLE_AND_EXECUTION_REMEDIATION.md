# REMEDIASI ORACLE HARGA, EVALUASI EXIT ON-CHAIN, & HARGA PAUS DETERMINISTIK

**Status:** IN_PROGRESS  
**Tanggal:** 2026-10-03  
**Target:** Menghilangkan seluruh distorsi harga DexScreener (kurva mati pump.fun), menghitung harga eksekusi paus murni dari settlement on-chain aktual, menggunakan kuotasi realizable Jupiter untuk evaluasi Stop-Loss/Take-Profit posisi aktif, dan rekonsiliasi otomatis jika posisi ditutup manual dari luar.

---

## 1. Analisis Akar Masalah (Root Cause Analysis)

### Masalah 1: Halusinasi Likuiditas DexScreener & Kurva Mati Pump.fun
- **Lokasi Kode:** `src/streams/solana_stream.rs` (baris 645–670).
- **Mekanisme Cacat:**  
  Kode menerapkan rumus `liq = mcap * 0.50` untuk pasangan berlabel `pumpfun` jika nilai likuiditas adalah nol/null. Pada token yang telah migrasi (seperti SLOPCORE), pasangan DexScreener `pumpfun` sudah mati dengan likuiditas riil $0 dan harga beku saat migrasi ($0.00004895). Namun rumus sintetis menghasilkan `liq` virtual sebesar ~$24.476, mengalahkan likuiditas pool DEX riil yang aktif (`pumpswap` dengan likuiditas $9.308 / ~$4.654 sisi SOL).
- **Dampak Fatal:**  
  Oracle terkunci pada harga beku $0.00004895 (menampilkan profit semu +33,65%) saat harga riil di pool DEX anjlok bebas ke $0.000015 (-57,7%) lalu ke $0.00000378 (-87,7%). Ambang Stop-Loss (-35%) tidak pernah terpicu karena mesin mengira posisi masih floating profit.

### Masalah 2: Pencatatan Harga Masuk Paus yang Terdistorsi & Guard Anti-Chasing Mati
- **Lokasi Kode:** `src/streams/solana_stream.rs` (baris 395–408) dan `src/bin/engine.rs` (baris 660–675).
- **Mekanisme Cacat:**  
  1. `solana_stream.rs` memiliki baris `// NOTE: Strict policy - No fallback price derivation from native SOL deltas`. Kode mengabaikan kalkulasi harga swap aktual dari delta token vs delta lamports on-chain, dan justru memanggil `fetch_token_price(&token_mint)`.
  2. Akibatnya, harga beli paus dicatat berdasarkan kuotasi DexScreener yang terlambat (misal $0.00002774 sebelum pump paus terindeks), padahal paus mengeksekusi di harga on-chain riil ($0.00004442).
  3. Ketika bot mengeksekusi di Jupiter 2 detik kemudian pada harga pasar riil ($0.00003662), database mencatat seolah-olah bot membeli 32% lebih mahal daripada paus.
  4. Karena harga paus dan harga pool sama-sama diambil dari panggilan DexScreener yang sama di `engine.rs`, nilai `price_delta_pct` selalu tercatat 0,0%. Filter perlindungan lonjakan harga (`PRICE_DELTA_TOO_HIGH` > +5%) menjadi tidak berfungsi.

### Masalah 3: Pemantauan Posisi Aktif Mengabaikan Nilai Likuidasi Eksekusi Jupiter
- **Lokasi Kode:** `src/bin/engine.rs` (baris 800–895).
- **Mekanisme Cacat:**  
  Siklus pemantauan posisi aktif hanya memanggil `fetch_token_price` (DexScreener). Panggilan kuotasi realizable Jupiter `get_realizable_exit_quote` hanya dilakukan jika PnL sudah berada di atas +10% atau di bawah -20%, dan hasilnya hanya dipakai untuk mencetak peringatan log `price_impact_pct > 15%`, bukan untuk memperbarui harga dan memicu eksekusi exit darurat.
- **Solusi:**  
  Untuk posisi Solana yang terbuka, nilai likuidasi realizable Jupiter (`outAmount` lamports SOL) adalah tolok ukur nilai aset riil yang dapat dieksekusi detik itu juga.

### Masalah 4: Desinkronisasi Posisi Tertutup Manual dari Luar Bot
- **Lokasi Kode:** `src/bin/engine.rs` (baris 790–905).
- **Mekanisme Cacat:**  
  Pengecekan saldo token on-chain (`get_token_balance`) hanya berjalan saat startup rekonsiliasi. Saat runtime, jika user melakukan cut-loss manual di dompet eksternal (Phantom/Solflare), mesin tetap menganggap posisi `OPEN` sampai engine direstart.
- **Solusi:**  
  Pemeriksaan saldo on-chain fail-closed terintegrasi dalam siklus pemantauan posisi; jika saldo token terdeteksi 0, posisi langsung ditandai `CLOSED` di SQLite.

---

## 2. Rencana Tindakan & Checklist Implementasi

- [ ] **T1. Isolasi Layanan & Proteksi State**: Memastikan service `zenithsw-engine.service` tetap dihentikan selama proses investigasi, perbaikan, dan pengujian.
- [ ] **T2. Perbaikan Oracle DexScreener di `solana_stream.rs`**:
  - Filter pasangan DEX: Jika ada pasangan dengan likuiditas riil (`liquidity.usd > 0`), buang seluruh pasangan mati/virtual (`pumpfun` dengan likuiditas null/0).
  - Larang keras sintesis likuiditas `mcap * 0.50` jika terdapat pool DEX aktif.
  - Batasi nilai likuiditas virtual kurva bonding pra-migrasi pada batas fisik riil SOL reserve (maks $12.000 USD), bukan perkalian arbitrer kapitalisasi pasar.
  - Memilih pool dengan likuiditas riil terverifikasi tertinggi.
- [ ] **T3. Kalkulasi Deterministik Harga Swap Paus On-Chain di `solana_stream.rs`**:
  - Mengambil delta pembayaran net: `delta_sol` (termasuk kompensasi `fee_offset`) dan `delta_stables_usd`.
  - Jika tipe aktivitas adalah `Buy` atau `Sell` terkonfirmasi, hitung harga eksekusi transaksi:
    `execution_price_usd = net_payment_usd / token_amount`.
  - Simpan harga eksekusi paus riil ke `WalletActivity` dan `shadow_signals`.
  - Aktifkan evaluasi selisih harga riil antara harga masuk paus vs harga pasar saat bot hendak masuk (`price_delta_pct`). Tolak jika harga sudah naik melebihi batas toleransi (`max_price_delta_pct`, 5%).
- [ ] **T4. Integrasi Evaluasi Exit Realizable Jupiter di `engine.rs`**:
  - Untuk setiap posisi terbuka Solana, ambil kuotasi likuidasi riil via `sol_exec_pos.get_fresh_exit_quote(&pos.token_mint, pos.raw_amount)`.
  - Turunkan nilai USD realizable dan harga realizable dari lamports SOL yang akan diterima:
    `realizable_sol = out_lamports / 1e9`
    `realizable_usd = realizable_sol * sol_price_usd`
    `unrealized_pnl_pct = (realizable_usd - pos.amount_usd) / pos.amount_usd * 100.0`.
  - Gunakan `unrealized_pnl_pct` realizable ini sebagai pemicu utama Hard Stop-Loss (-35%), Take-Profit, dan Trailing Stop.
  - Jika Jupiter mengembalikan error rute kolaps (`could_not_find_any_route`), langsung picu protokol `LIQUIDITY_DRAIN_EXIT`.
- [ ] **T5. Deteksi Penutupan Posisi Eksternal di `engine.rs`**:
  - Cek saldo akun token on-chain dompet secara periodik dalam pemantau posisi.
  - Jika saldo on-chain bernilai 0 untuk posisi yang masih berstatus `OPEN` di database lokal:
    Langsung tandai posisi `CLOSED` di SQLite tanpa mencoba mengirim transaksi jual saldo kosong.
- [ ] **T6. Pengujian Unit & Verifikasi Integrasi**:
  - Menulis unit test untuk seleksi pool DexScreener (memastikan pool pumpswap menang atas pumpfun dead curve).
  - Menulis unit test untuk kalkulasi harga eksekusi paus dari delta on-chain.
  - Menjalankan `cargo test --locked`, `cargo check --locked --all-targets`, dan `cargo clippy --locked --all-targets`.
- [ ] **T7. Konfirmasi & Verifikasi Sebelum Restart**:
  - Memberikan ringkasan perubahan dan bukti pengujian kepada user untuk disetujui sebelum mengaktifkan kembali layanan live.
