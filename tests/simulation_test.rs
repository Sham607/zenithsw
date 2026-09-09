use chrono::Utc;
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::SolanaExecutor;
use zenithsw::gating::ConfluenceEngine;
use zenithsw::indexer::scoring::ReputationScorer;
use zenithsw::ingestion::{FomoScraper, WalletResolver};
use zenithsw::models::{
    ActivityType, Chain, ShadowSignal, SmartWallet, WalletActivity, WalletTier,
};

fn get_mock_config() -> Config {
    Config {
        solana_rpc_url: "https://mainnet.helius-rpc.com/?api-key=test".to_string(),
        solana_ws_url: "wss://mainnet.helius-rpc.com/?api-key=test".to_string(),
        solana_private_key: None,
        arbitrum_rpc_url: "https://arb-mainnet.g.alchemy.com/v2/test".to_string(),
        arbitrum_ws_url: "wss://arb-mainnet.g.alchemy.com/v2/test".to_string(),
        base_rpc_url: "https://base-mainnet.g.alchemy.com/v2/test".to_string(),
        base_ws_url: "wss://base-mainnet.g.alchemy.com/v2/test".to_string(),
        evm_private_key: None,
        fomo_auth_token: None,
        fomo_sync_interval_sec: 60,
        stream_poll_interval_ms: 500,
        max_price_delta_pct: 5.0,
        min_liquidity_usd: 30000.0,
        position_size_usd: 15.0,
        trailing_stop_pct: 10.0,
        stop_loss_pct: 15.0,
        take_profit_pct: 50.0,
        shadow_exit_enabled: true,
        confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
        dry_run: true,
        db_path: ":memory:".to_string(),
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
    let signal_pass = confluence
        .evaluate_activity(&activity, &wallet, 1.03, 50000.0)
        .unwrap();
    assert!(signal_pass.is_passed);
    assert!(signal_pass.rejection_reason.is_none());

    // Case 2: Current Price = 1.08 (+8.0% delta) -> Should REJECT (> +5.0%)
    let signal_fail = confluence
        .evaluate_activity(&activity, &wallet, 1.08, 50000.0)
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
    assert_eq!(seeds[0].tier, WalletTier::TierS);

    let res_sol = WalletResolver::resolve_chain_and_address("ether_monk").unwrap();
    assert_eq!(res_sol.0, Chain::Solana);
    assert_eq!(res_sol.1, "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i");

    let res_rh = WalletResolver::resolve_chain_and_address("0xAvast").unwrap();
    assert_eq!(res_rh.0, Chain::Robinhood);
    assert_eq!(res_rh.1, "0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D");
}
