use anyhow::Result;
use std::env;
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::EvmExecutor;
use zenithsw::models::{Chain, ExecutionTrack, ShadowSignal, WalletTier};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env::set_var("DRY_RUN", "true");
    let token = env::var("ROBINHOOD_DRY_RUN_TOKEN")?;
    let symbol = env::var("ROBINHOOD_DRY_RUN_SYMBOL").unwrap_or_else(|_| "AC".to_string());
    let config = Config::load()?;
    let db = Arc::new(SmartWalletDb::new_with_wallet_path(
        &config.trading_db_path,
        &config.wallet_db_path,
    )?);
    let signal = ShadowSignal {
        chain: Chain::Robinhood,
        token_symbol: symbol,
        token_mint: token,
        whale_handle: "dry-run".to_string(),
        whale_address: "0x0000000000000000000000000000000000000001".to_string(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 1.0,
        current_pool_price: 1.0,
        price_delta_pct: 0.0,
        pool_liquidity_usd: 0.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 1,
        timestamp: chrono::Utc::now(),
    };
    let executor = EvmExecutor::new(config, db);
    executor.execute_shadow_buy(&signal).await?;
    println!("dry_run_position_persisted=false");
    println!("dry_run_broadcast=false");
    Ok(())
}
