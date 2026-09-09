use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

pub struct FomoApiStreamListener {
    config: Arc<Config>,
    client: reqwest::Client,
    processed_swap_ids: Arc<Mutex<HashSet<String>>>,
    user_id_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl FomoApiStreamListener {
    pub fn new(config: Arc<Config>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
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
            let auth_header = if token.starts_with("Bearer ") {
                token.to_string()
            } else {
                format!("Bearer {}", token)
            };

            let api_url = "https://prod-api.fomo.family/v2/leaderboard?timeframe=24h";
            let req = self.client.get(api_url)
                .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .header("Origin", "https://fomo.family")
                .header("Referer", "https://fomo.family/")
                .header("Accept", "application/json, text/plain, */*")
                .header("privy-app-id", "cm6h485v300n3zj9yl6vpedq7")
                .header("Authorization", auth_header);

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

        let auth_header = if token.starts_with("Bearer ") {
            token.to_string()
        } else {
            format!("Bearer {}", token)
        };

        let swaps_url = format!("https://prod-api.fomo.family/v2/users/{}/swaps", user_id);
        let req = self.client.get(&swaps_url)
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .header("Origin", "https://fomo.family")
            .header("Referer", "https://fomo.family/")
            .header("Accept", "application/json, text/plain, */*")
            .header("privy-app-id", "cm6h485v300n3zj9yl6vpedq7")
            .header("Authorization", auth_header);

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

                // Check deduplication
                {
                    let mut processed = self.processed_swap_ids.lock().unwrap();
                    if processed.contains(swap_id) {
                        continue;
                    }
                    processed.insert(swap_id.to_string());
                    if processed.len() > 1000 {
                        let to_remove: Vec<String> = processed.iter().take(200).cloned().collect();
                        for r in to_remove {
                            processed.remove(&r);
                        }
                    }
                }

                let in_token = swap.get("inTokenAddress").and_then(|v| v.as_str()).unwrap_or("");
                let out_token = swap.get("outTokenAddress").and_then(|v| v.as_str()).unwrap_or("");
                let amount_usd = swap.get("humanUsdAmountIn").and_then(|v| v.as_f64())
                    .or_else(|| swap.get("humanUsdAmountOut").and_then(|v| v.as_f64()))
                    .unwrap_or(50.0);

                let is_buy = in_token.contains("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v") // USDC
                    || in_token.contains("So11111111111111111111111111111111111111112") // SOL
                    || in_token.contains("0x");

                let (activity_type, token_mint, mut token_symbol) = if is_buy {
                    (ActivityType::Buy, out_token.to_string(), "FOMO_COIN".to_string())
                } else {
                    (ActivityType::Sell, in_token.to_string(), "FOMO_COIN".to_string())
                };

                let mut execution_price_usd = 0.0;
                if let Ok((sym, price, _)) = crate::streams::solana_stream::SolanaStreamListener::fetch_token_price(&token_mint).await {
                    if sym != "UNKNOWN" {
                        token_symbol = sym;
                    }
                    execution_price_usd = price;
                }

                let activity = WalletActivity {
                    id: None,
                    wallet_handle: handle.to_string(),
                    wallet_address: solana_address.to_string(),
                    chain: Chain::Solana,
                    activity_type,
                    token_symbol,
                    token_mint,
                    amount_usd,
                    token_amount: 1.0,
                    execution_price_usd,
                    tx_signature: swap_id.to_string(),
                    timestamp: Utc::now(),
                };

                let _ = db.insert_activity(&activity);
                return Ok(Some(activity));
            }
        }

        Ok(None)
    }
}
