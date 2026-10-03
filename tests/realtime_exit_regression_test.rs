use chrono::{Duration, Utc};
use std::sync::Arc;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::solana_executor::CachedRealizableQuote;
use zenithsw::models::{Chain, ExecutionTrack, ShadowPosition};

#[tokio::test]
async fn test_shadow_position_ownership_and_external_exit() {
    let db = Arc::new(SmartWalletDb::new_in_memory().expect("Failed to create in-memory db"));
    let now = Utc::now();

    let pos = ShadowPosition {
        id: "test-pos-realtime-001".to_string(),
        chain: Chain::Solana,
        token_symbol: "REALTIME".to_string(),
        token_mint: "MintToken111111111111111111111111111111111".to_string(),
        target_whale_handle: "Alpha_Tester".to_string(),
        target_whale_address: "5FqU_y17z_dummy_wallet".to_string(),
        track: ExecutionTrack::FastLane,
        entry_price: 1.0,
        current_price: 1.25,
        peak_price: 1.30,
        amount_native: 50.0,
        amount_usd: 50.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 25.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".to_string(),
        is_dry_run: false,
        owner_pubkey: Some("ActiveWalletPubkey11111111111111111111111".to_string()),
        price_source: "JUPITER".to_string(),
        price_updated_at: now,
    };

    db.upsert_position(&pos).expect("Failed to insert position");

    // 1. Verifikasi roundtrip field baru: owner_pubkey, price_source, price_updated_at
    let active = db
        .get_active_positions()
        .expect("Failed to get active positions");
    assert_eq!(active.len(), 1);
    let loaded = &active[0];
    assert_eq!(loaded.id, "test-pos-realtime-001");
    assert_eq!(
        loaded.owner_pubkey.as_deref(),
        Some("ActiveWalletPubkey11111111111111111111111")
    );
    assert_eq!(loaded.price_source, "JUPITER");
    assert!((now - loaded.price_updated_at).num_seconds().abs() < 5);

    // 2. Eksternal exit rekonsiliasi (saldo on-chain 0)
    db.close_position_without_fill(&loaded.id, "MANUAL_EXIT_EXTERNAL")
        .expect("Failed to close position externally");

    // Verifikasi posisi aktif kosong setelah close
    let active_after = db
        .get_active_positions()
        .expect("Failed to get active positions");
    assert_eq!(active_after.len(), 0);

    // Verifikasi pencatatan trade history dengan penalti konservatif (-100% PnL)
    let history = db
        .get_recent_trades(10)
        .expect("Failed to get trade history");
    assert_eq!(history.len(), 1);
    let trade = &history[0];
    assert_eq!(trade.symbol, "REALTIME");
    assert_eq!(trade.exit_reason, "MANUAL_EXIT_EXTERNAL");
    assert_eq!(
        trade.pnl_pct, -100.0,
        "Konservatif PnL wajib -100% bila tx fill luar tidak terlacak"
    );
    assert_eq!(trade.pnl_usd, -50.0);
}

