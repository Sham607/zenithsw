use chrono::Utc;
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::gating::ConfluenceEngine;
use zenithsw::models::{
    ActivityType, Chain, ExecutionTrack, ShadowPosition, SmartWallet, TrackedToken, WalletActivity,
    WalletTier,
};

#[tokio::test]
async fn test_dual_track_db_roundtrip_and_routing() {
    let db = Arc::new(SmartWalletDb::new_in_memory().expect("Failed to create in-memory db"));

    let now = Utc::now();
    // 1. Create Swing Whale (FastLane)
    let swing_whale = SmartWallet {
        id: None,
        rank: 1,
        name: "Alpha Swing Master".to_string(),
        handle: "st_5FqU_y17z".to_string(),
        chain: Chain::Solana,
        address: "5FqU_y17z_dummy_swing_address_solana".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 85000.0,
        win_rate_pct: 78.9,
        total_trades: 42,
        source: "SOLANA_TRACKER".to_string(),
        reputation_score: 95.0,
        execution_track: ExecutionTrack::FastLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&swing_whale).unwrap();

    // 2. Create Conviction Whale (VaultLane)
    let vault_whale = SmartWallet {
        id: None,
        rank: 2,
        name: "Solstice Diamond Hand".to_string(),
        handle: "The__Solstice".to_string(),
        chain: Chain::Solana,
        address: "4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 350000.0,
        win_rate_pct: 82.5,
        total_trades: 120,
        source: "FOMO".to_string(),
        reputation_score: 98.0,
        execution_track: ExecutionTrack::VaultLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&vault_whale).unwrap();

    // Verify wallets retrieved maintain correct ExecutionTrack
    let w_swing = db
        .get_wallet_by_address("5FqU_y17z_dummy_swing_address_solana")
        .unwrap()
        .expect("Swing whale missing");
    assert_eq!(w_swing.execution_track, ExecutionTrack::FastLane);

    let w_vault = db
        .get_wallet_by_address("4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo")
        .unwrap()
        .expect("Vault whale missing");
    assert_eq!(w_vault.execution_track, ExecutionTrack::VaultLane);

    // 3. Test Signal Creation & Track Tagging via Confluence Engine
    let config = Config {
        solana_rpc_url: "http://127.0.0.1:8899".to_string(),
        solana_ws_url: "ws://127.0.0.1:8900".to_string(),
        solana_execution_rpc_url: None,
        solana_portfolio_rpc_url: None,
        solana_private_key: None,
        robinhood_rpc_url: "http://127.0.0.1:8545".to_string(),
        robinhood_ws_url: "ws://127.0.0.1:8546".to_string(),
        robinhood_api_key: None,
        evm_private_key: None,
        stream_poll_interval_ms: 500,
        max_price_delta_pct: 5.0,
        min_liquidity_usd: 30000.0,
        position_size_usd: 15.0,
        min_whale_buy_usd: 100.0,
        max_token_exposure_usd: 50.0,
        max_price_impact_pct: 15.0,
        price_sanity_spike_mult: 5.0,
        min_market_cap_usd: 50000.0,
        max_market_cap_usd: 20000000.0,
        confluence_window_secs: 300,
        tier_b_min_confluence_buyers: 2,
        trailing_stop_pct: 10.0,
        trailing_stop_trigger_pct: 35.0,
        stop_loss_pct: 15.0,
        take_profit_pct: 100.0,
        take_profit_2_pct: 300.0,
        tp1_portion_pct: 50.0,
        tp2_portion_pct: 25.0,
        runner_min_pump_mult: 2.0,
        profit_lock_min_current_pnl_pct: 5.0,
        liquidity_drain_usd: 10000.0,
        peak_drawdown_pct: 50.0,
        profit_lock_floor_pct: 20.0,
        whale_full_dump_pct: 95.0,
        solana_buy_slippage_bps: 100,
        solana_tp_slippage_bps: 250,
        solana_trailing_slippage_bps: 300,
        solana_sl_slippage_bps: 500,
        solana_pumpfun_slippage_pct: 15.0,
        solana_pumpfun_priority_fee: 0.005,
        evm_slippage_bps: 50,
        evm_gas_reserve_wei: 2_000_000_000_000_000,
        evm_deadline_secs: 120,
        radar_min_holding_value_usd: 100.0,
        radar_prune_minutes: 30,
        quarantine_base_minutes: 15,
        quarantine_max_minutes: 240,
        demote_consecutive_losses: 2,
        demote_catastrophic_loss_pct: 30.0,
        copytrade_allowed_whales: vec![],
        shadow_exit_enabled: true,
        bot_take_profit_pct: 50.0,
        bot_stop_loss_pct: 20.0,
        bot_trailing_stop_trigger_pct: 30.0,
        bot_trailing_stop_pct: 10.0,
        confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
        dry_run: true,
        sim_initial_balance: 100.0,
        live_robinhood_enabled: false,
        enable_solana: true,
        enable_robinhood: false,
        enable_base: false,
        enable_solana_dex_worker: false,
        solana_dex_token_address: None,
        solana_dex_quote_asset: None,
        solana_dex_token_symbol: None,
        solana_dex_token_decimals: None,
        solana_dex_quote_decimals: None,
        solana_dex_quote_price_usd: None,
        trading_db_path: ":memory:".to_string(),
        wallet_db_path: ":memory:".to_string(),
        candidate_db_path: ":memory:".to_string(),
        ipc_socket_path: "/tmp/zenithsw_test.sock".to_string(),
    };

    let confluence = ConfluenceEngine::new(config, db.clone());

    // Seed token
    let token_mint = "TokenMintDualTrack111111111111111111111111";
    db.upsert_tracked_token(&TrackedToken {
        token_mint: token_mint.to_string(),
        token_symbol: "GEM_DUAL".to_string(),
        chain: Chain::Solana,
        market_cap_usd: 1500000.0,
        liquidity_usd: 250000.0,
        price_usd: 0.05,
        change_24h_pct: 25.0,
        change_6h_pct: 10.0,
        change_1h_pct: 5.0,
        whale_count: 2,
        total_whale_usd: 5000.0,
        top_whale_handles: "@st_5FqU_y17z".to_string(),
        status: "ACTIVE".to_string(),
        upside_tier: "10X_ALPHA".to_string(),
        last_scanned_at: now,
    })
    .unwrap();

    // Activity from Swing Whale
    let act_swing = WalletActivity {
        id: None,
        wallet_handle: "st_5FqU_y17z".to_string(),
        wallet_address: "5FqU_y17z_dummy_swing_address_solana".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GEM_DUAL".to_string(),
        token_mint: token_mint.to_string(),
        amount_usd: 500.0,
        token_amount: 10000.0,
        whale_balance_before: 0.0,
        whale_balance_after: 10000.0,
        execution_price_usd: 0.05,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_swing_buy_01".to_string(),
        timestamp: now,
        source_reference: None,
        parser_version: None,
        finality: None,
    };

    let sig_swing = confluence
        .evaluate_activity(&act_swing, &w_swing, 0.051, 250000.0)
        .unwrap();
    assert_eq!(sig_swing.track, ExecutionTrack::FastLane);
    assert!(sig_swing.is_passed);

    // Activity from Vault Whale
    let act_vault = WalletActivity {
        id: None,
        wallet_handle: "The__Solstice".to_string(),
        wallet_address: "4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GEM_DUAL".to_string(),
        token_mint: token_mint.to_string(),
        amount_usd: 2500.0,
        token_amount: 50000.0,
        whale_balance_before: 0.0,
        whale_balance_after: 50000.0,
        execution_price_usd: 0.05,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_vault_buy_01".to_string(),
        timestamp: now,
        source_reference: None,
        parser_version: None,
        finality: None,
    };

    let sig_vault = confluence
        .evaluate_activity(&act_vault, &w_vault, 0.051, 250000.0)
        .unwrap();
    assert_eq!(sig_vault.track, ExecutionTrack::VaultLane);
    assert!(sig_vault.is_passed);

    // 4. Test TrackPolicy difference
    let fast_policy = ExecutionTrack::FastLane.policy(15.0, 100.0);
    assert_eq!(fast_policy.fixed_stop_loss_pct, Some(15.0));
    assert_eq!(fast_policy.initial_take_profit_pct, 100.0);
    assert_eq!(fast_policy.max_hold_secs, Some(7200)); // 2 hours

    let vault_policy = ExecutionTrack::VaultLane.policy(15.0, 100.0);
    assert_eq!(vault_policy.fixed_stop_loss_pct, None); // Diamond hand: No fixed SL
    assert_eq!(vault_policy.initial_take_profit_pct, 150.0);
    assert_eq!(vault_policy.max_hold_secs, Some(604800)); // 7 days

    // 5. Test ShadowPosition & TradeRecord persistence with Track
    let fast_pos = ShadowPosition {
        id: "pos_fast_01".to_string(),
        chain: Chain::Solana,
        token_symbol: "GEM_DUAL".to_string(),
        token_mint: token_mint.to_string(),
        target_whale_handle: "@st_5FqU_y17z".to_string(),
        target_whale_address: "5FqU_y17z_dummy_swing_address_solana".to_string(),
        track: ExecutionTrack::FastLane,
        entry_price: 0.05,
        current_price: 0.055,
        peak_price: 0.055,
        amount_native: 1000.0,
        amount_usd: 50.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 10.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".to_string(),
        is_dry_run: true,
    };
    db.upsert_position(&fast_pos).unwrap();

    let vault_pos = ShadowPosition {
        id: "pos_vault_01".to_string(),
        chain: Chain::Solana,
        token_symbol: "GEM_DUAL".to_string(),
        token_mint: token_mint.to_string(),
        target_whale_handle: "@The__Solstice".to_string(),
        target_whale_address: "4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo".to_string(),
        track: ExecutionTrack::VaultLane,
        entry_price: 0.05,
        current_price: 0.055,
        peak_price: 0.055,
        amount_native: 400.0,
        amount_usd: 20.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 10.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".to_string(),
        is_dry_run: true,
    };
    db.upsert_position(&vault_pos).unwrap();

    let active_positions = db.get_active_positions().unwrap();
    assert_eq!(active_positions.len(), 2);
    let p_fast = active_positions
        .iter()
        .find(|p| p.id == "pos_fast_01")
        .unwrap();
    assert_eq!(p_fast.track, ExecutionTrack::FastLane);
    let p_vault = active_positions
        .iter()
        .find(|p| p.id == "pos_vault_01")
        .unwrap();
    assert_eq!(p_vault.track, ExecutionTrack::VaultLane);

    // Close position and verify trade_history retains track
    db.close_position_portion("pos_fast_01", 100.0, "FAST_TP", 0.06)
        .unwrap();
    let trades = db.get_recent_trades(5).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].track, ExecutionTrack::FastLane);
    assert_eq!(trades[0].exit_reason, "FAST_TP");
}
