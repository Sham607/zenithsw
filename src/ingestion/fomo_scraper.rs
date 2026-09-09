use crate::db::SmartWalletDb;
use crate::models::{Chain, SmartWallet, WalletTier};
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use std::fs;
use std::sync::Arc;

pub struct FomoScraper;

impl FomoScraper {
    pub fn new() -> Self {
        Self
    }

    pub fn get_default_seed_traders() -> Vec<SmartWallet> {
        if let Ok(content) = fs::read_to_string("data/verified_smart_wallets.json") {
            if let Ok(wallets) = serde_json::from_str::<Vec<SmartWallet>>(&content) {
                return wallets;
            }
        }
        Self::fallback_seed_traders()
    }

    pub async fn fetch_leaderboard() -> Result<Vec<SmartWallet>> {
        let mut verified_wallets = Self::get_default_seed_traders();

        let url = "https://fominal.family/leaderboard";
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()?;

        let resp = match client.get(url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => return Ok(verified_wallets),
        };

        let html = resp.text().await.unwrap_or_default();

        // Parse Leaderboard Stats (Cards & Table) to update PnL and Ranks of verified wallets
        let mut stats_map = std::collections::HashMap::new();

        // 1. Top Cards
        let card_re = Regex::new(
            r#"text-\[15px\] font-medium">([^<]+)</p>\s*<p class="[^"]*text-\[12px\] text-dim">@<!-- -->([^<]+)</p>.*?text-\[26px\][^"]*text-up">\+?\$?([0-9,]+\.[0-9]{2})</p>"#,
        );
        if let Ok(re) = card_re {
            for (idx, cap) in re.captures_iter(&html).enumerate() {
                let handle = cap[2].trim().to_string();
                let pnl: f64 = cap[3].replace(',', "").parse().unwrap_or(0.0);
                stats_map.insert(handle, ((idx + 1) as u32, pnl));
            }
        }

        // 2. Table Rows
        let row_re = Regex::new(
            r#"<td[^>]*>(\d+)</td>.*?font-medium">([^<]+)</span>.*?text-dim">@<!-- -->([^<]+)</span>.*?text-up">\+?\$?([0-9,]+\.[0-9]{2})</td>"#,
        );
        if let Ok(re) = row_re {
            for cap in re.captures_iter(&html) {
                let rank: u32 = cap[1].parse().unwrap_or(99);
                let handle = cap[3].trim().to_string();
                let pnl: f64 = cap[4].replace(',', "").parse().unwrap_or(0.0);
                stats_map.insert(handle, (rank, pnl));
            }
        }

        // Update verified wallets with live PnL / Rank from leaderboard
        for w in &mut verified_wallets {
            if let Some((rank, pnl)) = stats_map.get(&w.handle) {
                w.rank = *rank;
                w.pnl_24h_usd = *pnl;
                w.tier = if *pnl >= 100_000.0 {
                    WalletTier::TierS
                } else if *pnl >= 30_000.0 {
                    WalletTier::TierA
                } else {
                    WalletTier::TierB
                };
                w.updated_at = Utc::now();
            }
        }

        Ok(verified_wallets)
    }

    pub fn fallback_seed_traders() -> Vec<SmartWallet> {
        vec![
            SmartWallet {
                id: None,
                address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
                name: "Ethermonk Whale 📿".to_string(),
                handle: "ether_monk".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 1,
                pnl_24h_usd: 254395.02,
                win_rate_pct: 86.4,
                total_trades: 42,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1".to_string(),
                name: "Logan Lim 👑".to_string(),
                handle: "loganlim_x".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 2,
                pnl_24h_usd: 172605.34,
                win_rate_pct: 82.1,
                total_trades: 38,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU".to_string(),
                name: "TheS◎Lstice 🪐".to_string(),
                handle: "The__Solstice".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierS,
                rank: 3,
                pnl_24h_usd: 139151.04,
                win_rate_pct: 79.5,
                total_trades: 29,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D".to_string(),
                name: "Avast EVM 🛡️".to_string(),
                handle: "0xAvast".to_string(),
                chain: Chain::Robinhood,
                tier: WalletTier::TierS,
                rank: 4,
                pnl_24h_usd: 108471.36,
                win_rate_pct: 77.0,
                total_trades: 35,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
            SmartWallet {
                id: None,
                address: "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM".to_string(),
                name: "Onchaingambles 🎲".to_string(),
                handle: "Onchainmetrics".to_string(),
                chain: Chain::Solana,
                tier: WalletTier::TierA,
                rank: 5,
                pnl_24h_usd: 79495.58,
                win_rate_pct: 71.3,
                total_trades: 22,
                is_active: true,
                last_seen_at: Some(Utc::now()),
                updated_at: Utc::now(),
            },
        ]
    }

    pub async fn sync_and_persist(&self, db: Arc<SmartWalletDb>) -> Result<usize> {
        let wallets = Self::fetch_leaderboard().await?;
        let count = wallets.len();
        for w in wallets {
            let _ = db.upsert_wallet(&w);
        }
        Ok(count)
    }
}
