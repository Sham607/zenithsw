use chrono::Utc;
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::SolanaExecutor;
use zenithsw::gating::ConfluenceEngine;
use zenithsw::indexer::scoring::ReputationScorer;
use zenithsw::ingestion::{FomoScraper, WalletResolver};
use zenithsw::models::{
    ActivityType, Chain, ExecutionTrack, ShadowPosition, ShadowSignal, SmartWallet, TrackedToken,
    WalletActivity, WalletTier,
};

fn seed_test_token(
    db: &SmartWalletDb,
    mint: &str,
    symbol: &str,
    chain: Chain,
    now: chrono::DateTime<Utc>,
) {
    db.upsert_tracked_token(&TrackedToken {
        token_mint: mint.to_string(),
        token_symbol: symbol.to_string(),
        chain,
        market_cap_usd: 850_000.0,
        liquidity_usd: 1_000_000.0,
        price_usd: 1.0,
        change_24h_pct: 0.0,
        change_6h_pct: 0.0,
        change_1h_pct: 0.0,
        whale_count: 2,
        total_whale_usd: 5_000.0,
        top_whale_handles: "test".to_string(),
        status: "ACCUMULATION_BOTTOM".to_string(),
        upside_tier: "NANO_CAP".to_string(),
        last_scanned_at: now,
    })
    .unwrap();
}

fn get_mock_config() -> Config {
    Config {
        solana_rpc_url: "https://mainnet.helius-rpc.com/?api-key=test".to_string(),
        solana_ws_url: "wss://mainnet.helius-rpc.com/?api-key=test".to_string(),
        solana_private_key: None,
        robinhood_rpc_url: "https://robinhood-rpc.test".to_string(),
        robinhood_ws_url: "wss://robinhood-rpc.test".to_string(),
        robinhood_api_key: None,
        evm_private_key: None,
        fomo_auth_token: None,
        fomo_sync_interval_sec: 60,
        stream_poll_interval_ms: 500,
        max_price_delta_pct: 5.0,
        min_liquidity_usd: 30000.0,
        position_size_usd: 15.0,
        min_whale_buy_usd: 250.0,
        max_price_impact_pct: 15.0,
        min_market_cap_usd: 50000.0,
        max_market_cap_usd: 20000000.0,
        trailing_stop_pct: 10.0,
        stop_loss_pct: 15.0,
        take_profit_pct: 50.0,
        shadow_exit_enabled: true,
        confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
        dry_run: true,
        live_robinhood_enabled: false,
        enable_solana: true,
        enable_robinhood: false,
        enable_base: false,
        trading_db_path: ":memory:".to_string(),
        wallet_db_path: ":memory:".to_string(),
        candidate_db_path: ":memory:".to_string(),
        ipc_socket_path: "/tmp/zenithsw_test.sock".to_string(),
    }
}

#[tokio::test]
async fn test_database_wallet_and_activity_crud() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let now = Utc::now();

    let wallet = SmartWallet {
        id: None,
        rank: 1,
        name: "Ethermonk 📿".to_string(),
        handle: "ether_monk".to_string(),
        chain: Chain::Solana,
        address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 254395.02,
        win_rate_pct: 85.0,
        total_trades: 37,
        source: "FOMO".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 95.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };

    db.upsert_wallet(&wallet).unwrap();
    let wallets = db.get_active_wallets().unwrap();
    assert_eq!(wallets.len(), 1);
    assert_eq!(wallets[0].handle, "ether_monk");
    assert_eq!(wallets[0].tier, WalletTier::TierS);

    let activity = WalletActivity {
        id: None,
        wallet_handle: "ether_monk".to_string(),
        wallet_address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "STONK".to_string(),
        token_mint: "Mint111111111111111111111111111111111111111".to_string(),
        amount_usd: 5000.0,
        token_amount: 10000.0,
        execution_price_usd: 0.50,
        whale_balance_before: 0.0,
        whale_balance_after: 10000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "sig123".to_string(),
        timestamp: now,
    };

    db.insert_activity(&activity).unwrap();
    let recent = db.get_recent_activities(10).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].token_symbol, "STONK");
}

