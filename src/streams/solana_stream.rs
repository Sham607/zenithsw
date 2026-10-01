use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

type PriceCacheEntry = (String, f64, f64, f64, Instant);
type PriceCache = RwLock<HashMap<String, PriceCacheEntry>>;

static PRICE_CACHE: OnceLock<PriceCache> = OnceLock::new();

/// Quote/collateral mints yang dipakai whale untuk membayar.
pub fn is_collateral_mint(mint: &str) -> bool {
    mint == "So11111111111111111111111111111111111111112"
        || mint == "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        || mint == "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
        || mint == "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo"
}

/// Klasifikasi arah pergerakan token utk target wallet.
/// Buy/Sell hanya jika ada pembayaran: stables (USDC/USDT/PYUSD, ~$1/unit) atau SOL
/// bergerak BERLAWANAN arah di wallet yang sama. Ambang SOL -0.001 memisahkan
/// pembayaran swap dari biaya gas (~0.000005). Tanpa aliran pembayaran = transfer.
pub fn classify_token_movement(
    delta_token: f64,
    delta_stables_usd: f64,
    delta_sol: f64,
) -> ActivityType {
    const STABLE_PAYMENT_USD: f64 = 0.01;
    const SOL_PAYMENT: f64 = 0.001;
    let paid = delta_stables_usd < -STABLE_PAYMENT_USD || delta_sol < -SOL_PAYMENT;
    let received = delta_stables_usd > STABLE_PAYMENT_USD || delta_sol > SOL_PAYMENT;
    if delta_token > 0.0 {
        if paid {
            ActivityType::Buy
        } else {
            ActivityType::Transfer
        }
    } else if delta_token < 0.0 {
        if received {
            ActivityType::Sell
        } else {
            ActivityType::Transfer
        }
    } else {
        ActivityType::Transfer
    }
}

/// Menghitung selisih saldo Native SOL untuk akun tertentu,
/// dengan mengompensasi potongan fee transaksi jika akun tersebut adalah fee payer (index 0).
pub fn extract_native_sol_delta(meta: &Value, transaction: &Value, address: &str) -> f64 {
    let account_keys = match transaction["message"]["accountKeys"].as_array() {
        Some(keys) => keys,
        None => return 0.0,
    };

    let account_idx = match account_keys
        .iter()
        .position(|k| k.as_str() == Some(address) || k["pubkey"].as_str() == Some(address))
    {
        Some(idx) => idx,
        None => return 0.0,
    };

    let pre_balances = match meta["preBalances"].as_array() {
        Some(arr) => arr,
        None => return 0.0,
    };
    let post_balances = match meta["postBalances"].as_array() {
        Some(arr) => arr,
        None => return 0.0,
    };

    let pre_lamports = pre_balances
        .get(account_idx)
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let post_lamports = post_balances
        .get(account_idx)
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let fee_lamports = meta["fee"].as_f64().unwrap_or(0.0);

    let fee_offset = if account_idx == 0 { fee_lamports } else { 0.0 };
    (post_lamports - pre_lamports + fee_offset) / 1_000_000_000.0
}

fn get_price_cache() -> &'static PriceCache {
    PRICE_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

pub struct SolanaStreamListener {
    rpc_urls: Vec<String>,
    execution_rpc_url: Option<String>,
    current_rpc_idx: AtomicUsize,
    last_signatures: Arc<Mutex<HashMap<String, String>>>,
    processed_signatures: Arc<Mutex<HashSet<String>>>,
    client: Client,
}

impl SolanaStreamListener {
    pub fn new(primary_rpc_url: &str, execution_rpc_url: Option<String>) -> Self {
        let mut rpc_urls: Vec<String> = primary_rpc_url
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        // Public fallbacks remain read-capable options; live broadcast uses the same
        // pool only after signature reconciliation.
        for fallback in ["https://api.mainnet-beta.solana.com"] {
            if !rpc_urls.iter().any(|url| url == fallback) {
                rpc_urls.push(fallback.to_string());
            }
        }

        Self {
            rpc_urls,
            execution_rpc_url,
            current_rpc_idx: AtomicUsize::new(0),
            last_signatures: Arc::new(Mutex::new(HashMap::new())),
            processed_signatures: Arc::new(Mutex::new(HashSet::new())),
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .user_agent("ZenithSW/1.0")
                .build()
                .unwrap_or_default(),
        }
    }

