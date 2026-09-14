use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

static PRICE_CACHE: OnceLock<RwLock<HashMap<String, (String, f64, f64, f64, Instant)>>> =
    OnceLock::new();

fn get_price_cache() -> &'static RwLock<HashMap<String, (String, f64, f64, f64, Instant)>> {
    PRICE_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub struct SolanaStreamListener {
    rpc_urls: Vec<String>,
    current_rpc_idx: AtomicUsize,
    last_signatures: Arc<Mutex<HashMap<String, String>>>,
    processed_signatures: Arc<Mutex<HashSet<String>>>,
    client: Client,
}

impl SolanaStreamListener {
    pub fn new(primary_rpc_url: &str) -> Self {
        let mut rpc_urls: Vec<String> = primary_rpc_url
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        // Public fallbacks remain read-capable options; live broadcast uses the same
        // pool only after signature reconciliation.
        for fallback in [
            "https://solana-rpc.publicnode.com",
            "https://rpc.ankr.com/solana",
            "https://api.mainnet-beta.solana.com",
        ] {
            if !rpc_urls.iter().any(|url| url == fallback) {
                rpc_urls.push(fallback.to_string());
            }
        }

        Self {
            rpc_urls,
            current_rpc_idx: AtomicUsize::new(0),
            last_signatures: Arc::new(Mutex::new(HashMap::new())),
            processed_signatures: Arc::new(Mutex::new(HashSet::new())),
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap_or_default(),
        }
    }

    fn get_rpc_url(&self) -> String {
        let idx = self.current_rpc_idx.load(Ordering::Relaxed) % self.rpc_urls.len();
        self.rpc_urls[idx].clone()
    }

    fn rotate_rpc(&self) {
        self.current_rpc_idx.fetch_add(1, Ordering::Relaxed);
    }