#[tokio::test]
async fn test_gating_price_delta_max_5_pct() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let config = get_mock_config();
    let confluence = ConfluenceEngine::new(config, db.clone());
    let now = Utc::now();

    let wallet = SmartWallet {
        id: None,
        rank: 1,
        name: "Ethermonk".to_string(),
        handle: "ether_monk".to_string(),
        chain: Chain::Solana,
        address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 250000.0,
        win_rate_pct: 85.0,
        total_trades: 30,
        source: "FOMO".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 95.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };

    let activity = WalletActivity {
        id: None,
        wallet_handle: "ether_monk".to_string(),
        wallet_address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "ALPHA".to_string(),
        token_mint: "MintAlpha11111111111111111111111111111111".to_string(),
        amount_usd: 2000.0,
        token_amount: 2000.0,
        execution_price_usd: 1.00,
        whale_balance_before: 0.0,
        whale_balance_after: 2000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "sig_alpha".to_string(),
        timestamp: now,
    };

    // Case 1: Current Price = 1.03 (+3.0% delta) -> Should PASS (<= +5.0%)
    seed_test_token(
        &db,
        &activity.token_mint,
        &activity.token_symbol,
        activity.chain,
        now,
    );
    let signal_pass = confluence
        .evaluate_activity(&activity, &wallet, 1.03, 100000.0)
        .unwrap();
    assert!(signal_pass.is_passed);
    assert!(signal_pass.rejection_reason.is_none());

    // Case 2: Current Price = 1.08 (+8.0% delta) -> Should REJECT (> +5.0%)
    let signal_fail = confluence
        .evaluate_activity(&activity, &wallet, 1.08, 100000.0)
        .unwrap();
    assert!(!signal_fail.is_passed);
    assert!(signal_fail
        .rejection_reason
        .unwrap()
        .contains("PRICE_DELTA_TOO_HIGH"));
}

#[tokio::test]
async fn test_gating_liquidity_floor_and_tier_confluence() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let config = get_mock_config();
    let confluence = ConfluenceEngine::new(config, db.clone());
    let now = Utc::now();

    // Tier-B Hunter (Requires Confluence >= 2)
    let wallet_b = SmartWallet {
        id: None,
        rank: 25,
        name: "BetaHunter".to_string(),
        handle: "betahunter".to_string(),
        chain: Chain::Solana,
        address: "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM".to_string(),
        tier: WalletTier::TierB,
        pnl_24h_usd: 15000.0,
        win_rate_pct: 60.0,
        total_trades: 12,
        source: "FOMO".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 75.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };

    let activity = WalletActivity {
        id: None,
        wallet_handle: "betahunter".to_string(),
        wallet_address: "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "BETA".to_string(),
        token_mint: "MintBeta11111111111111111111111111111111".to_string(),
        amount_usd: 1000.0,
        token_amount: 1000.0,
        execution_price_usd: 1.00,
        whale_balance_before: 0.0,
        whale_balance_after: 1000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "sig_beta".to_string(),
        timestamp: now,
    };

    // Case 1: Low Liquidity $15,000 < $30,000 -> Should REJECT
    let signal_liq = confluence
        .evaluate_activity(&activity, &wallet_b, 1.01, 15000.0)
        .unwrap();
    assert!(!signal_liq.is_passed);
    assert!(signal_liq
        .rejection_reason
        .unwrap()
        .contains("LIQUIDITY_TOO_LOW"));

    // Case 2: Good Liquidity $45k, but only 1 Tier-B buyer -> Should REJECT for confluence
    seed_test_token(
        &db,
        &activity.token_mint,
        &activity.token_symbol,
        activity.chain,
        now,
    );
    let signal_conf = confluence
        .evaluate_activity(&activity, &wallet_b, 1.01, 45000.0)
        .unwrap();
    assert!(!signal_conf.is_passed);
    assert!(signal_conf
        .rejection_reason
        .unwrap()
        .contains("TIER_B_CONFLUENCE_REQUIRED"));

    // Insert another buyer for the same mint to achieve 2x confluence
    let activity2 = WalletActivity {
        id: None,
        wallet_handle: "hunter2".to_string(),
        wallet_address: "AddrHunter22222222222222222222222222222".to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "BETA".to_string(),
        token_mint: "MintBeta11111111111111111111111111111111".to_string(),
        amount_usd: 1200.0,
        token_amount: 1200.0,
        execution_price_usd: 1.01,
        whale_balance_before: 0.0,
        whale_balance_after: 1200.0,
        sell_ratio_pct: 0.0,
        tx_signature: "sig_beta2".to_string(),
        timestamp: now,
    };
    db.insert_activity(&activity).unwrap();
    db.insert_activity(&activity2).unwrap();

    let signal_conf_pass = confluence
        .evaluate_activity(&activity, &wallet_b, 1.02, 45000.0)
        .unwrap();
    assert!(signal_conf_pass.is_passed);
}

