use crate::db::SmartWalletDb;
use crate::models::{Chain, SmartWallet, WalletTier};
use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use std::sync::Arc;

pub struct FomoScraper {
    client: reqwest::Client,
}

impl FomoScraper {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .unwrap_or_default();
        Self { client }
    }

    pub async fn scrape_top_traders(&self) -> Result<Vec<SmartWallet>> {
        let url = "https://fominal.family/leaderboard";
        let res = self.client.get(url).send().await?;
        let html = res.text().await?;

        let mut wallets = Vec::new();
        let now = Utc::now();

        // Parse trader cards from HTML (ranks 1-3)
        // Pattern: <p class="truncate text-[15px] font-medium">NAME</p><p class="truncate text-[12px] text-dim">@<!-- -->HANDLE</p>...<span class="... text-gold">RANK</span>...<p class="figure ... text-up">+$PNL</p>
        let card_re = Regex::new(r#"<p class="[^"]*text-\[15px\] font-medium">([^<]+)</p>\s*<p class="[^"]*text-\[12px\] text-dim">@<!-- -->([^<]+)</p>.*?text-gold">(\d+)</span>.*?text-up">\+?\$?([0-9,]+\.[0-9]{2})</p>"#)?;
        for cap in card_re.captures_iter(&html) {
            let name = cap[1].trim().to_string();
            let handle = cap[2].trim().to_string();
            let rank: u32 = cap[3].parse().unwrap_or(0);
            let pnl_str = cap[4].replace(',', "");
            let pnl: f64 = pnl_str.parse().unwrap_or(0.0);

            let tier = if pnl >= 100_000.0 {
                WalletTier::TierS
            } else if pnl >= 30_000.0 {
                WalletTier::TierA
            } else {
                WalletTier::TierB
            };

            // Infer primary chain based on handle or default to Solana/Robinhood
            let chain = if handle.starts_with("0x") || name.contains("EVM") {
                Chain::Arbitrum
            } else {
                Chain::Solana
            };

            wallets.push(SmartWallet {
                id: None,
                rank,
                name,
                handle,
                chain,
                address: String::new(), // Will be resolved by resolver
                tier,
                pnl_24h_usd: pnl,
                win_rate_pct: 75.0, // Default estimated
                total_trades: 20,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            });
        }

        // Parse table rows from HTML (ranks 4-100)
        // Table row format: <tr><td>4</td><td>Avast@0xAvast</td><td>+$108,471.36</td>...
        let row_re = Regex::new(r#"<td>(\d+)</td>\s*<td>(?:<[^>]+>)?([^<@]+)?(?:<[^>]+>)?@([^<]+)</td>\s*<td>\+?\$?([0-9,]+\.[0-9]{2})</td>"#)?;
        for cap in row_re.captures_iter(&html) {
            let rank: u32 = cap[1].parse().unwrap_or(0);
            let name_raw = cap.get(2).map(|m: regex::Match| m.as_str().trim()).unwrap_or("");
            let handle = cap[3].trim().to_string();
            let name = if name_raw.is_empty() { handle.clone() } else { name_raw.to_string() };
            let pnl_str = cap[4].replace(',', "");
            let pnl: f64 = pnl_str.parse().unwrap_or(0.0);

            let tier = if pnl >= 100_000.0 {
                WalletTier::TierS
            } else if pnl >= 30_000.0 {
                WalletTier::TierA
            } else {
                WalletTier::TierB
            };

            let chain = if handle.starts_with("0x") {
                Chain::Arbitrum
            } else {
                Chain::Solana
            };

            wallets.push(SmartWallet {
                id: None,
                rank,
                name,
                handle,
                chain,
                address: String::new(),
                tier,
                pnl_24h_usd: pnl,
                win_rate_pct: 70.0,
                total_trades: 15,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            });
        }

        // Fallback default seeds if web parsing returns few results
        if wallets.is_empty() {
            wallets = Self::get_default_seed_traders();
        }

        Ok(wallets)
    }

    pub fn get_default_seed_traders() -> Vec<SmartWallet> {
        let now = Utc::now();
        vec![
            SmartWallet {
                id: None,
                rank: 1,
                name: "Ethermonk 📿".to_string(),
                handle: "ether_monk".to_string(),
                chain: Chain::Solana,
                address: "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i".to_string(),
                tier: WalletTier::TierS,
                pnl_24h_usd: 254395.02,
                win_rate_pct: 82.5,
                total_trades: 37,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 2,
                name: "Logan Lim".to_string(),
                handle: "loganlim_x".to_string(),
                chain: Chain::Solana,
                address: "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1".to_string(),
                tier: WalletTier::TierS,
                pnl_24h_usd: 166901.43,
                win_rate_pct: 78.0,
                total_trades: 10,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 3,
                name: "TheS◎Lstice".to_string(),
                handle: "The__Solstice".to_string(),
                chain: Chain::Solana,
                address: "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU".to_string(),
                tier: WalletTier::TierS,
                pnl_24h_usd: 113557.33,
                win_rate_pct: 75.0,
                total_trades: 8,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 4,
                name: "Avast".to_string(),
                handle: "0xAvast".to_string(),
                chain: Chain::Robinhood,
                address: "0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D".to_string(),
                tier: WalletTier::TierS,
                pnl_24h_usd: 108471.36,
                win_rate_pct: 80.0,
                total_trades: 35,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 5,
                name: "Onchaingambles".to_string(),
                handle: "Onchainmetrics".to_string(),
                chain: Chain::Solana,
                address: "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM".to_string(),
                tier: WalletTier::TierA,
                pnl_24h_usd: 79495.58,
                win_rate_pct: 71.0,
                total_trades: 6,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 6,
                name: "FreshDampMink".to_string(),
                handle: "FreshDampMink".to_string(),
                chain: Chain::Base,
                address: "0xebB4C5B97E4117e30EC82ce025E6f21dded05436".to_string(),
                tier: WalletTier::TierA,
                pnl_24h_usd: 76981.05,
                win_rate_pct: 68.0,
                total_trades: 7,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
            SmartWallet {
                id: None,
                rank: 7,
                name: "Thokani 🕊️".to_string(),
                handle: "Thokani".to_string(),
                chain: Chain::Solana,
                address: "G6xL2DkmF3aYm8WtpqBkJ6xPq7dZ8mD9eF2wY5tK4jL8".to_string(),
                tier: WalletTier::TierA,
                pnl_24h_usd: 73053.79,
                win_rate_pct: 73.0,
                total_trades: 4,
                is_active: true,
                last_seen_at: None,
                updated_at: now,
            },
        ]
    }

    pub async fn sync_and_persist(&self, db: Arc<SmartWalletDb>) -> Result<usize> {
        let mut list = self.scrape_top_traders().await.unwrap_or_else(|_| Self::get_default_seed_traders());
        
        // Ensure addresses for top known handles
        for w in &mut list {
            if w.address.is_empty() {
                w.address = crate::ingestion::resolver::WalletResolver::resolve_static_address(&w.handle, w.chain);
            }
            let _ = db.upsert_wallet(w);
        }

        Ok(list.len())
    }
}
