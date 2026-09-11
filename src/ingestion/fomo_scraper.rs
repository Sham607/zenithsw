use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{is_blacklisted_asset, Chain, SmartWallet, TrackedToken, WalletTier};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::time::sleep;

pub struct FomoScraper {
    config: Arc<Config>,
    db: Arc<SmartWalletDb>,
    client: reqwest::Client,
}

impl FomoScraper {
    pub fn new(config: Arc<Config>, db: Arc<SmartWalletDb>) -> Self {
        Self {
            config,
            db,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn fetch_top_traders(&self) -> Result<Vec<SmartWallet>> {
        // 1. Fetch from Official FOMO Backend API if token is provided
        if let Some(token) = &self.config.fomo_auth_token {
            let auth_header = if token.starts_with("Bearer ") {
                token.to_string()
            } else {
                format!("Bearer {}", token)
            };
            let raw_token = token.trim_start_matches("Bearer ").trim();

            let req = self
                .client
                .get("https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d")
                .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                .header("Origin", "https://fomo.family")
                .header("Referer", "https://fomo.family/")
                .header("Accept", "application/json, text/plain, */*")
                .header("privy-app-id", "cm6h485o300n3zj9yl6vpedq7")
                .header("privy-token", raw_token)
                .header("Authorization", auth_header);

            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(json) = resp.json::<Value>().await {
                        if let Some(items) = json
                            .get("responseObject")
                            .and_then(|r| r.get("leaderboard"))
                            .and_then(|l| l.as_array())
                        {
                            let mut wallets = Vec::new();
                            for (idx, item) in items.iter().enumerate() {
                                let name = item
                                    .get("displayName")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Trader")
                                    .to_string();
                                let handle = item
                                    .get("userHandle")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("trader")
                                    .to_string();
                                let sol_addr = item
                                    .get("address")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let evm_addr = item
                                    .get("evmAddress")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();

                                let pnl = item
                                    .get("totalPnL")
                                    .and_then(|v| v.as_f64())
                                    .or_else(|| item.get("pnl").and_then(|v| v.as_f64()))
                                    .unwrap_or(0.0);

                                let num_trades =
                                    item.get("numTrades")
                                        .and_then(|v| v.as_u64())
                                        .or_else(|| item.get("swapCount").and_then(|v| v.as_u64()))
                                        .unwrap_or(20) as u32;

                                let rank = (idx + 1) as u32;

                                let (chain, address) = if !sol_addr.is_empty() {
                                    (Chain::Solana, sol_addr)
                                } else if !evm_addr.is_empty() {
                                    (Chain::Robinhood, evm_addr)
                                } else {
                                    continue;
                                };

                                let tier = if rank <= 3 {
                                    WalletTier::TierS
                                } else if rank <= 15 {
                                    WalletTier::TierA
                                } else {
                                    WalletTier::TierB
                                };

                                let win_rate_pct = if pnl > 0.0 {
                                    (92.0 - (rank as f64 * 0.25)).clamp(68.0, 96.5)
                                } else {
                                    50.0
                                };

                                wallets.push(SmartWallet {
                                    id: None,
                                    address,
                                    name,
                                    handle,
                                    chain,
                                    tier,
                                    rank,
                                    pnl_24h_usd: pnl,
                                    win_rate_pct,
                                    total_trades: num_trades,
                                    source: "FOMO".to_string(),
                                    reputation_score: 85.0,
                                    is_active: true,
                                    last_seen_at: Some(Utc::now()),
                                    updated_at: Utc::now(),
                                });
                            }

                            if !wallets.is_empty() {
                                return Ok(wallets);
                            }
                        }
                    }
                }
            }
        }

        // 2. Fallback to Local Verified Seeds
        Ok(Self::get_default_seed_traders())
    }

