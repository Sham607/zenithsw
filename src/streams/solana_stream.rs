use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

pub struct SolanaStreamListener {
    rpc_url: String,
    client: reqwest::Client,
    processed_signatures: Arc<Mutex<HashSet<String>>>,
}

impl SolanaStreamListener {
    pub fn new(rpc_url: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()
            .unwrap_or_default();
        Self {
            rpc_url: rpc_url.to_string(),
            client,
            processed_signatures: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Pulls recent confirmed parsed transactions for target wallet using Helius RPC
    pub async fn check_wallet_activity(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        let url = if self.rpc_url.contains("helius-rpc.com") {
            let api_key = self.rpc_url.split("api-key=").nth(1).unwrap_or("");
            format!(
                "https://api.helius.xyz/v0/addresses/{}/transactions?api-key={}&limit=3",
                address, api_key
            )
        } else {
            return Ok(None);
        };

        let res = match self.client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => return Ok(None),
        };

        let txs: Vec<Value> = res.json().await.unwrap_or_default();
        let now = Utc::now();

        for tx in txs {
            let tx_type = tx.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let signature = tx.get("signature").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp_num = tx.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);

            if signature.is_empty() {
                continue;
            }

            // Deduplication Check
            {
                let mut cache = self.processed_signatures.lock().unwrap();
                if cache.contains(signature) {
                    continue;
                }
                cache.insert(signature.to_string());
                if cache.len() > 2000 {
                    cache.clear();
                }
            }

            // Check if transaction is within last 180 seconds
            let tx_age_secs = if timestamp_num > 0 {
                (now.timestamp() - timestamp_num).max(0)
            } else {
                0
            };

            if tx_type == "SWAP" && tx_age_secs < 180 {
                let token_transfers = tx.get("tokenTransfers").and_then(|v| v.as_array());

                let mut activity_type = ActivityType::Buy;
                let mut token_symbol = "UNKNOWN".to_string();
                let mut token_mint = "".to_string();
                let mut amount_usd = 0.0;
                let mut token_amount = 0.0;
                let mut price_usd = 0.0;

                if let Some(transfers) = token_transfers {
                    for t in transfers {
                        let to_user = t.get("toUserAccount").and_then(|v| v.as_str()).unwrap_or("");
                        let from_user = t.get("fromUserAccount").and_then(|v| v.as_str()).unwrap_or("");
                        let mint = t.get("mint").and_then(|v| v.as_str()).unwrap_or("");
                        let amount = t.get("tokenAmount").and_then(|v| v.as_f64()).unwrap_or(0.0);

                        // Native SOL or USDC filter
                        let is_collateral = mint == "So11111111111111111111111111111111111111112"
                            || mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                            || mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";

                        if !is_collateral {
                            if to_user == address {
                                activity_type = ActivityType::Buy;
                                token_mint = mint.to_string();
                                token_amount = amount;
                            } else if from_user == address {
                                activity_type = ActivityType::Sell;
                                token_mint = mint.to_string();
                                token_amount = amount;
                            }
                        }
                    }
                }

                if !token_mint.is_empty() {
                    // Fetch real-time token price & pool info
                    if let Ok((sym, price, _liq)) = Self::fetch_token_price(&token_mint).await {
                        token_symbol = sym;
                        price_usd = price;
                        amount_usd = if token_amount > 0.0 && price_usd > 0.0 {
                            token_amount * price_usd
                        } else {
                            500.0
                        };
                    }

                    let act = WalletActivity {
                        id: None,
                        wallet_handle: handle.to_string(),
                        wallet_address: address.to_string(),
                        chain: Chain::Solana,
                        activity_type,
                        token_symbol,
                        token_mint,
                        amount_usd,
                        token_amount,
                        execution_price_usd: price_usd,
                        tx_signature: signature.to_string(),
                        timestamp: now,
                    };

                    let _ = db.insert_activity(&act);
                    return Ok(Some(act));
                }
            }
        }

        Ok(None)
    }

    /// Fetches live price & liquidity with multi-source fallback (DexScreener + Jupiter Price API v2)
    pub async fn fetch_token_price(mint: &str) -> Result<(String, f64, f64)> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .build()?;

        // Source 1: DexScreener API
        let url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", mint);
        if let Ok(res) = client.get(&url).send().await {
            if let Ok(json) = res.json::<Value>().await {
                if let Some(pairs) = json.get("pairs").and_then(|v| v.as_array()) {
                    if let Some(pair) = pairs.first() {
                        let symbol = pair.get("baseToken")
                            .and_then(|b| b.get("symbol"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("TOKEN")
                            .to_string();
                        let price_usd: f64 = pair.get("priceUsd")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let liquidity_usd: f64 = pair.get("liquidity")
                            .and_then(|l| l.get("usd"))
                            .and_then(|u| u.as_f64())
                            .unwrap_or(0.0);
                        if price_usd > 0.0 {
                            return Ok((symbol, price_usd, liquidity_usd));
                        }
                    }
                }
            }
        }

        // Source 2: Jupiter Price API v2 Fallback
        let jup_url = format!("https://api.jup.ag/price/v2?ids={}", mint);
        if let Ok(res) = client.get(&jup_url).send().await {
            if let Ok(json) = res.json::<Value>().await {
                if let Some(data) = json.get("data").and_then(|d| d.get(mint)) {
                    let price_usd: f64 = data.get("price")
                        .and_then(|p| p.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);
                    if price_usd > 0.0 {
                        return Ok(("JUP_TOKEN".to_string(), price_usd, 40000.0));
                    }
                }
            }
        }

        Ok(("UNKNOWN".to_string(), 0.0, 0.0))
    }
}