#[tokio::test]
async fn test_proportional_partial_and_full_mirror_exit() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let config = get_mock_config();
    let sol_exec = SolanaExecutor::new(config.clone(), db.clone());
    let now = Utc::now();

    let signal = ShadowSignal {
        chain: Chain::Solana,
        token_symbol: "PUMP".to_string(),
        token_mint: "PumpMint1111111111111111111111111111111".to_string(),
        whale_handle: "ether_monk".to_string(),
        whale_address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 0.10,
        current_pool_price: 0.102,
        price_delta_pct: 2.0,
        pool_liquidity_usd: 60000.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 1,
        timestamp: now,
    };

    // 1. Open Position
    let pos = sol_exec.execute_shadow_buy(&signal).await.unwrap().unwrap();
    assert_eq!(pos.token_symbol, "PUMP");
    assert_eq!(pos.amount_usd, 15.0);
    assert_eq!(pos.remaining_portion_pct, 100.0);

    // 2. Whale sells 50% partial bag -> Bot sells 50% partial shadow position
    sol_exec
        .execute_partial_shadow_sell(&pos.id, 50.0, "WHALE_PARTIAL_DUMP", 0.15)
        .await
        .unwrap();

    let active_pos = db.get_active_positions().unwrap();
    assert_eq!(active_pos.len(), 1);
    assert_eq!(active_pos[0].remaining_portion_pct, 50.0);
    assert_eq!(active_pos[0].status, "PARTIAL_EXIT");

    // 3. Whale sells remaining 100% -> Bot closes position completely
    sol_exec
        .execute_shadow_sell(&pos.id, "WHALE_FULL_DUMP", 0.20)
        .await
        .unwrap();

    let remaining_positions = db.get_active_positions().unwrap();
    assert_eq!(remaining_positions.len(), 0);

    let trades = db.get_recent_trades(10).unwrap();
    assert_eq!(trades.len(), 2);
    assert_eq!(trades[0].action, "SELL"); // Most recent
    assert_eq!(trades[1].action, "PARTIAL_SELL");
}

#[test]
fn test_indexer_reputation_scoring() {
    // 1. Dev honeypot check -> Must be 0 score and rejected
    let dev_score = ReputationScorer::evaluate("DevAddr1111", true, 5, 90.0, 10, 50000.0, 30);
    assert!(dev_score.is_dev_or_creator);
    assert_eq!(dev_score.reputation_score, 0.0);
    assert!(!dev_score.is_eligible_tier_a);

    // 2. Disposable wallet (< 3 days) -> Must be rejected
    let fresh_score = ReputationScorer::evaluate("FreshAddr1111", false, 3, 80.0, 5, 20000.0, 1);
    assert!(!fresh_score.is_eligible_tier_a);

    // 3. Elite On-Chain Smart Money (>70 score, >$10k PnL, >60% Win Rate, multi-token)
    let elite_score = ReputationScorer::evaluate("WhaleAddr1111", false, 4, 75.0, 25, 45000.0, 60);
    assert!(elite_score.is_eligible_tier_a);
    assert!(elite_score.reputation_score >= 70.0);
}

#[test]
fn test_fomo_seed_traders_and_resolver() {
    let seeds = FomoScraper::get_default_seed_traders();
    assert!(seeds.len() >= 15);
    assert_eq!(seeds[0].handle, "unipcs");
    assert_eq!(seeds[0].chain, Chain::Solana);

    let res_sol = WalletResolver::resolve_chain_and_address("ether_monk").unwrap();
    assert_eq!(res_sol.0, Chain::Solana);
    assert_eq!(res_sol.1, "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i");

    let res_rh = WalletResolver::resolve_chain_and_address("0xAvast").unwrap();
    assert_eq!(res_rh.0, Chain::Robinhood);
    assert_eq!(res_rh.1, "0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D");
}

