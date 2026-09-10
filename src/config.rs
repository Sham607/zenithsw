use anyhow::Result;
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Solana settings
    pub solana_rpc_url: String,
    pub solana_ws_url: String,
    pub solana_private_key: Option<String>,

    // EVM settings (Arbitrum & Base)
    pub arbitrum_rpc_url: String,
    pub arbitrum_ws_url: String,
    pub base_rpc_url: String,
    pub base_ws_url: String,
    pub evm_private_key: Option<String>,

    // FOMO Auth & Ingestion settings
    pub fomo_auth_token: Option<String>,
    pub fomo_sync_interval_sec: u64,
    pub stream_poll_interval_ms: u64,

    // Gating parameters
    pub max_price_delta_pct: f64,   // Max +5.0% price impact / slippage delta
    pub min_liquidity_usd: f64,      // Min $30,000 liquidity in pool
    pub position_size_usd: f64,      // $15 default position size
    pub min_whale_buy_usd: f64,      // Min $250 whale order size to filter dust transactions
    pub max_price_impact_pct: f64,   // Max 3.5% price impact on pool to prevent buying local tops
    pub trailing_stop_pct: f64,      // 10% trailing stop
    pub stop_loss_pct: f64,          // 15% stop loss
    pub take_profit_pct: f64,        // 50% take profit
    pub shadow_exit_enabled: bool,   // Mirror sell when whale dumps
    pub confluence_mode: String,     // "TIER_S_OR_DUAL_A"
    pub dry_run: bool,

    // Persistence & IPC
    pub db_path: String,
    pub ipc_socket_path: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let _ = dotenv();

        let solana_rpc_url = env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://mainnet.helius-rpc.com/?api-key=def963d9-3753-4fe1-a609-f0534ead614f".to_string());
        
        let solana_ws_url = env::var("SOLANA_WS_URL")
            .unwrap_or_else(|_| "wss://mainnet.helius-rpc.com/?api-key=def963d9-3753-4fe1-a609-f0534ead614f".to_string());

        let solana_private_key = env::var("SOLANA_PRIVATE_KEY").ok();

        let arbitrum_rpc_url = env::var("ARBITRUM_RPC_URL")
            .unwrap_or_else(|_| "https://arb-mainnet.g.alchemy.com/v2/alch_PS21nEmHejPxYhypdUH1A".to_string());
        let arbitrum_ws_url = env::var("ARBITRUM_WS_URL")
            .unwrap_or_else(|_| "wss://arb-mainnet.g.alchemy.com/v2/alch_PS21nEmHejPxYhypdUH1A".to_string());

        let base_rpc_url = env::var("BASE_RPC_URL")
            .unwrap_or_else(|_| "https://base-mainnet.g.alchemy.com/v2/demo".to_string());
        let base_ws_url = env::var("BASE_WS_URL")
            .unwrap_or_else(|_| "wss://base-mainnet.g.alchemy.com/v2/demo".to_string());

        let evm_private_key = env::var("EVM_PRIVATE_KEY").ok();

        let fomo_auth_token = env::var("FOMO_AUTH_TOKEN").ok().filter(|s| !s.trim().is_empty());

        let fomo_sync_interval_sec = env::var("FOMO_SYNC_INTERVAL_SEC")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);

        let stream_poll_interval_ms = env::var("STREAM_POLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(500);

        let max_price_delta_pct = env::var("MAX_PRICE_DELTA_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5.0);

        let min_liquidity_usd = env::var("MIN_LIQUIDITY_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30000.0);

        let position_size_usd = env::var("POSITION_SIZE_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15.0);

        let min_whale_buy_usd = env::var("MIN_WHALE_BUY_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(250.0);

        let max_price_impact_pct = env::var("MAX_PRICE_IMPACT_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3.5);

        let trailing_stop_pct = env::var("TRAILING_STOP_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);

        let stop_loss_pct = env::var("STOP_LOSS_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15.0);

        let take_profit_pct = env::var("TAKE_PROFIT_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50.0);

        let shadow_exit_enabled = env::var("SHADOW_EXIT_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        let confluence_mode = env::var("CONFLUENCE_MODE")
            .unwrap_or_else(|_| "TIER_S_OR_DUAL_A".to_string());

        let dry_run = env::var("DRY_RUN")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        let db_path = env::var("DB_PATH")
            .unwrap_or_else(|_| "data/smart_wallets.db".to_string());

        let ipc_socket_path = env::var("IPC_SOCKET_PATH")
            .unwrap_or_else(|_| "/tmp/zenithsw.sock".to_string());

        Ok(Self {
            solana_rpc_url,
            solana_ws_url,
            solana_private_key,
            arbitrum_rpc_url,
            arbitrum_ws_url,
            base_rpc_url,
            base_ws_url,
            evm_private_key,
            fomo_auth_token,
            fomo_sync_interval_sec,
            stream_poll_interval_ms,
            max_price_delta_pct,
            min_liquidity_usd,
            position_size_usd,
            min_whale_buy_usd,
            max_price_impact_pct,
            trailing_stop_pct,
            stop_loss_pct,
            take_profit_pct,
            shadow_exit_enabled,
            confluence_mode,
            dry_run,
            db_path,
            ipc_socket_path,
        })
    }
}