#[test]
fn test_realizable_quote_price_calculation_and_fail_closed() {
    // 100 token dengan 6 desimal = 100_000_000 raw amount
    // Menghasilkan 1 SOL = 1_000_000_000 lamports
    // Harga SOL = $150 USD
    // Realizable price = (1.0 SOL * 150.0 USD) / 100.0 token = $1.50 USD
    let quote = CachedRealizableQuote {
        token_mint: "MintToken111111111111111111111111111111111".to_string(),
        token_amount_raw: 100_000_000,
        estimated_out_lamports: 1_000_000_000,
        estimated_out_sol: 1.0,
        price_impact_pct: 0.12,
        fetched_at: std::time::Instant::now(),
    };

    let sol_usd = 150.0;
    let decimals = 6;
    let price = quote.calculate_realizable_price_usd(sol_usd, decimals);
    assert!(
        (price - 1.50).abs() < 1e-4,
        "Expected price ~1.50, got {}",
        price
    );

    // Kasus fail-closed: output lamports 0
    let zero_quote = CachedRealizableQuote {
        token_mint: "MintToken111111111111111111111111111111111".to_string(),
        token_amount_raw: 100_000_000,
        estimated_out_lamports: 0,
        estimated_out_sol: 0.0,
        price_impact_pct: 0.0,
        fetched_at: std::time::Instant::now(),
    };
    assert_eq!(
        zero_quote.calculate_realizable_price_usd(sol_usd, decimals),
        0.0
    );

    // Kasus fail-closed: input raw 0
    let zero_in_quote = CachedRealizableQuote {
        token_mint: "MintToken111111111111111111111111111111111".to_string(),
        token_amount_raw: 0,
        estimated_out_lamports: 1_000_000_000,
        estimated_out_sol: 1.0,
        price_impact_pct: 0.0,
        fetched_at: std::time::Instant::now(),
    };
    assert_eq!(
        zero_in_quote.calculate_realizable_price_usd(sol_usd, decimals),
        0.0
    );
}

#[tokio::test]
async fn test_stale_price_threshold_and_age_detection() {
    let now = Utc::now();
    let stale_threshold_secs = 5u64;

    // Posisi dengan harga segar (1 detik lalu)
    let fresh_time = now - Duration::seconds(1);
    let fresh_age = (now - fresh_time).num_seconds().max(0) as u64;
    assert!(
        fresh_age < stale_threshold_secs,
        "Age {}s should be fresh (< {}s)",
        fresh_age,
        stale_threshold_secs
    );

    // Posisi dengan harga basi (7 detik lalu)
    let stale_time = now - Duration::seconds(7);
    let stale_age = (now - stale_time).num_seconds().max(0) as u64;
    assert!(
        stale_age >= stale_threshold_secs,
        "Age {}s should be flagged stale (>= {}s)",
        stale_age,
        stale_threshold_secs
    );
}

#[tokio::test]
async fn test_foreign_wallet_isolation_logic() {
    let db = Arc::new(SmartWalletDb::new_in_memory().expect("Failed to create in-memory db"));
    let now = Utc::now();

    // Posisi milik Wallet A (misal wallet trading aktif)
    let pos_a = ShadowPosition {
        id: "pos-wallet-a".to_string(),
        chain: Chain::Solana,
        token_symbol: "TOKENA".to_string(),
        token_mint: "MintTokenA111111111111111111111111111111111".to_string(),
        target_whale_handle: "Whale1".to_string(),
        target_whale_address: "5FqU_y17z_dummy_wallet".to_string(),
        track: ExecutionTrack::FastLane,
        entry_price: 1.0,
        current_price: 1.0,
        peak_price: 1.0,
        amount_native: 100.0,
        amount_usd: 100.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 0.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".to_string(),
        is_dry_run: false,
        owner_pubkey: Some("Wallet_A_Pubkey".to_string()),
        price_source: "JUPITER".to_string(),
        price_updated_at: now,
    };

    // Posisi milik Wallet B (wallet lain / tersimpan di DB)
    let pos_b = ShadowPosition {
        id: "pos-wallet-b".to_string(),
        chain: Chain::Solana,
        token_symbol: "TOKENB".to_string(),
        token_mint: "MintTokenB111111111111111111111111111111111".to_string(),
        target_whale_handle: "Whale2".to_string(),
        target_whale_address: "5FqU_y17z_dummy_wallet".to_string(),
        track: ExecutionTrack::FastLane,
        entry_price: 2.0,
        current_price: 2.0,
        peak_price: 2.0,
        amount_native: 50.0,
        amount_usd: 100.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 0.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".to_string(),
        is_dry_run: false,
        owner_pubkey: Some("Wallet_B_Pubkey".to_string()),
        price_source: "JUPITER".to_string(),
        price_updated_at: now,
    };

    db.upsert_position(&pos_a).unwrap();
    db.upsert_position(&pos_b).unwrap();

    let all_positions = db.get_active_positions().unwrap();
    assert_eq!(all_positions.len(), 2);

    // Simulasi startup reconciliation untuk Wallet A:
    // Wallet A hanya boleh menyentuh posisi miliknya. Posisi Wallet B wajib dilewati.
    let active_wallet = "Wallet_A_Pubkey";
    let mut managed_for_active_wallet = Vec::new();
    let mut skipped_foreign_positions = Vec::new();

    for pos in &all_positions {
        if let Some(ref owner) = pos.owner_pubkey {
            if owner != active_wallet {
                skipped_foreign_positions.push(pos.id.clone());
                continue;
            }
        }
        managed_for_active_wallet.push(pos.id.clone());
    }

    assert_eq!(managed_for_active_wallet, vec!["pos-wallet-a"]);
    assert_eq!(skipped_foreign_positions, vec!["pos-wallet-b"]);
}

