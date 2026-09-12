use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{is_blacklisted_asset, ShadowPosition, ShadowSignal};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

pub struct EvmExecutor {
    config: Config,
    db: Arc<SmartWalletDb>,
}

impl EvmExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self {
        Self { config, db }
    }

    pub async fn execute_shadow_buy(
        &self,
        signal: &ShadowSignal,
    ) -> Result<Option<ShadowPosition>> {
        if !signal.is_passed || is_blacklisted_asset(&signal.token_mint, &signal.token_symbol) {
            if is_blacklisted_asset(&signal.token_mint, &signal.token_symbol) {
                eprintln!(
                    "[EXECUTION BLOCK 🛡️] Refusing blacklisted base/wrapped asset {} ({})",
                    signal.token_symbol, signal.token_mint
                );
            }
            return Ok(None);
        }

        let now = Utc::now();
        let entry_price = if signal.current_pool_price > 0.0 {
            signal.current_pool_price
        } else {
            signal.whale_entry_price
        };

        if entry_price <= 0.0 {
            return Ok(None);
        }

        let amount_usd = self.config.position_size_usd;
        let new_tokens = if entry_price > 0.0 {
            amount_usd / entry_price
        } else {
            0.0
        };
        let position_id = format!("{}-{}", signal.chain, signal.token_mint);

        // Check if position already exists for this token mint on EVM -> Average in!
        if let Ok(active_pos) = self.db.get_active_positions() {
            if let Some(mut existing) = active_pos.into_iter().find(|p| {
                p.token_mint.eq_ignore_ascii_case(&signal.token_mint) && p.chain == signal.chain
            }) {
                let prev_usd = existing.amount_usd;
                let prev_tokens = existing.amount_native;
                let total_usd = prev_usd + amount_usd;
                let total_tokens = prev_tokens + new_tokens;
                let weighted_avg_entry = if total_tokens > 0.0 {
                    total_usd / total_tokens
                } else {
                    entry_price
                };

                println!(
                    "[POSITION AVERAGING ➕] Adding ${:.2} to existing {} position (ID: {}). Prev Entry: ${:.6} -> New Avg Entry: ${:.6} (Total: ${:.2})",
                    amount_usd, existing.token_symbol, existing.id, existing.entry_price, weighted_avg_entry, total_usd
                );

                existing.entry_price = weighted_avg_entry;
                existing.amount_usd = total_usd;
                existing.amount_native = total_tokens;
                existing.current_price = entry_price;
                existing.peak_price = existing.peak_price.max(entry_price);
                existing.remaining_portion_pct = 100.0;
                existing.last_update = now;
                existing.status = "OPEN".to_string();

                if !existing.target_whale_handle.contains(&signal.whale_handle) {
                    existing.target_whale_handle =
                        format!("{}, @{}", existing.target_whale_handle, signal.whale_handle);
                }
                if !existing
                    .target_whale_address
                    .contains(&signal.whale_address)
                {
                    existing.target_whale_address = format!(
                        "{}, {}",
                        existing.target_whale_address, signal.whale_address
                    );
                }

                self.db.upsert_position(&existing)?;
                return Ok(Some(existing));
            }
        }

        let position = ShadowPosition {
            id: position_id,
            chain: signal.chain,
            token_symbol: signal.token_symbol.clone(),
            token_mint: signal.token_mint.clone(),
            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            track: signal.track,
            entry_price,
            current_price: entry_price,
            peak_price: entry_price,
            amount_native: new_tokens,
            amount_usd,
            remaining_portion_pct: 100.0,
            unrealized_pnl_pct: 0.0,
            realized_pnl_usd: 0.0,
            entry_time: now,
            last_update: now,
            status: "OPEN".to_string(),
        };

        self.db.upsert_position(&position)?;
        Ok(Some(position))
    }

    pub async fn execute_partial_shadow_sell(
        &self,
        position_id: &str,
        sell_ratio_pct: f64,
        exit_reason: &str,
        current_price: f64,
    ) -> Result<()> {
        self.db
            .close_position_portion(position_id, sell_ratio_pct, exit_reason, current_price)?;
        Ok(())
    }

    pub async fn execute_shadow_sell(
        &self,
        position_id: &str,
        exit_reason: &str,
        current_price: f64,
    ) -> Result<()> {
        self.db
            .close_position(position_id, exit_reason, current_price)?;
        Ok(())
    }
}