    fn next_rpc_index(&self) -> usize {
        self.current_rpc_idx.fetch_add(1, Ordering::Relaxed) % self.rpc_urls.len()
    }

    /// Checks the latest on-chain transaction for a Solana smart wallet with Multi-RPC Failover
    pub async fn check_recent_txs(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        self.check_transaction_for_signature(handle, address, None, db)
            .await
    }

    async fn check_transaction_for_signature(
        &self,
        handle: &str,
        address: &str,
        notified_signature: Option<&str>,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        if self.rpc_urls.is_empty() {
            return Ok(None);
        }

        // 1. Event WebSocket memberi signature langsung; polling mengambil signature terbaru.
        let (signature, block_time) = if let Some(notified) = notified_signature {
            (notified.to_string(), 0i64)
        } else {
            let sig_payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getSignaturesForAddress",
                "params": [address, { "limit": 1 }]
            });
            let mut sig_items = None;
            let start_idx = self.next_rpc_index();
            for offset in 0..self.rpc_urls.len() {
                let rpc = &self.rpc_urls[(start_idx + offset) % self.rpc_urls.len()];
                match self.client.post(rpc).json(&sig_payload).send().await {
                    Ok(res) if res.status().is_success() => {
                        if let Ok(sig_json) = res.json::<Value>().await {
                            if let Some(arr) = sig_json["result"].as_array() {
                                sig_items = Some(arr.clone());
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            let Some(arr) = sig_items.filter(|arr| !arr.is_empty()) else {
                return Ok(None);
            };
            let item = &arr[0];
            (
                item["signature"].as_str().unwrap_or("").to_string(),
                item["blockTime"].as_i64().unwrap_or(0),
            )
        };
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
                if last_sig == &signature {
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
            if processed.contains(&signature) {
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
                    "maxSupportedTransactionVersion": 1
                }
            ]
        });

        let mut tx_result = None;
        // Dual-channel: prioritas kanal presisi (Helius) untuk parsing mendalam getTransaction
        if let Some(ref exec_rpc) = self.execution_rpc_url {
            if let Ok(res) = self.client.post(exec_rpc).json(&tx_payload).send().await {
                if res.status().is_success() {
                    if let Ok(tx_json) = res.json::<Value>().await {
                        if !tx_json["result"].is_null() {
                            tx_result = Some(tx_json["result"].clone());
                        }
                    }
                }
            }
        }

        // Fallback ke pool RPC publik jika eksekusi Helius tidak tersedia / gagal
        if tx_result.is_none() {
            let start_idx = self.next_rpc_index();
            for offset in 0..self.rpc_urls.len() {
                let rpc = &self.rpc_urls[(start_idx + offset) % self.rpc_urls.len()];
                match self.client.post(rpc).json(&tx_payload).send().await {
                    Ok(res) if res.status().is_success() => {
                        if let Ok(tx_json) = res.json::<Value>().await {
                            if !tx_json["result"].is_null() {
                                tx_result = Some(tx_json["result"].clone());
                                break;
                            }
                        }
                    }
                    _ => {}
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
        let mut activity_type = ActivityType::Transfer;
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

            // Pilih target mint non-collateral dengan |delta| terbesar,
            // termasuk mint yang HILANG dari post_map (exit/transfer penuh).
            let mut best: Option<(String, f64, f64, f64)> = None; // (mint, delta, bal_before, bal_after)
            for (mint, post_amt) in &post_map {
                if is_collateral_mint(mint) {
                    continue;
                }
                let pre_amt = pre_map.get(mint).copied().unwrap_or(0.0);
                let delta = post_amt - pre_amt;
                if delta.abs() > 0.000001
                    && best
                        .as_ref()
                        .map(|(_, bd, _, _)| delta.abs() > bd.abs())
                        .unwrap_or(true)
                {
                    best = Some((mint.clone(), delta, pre_amt, *post_amt));
                }
            }
            for (mint, pre_amt) in &pre_map {
                if post_map.contains_key(mint) || is_collateral_mint(mint) || *pre_amt <= 0.000001 {
                    continue;
                }
                if best
                    .as_ref()
                    .map(|(_, bd, _, _)| *pre_amt > bd.abs())
                    .unwrap_or(true)
                {
                    best = Some((mint.clone(), -*pre_amt, *pre_amt, 0.0));
                }
            }

            if let Some((mint, delta, bal_before, bal_after)) = best {
                let native_sol_delta =
                    extract_native_sol_delta(meta, &tx_result["transaction"], address);
                // Delta quote asset di wallet yang sama = bukti pembayaran swap (Native SOL + WSOL + Stables).
                let q = |m: &str| {
                    post_map.get(m).copied().unwrap_or(0.0) - pre_map.get(m).copied().unwrap_or(0.0)
                };
                let delta_wsol = q("So11111111111111111111111111111111111111112");
                let delta_sol = native_sol_delta + delta_wsol;
                let delta_stables_usd = q("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
                    + q("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB")
                    + q("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");

                activity_type = classify_token_movement(delta, delta_stables_usd, delta_sol);
                token_mint = mint;
                whale_balance_before = bal_before;
                whale_balance_after = bal_after;
                token_amount = delta.abs();
                if activity_type == ActivityType::Sell && bal_before > 0.0 {
                    sell_ratio_pct = ((token_amount / bal_before) * 100.0).clamp(1.0, 100.0);
                } else {
                    sell_ratio_pct = 0.0;
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
            source_reference: None,
            parser_version: None,
            finality: None,
        };

        let _ = db.insert_activity(&activity);
        Ok(Some(activity))
    }

    /// Mengambil signature dan slot dari `logsNotification` Solana.
    /// Hanya notifikasi sukses yang dikembalikan; detail transaksi tetap diverifikasi
    /// melalui `getTransaction` oleh collector HTTP yang sudah ada.
    pub fn parse_logs_notification(message: &str) -> Option<(String, u64)> {
        let value: Value = serde_json::from_str(message).ok()?;
        if value.get("method")?.as_str()? != "logsNotification" {
            return None;
        }
        let result = value.get("params")?.get("result")?;
        let slot = result.get("context")?.get("slot")?.as_u64()?;
        let notification = result.get("value")?;
        if !notification.get("err")?.is_null() {
            return None;
        }
        let signature = notification.get("signature")?.as_str()?.trim();
        if signature.is_empty() {
            return None;
        }
        Some((signature.to_string(), slot))
    }

    /// Memantau wallet qualified melalui Solana `logsSubscribe` secara read-only.
    /// Notifikasi hanya memicu verifikasi `getTransaction`; transfer mentah tidak
    /// langsung dianggap sebagai whale entry.
    pub async fn run_websocket_monitor(
        &self,
        websocket_url: &str,
        wallets: Vec<crate::db::WalletScanAddress>,
        db: Arc<SmartWalletDb>,
    ) -> Result<()> {
        if !websocket_url.starts_with("ws://") && !websocket_url.starts_with("wss://") {
            anyhow::bail!("SOLANA_WS_URL harus memakai ws:// atau wss://");
        }
        if wallets.is_empty() {
            return Ok(());
        }
        // Handle resmi dari registry: address tidak boleh dipakai sebagai handle,
        // agar confluence (dedupe identitas) tidak terpecah palsu.
        let handle_by_address: std::collections::HashMap<String, String> = db
            .get_active_wallets()
            .map(|list| {
                list.into_iter()
                    .map(|w| (w.address.clone(), w.handle))
                    .collect()
            })
            .unwrap_or_default();
        let mut socket = connect_async(websocket_url)
            .await
            .map_err(|error| anyhow::anyhow!("Solana WebSocket connect gagal: {error}"))?
            .0;
        let mut request_id = 1u64;
        let mut subscriptions = HashMap::new();
        for wallet in wallets {
            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "logsSubscribe",
                "params": [{ "mentions": [wallet.address] }, { "commitment": "confirmed" }]
            });
            socket
                .send(Message::Text(request.to_string()))
                .await
                .map_err(|error| anyhow::anyhow!("Solana WebSocket subscribe gagal: {error}"))?;
            let request_wallet = request_id;
            request_id += 1;
            subscriptions.insert(request_wallet, wallet);
        }
        let mut subscription_to_wallet = HashMap::new();
        while let Some(frame) = socket.next().await {
            let frame =
                frame.map_err(|error| anyhow::anyhow!("Solana WebSocket read gagal: {error}"))?;
            let Message::Text(text) = frame else { continue };
            let value: Value = match serde_json::from_str(&text) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if let (Some(request), Some(subscription)) = (
                value.get("id").and_then(Value::as_u64),
                value.get("result").and_then(Value::as_u64),
            ) {
                if let Some(wallet) = subscriptions.remove(&request) {
                    subscription_to_wallet.insert(subscription, wallet);
                }
                continue;
            }
            let subscription = value
                .get("params")
                .and_then(|params| params.get("subscription"))
                .and_then(Value::as_u64);
            let Some(subscription) = subscription else {
                continue;
            };
            let Some(wallet) = subscription_to_wallet.get(&subscription) else {
                continue;
            };
            let Some((signature, slot)) = Self::parse_logs_notification(&text) else {
                continue;
            };
            let resolved_handle = handle_by_address
                .get(&wallet.address)
                .cloned()
                .unwrap_or_else(|| wallet.address.clone());
            println!(
                "[SOLANA WS] wallet={} slot={} signature={}",
                wallet.address, slot, signature
            );
            if let Err(error) = self
                .check_transaction_for_signature(
                    &resolved_handle,
                    &wallet.address,
                    Some(&signature),
                    db.clone(),
                )
                .await
            {
                eprintln!(
                    "[SOLANA WS VERIFY ERROR] wallet={} signature={} error={error}",
                    wallet.address, signature
                );
            }
        }
        anyhow::bail!("Solana WebSocket disconnected")
    }

    /// Fetches live price & aggregated liquidity with multi-pool scanning & in-memory TTL caching (DexScreener + Search Fallback)
    pub async fn fetch_token_price(mint: &str) -> Result<(String, f64, f64, f64)> {
        Self::fetch_token_price_internal(mint, false).await
    }

    /// Fetches live price bypassing in-memory cache to verify sudden price crashes / stop-loss ticks
    pub async fn fetch_token_price_fresh(mint: &str) -> Result<(String, f64, f64, f64)> {
        Self::fetch_token_price_internal(mint, true).await
    }

    async fn fetch_token_price_internal(
        mint: &str,
        bypass_cache: bool,
    ) -> Result<(String, f64, f64, f64)> {
        // Step 1: Check in-memory price cache (TTL 3 seconds) unless bypass_cache is requested
        if !bypass_cache {
            if let Ok(cache) = get_price_cache().read() {
                if let Some((sym, price, liq, mcap, ts)) = cache.get(mint) {
                    if ts.elapsed() < Duration::from_secs(3) {
                        return Ok((sym.clone(), *price, *liq, *mcap));
                    }
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

                                let dex_id =
                                    pair.get("dexId").and_then(|s| s.as_str()).unwrap_or("");
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

                                // Anti-CLMM Spoof: use DexScreener's liquidity.usd directly as real_quote_usd (half of reported USD liquidity).
                                // DexScreener already converts quote reserves to USD at live prices, so no static price mapping needed.
                                let real_quote_usd = liq * 0.5;

                                // Clamp thin pools: if quote-side USD < $10k, treat that as effective liquidity
                                if dex_id != "pumpfun" && real_quote_usd < 10_000.0 {
                                    liq = real_quote_usd;
                                }

                                // For Pump.fun bonding curves where liquidity object is null/0, derive virtual liquidity from marketCap / fdv
                                if dex_id == "pumpfun" && liq <= 0.0 && mcap > 0.0 {
                                    liq = mcap * 0.50; // Pump.fun virtual pool reserve ~ 50% of MC
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

        // Source 2: DexScreener Search Endpoint Fallback with strict baseToken matching & max liquidity selection
        let search_url = format!("https://api.dexscreener.com/latest/dex/search?q={}", mint);
        if let Ok(res) = client.get(&search_url).send().await {
            if res.status().is_success() {
                if let Ok(json) = res.json::<Value>().await {
                    if let Some(pairs) = json.get("pairs").and_then(|v| v.as_array()) {
                        let mut best_pair: Option<(String, f64, f64, f64)> = None;
                        let mut max_liq = 0.0;

                        for pair in pairs {
                            let matches_mint = pair
                                .get("baseToken")
                                .and_then(|b| b.get("address"))
                                .and_then(|s| s.as_str())
                                .map(|addr| addr.eq_ignore_ascii_case(mint))
                                .unwrap_or(false);

                            if !matches_mint {
                                continue;
                            }

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

                            if price_usd > 0.0 && liquidity_usd > max_liq {
                                max_liq = liquidity_usd;
                                best_pair = Some((symbol, price_usd, liquidity_usd, mcap));
                            }
                        }

                        if let Some((symbol, price_usd, liquidity_usd, mcap)) = best_pair {
                            if let Ok(mut cache) = get_price_cache().write() {
                                cache.insert(
                                    mint.to_string(),
                                    (
                                        symbol.clone(),
                                        price_usd,
                                        liquidity_usd,
                                        mcap,
                                        Instant::now(),
                                    ),
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

#[cfg(test)]
mod tests {
    use super::{classify_token_movement, is_collateral_mint, SolanaStreamListener};
    use crate::models::ActivityType;

    #[test]
    fn buy_with_usdc_payment() {
        // Kasus nyata 21 Sep: +632.260 QUEEF dibayar 1.923 USDC
        assert_eq!(
            classify_token_movement(632_260.0, -1_923.0, 0.0),
            ActivityType::Buy
        );
    }

    #[test]
    fn buy_with_sol_payment() {
        assert_eq!(
            classify_token_movement(500.0, 0.0, -0.05),
            ActivityType::Buy
        );
    }

    #[test]
    fn inbound_transfer_without_payment_is_transfer() {
        // +token tanpa dana keluar = transfer dari feeder, bukan buy
        assert_eq!(
            classify_token_movement(669_000.0, 0.0, 0.0),
            ActivityType::Transfer
        );
        assert_eq!(
            classify_token_movement(669_000.0, 0.0, 0.000005), // cuma gas
            ActivityType::Transfer
        );
    }

    #[test]
    fn sell_requires_quote_inflow() {
        // Jual: token keluar, USDC masuk
        assert_eq!(
            classify_token_movement(-100.0, 50.0, 0.0),
            ActivityType::Sell
        );
        // Token keluar tanpa dana masuk = transfer keluar, bukan sell
        assert_eq!(
            classify_token_movement(-100.0, 0.0, 0.0),
            ActivityType::Transfer
        );
    }

    #[test]
    fn gas_only_sol_delta_is_not_payment() {
        // Gas ~0.000005 SOL jauh di bawah ambang 0.001
        assert_eq!(
            classify_token_movement(1000.0, 0.0, -0.000005),
            ActivityType::Transfer
        );
    }

    #[test]
    fn zero_delta_is_transfer() {
        assert_eq!(
            classify_token_movement(0.0, -5.0, -0.1),
            ActivityType::Transfer
        );
    }

    #[test]
    fn collateral_mints_recognized() {
        assert!(is_collateral_mint(
            "So11111111111111111111111111111111111111112"
        ));
        assert!(is_collateral_mint(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        ));
        assert!(is_collateral_mint(
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"
        ));
        assert!(is_collateral_mint(
            "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo"
        ));
        assert!(!is_collateral_mint(
            "4UHmZGe6X4DZ5dxYGiGXMhi3Sp34uPWtHB7qDMyvRbYB"
        ));
    }

    #[test]
    fn extract_native_sol_delta_handles_fee_payer_and_offset() {
        let meta = serde_json::json!({
            "preBalances": [1047564054976u64, 1574800u64],
            "postBalances": [1002564037066u64, 1574800u64],
            "fee": 17910u64
        });
        let tx = serde_json::json!({
            "message": {
                "accountKeys": [
                    {"pubkey": "2UknaPAYpAEP6mbxHRy9mcBgZCHPLWVijQsPBFGJ7Fea", "signer": true},
                    {"pubkey": "SomeOtherPubkey", "signer": false}
                ]
            }
        });
        let delta = super::extract_native_sol_delta(
            &meta,
            &tx,
            "2UknaPAYpAEP6mbxHRy9mcBgZCHPLWVijQsPBFGJ7Fea",
        );
        // -45.0 SOL net delta (fee offset added back to avoid priority fee confusion)
        assert!((delta - (-45.0)).abs() < 0.0001);
    }

    #[test]
    fn extract_native_sol_delta_returns_zero_for_unrelated_address() {
        let meta = serde_json::json!({
            "preBalances": [1000000000u64],
            "postBalances": [500000000u64],
            "fee": 5000u64
        });
        let tx = serde_json::json!({
            "message": {
                "accountKeys": ["RandomPubkey"]
            }
        });
        let delta = super::extract_native_sol_delta(&meta, &tx, "NonExistentWallet");
        assert_eq!(delta, 0.0);
    }

    #[test]
    fn buy_with_native_sol_classified_as_buy() {
        let meta = serde_json::json!({
            "preBalances": [50000000000u64],
            "postBalances": [5000000000u64],
            "fee": 5000u64
        });
        let tx = serde_json::json!({
            "message": {
                "accountKeys": [{"pubkey": "BuyerWallet", "signer": true}]
            }
        });
        let native_sol = super::extract_native_sol_delta(&meta, &tx, "BuyerWallet");
        assert_eq!(
            classify_token_movement(3467944.0, 0.0, native_sol),
            ActivityType::Buy
        );
    }

    #[test]
    fn logs_notification_accepts_successful_event() {
        let message = r#"{"jsonrpc":"2.0","method":"logsNotification","params":{"result":{"context":{"slot":3557},"value":{"signature":"sig-1","err":null,"logs":[]}},"subscription":14}}"#;
        assert_eq!(
            SolanaStreamListener::parse_logs_notification(message),
            Some(("sig-1".to_string(), 3557))
        );
    }

    #[test]
    fn logs_notification_rejects_failed_event() {
        let message = r#"{"method":"logsNotification","params":{"result":{"context":{"slot":3557},"value":{"signature":"sig-1","err":{"InstructionError":[0,"Custom"]}}}}}"#;
        assert_eq!(SolanaStreamListener::parse_logs_notification(message), None);
    }

    #[test]
    fn logs_notification_rejects_wrong_method_or_missing_fields() {
        assert_eq!(
            SolanaStreamListener::parse_logs_notification(r#"{"method":"accountNotification"}"#),
            None
        );
        assert_eq!(
            SolanaStreamListener::parse_logs_notification(
                r#"{"method":"logsNotification","params":{"result":{"context":{"slot":0},"value":{"signature":"","err":null}}}}"#
            ),
            None
        );
    }
}
