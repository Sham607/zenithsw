use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub struct FomoApiStreamListener {
    config: Config,
    client: reqwest::Client,
    processed_swap_ids: Arc<Mutex<HashSet<String>>>,
    user_id_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl FomoApiStreamListener {
    pub fn new(config: Config) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            config,
            client,
            processed_swap_ids: Arc::new(Mutex::new(HashSet::new())),
            user_id_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Refresh User ID Cache from Leaderboard
    pub async fn refresh_user_ids(&self) -> Result<()> {
        if let Some(token) = &self.config.fomo_auth_token {
            let api_url = "https://prod-api.fomo.family/v2/leaderboard?timeframe=24h";
            let mut req = self.client.get(api_url)
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
                .header("Accept", "application/json")
                .header("Origin", "https://fomo.family")
                .header("Referer", "https://fomo.family/");

            if !token.starts_with("Bearer ") {
                req = req.header("Authorization", format!("Bearer {}", token));
            } else {
                req = req.header("Authorization", token);
            }

            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<Value>().await {
                        if let Some(items) = json.get("responseObject").and_then(|r| r.get("leaderboard")).and_then(|l| l.as_array()) {
                            let mut cache = self.user_id_cache.lock().unwrap();
                            for item in items {
                                let handle = item.get("userHandle").and_then(|v| v.as_str()).unwrap_or("");
                                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                if !handle.is_empty() && !id.is_empty() {
                                    cache.insert(handle.to_string(), id.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Checks the live swaps for a target trader handle from FOMO API
    pub async fn check_trader_swaps(
        &self,
        handle: &str,
        solana_address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        let token = match &self.config.fomo_auth_token {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(None),
        };

        let user_id = {
            let cache = self.user_id_cache.lock().unwrap();
            cache.get(handle).cloned()
        };

        let user_id = match user_id {
            Some(id) => id,
            None => {
                let _ = self.refresh_user_ids().await;
                let cache = self.user_id_cache.lock().unwrap();
                match cache.get(handle) {
                    Some(id) => id.clone(),
                    None => return Ok(None),
                }
            }
        };

        let swaps_url = format!("https://prod-api.fomo.family/v2/users/{}/swaps", user_id);
        let mut req = self.client.get(&swaps_url)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .header("Accept", "application/json")
            .header("Origin", "https://fomo.family")
            .header("Referer", "https://fomo.family/");

        if !token.starts_with("Bearer ") {
            req = req.header("Authorization", format!("Bearer {}", token));
        } else {
            req = req.header("Authorization", token);
        }

        let resp = match req.send().await {
            Ok(r) if r.status().is_success() => r,
            _ => return Ok(None),
        };

        let json: Value = resp.json().await.unwrap_or_default();
        let swaps = json.get("responseObject").and_then(|r| r.get("swaps")).and_then(|s| s.as_array());

        if let Some(swaps_list) = swaps {
            for swap in swaps_list {
                let swap_id = swap.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if swap_id.is_empty() {
                    continue;
                }

                // Check Deduplication Cache
                {
                    let mut cache = self.processed_swap_ids.lock().unwrap();
                    if cache.contains(swap_id) {
                        continue;
                    }
                    cache.insert(swap_id.to_string());
                    if cache.len() > 2000 {
                        cache.clear();
                    }
                }

                let in_token = swap.get("inTokenAddress").and_then(|v| v.as_str()).unwrap_or("");
                let out_token = swap.get("outTokenAddress").and_then(|v| v.as_str()).unwrap_or("");
                let in_usd = swap.get("humanUsdAmountIn").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let out_usd = swap.get("humanUsdAmountOut").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let amount_usd = if in_usd > 0.0 { in_usd } else { out_usd };

                let is_in_collateral = in_token == "So11111111111111111111111111111111111111112"
                    || in_token == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                    || in_token == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";

                let is_out_collateral = out_token == "So11111111111111111111111111111111111111112"
                    || out_token == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                    || out_token == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";

                let (activity_type, token_mint) = if is_in_collateral && !is_out_collateral {
                    (ActivityType::Buy, out_token.to_string())
                } else if !is_in_collateral && is_out_collateral {
                    (ActivityType::Sell, in_token.to_string())
                } else {
                    continue;
                };

                if token_mint.is_empty() {
                    continue;
                }

                // Fetch real-time token symbol & price
                let (token_symbol, price_usd, _liq) = crate::streams::SolanaStreamListener::fetch_token_price(&token_mint)
                    .await
                    .unwrap_or(("TOKEN".to_string(), 0.0, 0.0));

                let act = WalletActivity {
                    id: None,
                    wallet_handle: handle.to_string(),
                    wallet_address: solana_address.to_string(),
                    chain: Chain::Solana,
                    activity_type,
                    token_symbol,
                    token_mint,
                    amount_usd,
                    token_amount: 1000.0,
                    execution_price_usd: price_usd,
                    tx_signature: format!("FOMO-{}", swap_id),
                    timestamp: Utc::now(),
                };

                let _ = db.insert_activity(&act);
                return Ok(Some(act));
            }
        }

        Ok(None)
    }
}
