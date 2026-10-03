//! Unit tests for the shared TUI IPC handler state machine.

use std::sync::{Arc, Mutex};
use zenithsw::config::Config;
use zenithsw::ipc::IpcMessage;
use zenithsw::models::{
    ActivityType, Chain, ExecutionTrack, ShadowPosition, ShadowSignal, SmartWallet, TrackedToken,
    TradeRecord, WalletActivity, WalletTier,
};
use zenithsw::tui::{apply_ipc_message, App};

fn test_config() -> Config {
    Config {
        solana_rpc_url: "https://127.0.0.1:1".to_string(),
        solana_ws_url: "ws://127.0.0.1:1".to_string(),
        solana_execution_rpc_url: None,
        solana_portfolio_rpc_url: None,
        solana_private_key: None,
        robinhood_api_key: None,
        evm_private_key: None,
        robinhood_rpc_url: "http://127.0.0.1:1".to_string(),
        robinhood_ws_url: "ws://127.0.0.1:1".to_string(),
        stream_poll_interval_ms: 500,
        max_price_delta_pct: 5.0,
        min_liquidity_usd: 30000.0,
        position_size_usd: 15.0,
        min_whale_buy_usd: 250.0,
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
        take_profit_pct: 50.0,
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
        solana_priority_fee_max_lamports: 100_000,
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
        sim_initial_balance: 100.0,
        shadow_exit_enabled: true,
        bot_take_profit_pct: 50.0,
        bot_stop_loss_pct: 20.0,
        bot_trailing_stop_trigger_pct: 30.0,
        bot_trailing_stop_pct: 10.0,
        confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
        dry_run: true,
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
        trading_db_path: "/tmp/zenithsw_test_trading.db".to_string(),
        wallet_db_path: "/tmp/zenithsw_test_wallet.db".to_string(),
        candidate_db_path: "/tmp/zenithsw_test_candidate.db".to_string(),
        ipc_socket_path: "/tmp/zenithsw_test.sock".to_string(),
    }
}

fn wallet(handle: &str, chain: Chain, tier: WalletTier, active: bool) -> SmartWallet {
    SmartWallet {
        id: None,
        rank: 1,
        name: handle.to_string(),
        handle: handle.to_string(),
        chain,
        address: format!("addr-{handle}"),
        tier,
        pnl_24h_usd: 1000.0,
        win_rate_pct: 70.0,
        total_trades: 10,
        source: "FOMO".to_string(),
        reputation_score: 85.0,
        execution_track: ExecutionTrack::FastLane,
        is_active: active,
        last_seen_at: None,
        updated_at: chrono::Utc::now(),
    }
}

fn tracked_token(chain: Chain) -> TrackedToken {
    TrackedToken {
        token_mint: "mint-token".to_string(),
        token_symbol: "TKN".to_string(),
        chain,
        market_cap_usd: 1_000_000.0,
        liquidity_usd: 100_000.0,
        price_usd: 1.0,
        change_24h_pct: 1.0,
        change_6h_pct: 0.5,
        change_1h_pct: 0.1,
        whale_count: 2,
        total_whale_usd: 5000.0,
        top_whale_handles: "whale-a".to_string(),
        status: "READY".to_string(),
        upside_tier: "B".to_string(),
        last_scanned_at: chrono::Utc::now(),
    }
}

fn activity(chain: Chain) -> WalletActivity {
    WalletActivity {
        id: None,
        wallet_handle: "whale-a".to_string(),
        wallet_address: "addr-whale-a".to_string(),
        chain,
        activity_type: ActivityType::Buy,
        token_symbol: "TKN".to_string(),
        token_mint: "mint-token".to_string(),
        amount_usd: 500.0,
        token_amount: 100.0,
        execution_price_usd: 5.0,
        whale_balance_before: 0.0,
        whale_balance_after: 100.0,
        sell_ratio_pct: 0.0,
        tx_signature: "sig-activity".to_string(),
        timestamp: chrono::Utc::now(),
        source_reference: None,
        parser_version: None,
        finality: None,
    }
}

