use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct SolanaStreamListener {
    rpc_url: String,
    last_signatures: Arc<Mutex<HashMap<String, String>>>,
    processed_signatures: Arc<Mutex<HashSet<String>>>,
    client: Client,
}

impl SolanaStreamListener {
    pub fn new(rpc_url: &str) -> Self {
        Self {
            rpc_url: rpc_url.to_string(),
            last_signatures: Arc::new(Mutex::new(HashMap::new())),
            processed_signatures: Arc::new(Mutex::new(HashSet::new())),
            client: Client::builder()
                .timeout(Duration::from_secs(6))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Checks the latest on-chain transaction for a Solana smart wallet via standard JSON-RPC
    pub async fn check_recent_txs(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        if self.rpc_url.is_empty() {
            return Ok(None);
        }

        // 1. Get latest signature via getSignaturesForAddress
        let sig_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignaturesForAddress",
            "params": [
                address,
                { "limit": 1 }
            ]
        });

        let sig_res = match self.client.post(&self.rpc_url).json(&sig_payload).send().await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let sig_json: Value = match sig_res.json().await {
            Ok(j) => j,
            Err(_) => return Ok(None),
        };

        let sig_items = match sig_json["result"].as_array() {
            Some(arr) if !arr.is_empty() => arr,
            _ => return Ok(None),
        };

        let latest_sig_obj = &sig_items[0];
        let signature = latest_sig_obj["signature"].as_str().unwrap_or("");
        let block_time = latest_sig_obj["blockTime"].as_i64().unwrap_or(0);

        if signature.is_empty() {
            return Ok(None);
        }

        let now = Utc::now();
        let tx_age_secs = if block_time > 0 {
            (now.timestamp() - block_time).max(0)
        } else {
            0
        };

        // Deduplication & Signature diff check
        {
            let mut last_map = self.last_signatures.lock().unwrap();
            if let Some(last_sig) = last_map.get(address) {
                if last_sig == signature {
                    return Ok(None); // No new transaction, skip
                }
            } else {
                // First time discovering this wallet on startup:
                // Record baseline signature. If tx is older than 2 minutes, ignore as historical past trade
                last_map.insert(address.to_string(), signature.to_string());
                if tx_age_secs > 120 {
                    return Ok(None);
                }
            }
            last_map.insert(address.to_string(), signature.to_string());
        }

        {
            let mut processed = self.processed_signatures.lock().unwrap();
            if processed.contains(signature) {
                return Ok(None);
            }
            processed.insert(signature.to_string());
            if processed.len() > 1000 {
                let to_remove: Vec<String> = processed.iter().take(200).cloned().collect();
                for r in to_remove {
                    processed.remove(&r);
                }
            }
        }

        // 2. Fetch full parsed transaction via JSON-RPC getTransaction
        let tx_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [
                signature,
                {
                    "encoding": "jsonParsed",
                    "maxSupportedTransactionVersion": 0
                }
            ]
        });

        let tx_res = match self.client.post(&self.rpc_url).json(&tx_payload).send().await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let tx_json: Value = match tx_res.json().await {
            Ok(j) => j,
            Err(_) => return Ok(None),
        };

        let tx_result = &tx_json["result"];
        let meta = &tx_result["meta"];
        if meta.is_null() {
            return Ok(None);
        }

        let pre_token_balances = meta["preTokenBalances"].as_array();
        let post_token_balances = meta["postTokenBalances"].as_array();

        let mut token_mint = "".to_string();
        let mut token_symbol = "UNKNOWN".to_string();
        let mut activity_type = ActivityType::Buy;
        let mut token_amount = 0.0;
        let mut whale_balance_before = 0.0;
        let mut whale_balance_after = 0.0;
        let mut sell_ratio_pct = 0.0;

        // Parse token balance delta for the smart wallet
        if let (Some(pre_arr), Some(post_arr)) = (pre_token_balances, post_token_balances) {
            let mut pre_map: HashMap<String, f64> = HashMap::new();
            for item in pre_arr {
                let owner = item["owner"].as_str().unwrap_or("");
                if owner == address {
                    let mint = item["mint"].as_str().unwrap_or("");
                    let amount = item["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
                    pre_map.insert(mint.to_string(), amount);
                }
            }

            let mut post_map: HashMap<String, f64> = HashMap::new();
            for item in post_arr {
                let owner = item["owner"].as_str().unwrap_or("");
                if owner == address {
                    let mint = item["mint"].as_str().unwrap_or("");
                    let amount = item["uiTokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
                    post_map.insert(mint.to_string(), amount);
                }
            }

            // Find changing SPL token (excluding native WSOL/USDC/USDT)
            for (mint, post_amt) in &post_map {
                let is_collateral = mint == "So11111111111111111111111111111111111111112"
                    || mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                    || mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";

                if is_collateral {
                    continue;
                }

                let pre_amt = pre_map.get(mint).copied().unwrap_or(0.0);
                if (post_amt - pre_amt).abs() > 0.000001 {
                    token_mint = mint.clone();
                    whale_balance_before = pre_amt;
                    whale_balance_after = *post_amt;

                    if *post_amt > pre_amt {
                        activity_type = ActivityType::Buy;
                        token_amount = *post_amt - pre_amt;
                        sell_ratio_pct = 0.0;
                    } else {
                        activity_type = ActivityType::Sell;
                        token_amount = pre_amt - *post_amt;
                        sell_ratio_pct = if pre_amt > 0.0 {
                            ((token_amount / pre_amt) * 100.0).clamp(1.0, 100.0)
                        } else {
                            100.0
                        };
                    }
                    break;
                }
            }

            // Check if sold 100% (present in pre_map, missing in post_map)
            if token_mint.is_empty() {
                for (mint, pre_amt) in &pre_map {
                    let is_collateral = mint == "So11111111111111111111111111111111111111112"
                        || mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                        || mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
                    if is_collateral {
                        continue;
                    }

                    if !post_map.contains_key(mint) && *pre_amt > 0.0 {
                        token_mint = mint.clone();
                        activity_type = ActivityType::Sell;
                        token_amount = *pre_amt;
                        whale_balance_before = *pre_amt;
                        whale_balance_after = 0.0;
                        sell_ratio_pct = 100.0;
                        break;
                    }
                }
            }
        }

        if token_mint.is_empty() {
            return Ok(None);
        }

        // Calculate native SOL spent/received from transaction balance deltas
        let mut native_sol_delta = 0.0;
        let account_keys = tx_result["transaction"]["message"]["accountKeys"].as_array();
        let pre_balances = meta["preBalances"].as_array();
        let post_balances = meta["postBalances"].as_array();

        if let (Some(keys), Some(pre_b), Some(post_b)) = (account_keys, pre_balances, post_balances) {
            for (idx, key_obj) in keys.iter().enumerate() {
                let pubkey_str = key_obj["pubkey"].as_str().or_else(|| key_obj.as_str()).unwrap_or("");
                if pubkey_str == address {
                    let pre_lamports = pre_b.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let post_lamports = post_b.get(idx).and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let diff = (pre_lamports - post_lamports).abs() / 1_000_000_000.0;
                    if diff > 0.001 {
                        native_sol_delta = diff;
                    }
                    break;
                }
            }
        }

        let sol_price_usd = 140.0;
        let mut execution_price_usd = 0.0;
        if let Ok((sym, price, _)) = Self::fetch_token_price(&token_mint).await {
            if sym != "UNKNOWN" {
                token_symbol = sym;
            }
            execution_price_usd = price;
        }

        // If DEX price is not yet available, derive price from native SOL spent on-chain
        if execution_price_usd <= 0.0 && native_sol_delta > 0.0 && token_amount > 0.0 {
            execution_price_usd = (native_sol_delta * sol_price_usd) / token_amount;
        }

        let amount_usd = if token_amount > 0.0 && execution_price_usd > 0.0 {
            token_amount * execution_price_usd
        } else if native_sol_delta > 0.0 {
            native_sol_delta * sol_price_usd
        } else {
            0.0
        };

        let tx_time = if block_time > 0 {
            DateTime::from_timestamp(block_time, 0)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now)
        } else {
            now
        };

        let activity = WalletActivity {
            id: None,
            wallet_handle: handle.to_string(),
            wallet_address: address.to_string(),
            chain: Chain::Solana,
            activity_type,
            token_symbol,
            token_mint,
            amount_usd,
            token_amount,
            execution_price_usd,
            whale_balance_before,
            whale_balance_after,
            sell_ratio_pct,
            tx_signature: signature.to_string(),
            timestamp: tx_time,
        };

        let _ = db.insert_activity(&activity);
        Ok(Some(activity))
    }

    /// Fetches live price & liquidity with multi-source fallback (DexScreener + Pump.fun API + Jupiter API v2)
    pub async fn fetch_token_price(mint: &str) -> Result<(String, f64, f64)> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
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

        // Source 2: Pump.fun API for newly launched bonding curves
        let pump_url = format!("https://frontend-api.pump.fun/coins/{}", mint);
        if let Ok(res) = client.get(&pump_url).send().await {
            if let Ok(json) = res.json::<Value>().await {
                let symbol = json.get("symbol").and_then(|s| s.as_str()).unwrap_or("PUMP_TOKEN").to_string();
                let usd_market_cap = json.get("usd_market_cap").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let virtual_sol_reserves = json.get("virtual_sol_reserves").and_then(|v| v.as_f64()).unwrap_or(0.0);
                
                // Virtual liquidity in USD on Pump.fun curve (2x Virtual SOL in USD)
                let liq_usd = (virtual_sol_reserves / 1_000_000_000.0) * 140.0 * 2.0;
                let price_usd = if usd_market_cap > 0.0 {
                    usd_market_cap / 1_000_000_000.0 // 1B total supply on pump.fun
                } else {
                    0.000005
                };

                if price_usd > 0.0 {
                    return Ok((symbol, price_usd, liq_usd));
                }
            }
        }

        // Source 3: Jupiter Price API v2
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