#[tokio::test]
async fn test_qualified_wallets_and_auto_tp_protocol() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let now = Utc::now();

    // 1. Insert Tier-S, Tier-A, and Tier-B wallets
    let tier_s = SmartWallet {
        id: None,
        rank: 1,
        name: "Tier S Whale".to_string(),
        handle: "whale_s".to_string(),
        chain: Chain::Solana,
        address: "AddrS11111111111111111111111111111111111111".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 150000.0,
        win_rate_pct: 85.0,
        total_trades: 120,
        source: "ONCHAIN".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 95.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };
    let tier_a = SmartWallet {
        id: None,
        rank: 2,
        name: "Tier A Sniper".to_string(),
        handle: "sniper_a".to_string(),
        chain: Chain::Solana,
        address: "AddrA11111111111111111111111111111111111111".to_string(),
        tier: WalletTier::TierA,
        pnl_24h_usd: 25000.0,
        win_rate_pct: 70.0,
        total_trades: 45,
        source: "ONCHAIN".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 82.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };
    let tier_b = SmartWallet {
        id: None,
        rank: 3,
        name: "Tier B Micro".to_string(),
        handle: "micro_b".to_string(),
        chain: Chain::Solana,
        address: "AddrB11111111111111111111111111111111111111".to_string(),
        tier: WalletTier::TierB,
        pnl_24h_usd: 4.0,
        win_rate_pct: 50.0,
        total_trades: 10,
        source: "ONCHAIN".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 45.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };

    db.upsert_wallet(&tier_s).unwrap();
    db.upsert_wallet(&tier_a).unwrap();
    db.upsert_wallet(&tier_b).unwrap();

    // Verify all active vs qualified filter
    let all_wallets = db.get_active_wallets().unwrap();
    assert_eq!(all_wallets.len(), 3);

    let qualified = db.get_qualified_wallets().unwrap();
    assert_eq!(qualified.len(), 2);
    assert_eq!(qualified[0].tier, WalletTier::TierS);
    assert_eq!(qualified[1].tier, WalletTier::TierA);

    // 2. Test Auto Take Profit 50% Protocol on +100% PnL
    let config = get_mock_config();
    let sol_exec = SolanaExecutor::new(config, db.clone());
    let signal = ShadowSignal {
        chain: Chain::Solana,
        token_symbol: "MOON".to_string(),
        token_mint: "MoonMint111111111111111111111111111111111111".to_string(),
        whale_handle: "whale_s".to_string(),
        whale_address: "AddrS11111111111111111111111111111111111111".to_string(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 1.0,
        current_pool_price: 1.0,
        price_delta_pct: 0.0,
        pool_liquidity_usd: 50000.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 1,
        timestamp: now,
    };

    let pos = sol_exec.execute_shadow_buy(&signal).await.unwrap().unwrap();
    assert_eq!(pos.remaining_portion_pct, 100.0);

    // Simulate +100% price pump (Entry 1.0 -> 2.0)
    sol_exec
        .execute_partial_shadow_sell(&pos.id, 50.0, "AUTO_TP_100PCT_FREE_ROLL", 2.0)
        .await
        .unwrap();

    let active_pos = db.get_active_positions().unwrap();
    assert_eq!(active_pos.len(), 1);
    assert_eq!(active_pos[0].remaining_portion_pct, 50.0);
    assert_eq!(active_pos[0].status, "PARTIAL_EXIT");

    // Verify Trade History records PARTIAL_SELL
    let trades = db.get_recent_trades(5).unwrap();
    assert_eq!(trades.len(), 1);
    assert_eq!(trades[0].action, "PARTIAL_SELL");
    assert_eq!(trades[0].exit_reason, "AUTO_TP_100PCT_FREE_ROLL");
    assert_eq!(trades[0].pnl_pct, 100.0);
}