fn signal(chain: Chain) -> ShadowSignal {
    ShadowSignal {
        chain,
        token_symbol: "TKN".to_string(),
        token_mint: "mint-token".to_string(),
        whale_handle: "whale-a".to_string(),
        whale_address: "addr-whale-a".to_string(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 1.0,
        current_pool_price: 1.05,
        price_delta_pct: 5.0,
        pool_liquidity_usd: 100_000.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 2,
        timestamp: chrono::Utc::now(),
    }
}

fn position(chain: Chain, status: &str) -> ShadowPosition {
    ShadowPosition {
        id: "pos-1".to_string(),
        is_dry_run: true,
        chain,
        token_symbol: "TKN".to_string(),
        token_mint: "mint-token".to_string(),
        target_whale_handle: "whale-a".to_string(),
        target_whale_address: "addr-whale-a".to_string(),
        track: ExecutionTrack::FastLane,
        entry_price: 1.0,
        current_price: 1.1,
        peak_price: 1.2,
        amount_native: 10.0,
        amount_usd: 15.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 10.0,
        realized_pnl_usd: 0.0,
        entry_time: chrono::Utc::now(),
        last_update: chrono::Utc::now(),
        status: status.to_string(),
    }
}

fn trade(chain: Chain) -> TradeRecord {
    TradeRecord {
        timestamp: chrono::Utc::now(),
        chain,
        action: "SELL".to_string(),
        symbol: "TKN".to_string(),
        mint: "mint-token".to_string(),
        target_whale: "whale-a".to_string(),
        exit_reason: "TEST".to_string(),
        track: ExecutionTrack::FastLane,
        portion_sold_pct: 100.0,
        entry_price: 1.0,
        exit_price: 1.2,
        pnl_pct: 20.0,
        pnl_usd: 3.0,
        duration_secs: 60,
    }
}

#[test]
fn snapshot_messages_replace_state_and_respect_chain_filter() {
    let config = test_config();
    let mut app = App::new();

    // Seed stale state; a zero-row sync must clear it.
    app.wallets = vec![wallet("stale", Chain::Solana, WalletTier::TierA, true)];
    app.tracked_tokens = vec![tracked_token(Chain::Solana)];
    app.positions = vec![position(Chain::Solana, "OPEN")];
    app.trades = vec![trade(Chain::Solana)];
    app.activities = vec![activity(Chain::Solana)];
    app.signals = vec![signal(Chain::Solana)];

    apply_ipc_message(
        &mut app,
        IpcMessage::WalletSync { wallets: vec![] },
        &config,
    );
    apply_ipc_message(
        &mut app,
        IpcMessage::TrackedTokensUpdate { tokens: vec![] },
        &config,
    );
    apply_ipc_message(
        &mut app,
        IpcMessage::PositionUpdate { positions: vec![] },
        &config,
    );
    apply_ipc_message(&mut app, IpcMessage::TradeSync { trades: vec![] }, &config);
    apply_ipc_message(
        &mut app,
        IpcMessage::ActivitySync { activities: vec![] },
        &config,
    );
    apply_ipc_message(
        &mut app,
        IpcMessage::SignalSync { signals: vec![] },
        &config,
    );
    assert!(app.wallets.is_empty());
    assert!(app.tracked_tokens.is_empty());
    assert!(app.positions.is_empty());
    assert!(app.trades.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.signals.is_empty());

    // Qualified wallet arrives; a disabled-chain wallet must be filtered out.
    let solana_wallet = wallet("qualified", Chain::Solana, WalletTier::TierS, true);
    let rh_wallet = wallet("rh-only", Chain::Robinhood, WalletTier::TierS, true);
    apply_ipc_message(
        &mut app,
        IpcMessage::WalletSync {
            wallets: vec![solana_wallet.clone(), rh_wallet],
        },
        &config,
    );
    assert_eq!(app.wallets.len(), 1);
    assert_eq!(app.wallets[0].handle, "qualified");
    assert_eq!(app.wallets[0].address, solana_wallet.address);

    // Out-of-scope chain data must never enter TUI state.
    apply_ipc_message(
        &mut app,
        IpcMessage::PositionUpdate {
            positions: vec![position(Chain::Robinhood, "OPEN")],
        },
        &config,
    );
    assert!(app.positions.is_empty());
    apply_ipc_message(
        &mut app,
        IpcMessage::TradeSync {
            trades: vec![trade(Chain::Base)],
        },
        &config,
    );
    assert!(app.trades.is_empty());
}

#[test]
fn event_messages_append_and_snapshot_replaces_them() {
    let config = test_config();
    let mut app = App::new();

    apply_ipc_message(
        &mut app,
        IpcMessage::ActivityBroadcast {
            activity: activity(Chain::Solana),
        },
        &config,
    );
    apply_ipc_message(
        &mut app,
        IpcMessage::SignalBroadcast {
            signal: signal(Chain::Solana),
        },
        &config,
    );
    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.signals.len(), 1);

    // Periodic snapshot is authoritative: it replaces event-appended rows.
    apply_ipc_message(
        &mut app,
        IpcMessage::ActivitySync { activities: vec![] },
        &config,
    );
    apply_ipc_message(
        &mut app,
        IpcMessage::SignalSync { signals: vec![] },
        &config,
    );
    assert!(app.activities.is_empty());
    assert!(app.signals.is_empty());
}

