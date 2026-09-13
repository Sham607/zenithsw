use anyhow::{Context, Result};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Solana settings
    pub solana_rpc_url: String,
    pub solana_ws_url: String,
    pub solana_private_key: Option<String>,

    // Robinhood Chain settings
    pub robinhood_rpc_url: String,
    pub robinhood_ws_url: String,
    pub robinhood_api_key: Option<String>,
    pub evm_private_key: Option<String>,

    // FOMO Auth & Ingestion settings
    pub fomo_auth_token: Option<String>,
    pub fomo_sync_interval_sec: u64,
    pub stream_poll_interval_ms: u64,

    // Gating parameters
    pub max_price_delta_pct: f64, // Max +5.0% price impact / slippage delta
    pub min_liquidity_usd: f64,   // Min $30,000 liquidity in pool
    pub position_size_usd: f64,   // $15 default position size
    pub min_whale_buy_usd: f64,   // Min $250 whale order size to filter dust transactions
    pub max_price_impact_pct: f64, // Max 15.0% price impact on pool to prevent buying local tops
    pub min_market_cap_usd: f64,  // Min $50k market cap
    pub max_market_cap_usd: f64,  // Max $20M market cap
    pub trailing_stop_pct: f64,   // 10% trailing stop
    pub stop_loss_pct: f64,       // 15% stop loss
    pub take_profit_pct: f64,     // 100% take profit trigger (50% partial exit)
    pub shadow_exit_enabled: bool, // Mirror sell when whale dumps
    pub confluence_mode: String,  // "TIER_S_OR_DUAL_A"
    pub dry_run: bool,
    /// Live Robinhood gate tambahan; tidak cukup sendiri untuk broadcast.
    pub live_robinhood_enabled: bool,

    // Chains enabled
    pub enable_solana: bool,
    pub enable_robinhood: bool,
    pub enable_base: bool,

    // Persistence & IPC
    pub trading_db_path: String,
    pub wallet_db_path: String,
    pub ipc_socket_path: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        let _ = dotenv();

        let solana_rpc_url = env::var("SOLANA_RPC_URL")
            .context("SOLANA_RPC_URL wajib diatur di .env/secret manager")?;
        let solana_ws_url = env::var("SOLANA_WS_URL")
            .context("SOLANA_WS_URL wajib diatur di .env/secret manager")?;

        let solana_private_key = env::var("SOLANA_PRIVATE_KEY").ok();

        let robinhood_rpc_url = env::var("ROBINHOOD_RPC_URL")
            .context("ROBINHOOD_RPC_URL wajib diatur di .env/secret manager")?;
        let robinhood_ws_url = env::var("ROBINHOOD_WS_URL")
            .context("ROBINHOOD_WS_URL wajib diatur di .env/secret manager")?;

        let robinhood_api_key = env::var("ROBINHOOD_BLOCKSCOUT_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty());

        // Robinhood uses its own explicit credential name; retain EVM_PRIVATE_KEY as a
        // compatibility fallback without ever logging or exposing the secret.
        let evm_private_key = env::var("ROBINHOOD_PRIVATE_KEY")
            .ok()
            .or_else(|| env::var("EVM_PRIVATE_KEY").ok());

        let fomo_auth_token = env::var("FOMO_AUTH_TOKEN")
            .ok()
            .filter(|s| !s.trim().is_empty());

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
            .unwrap_or(15000.0);

        let position_size_usd = env::var("POSITION_SIZE_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0);

        let min_whale_buy_usd = env::var("MIN_WHALE_BUY_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50.0);

        let max_price_impact_pct = env::var("MAX_PRICE_IMPACT_PCT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15.0);

        let min_market_cap_usd = env::var("MIN_MARKET_CAP_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50000.0);

        let max_market_cap_usd = env::var("MAX_MARKET_CAP_USD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20000000.0);

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
            .unwrap_or(100.0);

        let shadow_exit_enabled = env::var("SHADOW_EXIT_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        let confluence_mode =
            env::var("CONFLUENCE_MODE").unwrap_or_else(|_| "TIER_S_OR_DUAL_A".to_string());

        let dry_run = env::var("DRY_RUN")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        let live_robinhood_enabled = env::var("LIVE_ROBINHOOD_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let enable_solana = env::var("ENABLE_SOLANA")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(true);

        let enable_robinhood = env::var("ENABLE_ROBINHOOD")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let enable_base = env::var("ENABLE_BASE")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let trading_db_path = env::var("TRADING_DB_PATH")
            .unwrap_or_else(|_| "data/trading_state.db".to_string());
        let wallet_db_path = env::var("WALLET_DB_PATH")
            .unwrap_or_else(|_| "data/wallet_registry.db".to_string());

        let ipc_socket_path =
            env::var("IPC_SOCKET_PATH").unwrap_or_else(|_| "/tmp/zenithsw.sock".to_string());

        Ok(Self {
            solana_rpc_url,
            solana_ws_url,
            solana_private_key,
            robinhood_rpc_url,
            robinhood_ws_url,
            robinhood_api_key,
            evm_private_key,
            fomo_auth_token,
            fomo_sync_interval_sec,
            stream_poll_interval_ms,
            max_price_delta_pct,
            min_liquidity_usd,
            position_size_usd,
            min_whale_buy_usd,
            max_price_impact_pct,
            min_market_cap_usd,
            max_market_cap_usd,
            trailing_stop_pct,
            stop_loss_pct,
            take_profit_pct,
            shadow_exit_enabled,
            confluence_mode,
            dry_run,
            live_robinhood_enabled,
            enable_solana,
            enable_robinhood,
            enable_base,
            trading_db_path,
            wallet_db_path,
            ipc_socket_path,
        })
    }
}