#[tokio::test]
async fn test_multi_whale_averaging_and_same_whale_reentry_accumulation() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let mut config = get_mock_config();
    config.position_size_usd = 10.0;
    config.min_whale_buy_usd = 150.0;
    config.max_price_delta_pct = 10.0;
    config.max_market_cap_usd = 25000000.0;

    let confluence = ConfluenceEngine::new(config.clone(), db.clone());
    let sol_exec = SolanaExecutor::new(config.clone(), db.clone());

    let mint = "TokenMint1234567890abcdef1234567890abcdef12".to_string();
    let now = Utc::now();
    seed_test_token(&db, &mint, "GEM", Chain::Solana, now);

    let whale1 = SmartWallet {
        id: None,
        rank: 1,
        name: "Sol Whale Alpha".to_string(),
        handle: "sol_whale_alpha".to_string(),
        chain: Chain::Solana,
        address: "AlphaWhale111111111111111111111111111111111".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 250000.0,
        win_rate_pct: 82.0,
        total_trades: 120,
        source: "FOMO".to_string(),
        reputation_score: 95.0,
        execution_track: ExecutionTrack::VaultLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&whale1).unwrap();

    let whale2 = SmartWallet {
        id: None,
        rank: 2,
        name: "Sol Whale Beta".to_string(),
        handle: "sol_whale_beta".to_string(),
        chain: Chain::Solana,
        address: "BetaWhale2222222222222222222222222222222222".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 180000.0,
        win_rate_pct: 78.0,
        total_trades: 95,
        source: "FOMO".to_string(),
        reputation_score: 90.0,
        execution_track: ExecutionTrack::VaultLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&whale2).unwrap();

    // 1. Whale 1 Buys Token at $0.010 -> Pos size $10.0
    let act1 = WalletActivity {
        id: None,
        wallet_address: whale1.address.clone(),
        wallet_handle: whale1.handle.clone(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GEM".to_string(),
        token_mint: mint.clone(),
        amount_usd: 5000.0,
        token_amount: 500_000.0,
        execution_price_usd: 0.010,
        whale_balance_before: 0.0,
        whale_balance_after: 500_000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_buy_1".to_string(),
        timestamp: now,
    };

    let sig1 = confluence
        .evaluate_activity(&act1, &whale1, 0.010, 250000.0)
        .unwrap();
    assert!(sig1.is_passed, "Whale 1 initial buy should pass gating");

    let pos1: Option<ShadowPosition> = sol_exec.execute_shadow_buy(&sig1).await.unwrap();
    let pos1 = pos1.unwrap();
    assert_eq!(pos1.entry_price, 0.010);
    assert_eq!(pos1.amount_usd, 10.0);
    assert_eq!(pos1.amount_native, 1000.0); // 10.0 / 0.010 = 1000 tokens

    // 2. Whale 1 attempts to buy AGAIN on the same token -> MUST BE BLOCKED (same-whale re-entry rule)
    let act1_repeat = WalletActivity {
        id: None,
        wallet_address: whale1.address.clone(),
        wallet_handle: whale1.handle.clone(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GEM".to_string(),
        token_mint: mint.clone(),
        amount_usd: 3000.0,
        token_amount: 300_000.0,
        execution_price_usd: 0.012,
        whale_balance_before: 500_000.0,
        whale_balance_after: 800_000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_buy_repeat".to_string(),
        timestamp: now,
    };

    let sig1_repeat = confluence
        .evaluate_activity(&act1_repeat, &whale1, 0.012, 250000.0)
        .unwrap();
    assert!(
        sig1_repeat.is_passed,
        "Signal should still pass gating (confluence engine tidak tahu state posisi)"
    );

    // Executor HARUS memblokir karena whale1 sudah berkontribusi ke posisi ini
    let pos1_reentry: Option<ShadowPosition> =
        sol_exec.execute_shadow_buy(&sig1_repeat).await.unwrap();
    assert!(
        pos1_reentry.is_none(),
        "Same-whale re-entry HARUS diblokir oleh executor — 1 whale = 1 kontribusi per posisi terbuka"
    );

    // Verifikasi posisi tidak berubah setelah attempt re-entry
    let positions_after_block = db.get_active_positions().unwrap();
    let unchanged = positions_after_block
        .iter()
        .find(|p| p.token_mint == mint)
        .unwrap();
    assert_eq!(
        unchanged.amount_usd, 10.0,
        "Posisi tidak boleh berubah setelah same-whale re-entry diblokir"
    );

    // 3. Whale 2 (DIFFERENT WHALE) buys Token at $0.020 -> MUST BE ALLOWED & AVERAGED IN!
    let act2 = WalletActivity {
        id: None,
        wallet_address: whale2.address.clone(),
        wallet_handle: whale2.handle.clone(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GEM".to_string(),
        token_mint: mint.clone(),
        amount_usd: 7000.0,
        token_amount: 350_000.0,
        execution_price_usd: 0.020,
        whale_balance_before: 0.0,
        whale_balance_after: 350_000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_buy_2".to_string(),
        timestamp: now,
    };

    let sig2 = confluence
        .evaluate_activity(&act2, &whale2, 0.020, 250000.0)
        .unwrap();
    assert!(sig2.is_passed, "Whale 2 buy should pass gating");

    let pos2: Option<ShadowPosition> = sol_exec.execute_shadow_buy(&sig2).await.unwrap();
    let pos2 = pos2.unwrap();

    // Total USD = 10.0 (whale1 initial) + 10.0 (whale2) = 20.0
    // Whale1 re-entry diblokir — posisi_usd tidak berubah dari 10.0 sebelum whale2 masuk
    // Total tokens = 1000.0 (whale1 @0.010) + (10.0 / 0.020 = 500.0) (whale2) = 1500.0
    assert_eq!(pos2.amount_usd, 20.0);
    assert!(pos2.amount_native > 1400.0);

    // Verify handles and addresses recorded
    assert!(pos2.target_whale_handle.contains("@sol_whale_alpha"));
    assert!(pos2.target_whale_handle.contains("@sol_whale_beta"));
}

#[tokio::test]
async fn test_min_whale_buy_and_price_impact_gating() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let config = get_mock_config(); // min_whale_buy_usd: 250.0, max_price_impact_pct: 3.5
    let confluence = ConfluenceEngine::new(config, db.clone());
    let now = Utc::now();

    let whale = SmartWallet {
        id: None,
        rank: 1,
        name: "Aurelius0121 👑".to_string(),
        handle: "Aurelius0121".to_string(),
        chain: Chain::Robinhood,
        address: "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 341442.0,
        win_rate_pct: 92.0,
        total_trades: 50,
        source: "FOMO".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 98.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };
    db.upsert_wallet(&whale).unwrap();

    let mint = "0xd7321801caae694090694ff55a9323139f043b88".to_string();

    // 1. Dust Buy ($15.00 < $250.00 min) -> MUST BE REJECTED!
    let dust_act = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "JUGGERNAUT".to_string(),
        token_mint: mint.clone(),
        amount_usd: 15.0,
        token_amount: 1000.0,
        execution_price_usd: 0.015,
        whale_balance_before: 0.0,
        whale_balance_after: 1000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_dust".to_string(),
        timestamp: now,
    };

    let sig_dust = confluence
        .evaluate_activity(&dust_act, &whale, 0.015, 100_000.0)
        .unwrap();
    assert!(!sig_dust.is_passed, "Dust buy under $250 must be rejected");
    assert!(
        sig_dust
            .rejection_reason
            .unwrap()
            .contains("REJECTED_WHALE_BUY_TOO_SMALL"),
        "Rejection reason must indicate buy too small"
    );

    // 2. Whale Big Buy with High Price Impact ($10,000 buy on $50,000 pool = 20% impact > 15.0%) -> MUST BE REJECTED!
    let high_impact_act = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "JUGGERNAUT".to_string(),
        token_mint: mint.clone(),
        amount_usd: 10000.0,
        token_amount: 666_666.0,
        execution_price_usd: 0.015,
        whale_balance_before: 0.0,
        whale_balance_after: 666_666.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_high_impact".to_string(),
        timestamp: now,
    };

    let sig_high_impact = confluence
        .evaluate_activity(&high_impact_act, &whale, 0.015, 50_000.0)
        .unwrap(); // 10000 / 50000 = 20% > 15.0%
    assert!(
        !sig_high_impact.is_passed,
        "High price impact buy must be rejected"
    );
    assert!(
        sig_high_impact
            .rejection_reason
            .unwrap()
            .contains("PRICE_IMPACT_TOO_HIGH"),
        "Rejection reason must indicate price impact too high"
    );

    // 3. High-Conviction Whale Buy ($1,500 buy on $900,000 pool = 0.16% impact) -> MUST PASS!
    let valid_act = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "JUGGERNAUT".to_string(),
        token_mint: mint.clone(),
        amount_usd: 1500.0,
        token_amount: 100_000.0,
        execution_price_usd: 0.015,
        whale_balance_before: 0.0,
        whale_balance_after: 100_000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_valid".to_string(),
        timestamp: now,
    };

    seed_test_token(
        &db,
        &valid_act.token_mint,
        &valid_act.token_symbol,
        valid_act.chain,
        now,
    );
    let sig_valid = confluence
        .evaluate_activity(&valid_act, &whale, 0.015, 900_000.0)
        .unwrap();
    assert!(
        sig_valid.is_passed,
        "Valid high conviction buy must pass gating"
    );
    assert_eq!(sig_valid.rejection_reason, None);
}

