use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{ActivityType, ShadowSignal, SmartWallet, WalletActivity, WalletTier};
use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

pub struct ConfluenceEngine {
    config: Config,
    db: Arc<SmartWalletDb>,
}

impl ConfluenceEngine {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self {
        Self { config, db }
    }

    pub fn evaluate_activity(
        &self,
        activity: &WalletActivity,
        wallet: &SmartWallet,
        current_pool_price: f64,
        pool_liquidity_usd: f64,
    ) -> Result<ShadowSignal> {
        let now = Utc::now();
        let mut is_passed = true;
        let mut rejection_reason = None;

        // 1. Only process BUY activities for entering new positions
        if activity.activity_type != ActivityType::Buy {
            return Ok(ShadowSignal {
                chain: activity.chain,
                token_symbol: activity.token_symbol.clone(),
                token_mint: activity.token_mint.clone(),
                whale_handle: activity.wallet_handle.clone(),
                whale_address: activity.wallet_address.clone(),
                whale_tier: wallet.tier,
                whale_entry_price: activity.execution_price_usd,
                current_pool_price,
                price_delta_pct: 0.0,
                pool_liquidity_usd,
                is_passed: false,
                rejection_reason: Some("ACTIVITY_IS_NOT_BUY".to_string()),
                confluence_count: 0,
                timestamp: now,
            });
        }

        // 2. Calculate Price Delta (Whale Entry vs Current Pool Price)
        let price_delta_pct = if activity.execution_price_usd > 0.0 && current_pool_price > 0.0 {
            ((current_pool_price - activity.execution_price_usd) / activity.execution_price_usd) * 100.0
        } else {
            0.0
        };

        // Gating Check A: Price Delta / Slippage Impact (Max +5.0% per user calibrated rule)
        if price_delta_pct > self.config.max_price_delta_pct {
            is_passed = false;
            rejection_reason = Some(format!(
                "PRICE_DELTA_TOO_HIGH (+{:.2}% > +{:.2}%)",
                price_delta_pct, self.config.max_price_delta_pct
            ));
        }

        // Gating Check B: Pool Liquidity Safety Floor (>= $30,000)
        if pool_liquidity_usd > 0.0 && pool_liquidity_usd < self.config.min_liquidity_usd {
            is_passed = false;
            if rejection_reason.is_none() {
                rejection_reason = Some(format!(
                    "LIQUIDITY_TOO_LOW (${:.0} < ${:.0})",
                    pool_liquidity_usd, self.config.min_liquidity_usd
                ));
            }
        }

        // Gating Check C: Multi-Wallet Confluence Rule
        // Count recent smart buyers in last 300s (5m)
        let confluence_count = self.db.count_recent_buyers_for_mint(&activity.token_mint, 300).unwrap_or(1);

        if wallet.tier != WalletTier::TierS && confluence_count < 2 {
            is_passed = false;
            if rejection_reason.is_none() {
                rejection_reason = Some(format!(
                    "TIER_CONFLUENCE_REQUIRED (Tier: {}, Buyers: {} < 2)",
                    wallet.tier, confluence_count
                ));
            }
        }

        let signal = ShadowSignal {
            chain: activity.chain,
            token_symbol: activity.token_symbol.clone(),
            token_mint: activity.token_mint.clone(),
            whale_handle: activity.wallet_handle.clone(),
            whale_address: activity.wallet_address.clone(),
            whale_tier: wallet.tier,
            whale_entry_price: activity.execution_price_usd,
            current_pool_price,
            price_delta_pct,
            pool_liquidity_usd,
            is_passed,
            rejection_reason,
            confluence_count,
            timestamp: now,
        };

        let _ = self.db.insert_signal(&signal);
        Ok(signal)
    }
}