    pub async fn harvest_7d_whale_holdings_and_radar(&self) -> Result<Vec<TrackedToken>> {
        // Ingest top 50 wallets to DB and collect holdings
        #[derive(Default)]
        struct TokenHoldingAgg {
            total_usd: f64,
            whale_handles: Vec<String>,
            sample_network: u64,
        }

        let mut token_map: HashMap<String, TokenHoldingAgg> = HashMap::new();

        let mut items_opt = None;
        if let Some(token) = &self.config.fomo_auth_token {
            if !token.trim().is_empty() {
                let auth_header = if token.starts_with("Bearer ") {
                    token.to_string()
                } else {
                    format!("Bearer {}", token)
                };
                let raw_token = token.trim_start_matches("Bearer ").trim();

                let req = self
                    .client
                    .get("https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d")
                    .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                    .header("Origin", "https://fomo.family")
                    .header("Referer", "https://fomo.family/")
                    .header("Accept", "application/json, text/plain, */*")
                    .header("privy-app-id", "cm6h485o300n3zj9yl6vpedq7")
                    .header("privy-token", raw_token)
                    .header("Authorization", auth_header);

                if let Ok(resp) = req.send().await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<Value>().await {
                            if let Some(arr) = json
                                .get("responseObject")
                                .and_then(|r| r.get("leaderboard"))
                                .and_then(|l| l.as_array())
                            {
                                items_opt = Some(arr.clone());
                            }
                        }
                    }
                }
            }
        }

        if let Some(items) = items_opt {
            for (idx, item) in items.iter().take(50).enumerate() {
                let handle = item
                    .get("userHandle")
                    .and_then(|v| v.as_str())
                    .unwrap_or("trader")
                    .to_string();
                let name = item
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Trader")
                    .to_string();
                let sol_addr = item
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let evm_addr = item
                    .get("evmAddress")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let pnl = item.get("totalPnL").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let rank = (idx + 1) as u32;

                let (chain, address) = if !sol_addr.is_empty() {
                    (Chain::Solana, sol_addr)
                } else if !evm_addr.is_empty() {
                    (Chain::Robinhood, evm_addr)
                } else {
                    continue;
                };

                let tier = if rank <= 3 {
                    WalletTier::TierS
                } else if rank <= 15 {
                    WalletTier::TierA
                } else {
                    WalletTier::TierB
                };

                let win_rate_pct = (92.0 - (rank as f64 * 0.25)).clamp(68.0, 96.5);

                let smart_wallet = SmartWallet {
                    id: None,
                    address,
                    name,
                    handle: handle.clone(),
                    chain,
                    tier,
                    rank,
                    pnl_24h_usd: pnl,
                    win_rate_pct,
                    total_trades: 50,
                    source: "FOMO".to_string(),
                    reputation_score: 90.0,
                    is_active: true,
                    last_seen_at: Some(Utc::now()),
                    updated_at: Utc::now(),
                };
                let _ = self.db.upsert_wallet(&smart_wallet);

                if let Some(holdings) = item.get("topHoldings").and_then(|h| h.as_array()) {
                    for h in holdings {
                        if let Some(token_addr) = h.get("tokenAddress").and_then(|t| t.as_str()) {
                            let val = h.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                            let net_id = h.get("networkId").and_then(|n| n.as_u64()).unwrap_or(0);
                            if val > 10000.0 {
                                let entry = token_map.entry(token_addr.to_string()).or_default();
                                entry.total_usd += val;
                                if !entry.whale_handles.contains(&handle) {
                                    entry.whale_handles.push(handle.clone());
                                }
                                entry.sample_network = net_id;
                            }
                        }
                    }
                }
            }
        }

        // 2. Ingest Dynamic Live Tokens from Recent Smart Wallet Activities & Positions
        if let Ok(act_tokens) = self.db.get_recent_whale_activity_tokens(50) {
            for (mint, _sym, _chain_str, _count, val, handles) in act_tokens {
                if is_blacklisted_asset(&mint, &_sym) {
                    continue;
                }
                let entry = token_map.entry(mint).or_default();
                entry.total_usd += val;
                for h in handles {
                    if !entry.whale_handles.contains(&h) {
                        entry.whale_handles.push(h);
                    }
                }
            }
        }

        // 3. Include Tokens from Open Shadow Positions
        if let Ok(positions) = self.db.get_active_positions() {
            for p in positions {
                if !is_blacklisted_asset(&p.token_mint, &p.token_symbol) {
                    let entry = token_map.entry(p.token_mint).or_default();
                    entry.total_usd += p.amount_usd;
                    let h = p.target_whale_handle;
                    if !entry.whale_handles.contains(&h) {
                        entry.whale_handles.push(h);
                    }
                }
            }
        }

        // 4. Fallback to Verified Seed Gems if token map is sparse (< 5 tokens)
        if token_map.len() < 5 {
            let seed_path = "data/default_whale_gems.json";
            if let Ok(data) = fs::read_to_string(seed_path) {
                if let Ok(seeds) =
                    serde_json::from_str::<Vec<(String, String, String, u32, f64, String)>>(&data)
                {
                    for (_sym, mint, _chain_str, _count, val, handles_str) in seeds {
                        let entry = token_map.entry(mint).or_default();
                        entry.total_usd += val;
                        for s in handles_str.split(',') {
                            let h = s.trim().trim_start_matches('@').to_string();
                            if !entry.whale_handles.contains(&h) {
                                entry.whale_handles.push(h);
                            }
                        }
                    }
                }
            }
        }

        // Sort tokens by total whale holding USD descending
        let mut sorted_tokens: Vec<(String, TokenHoldingAgg)> = token_map.into_iter().collect();
        sorted_tokens.sort_by(|a, b| {
            b.1.total_usd
                .partial_cmp(&a.1.total_usd)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut tracked_tokens = Vec::new();

        // Inspect top 50 high-conviction tokens via DexScreener
        for (token_addr, agg) in sorted_tokens.into_iter().take(50) {
            if is_blacklisted_asset(&token_addr, "") {
                continue;
            }

            let dex_url = format!(
                "https://api.dexscreener.com/latest/dex/tokens/{}",
                token_addr
            );
            let dex_resp = match self
                .client
                .get(&dex_url)
                .header("User-Agent", "Mozilla/5.0")
                .send()
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };

            if let Ok(dex_json) = dex_resp.json::<Value>().await {
                if let Some(pairs) = dex_json.get("pairs").and_then(|p| p.as_array()) {
                    // Multi-pool scanner: filter exact base token matching and find best liquid pool
                    let mut best_pair_opt: Option<&Value> = None;
                    let mut max_liq = 0.0;

                    for pair in pairs {
                        let base_addr = pair
                            .get("baseToken")
                            .and_then(|b| b.get("address"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("");
                        if !base_addr.eq_ignore_ascii_case(&token_addr) {
                            continue;
                        }

                        let mut liq: f64 = pair
                            .get("liquidity")
                            .and_then(|l| l.get("usd"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let dex_id = pair.get("dexId").and_then(|s| s.as_str()).unwrap_or("");
                        if dex_id == "pumpfun" && liq <= 0.0 {
                            let mcap: f64 = pair
                                .get("marketCap")
                                .or_else(|| pair.get("fdv"))
                                .and_then(|v| v.as_f64())
                                .unwrap_or(0.0);
                            if mcap > 0.0 {
                                liq = mcap * 0.50;
                            }
                        }

                        if best_pair_opt.is_none() || liq > max_liq {
                            max_liq = liq;
                            best_pair_opt = Some(pair);
                        }
                    }

                    if let Some(pair) = best_pair_opt {
                        let symbol = pair
                            .get("baseToken")
                            .and_then(|b| b.get("symbol"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("UNKNOWN")
                            .to_string();

                        if is_blacklisted_asset(&token_addr, &symbol) {
                            continue;
                        }

                        let chain_str = pair
                            .get("chainId")
                            .and_then(|c| c.as_str())
                            .unwrap_or("solana");
                        let chain = match chain_str.to_lowercase().as_str() {
                            "solana" => Chain::Solana,
                            "arbitrum" => Chain::Robinhood,
                            "base" => Chain::Base,
                            _ => {
                                if token_addr.starts_with("0x") {
                                    Chain::Robinhood
                                } else {
                                    Chain::Solana
                                }
                            }
                        };

                        let price_usd = pair
                            .get("priceUsd")
                            .and_then(|p| p.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let mut liquidity_usd = pair
                            .get("liquidity")
                            .and_then(|l| l.get("usd"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let market_cap_usd = pair
                            .get("marketCap")
                            .and_then(|m| m.as_f64())
                            .or_else(|| pair.get("fdv").and_then(|f| f.as_f64()))
                            .unwrap_or(0.0);

                        let dex_id = pair.get("dexId").and_then(|s| s.as_str()).unwrap_or("");
                        if dex_id == "pumpfun" && liquidity_usd <= 0.0 && market_cap_usd > 0.0 {
                            liquidity_usd = market_cap_usd * 0.50;
                        }

                        let quote_symbol = pair
                            .get("quoteToken")
                            .and_then(|q| q.get("symbol"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_uppercase();
                        let quote_amount = pair
                            .get("liquidity")
                            .and_then(|l| l.get("quote"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);

                        let real_quote_reserve_usd = match quote_symbol.as_str() {
                            "USDC" | "USDT" | "USD" | "USDG" | "DAI" | "USDS" | "PYUSD" => {
                                quote_amount
                            }
                            "SOL" | "WSOL" => quote_amount * 102.0,
                            "ETH" | "WETH" => quote_amount * 2500.0,
                            _ => quote_amount.min(liquidity_usd * 0.5),
                        };

                        // 1. Strict Blacklist check with live symbol
                        if is_blacklisted_asset(&token_addr, &symbol) {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        // 2. Strict Price & Minimum Liquidity Filter
                        let min_liq_threshold = self.config.min_liquidity_usd.min(10000.0);
                        if price_usd <= 0.0 || (dex_id != "pumpfun" && liquidity_usd < min_liq_threshold) {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        // 3. Strict Real Quote Collateral Floor (Anti-CLMM spoof for non-pump pools)
                        if dex_id != "pumpfun" && liquidity_usd >= 100_000.0 && real_quote_reserve_usd < 5_000.0 {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        // 4. Anti-CLMM Asymmetric Spoof Guard (real quote must be at least 15% of reported liquidity)
                        if liquidity_usd >= 100_000.0
                            && (real_quote_reserve_usd / liquidity_usd) < 0.15
                        {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        // 5. Strict Nano/Micro-Cap Radar Filter ($50k - $20M Market Cap)
                        if market_cap_usd > 20_000_000.0 || market_cap_usd < 50_000.0 {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        // 6. Sanity check: Reject distorted pools where liquidity > 2x MCap (synthetic stocks like CRCLx)
                        if liquidity_usd > market_cap_usd * 2.0 {
                            let _ = self.db.delete_tracked_token(&token_addr);
                            continue;
                        }

                        let change_24h_pct = pair
                            .get("priceChange")
                            .and_then(|pc| pc.get("h24"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let change_6h_pct = pair
                            .get("priceChange")
                            .and_then(|pc| pc.get("h6"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);
                        let change_1h_pct = pair
                            .get("priceChange")
                            .and_then(|pc| pc.get("h1"))
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);

                        // Classify Upside Potential (Nano/Micro/Low/Mid Cap)
                        let upside_tier = if market_cap_usd > 0.0 && market_cap_usd < 1_000_000.0 {
                            "NANO_CAP 🚀🚀 (<$1M)".to_string()
                        } else if market_cap_usd >= 1_000_000.0 && market_cap_usd < 3_000_000.0 {
                            "MICRO_CAP 🚀 ($1M-$3M)".to_string()
                        } else if market_cap_usd >= 3_000_000.0 && market_cap_usd < 15_000_000.0 {
                            "LOW_CAP 💎 ($3M-$15M)".to_string()
                        } else if market_cap_usd >= 15_000_000.0 && market_cap_usd < 50_000_000.0 {
                            "SMALL_CAP 💎 ($15M-$50M)".to_string()
                        } else {
                            "MID_LARGE 🛡️ (>$50M)".to_string()
                        };

                        // Classify Bottom vs Pucuk Status
                        let status = if change_24h_pct > 100.0 || change_6h_pct > 35.0 {
                            "LOCAL_TOP 🔴 (Wait Pullback)".to_string()
                        } else if change_24h_pct >= -30.0
                            && change_24h_pct <= 25.0
                            && liquidity_usd >= 15_000.0
                        {
                            "ACCUMULATION_BOTTOM 🟢 (Ready Buy)".to_string()
                        } else if change_24h_pct < -30.0 {
                            "OVERSOLD_RECOVERY 🟡 (Watch Floor)".to_string()
                        } else {
                            "NEUTRAL_BASE ⚪".to_string()
                        };

                        let top_handles = agg
                            .whale_handles
                            .iter()
                            .take(3)
                            .map(|h| format!("@{}", h))
                            .collect::<Vec<_>>()
                            .join(", ");

                        let tracked = TrackedToken {
                            token_mint: token_addr.clone(),
                            token_symbol: symbol,
                            chain,
                            market_cap_usd,
                            liquidity_usd,
                            price_usd,
                            change_24h_pct,
                            change_6h_pct,
                            change_1h_pct,
                            whale_count: agg.whale_handles.len() as u32,
                            total_whale_usd: agg.total_usd,
                            top_whale_handles: top_handles,
                            status,
                            upside_tier,
                            last_scanned_at: Utc::now(),
                        };

                        let _ = self.db.upsert_tracked_token(&tracked);
                        tracked_tokens.push(tracked);
                    }
                }
            }
            sleep(std::time::Duration::from_millis(200)).await;
        }

        if !tracked_tokens.is_empty() {
            let active_mints: Vec<String> = tracked_tokens
                .iter()
                .map(|t| t.token_mint.clone())
                .collect();
            let _ = self.db.prune_stale_tracked_tokens(&active_mints, 600);
        }

        Ok(tracked_tokens)
    }

    pub fn fallback_seed_traders() -> Vec<SmartWallet> {
        vec![
            SmartWallet {
                id: None,
                address: "6cerGpSGzCrA1NeHUNtyoNm4rWQx5SnK3DSAXb4e615t".to_string(),
                name: "point farm capital 🌾".to_string(),
                handle: "pointfarmcap".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 1,
                pnl_24h_usd: 7243735.10,
                win_rate_pct: 88.5,
                total_trades: 2527,
                source: "FOMO".to_string(),
                reputation_score: 95.0,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1".to_string(),
                name: "Unipcs 🦄".to_string(),
                handle: "unipcs".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 2,
                pnl_24h_usd: 16201347.83,
                win_rate_pct: 91.2,
                total_trades: 3855,
                source: "FOMO".to_string(),
                reputation_score: 98.0,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "GmfY52CC5BKt43xt9GSmZWNzsAv4ywE6qgguRM5fvAtN".to_string(),
                name: "TheS◎Lstice ☀️".to_string(),
                handle: "The__Solstice".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierA,
                rank: 5,
                pnl_24h_usd: 5093499.09,
                win_rate_pct: 84.0,
                total_trades: 1917,
                source: "FOMO".to_string(),
                reputation_score: 88.0,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
        ]
    }

    pub fn get_default_seed_traders() -> Vec<SmartWallet> {
        let path = "data/verified_smart_wallets.json";
        if let Ok(data) = fs::read_to_string(path) {
            if let Ok(wallets) = serde_json::from_str::<Vec<SmartWallet>>(&data) {
                return wallets;
            }
        }
        Self::fallback_seed_traders()
    }

    pub async fn sync_to_db(&self) -> Result<usize> {
        let top_wallets = self.fetch_top_traders().await?;
        let count = top_wallets.len();
        for wallet in top_wallets {
            let _ = self.db.upsert_wallet(&wallet)?;
        }
        Ok(count)
    }
}