#[test]
fn heartbeat_updates_connection_and_dry_run_mode() {
    let config = test_config();
    let mut app = App::new();
    apply_ipc_message(
        &mut app,
        IpcMessage::Heartbeat {
            uptime_secs: 120,
            heartbeat_seq: 7,
            solana_latency_ms: 25,
            arbitrum_latency_ms: 30,
            base_latency_ms: 0,
            tracked_wallets_count: 3,
            active_positions_count: 1,
            wallet_sol_balance: 1.5,
            sol_price_usd: 150.0,
            wallet_equity_usd: 300.0,
            total_realized_pnl_usd: 12.5,
            min_liquidity_usd: 30000.0,
            max_price_delta_pct: 5.0,
            position_size_usd: 15.0,
            stop_loss_pct: 15.0,
            take_profit_pct: 50.0,
            shadow_exit_enabled: false,
            min_whale_buy_usd: 250.0,
            is_live_mode: false,
            wallet_pubkey: "pubkey".to_string(),
            heartbeat_timestamp_unix: 1_000,
            solana_rpc_requests: 10,
            solana_rpc_successes: 9,
            solana_rpc_rate_limited: 1,
            solana_rpc_failures: 0,
            robinhood_rpc_requests: 0,
            robinhood_rpc_successes: 0,
            robinhood_rpc_rate_limited: 0,
            robinhood_rpc_failures: 0,
            qualified_wallets_count: 2,
            polled_wallets_count: 3,
            global_scanned_wallets_count: 40,
            candidate_wallets_count: 5,
        },
        &config,
    );
    assert!(app.is_connected_ipc);
    assert!(app.heartbeat_received_at.is_some());
    assert_eq!(app.heartbeat_seq, 7);
    assert_eq!(app.uptime_secs, 120);
    assert!(!app.is_live_mode);
    assert!(!app.shadow_exit_enabled);
    assert_eq!(app.qualified_wallets_count, 2);
    assert_eq!(app.candidate_wallets_count, 5);
}

#[test]
fn handler_is_shareable_across_threads_like_production_tui() {
    let config = Arc::new(test_config());
    let app = Arc::new(Mutex::new(App::new()));
    let handle = {
        let app = Arc::clone(&app);
        let config = Arc::clone(&config);
        std::thread::spawn(move || {
            if let Ok(mut state) = app.lock() {
                apply_ipc_message(
                    &mut state,
                    IpcMessage::WalletSync {
                        wallets: vec![wallet("threaded", Chain::Solana, WalletTier::TierA, true)],
                    },
                    &config,
                );
            }
        })
    };
    handle.join().expect("thread tidak boleh panic");
    assert_eq!(app.lock().unwrap().wallets.len(), 1);
}