    /// Checks the latest on-chain transaction for a Solana smart wallet with Multi-RPC Failover
    pub async fn check_recent_txs(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        if self.rpc_urls.is_empty() {
            return Ok(None);
        }

        // 1. Get latest signature via getSignaturesForAddress with RPC failover
        let sig_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignaturesForAddress",
            "params": [
                address,
                { "limit": 1 }
            ]
        });

        let mut sig_items = None;
        for _ in 0..self.rpc_urls.len() {
            let rpc = self.get_rpc_url();
            match self.client.post(&rpc).json(&sig_payload).send().await {
                Ok(res) if res.status().is_success() => {
                    if let Ok(sig_json) = res.json::<Value>().await {
                        if let Some(arr) = sig_json["result"].as_array() {
                            sig_items = Some(arr.clone());
                            break;
                        }
                    }
                }
                _ => {
                    self.rotate_rpc(); // Failover to next RPC on 429 / timeout
                }
            }
        }

        let sig_items = match sig_items {
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
                // First time baseline registration on startup
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

        // 2. Fetch full parsed transaction via JSON-RPC getTransaction with RPC failover
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

        let mut tx_result = None;
        for _ in 0..self.rpc_urls.len() {
            let rpc = self.get_rpc_url();
            match self.client.post(&rpc).json(&tx_payload).send().await {
                Ok(res) if res.status().is_success() => {
                    if let Ok(tx_json) = res.json::<Value>().await {
                        if !tx_json["result"].is_null() {
                            tx_result = Some(tx_json["result"].clone());
                            break;
                        }
                    }
                }
                _ => {
                    self.rotate_rpc();
                }
            }
        }

        let tx_result = match tx_result {
            Some(res) => res,
            None => return Ok(None),
        };

        let meta = &tx_result["meta"];
        if meta.is_null() {
            return Ok(None);
        }

        let pre_token_balances = meta["preTokenBalances"].as_array();
        let post_token_balances = meta["post_tokenBalances"]
            .as_array()
            .or_else(|| meta["postTokenBalances"].as_array());

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

        let mut execution_price_usd = 0.0;
        if let Ok((sym, price, _, _)) = Self::fetch_token_price(&token_mint).await {
            if sym != "UNKNOWN" {
                token_symbol = sym;
            }
            execution_price_usd = price;
        }

        // NOTE: Strict policy - No fallback price derivation from native SOL deltas to prevent anomalous price spikes
        let amount_usd = if token_amount > 0.0 && execution_price_usd > 0.0 {
            token_amount * execution_price_usd
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

    /// Fetches live price & aggregated liquidity with multi-pool scanning & in-memory TTL caching (DexScreener + Search Fallback)
    pub async fn fetch_token_price(mint: &str) -> Result<(String, f64, f64, f64)> {
        // Step 1: Check in-memory price cache (TTL 3 seconds)
        if let Ok(cache) = get_price_cache().read() {
            if let Some((sym, price, liq, mcap, ts)) = cache.get(mint) {
                if ts.elapsed() < Duration::from_secs(3) {
                    return Ok((sym.clone(), *price, *liq, *mcap));
                }
            }
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(4))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()?;

        // Source 1: DexScreener API with strict baseToken matching & aggregated liquidity
        let url = format!("https://api.dexscreener.com/latest/dex/tokens/{}", mint);
        if let Ok(res) = client.get(&url).send().await {
            if res.status().is_success() {
                if let Ok(json) = res.json::<Value>().await {
                if let Some(pairs) = json.get("pairs").and_then(|v| v.as_array()) {
                    if !pairs.is_empty() {
                        let mut max_liq = 0.0;
                        let mut total_liq = 0.0;
                        let mut best_symbol = "TOKEN".to_string();
                        let mut best_price = 0.0;
                        let mut best_mcap = 0.0;

                        for pair in pairs {
                            let base_addr = pair
                                .get("baseToken")
                                .and_then(|b| b.get("address"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("");

                            // STRICT: Base token address MUST match query mint (case-insensitive)
                            if !base_addr.eq_ignore_ascii_case(mint) {
                                continue;
                            }

                            let dex_id = pair.get("dexId").and_then(|s| s.as_str()).unwrap_or("");
                            let sym = pair
                                .get("baseToken")
                                .and_then(|b| b.get("symbol"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("");
                            let price: f64 = pair
                                .get("priceUsd")
                                .and_then(|p| p.as_str())
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0.0);

                            let mut liq: f64 = pair
                                .get("liquidity")
                                .and_then(|l| l.get("usd"))
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);

                            // Market cap harus berasal dari field marketCap; FDV bukan market cap.
                            let mcap: f64 = pair
                                .get("marketCap")
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);

                            let quote_sym = pair
                                .get("quoteToken")
                                .and_then(|q| q.get("symbol"))
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_uppercase();
                            let quote_amt = pair
                                .get("liquidity")
                                .and_then(|l| l.get("quote"))
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);

                            let real_quote_usd = match quote_sym.as_str() {
                                "USDC" | "USDT" | "USD" | "USDG" | "DAI" | "USDS" | "PYUSD" => {
                                    quote_amt
                                }
                                "SOL" | "WSOL" => quote_amt * 102.0,
                                "ETH" | "WETH" => quote_amt * 2500.0,
                                _ => quote_amt.min(liq * 0.5),
                            };

                            // Anti-CLMM Spoof: If single-sided fake liquidity has < $10k real quote collateral, clamp liquidity to real quote
                            if dex_id != "pumpfun" {
                                if liq > 100_000.0 && (real_quote_usd / liq) < 0.10 {
                                    liq = real_quote_usd * 2.0;
                                } else if real_quote_usd < 10_000.0 {
                                    liq = real_quote_usd;
                                }
                            }

                            // For Pump.fun bonding curves where liquidity object is null/0, derive virtual liquidity from marketCap / fdv
                            if dex_id == "pumpfun" && liq <= 0.0 {
                                if mcap > 0.0 {
                                    liq = mcap * 0.50; // Pump.fun virtual pool reserve ~ 50% of MC
                                }
                            }

                            total_liq += liq;

                            if liq >= max_liq && price > 0.0 {
                                max_liq = liq;
                                best_price = price;
                                if mcap > 0.0 {
                                    best_mcap = mcap;
                                }
                                if !sym.is_empty() {
                                    best_symbol = sym.to_string();
                                }
                            } else if best_price <= 0.0 && price > 0.0 {
                                best_price = price;
                                if mcap > 0.0 && best_mcap <= 0.0 {
                                    best_mcap = mcap;
                                }
                                if !sym.is_empty() {
                                    best_symbol = sym.to_string();
                                }
                            }
                        }

                        let effective_liq = max_liq.max(total_liq);
                        if best_price > 0.0 {
                            if let Ok(mut cache) = get_price_cache().write() {
                                cache.insert(
                                    mint.to_string(),
                                    (
                                        best_symbol.clone(),
                                        best_price,
                                        effective_liq,
                                        best_mcap,
                                        Instant::now(),
                                    ),
                                );
                            }
                            return Ok((best_symbol, best_price, effective_liq, best_mcap));
                        }
                    }
                }
            }
        }
        }

        // Source 2: DexScreener Search Endpoint Fallback with strict baseToken matching
        let search_url = format!("https://api.dexscreener.com/latest/dex/search?q={}", mint);
        if let Ok(res) = client.get(&search_url).send().await {
            if let Ok(json) = res.json::<Value>().await {
                if let Some(pairs) = json.get("pairs").and_then(|v| v.as_array()) {
                    if let Some(pair) = pairs.iter().find(|p| {
                        p.get("baseToken")
                            .and_then(|b| b.get("address"))
                            .and_then(|s| s.as_str())
                            .map(|addr| addr.eq_ignore_ascii_case(mint))
                            .unwrap_or(false)
                    }) {
                        let symbol = pair
                            .get("baseToken")
                            .and_then(|b| b.get("symbol"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("TOKEN")
                            .to_string();
                        let price_usd: f64 = pair
                            .get("priceUsd")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0);
                        let liquidity_usd: f64 = pair
                            .get("liquidity")
                            .and_then(|l| l.get("usd"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let mcap: f64 = pair
                            .get("marketCap")
                            .or_else(|| pair.get("fdv"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        if price_usd > 0.0 && liquidity_usd > 0.0 {
                            if let Ok(mut cache) = get_price_cache().write() {
                                cache.insert(
                                    mint.to_string(),
                                    (symbol.clone(), price_usd, liquidity_usd, mcap, Instant::now()),
                                );
                            }
                            return Ok((symbol, price_usd, liquidity_usd, mcap));
                        }
                    }
                }
            }
        }

        Ok(("UNKNOWN".to_string(), 0.0, 0.0, 0.0))
    }
}
