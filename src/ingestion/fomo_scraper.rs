use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{Chain, SmartWallet, WalletTier};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::fs;
use std::sync::Arc;

pub struct FomoScraper {
    config: Config,
}

impl FomoScraper {
    pub fn new() -> Self {
        let config = Config::load().unwrap_or_else(|_| Config {
            solana_rpc_url: "".to_string(),
            solana_ws_url: "".to_string(),
            solana_private_key: None,
            arbitrum_rpc_url: "".to_string(),
            arbitrum_ws_url: "".to_string(),
            base_rpc_url: "".to_string(),
            base_ws_url: "".to_string(),
            evm_private_key: None,
            fomo_auth_token: None,
            fomo_sync_interval_sec: 5,
            stream_poll_interval_ms: 500,
            max_price_delta_pct: 5.0,
            min_liquidity_usd: 30000.0,
            position_size_usd: 15.0,
            trailing_stop_pct: 10.0,
            stop_loss_pct: 15.0,
            take_profit_pct: 50.0,
            shadow_exit_enabled: true,
            confluence_mode: "TIER_S_OR_DUAL_A".to_string(),
            dry_run: true,
            db_path: "data/smart_wallets.db".to_string(),
            ipc_socket_path: "/tmp/zenithsw.sock".to_string(),
        });
        Self { config }
    }

    pub fn get_default_seed_traders() -> Vec<SmartWallet> {
        if let Ok(content) = fs::read_to_string("data/verified_smart_wallets.json") {
            if let Ok(wallets) = serde_json::from_str::<Vec<SmartWallet>>(&content) {
                return wallets;
            }
        }
        Self::fallback_seed_traders()
    }

    pub async fn fetch_leaderboard(&self) -> Result<Vec<SmartWallet>> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()?;

        // 1. Authenticated Direct API Fetch from FOMO Backend
        if let Some(token) = &self.config.fomo_auth_token {
            let api_url = "https://prod-api.fomo.family/v2/leaderboard?timeframe=24h";
            let mut req = client.get(api_url)
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
                            let mut wallets = Vec::new();
                            for (idx, item) in items.iter().enumerate() {
                                let name = item.get("displayName").and_then(|v| v.as_str()).unwrap_or("Trader").to_string();
                                let handle = item.get("userHandle").and_then(|v| v.as_str()).unwrap_or("trader").to_string();
                                let sol_addr = item.get("address").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let evm_addr = item.get("evmAddress").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let pnl = item.get("pnl").and_then(|v| v.as_f64()).unwrap_or(0.0);
                                let rank = (idx + 1) as u32;

                                let (chain, address) = if !sol_addr.is_empty() {
                                    (Chain::Solana, sol_addr)
                                } else if !evm_addr.is_empty() {
                                    (Chain::Robinhood, evm_addr)
                                } else {
                                    continue;
                                };

                                let tier = if pnl >= 100_000.0 || rank <= 3 {
                                    WalletTier::TierS
                                } else if pnl >= 30_000.0 || rank <= 15 {
                                    WalletTier::TierA
                                } else {
                                    WalletTier::TierB
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
                                    win_rate_pct: 75.0,
                                    total_trades: 20,
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
                pnl_24h_usd: 2183981.02,
                win_rate_pct: 88.0,
                total_trades: 2500,
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
                pnl_24h_usd: 1416868.37,
                win_rate_pct: 84.0,
                total_trades: 120,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "GmfY52CC5BKt43xt9GSmZWNzsAv4ywE6qgguRM5fvAtN".to_string(),
                name: "TheS◎Lstice 🪐".to_string(),
                handle: "The__Solstice".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 3,
                pnl_24h_usd: 1069432.77,
                win_rate_pct: 79.5,
                total_trades: 45,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
        ]
    }

    pub async fn sync_and_persist(&self, db: Arc<SmartWalletDb>) -> Result<usize> {
        let wallets = self.fetch_leaderboard().await?;
        let count = wallets.len();
        for w in wallets {
            let _ = db.upsert_wallet(&w);
        }
        Ok(count)
    }
}