#[test]
fn radar_snapshot_replaces_state_and_filters_disabled_chain() {
    let config = test_config();
    let mut app = App::new();
    let solana_projection = zenithsw::radar::RadarProjection {
        chain: Chain::Solana,
        token_address: "Mint111".to_string(),
        symbol: Some("TEST".to_string()),
        pool_address: Some("Pool111".to_string()),
        dex_id: Some("test-dex".to_string()),
        market_cap_usd: 1_000_000.0,
        liquidity_usd: 50_000.0,
        price_usd: Some(1.0),
        change_24h_pct: None,
        change_6h_pct: None,
        change_1h_pct: None,
        market_structure: "UNAVAILABLE_OHLCV".to_string(),
        price_action: "UNAVAILABLE_OHLCV".to_string(),
        qualified_holder_count: 1,
        candidate_holder_count: 0,
        independent_cluster_count: 1,
        direct_accumulator_count: 1,
        whales: None,
        status: zenithsw::radar::RadarStatus::PortfolioOnly,
    };
    let robinhood_projection = zenithsw::radar::RadarProjection {
        chain: Chain::Robinhood,
        token_address: "0x1111111111111111111111111111111111111111".to_string(),
        symbol: Some("TEST".to_string()),
        pool_address: Some("Pool111".to_string()),
        dex_id: Some("test-dex".to_string()),
        market_cap_usd: 1_000_000.0,
        liquidity_usd: 50_000.0,
        price_usd: Some(1.0),
        change_24h_pct: None,
        change_6h_pct: None,
        change_1h_pct: None,
        market_structure: "UNAVAILABLE_OHLCV".to_string(),
        price_action: "UNAVAILABLE_OHLCV".to_string(),
        qualified_holder_count: 1,
        candidate_holder_count: 0,
        independent_cluster_count: 1,
        direct_accumulator_count: 1,
        whales: None,
        status: zenithsw::radar::RadarStatus::StructureConfirmed,
    };

    apply_ipc_message(
        &mut app,
        IpcMessage::RadarSync {
            projections: vec![solana_projection, robinhood_projection],
        },
        &config,
    );
    assert_eq!(app.radar_projections.len(), 1);
    assert_eq!(app.radar_projections[0].chain, Chain::Solana);

    apply_ipc_message(
        &mut app,
        IpcMessage::RadarSync {
            projections: vec![],
        },
        &config,
    );
    assert!(app.radar_projections.is_empty());
}

#[test]
fn activity_broadcast_preserves_provenance_and_verification_tag() {
    let mut app = App::new();
    let config = test_config();
    let now = chrono::Utc::now();

    let verified_act = WalletActivity {
        id: None,
        wallet_handle: "whale-dex".to_string(),
        wallet_address: "Qualified111".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "TOK".to_string(),
        token_mint: "Token111".to_string(),
        amount_usd: 1500.0,
        token_amount: 100.0,
        execution_price_usd: 15.0,
        whale_balance_before: 0.0,
        whale_balance_after: 100.0,
        sell_ratio_pct: 0.0,
        tx_signature: "SigHarness12345678".to_string(),
        timestamp: now,
        source_reference: Some("SigHarness12345678".to_string()),
        parser_version: Some("dex-materializer-v1".to_string()),
        finality: Some("UNKNOWN".to_string()),
    };

    apply_ipc_message(
        &mut app,
        IpcMessage::ActivityBroadcast {
            activity: verified_act.clone(),
        },
        &config,
    );

    assert_eq!(app.activities.len(), 1);
    let stored = &app.activities[0];
    assert_eq!(stored.tx_signature, "SigHarness12345678");
    assert_eq!(
        stored.parser_version.as_deref(),
        Some("dex-materializer-v1")
    );
    assert_eq!(
        stored.source_reference.as_deref(),
        Some("SigHarness12345678")
    );
}
