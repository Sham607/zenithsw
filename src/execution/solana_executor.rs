use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{Chain, ShadowPosition, ShadowSignal};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

pub struct SolanaExecutor {
    config: Config,
    db: Arc<SmartWalletDb>,
}

impl SolanaExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self {
        Self { config, db }
    }

    pub async fn execute_shadow_buy(&self, signal: &ShadowSignal) -> Result<Option<ShadowPosition>> {
        if !signal.is_passed {
            return Ok(None);
        }

        let now = Utc::now();
        let position_id = format!("SOL-{}-{}", signal.token_symbol, now.timestamp());
        let entry_price = if signal.current_pool_price > 0.0 {
            signal.current_pool_price
        } else {
            signal.whale_entry_price
        };

        let amount_usd = self.config.position_size_usd;
        let amount_native = amount_usd / 140.0; // Approx 0.11 SOL proxy

        let position = ShadowPosition {
            id: position_id,
            chain: Chain::Solana,
            token_symbol: signal.token_symbol.clone(),
            token_mint: signal.token_mint.clone(),
            target_whale_handle: signal.whale_handle.clone(),
            target_whale_address: signal.whale_address.clone(),
            entry_price,
            current_price: entry_price,
            peak_price: entry_price,
            amount_native,
            amount_usd,
            unrealized_pnl_pct: 0.0,
            entry_time: now,
            last_update: now,
            status: "OPEN".to_string(),
        };

        self.db.upsert_position(&position)?;
        Ok(Some(position))
    }

    pub async fn execute_shadow_sell(&self, position_id: &str, exit_reason: &str, current_price: f64) -> Result<()> {
        self.db.close_position(position_id, exit_reason, current_price)?;
        Ok(())
    }
}