#[tokio::test]
async fn test_market_cap_range_gating_50k_to_20m() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let config = get_mock_config();
    let confluence = ConfluenceEngine::new(config, db.clone());
    let now = Utc::now();

    let whale = SmartWallet {
        id: None,
        rank: 1,
        name: "Aurelius".to_string(),
        handle: "Aurelius0121".to_string(),
        chain: Chain::Robinhood,
        address: "0xaurelius1234567890abcdef1234567890abcdef".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 850000.0,
        win_rate_pct: 91.0,
        total_trades: 45,
        source: "FOMO".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 98.0,
        is_active: true,
        last_seen_at: None,
        updated_at: now,
    };

    // 1. Insert Tracked Token with MC = $30k (< $50k min)
    let token_sub_50k = zenithsw::models::TrackedToken {
        token_mint: "0xtoken_low_mc".to_string(),
        token_symbol: "LOWMC".to_string(),
        chain: Chain::Robinhood,
        market_cap_usd: 30_000.0,
        liquidity_usd: 35_000.0,
        price_usd: 0.003,
        change_24h_pct: 5.0,
        change_6h_pct: 2.0,
        change_1h_pct: 1.0,
        whale_count: 2,
        total_whale_usd: 50_000.0,
        top_whale_handles: "@Aurelius0121".to_string(),
        status: "ACCUMULATION_BOTTOM".to_string(),
        upside_tier: "NANO_CAP".to_string(),
        last_scanned_at: now,
    };
    db.upsert_tracked_token(&token_sub_50k).unwrap();

    let act_low = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "LOWMC".to_string(),
        token_mint: "0xtoken_low_mc".to_string(),
        amount_usd: 500.0,
        token_amount: 166_666.0,
        execution_price_usd: 0.003,
        whale_balance_before: 0.0,
        whale_balance_after: 166_666.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_low_mc".to_string(),
        timestamp: now,
    };

    let sig_low = confluence
        .evaluate_activity(&act_low, &whale, 0.003, 35_000.0)
        .unwrap();
    assert!(!sig_low.is_passed);
    assert!(sig_low
        .rejection_reason
        .unwrap()
        .contains("REJECTED_MARKET_CAP_TOO_LOW"));

    // 2. Insert Tracked Token with MC = $50M (> $20M max)
    let token_high_mc = zenithsw::models::TrackedToken {
        token_mint: "0xtoken_high_mc".to_string(),
        token_symbol: "HIGHMC".to_string(),
        chain: Chain::Robinhood,
        market_cap_usd: 50_000_000.0,
        liquidity_usd: 500_000.0,
        price_usd: 5.0,
        change_24h_pct: 5.0,
        change_6h_pct: 2.0,
        change_1h_pct: 1.0,
        whale_count: 5,
        total_whale_usd: 500_000.0,
        top_whale_handles: "@Aurelius0121".to_string(),
        status: "ACCUMULATION_BOTTOM".to_string(),
        upside_tier: "MID_LARGE".to_string(),
        last_scanned_at: now,
    };
    db.upsert_tracked_token(&token_high_mc).unwrap();

    let act_high = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "HIGHMC".to_string(),
        token_mint: "0xtoken_high_mc".to_string(),
        amount_usd: 1000.0,
        token_amount: 200.0,
        execution_price_usd: 5.0,
        whale_balance_before: 0.0,
        whale_balance_after: 200.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_high_mc".to_string(),
        timestamp: now,
    };

    let sig_high = confluence
        .evaluate_activity(&act_high, &whale, 5.0, 500_000.0)
        .unwrap();
    assert!(!sig_high.is_passed);
    assert!(sig_high
        .rejection_reason
        .unwrap()
        .contains("REJECTED_MARKET_CAP_TOO_HIGH"));

    // 3. Insert Tracked Token with MC = $850k (In sweet spot $50k - $20M) -> PASS!
    let token_sweet = zenithsw::models::TrackedToken {
        token_mint: "0xtoken_sweet_spot".to_string(),
        token_symbol: "SWEET".to_string(),
        chain: Chain::Robinhood,
        market_cap_usd: 850_000.0,
        liquidity_usd: 120_000.0,
        price_usd: 0.085,
        change_24h_pct: 5.0,
        change_6h_pct: 2.0,
        change_1h_pct: 1.0,
        whale_count: 3,
        total_whale_usd: 150_000.0,
        top_whale_handles: "@Aurelius0121".to_string(),
        status: "ACCUMULATION_BOTTOM".to_string(),
        upside_tier: "NANO_CAP".to_string(),
        last_scanned_at: now,
    };
    db.upsert_tracked_token(&token_sweet).unwrap();

    let act_sweet = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Robinhood,
        activity_type: ActivityType::Buy,
        token_symbol: "SWEET".to_string(),
        token_mint: "0xtoken_sweet_spot".to_string(),
        amount_usd: 1000.0,
        token_amount: 11764.0,
        execution_price_usd: 0.085,
        whale_balance_before: 0.0,
        whale_balance_after: 11764.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_sweet".to_string(),
        timestamp: now,
    };

    let sig_sweet = confluence
        .evaluate_activity(&act_sweet, &whale, 0.085, 120_000.0)
        .unwrap();
    assert!(sig_sweet.is_passed);
    assert_eq!(sig_sweet.rejection_reason, None);
}

