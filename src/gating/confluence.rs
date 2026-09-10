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

        // 1b. Filter out Base Currencies, Gas Tokens, and Stablecoins (USDC, USDT, WETH, etc.)
        if crate::models::is_blacklisted_asset(&activity.token_mint, &activity.token_symbol) {
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
                rejection_reason: Some(format!(
                    "REJECTED_BASE_ASSET_OR_STABLECOIN ({})",
                    activity.token_symbol
                )),
                confluence_count: 0,
                timestamp: now,
            });
        }

        // 1c. Duplicate Whale & Max Token Exposure Check on Active Positions
        if let Ok(active_positions) = self.db.get_active_positions() {
            if let Some(pos) = active_positions.iter().find(|p| {
                p.token_mint.eq_ignore_ascii_case(&activity.token_mint) && p.chain == activity.chain
            }) {
                // Reject if THIS specific whale address is already part of this active position
                if pos
                    .target_whale_address
                    .to_lowercase()
                    .contains(&activity.wallet_address.to_lowercase())
                {
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
                        rejection_reason: Some(format!(
                            "REJECTED_WHALE_ALREADY_IN_POSITION (@{})",
                            activity.wallet_handle
                        )),
                        confluence_count: 0,
                        timestamp: now,
                    });
                }

                // Reject if max exposure per token is reached ($15.0 max)
                let max_exposure_usd = 15.0;
                if pos.amount_usd >= max_exposure_usd {
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
                        rejection_reason: Some(format!(
                            "REJECTED_MAX_TOKEN_EXPOSURE_REACHED (${:.2} >= ${:.2})",
                            pos.amount_usd, max_exposure_usd
                        )),
                        confluence_count: 0,
                        timestamp: now,
                    });
                }
            }
        }

        // 1d. Minimum Whale Buy Filter (Filter out dust test transactions < $250.00)
        if activity.amount_usd > 0.0 && activity.amount_usd < self.config.min_whale_buy_usd {
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
                rejection_reason: Some(format!(
                    "REJECTED_WHALE_BUY_TOO_SMALL (${:.2} < ${:.2})",
                    activity.amount_usd, self.config.min_whale_buy_usd
                )),
                confluence_count: 0,
                timestamp: now,
            });
        }

        // 2. Strict Price Discovery Gate (Zero price is an immediate reject)
        let whale_entry_price = if activity.execution_price_usd > 0.0 {
            activity.execution_price_usd
        } else {
            current_pool_price
        };

        if whale_entry_price <= 0.0 || current_pool_price <= 0.0 {
            is_passed = false;
            rejection_reason = Some(format!(
                "INVALID_PRICE_OR_UNINDEXED_POOL (Whale: ${:.6}, Pool: ${:.6})",
                whale_entry_price, current_pool_price
            ));
        }

        // 3. Strict Liquidity Safety Floor (Zero or Sub-Min Liquidity is rejected)
        if pool_liquidity_usd < self.config.min_liquidity_usd {
            is_passed = false;
            if rejection_reason.is_none() {
                rejection_reason = Some(format!(
                    "LIQUIDITY_TOO_LOW (${:.0} < ${:.0})",
                    pool_liquidity_usd, self.config.min_liquidity_usd
                ));
            }
        }

        // 4. Calculate Price Delta (Whale Entry vs Current Pool Price)
        let price_delta_pct = if whale_entry_price > 0.0 && current_pool_price > 0.0 {
            ((current_pool_price - whale_entry_price) / whale_entry_price) * 100.0
        } else {
            0.0
        };

        // Gating Check A: Price Delta / Slippage Impact (Max +5.0%)
        if price_delta_pct > self.config.max_price_delta_pct {
            is_passed = false;
            if rejection_reason.is_none() {
                rejection_reason = Some(format!(
                    "PRICE_DELTA_TOO_HIGH (+{:.2}% > +{:.2}%)",
                    price_delta_pct, self.config.max_price_delta_pct
                ));
            }
        }

        // Gating Check A2: Pool Price Impact Guard (Max 15.0%)
        if pool_liquidity_usd > 0.0 && activity.amount_usd > 0.0 {
            let price_impact_pct = (activity.amount_usd / pool_liquidity_usd) * 100.0;
            if price_impact_pct > self.config.max_price_impact_pct {
                is_passed = false;
                if rejection_reason.is_none() {
                    rejection_reason = Some(format!(
                        "PRICE_IMPACT_TOO_HIGH ({:.2}% > {:.2}%)",
                        price_impact_pct, self.config.max_price_impact_pct
                    ));
                }
            }
        }

        // Gating Check A3: Market Cap Range Gate ($50k Min to $20M Max).
        // Fail closed: a missing/zero market-cap snapshot is not evidence of eligibility.
        match self.db.get_tracked_token_by_mint(&activity.token_mint) {
            Ok(Some(tracked)) if tracked.market_cap_usd > 0.0 => {
                if tracked.market_cap_usd < self.config.min_market_cap_usd {
                    is_passed = false;
                    if rejection_reason.is_none() {
                        rejection_reason = Some(format!(
                            "REJECTED_MARKET_CAP_TOO_LOW (${:.0} < ${:.0})",
                            tracked.market_cap_usd, self.config.min_market_cap_usd
                        ));
                    }
                } else if tracked.market_cap_usd > self.config.max_market_cap_usd {
                    is_passed = false;
                    if rejection_reason.is_none() {
                        rejection_reason = Some(format!(
                            "REJECTED_MARKET_CAP_TOO_HIGH (${:.0} > ${:.0})",
                            tracked.market_cap_usd, self.config.max_market_cap_usd
                        ));
                    }
                }
            }
            Ok(Some(_)) | Ok(None) | Err(_) => {
                is_passed = false;
                if rejection_reason.is_none() {
                    rejection_reason = Some("REJECTED_MARKET_CAP_UNVERIFIED".to_string());
                }
            }
        }

        // Gating Check B: Multi-Wallet Confluence Rule
        // Tier-S & Tier-A (Rank 1-15): Direct copy execution
        // Tier-B (Rank >15): Passive monitoring only, requires >= 2 multi-wallet confluence
        let confluence_count = self
            .db
            .count_recent_buyers_for_mint(&activity.token_mint, 300)
            .unwrap_or(1);

        if wallet.tier == WalletTier::TierB && confluence_count < 2 {
            is_passed = false;
            if rejection_reason.is_none() {
                rejection_reason = Some(format!(
                    "TIER_B_CONFLUENCE_REQUIRED (Tier: {}, Buyers: {} < 2)",
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
