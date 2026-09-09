use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct SolanaStreamListener {
    rpc_url: String,
    processed_signatures: Arc<Mutex<HashSet<String>>>,
}

impl SolanaStreamListener {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            processed_signatures: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Pulls recent transactions for a wallet and converts them to WalletActivity
    pub async fn check_recent_txs(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        if self.rpc_url.is_empty() {
            return Ok(None);
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let helius_url = if self.rpc_url.contains("helius") {
            let base = self.rpc_url.split('?').next().unwrap_or(&self.rpc_url);
            let api_key = self.rpc_url.split("api-key=").nth(1).unwrap_or("");
            if !api_key.is_empty() {
                format!("https://api.helius.xyz/v0/addresses/{}/transactions?api-key={}&limit=2", address, api_key)
            } else {
                format!("{}/v0/addresses/{}/transactions?limit=2", base, address)
            }
        } else {
            return Ok(None);
        };

        let res = match client.get(&helius_url).send().await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        if !res.status().is_success() {
            return Ok(None);
        }

        let txs: Value = match res.json().await {
            Ok(j) => j,
            Err(_) => return Ok(None),
        };

        let tx_array = match txs.as_array() {
            Some(arr) => arr,
            None => return Ok(None),
        };

        let now = Utc::now();

        for tx in tx_array {
            let signature = tx.get("signature").and_then(|v| v.as_str()).unwrap_or("");
            if signature.is_empty() {
                continue;
            }

            // In-memory signature deduplication
            {
                let mut processed = self.processed_signatures.lock().unwrap();
                if processed.contains(signature) {
                    continue;
                }
                processed.insert(signature.to_string());
                if processed.len() > 1000 {
                    let to_remove: Vec<String> = processed.iter().take(200).cloned().collect();
                    for r in to_remove {
                        processed.remove(&r);
                    }
                }
            }

            let tx_type = tx.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let timestamp_num = tx.get("timestamp").and_then(|v| v.as_i64()).unwrap_or(0);
            
            // Only process fresh transactions (< 180s old)
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
                let mut whale_balance_before = 0.0;
                let mut whale_balance_after = 0.0;
                let mut sell_ratio_pct = 0.0;

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

                    // Calculate whale balance before/after and sell portion
                    if activity_type == ActivityType::Sell {
                        // Check accountData balance changes if present
                        if let Some(account_data) = tx.get("accountData").and_then(|v| v.as_array()) {
                            for acc in account_data {
                                let acc_owner = acc.get("account").and_then(|v| v.as_str()).unwrap_or("");
                                if acc_owner == address {
                                    if let Some(token_balances) = acc.get("tokenBalanceChanges").and_then(|v| v.as_array()) {
                                        for tb in token_balances {
                                            let tb_mint = tb.get("mint").and_then(|v| v.as_str()).unwrap_or("");
                                            if tb_mint == token_mint {
                                                let raw_amount = tb.get("rawTokenAmount").and_then(|r| r.get("tokenAmount")).and_then(|v| v.as_str()).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
                                                whale_balance_after = raw_amount.max(0.0);
                                                whale_balance_before = whale_balance_after + token_amount;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if whale_balance_before > 0.0 {
                            sell_ratio_pct = ((token_amount / whale_balance_before) * 100.0).clamp(1.0, 100.0);
                        } else {
                            // Default to full exit if balance breakdown isn't explicit
                            sell_ratio_pct = 100.0;
                        }
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
                        whale_balance_before,
                        whale_balance_after,
                        sell_ratio_pct,
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
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);

                        if price_usd > 0.0 {
                            return Ok((symbol, price_usd, liquidity_usd));
                        }
                    }
                }
            }
        }

        // Source 2: Jupiter Price API v2
        let jup_url = format!("https://api.jup.ag/price/v2?ids={}", mint);
        if let Ok(res) = client.get(&jup_url).send().await {
            if let Ok(json) = res.json::<Value>().await {
                if let Some(data) = json.get("data").and_then(|d| d.get(mint)) {
                    let price_usd: f64 = data.get("price")
                        .and_then(|p| p.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0.0);

                    if price_usd > 0.0 {
                        return Ok(("JUP_TOKEN".to_string(), price_usd, 50_000.0));
                    }
                }
            }
        }

        Ok(("UNKNOWN".to_string(), 0.0, 0.0))
    }
}