#[tokio::test]
async fn test_whale_cluster_detective_and_subsidiary_discovery() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let now = Utc::now();

    // 1. Seed Parent Whale Tier-S
    let parent = SmartWallet {
        id: None,
        rank: 1,
        name: "Unipcs Master".to_string(),
        handle: "unipcs".to_string(),
        chain: Chain::Solana,
        address: "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 150_000.0,
        win_rate_pct: 88.0,
        total_trades: 120,
        source: "ONCHAIN_TOP".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 98.0,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&parent).unwrap();

    // 2. Discover Subsidiary / Child Wallet via Forensic Trace
    let child_wallet = SmartWallet {
        id: None,
        rank: 51,
        name: "Unipcs Master Shadow #1".to_string(),
        handle: "unipcs_sub_e2a1".to_string(),
        chain: Chain::Robinhood,
        address: "0x5a8e12d1f7148886fe35f6fa04f5a2c2b60ac354".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 52_500.0,
        win_rate_pct: 83.6,
        total_trades: 35,
        source: "CLUSTER_DETECTIVE".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 93.1,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };
    db.upsert_wallet(&child_wallet).unwrap();

    // 3. Verify querying by tier and address
    let tier_s_wallets = db.get_wallets_by_tier(WalletTier::TierS).unwrap();
    assert_eq!(tier_s_wallets.len(), 2);

    let retrieved_child = db
        .get_wallet_by_address("0x5a8e12d1f7148886fe35f6fa04f5a2c2b60ac354")
        .unwrap()
        .unwrap();
    assert_eq!(retrieved_child.source, "CLUSTER_DETECTIVE");
    assert_eq!(retrieved_child.handle, "unipcs_sub_e2a1");
    assert_eq!(retrieved_child.tier, WalletTier::TierS);
}