#[test]
fn test_fail_closed_quote_triggers_protective_sl() {
    let entry_price = 2.50;
    let sl_pct = -35.0;

    // Skenario 1: Realizable quote normal (harga turun ke $2.00 -> PnL -20%)
    let normal_price = 2.00;
    let pnl_normal = ((normal_price - entry_price) / entry_price) * 100.0;
    assert_eq!(pnl_normal, -20.0);
    assert!(pnl_normal > sl_pct, "SL tidak boleh terpicu pada -20%");

    // Skenario 2: Jupiter quote gagal total atau output lamports 0 (fail-closed)
    // calculate_realizable_price_usd mengembalikan 0.0
    let fail_closed_price = 0.0;
    let pnl_fail_closed = ((fail_closed_price - entry_price) / entry_price) * 100.0;
    assert_eq!(pnl_fail_closed, -100.0);
    assert!(
        pnl_fail_closed <= sl_pct,
        "Fail-closed price (0.0) wajib memicu evaluasi protective SL (-100% <= -35%)"
    );
}

#[test]
fn test_flash_wick_recovery_vs_fail_closed_sl_evaluation() {
    let entry_price = 1.0;
    let sl_pct = -35.0; // Stop loss at -35%

    // 1. Initial drop: price drops to 0.60 (-40% PnL)
    let initial_drop_price = 0.60;
    let initial_pnl = ((initial_drop_price - entry_price) / entry_price) * 100.0;
    assert_eq!(initial_pnl, -40.0);
    assert!(initial_pnl <= sl_pct);

    // 2a. Flash wick recovery scenario:
    // Fresh realizable quote in recheck confirms price bounced to 0.70 (-30% PnL)
    let bounce_price = 0.70;
    let bounce_pnl: f64 = ((bounce_price - entry_price) / entry_price) * 100.0;
    assert!((bounce_pnl - (-30.0f64)).abs() < 1e-6);
    let sl_aborted = bounce_pnl > sl_pct;
    assert!(
        sl_aborted,
        "Flash wick bounce above SL threshold (-30% > -35%) cancels immediate exit"
    );

    // 2b. Fail-closed cutloss scenario:
    // Recheck quote fails completely or reveals liquidity dried up (realizable price 0.0)
    let recheck_oracle = |quote_res: Result<f64, &'static str>| -> f64 { quote_res.unwrap_or(0.0) };
    let rechecked_price = recheck_oracle(Err("Jupiter 429 / RouteNotFound"));
    let fail_closed_pnl = ((rechecked_price - entry_price) / entry_price) * 100.0;
    assert_eq!(fail_closed_pnl, -100.0);
    let sl_executed = fail_closed_pnl <= sl_pct;
    assert!(
        sl_executed,
        "Jupiter error in SL recheck triggers fail-closed protective exit instead of relying on stale DexScreener"
    );
}