#[tokio::test]
async fn test_wrapped_btc_blacklist_and_market_cap_fail_closed() {
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let confluence = ConfluenceEngine::new(get_mock_config(), db.clone());
    let now = Utc::now();
    let whale = SmartWallet {
        id: None,
        rank: 1,
        name: "Wrapped BTC Test Whale".to_string(),
        handle: "wrapped_btc_test".to_string(),
        chain: Chain::Ethereum,
        address: "0xtest_whale".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 100_000.0,
        win_rate_pct: 90.0,
        total_trades: 100,
        source: "TEST".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 95.0,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };

    assert!(zenithsw::models::is_blacklisted_asset(
        "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf",
        "cbBTC"
    ));
    assert!(zenithsw::models::is_blacklisted_asset(
        "XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB",
        "TSLAx"
    ));

    let cb_btc = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Ethereum,
        activity_type: ActivityType::Buy,
        token_symbol: "cbBTC".to_string(),
        token_mint: "0xcbb7c0000ab88b473b1f5afd9ef808440eed33bf".to_string(),
        amount_usd: 1_000.0,
        token_amount: 0.013,
        execution_price_usd: 77_053.28,
        whale_balance_before: 0.0,
        whale_balance_after: 0.389,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_cbbtc_blacklist".to_string(),
        timestamp: now,
    };
    let cb_signal = confluence
        .evaluate_activity(&cb_btc, &whale, 77_053.28, 1_000_000.0)
        .unwrap();
    assert!(!cb_signal.is_passed);
    assert!(cb_signal
        .rejection_reason
        .unwrap()
        .contains("REJECTED_BASE_ASSET_OR_STABLECOIN"));

    let unknown = WalletActivity {
        token_symbol: "UNKNOWN".to_string(),
        token_mint: "0xunknown_without_market_cap".to_string(),
        tx_signature: "tx_unknown_market_cap".to_string(),
        ..cb_btc
    };
    let unknown_signal = confluence
        .evaluate_activity(&unknown, &whale, 1.0, 100_000.0)
        .unwrap();
    assert!(!unknown_signal.is_passed);
    assert_eq!(
        unknown_signal.rejection_reason.as_deref(),
        Some("REJECTED_MARKET_CAP_UNVERIFIED")
    );
}

#[test]
fn test_clmm_single_sided_spoofed_liquidity_gating() {
    let now = Utc::now();
    let db = Arc::new(SmartWalletDb::new_in_memory().unwrap());
    let mut config = get_mock_config();
    config.min_liquidity_usd = 30000.0;
    let confluence = ConfluenceEngine::new(config, db.clone());

    let whale = SmartWallet {
        id: None,
        rank: 1,
        name: "test_whale".to_string(),
        handle: "whale_clmm".to_string(),
        chain: Chain::Solana,
        address: "whale_clmm_addr".to_string(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 500_000.0,
        win_rate_pct: 85.0,
        total_trades: 200,
        source: "TEST".to_string(),
        execution_track: ExecutionTrack::FastLane,
        reputation_score: 95.0,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    };

    // Case 1: Real standard pool ($50k liquidity, genuine quote)
    seed_test_token(&db, "genuine_mint", "GENUINE", Chain::Solana, now);
    let genuine_act = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "GENUINE".to_string(),
        token_mint: "genuine_mint".to_string(),
        amount_usd: 500.0,
        token_amount: 500.0,
        execution_price_usd: 1.0,
        whale_balance_before: 0.0,
        whale_balance_after: 500.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_genuine".to_string(),
        timestamp: now,
    };
    let genuine_sig = confluence
        .evaluate_activity(&genuine_act, &whale, 1.0, 50_000.0)
        .unwrap();
    assert!(genuine_sig.is_passed);

    // Case 2: Fake CLMM pool with clamped real quote liquidity below min threshold ($31 USDC)
    let fake_clmm_act = WalletActivity {
        id: None,
        wallet_address: whale.address.clone(),
        wallet_handle: whale.handle.clone(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "AKE".to_string(),
        token_mint: "2AudYNuUH44TwW9MFYYCqyPDXQe4bWoo3vHbHA8KQE9y".to_string(),
        amount_usd: 500.0,
        token_amount: 1000.0,
        execution_price_usd: 0.0039,
        whale_balance_before: 0.0,
        whale_balance_after: 1000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "tx_fake_clmm".to_string(),
        timestamp: now,
    };
    // When effective liquidity is clamped to real quote ($31.11 * 2 = $62.22), it must be rejected by gating
    let fake_sig = confluence
        .evaluate_activity(&fake_clmm_act, &whale, 0.0039, 62.22)
        .unwrap();
    assert!(!fake_sig.is_passed);
    assert!(fake_sig
        .rejection_reason
        .unwrap()
        .contains("LIQUIDITY_TOO_LOW"));
}
