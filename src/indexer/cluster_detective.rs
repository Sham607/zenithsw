use crate::chains::robinhood::RobinhoodRpc;
use crate::db::{CandidateWalletDb, SmartWalletDb};
use crate::models::{Chain, SmartWallet, WalletTier};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;

pub struct WhaleClusterDetective {
    robinhood_rpc_url: String,
    db: Arc<SmartWalletDb>,
    candidate_db: Arc<CandidateWalletDb>,
    excluded_contracts: HashSet<String>,
}

impl WhaleClusterDetective {
    pub fn new(
        _solana_rpc_url: &str,
        robinhood_rpc_url: &str,
        db: Arc<SmartWalletDb>,
        candidate_db: Arc<CandidateWalletDb>,
    ) -> Self {
        let mut excluded = HashSet::new();
        // Common DEX Routers, bridges, & protocols on Arbitrum
        excluded.insert("0x68b3465833fb72a70ecdf485e0e4c7bd8665fc45".to_lowercase()); // Uniswap SwapRouter02
        excluded.insert("0xe592427a0aece92de3edee1f18e0157c05861564".to_lowercase()); // Uniswap V3 Router
        excluded.insert("0x1111111254eeb25477b68fb85ed929f73a960582".to_lowercase()); // 1inch
        excluded.insert("0xdef1c0ded9bec7f1a1670819833240f027b25eff".to_lowercase()); // 0x
        excluded.insert("0x000000000022d473030f116ddee9f6b43ac78ba3".to_lowercase()); // Permit2
        excluded.insert("0xba12222222228d8ba445958a75a0704d566bf2c8".to_lowercase()); // Balancer
        excluded.insert("0x80c67432656d59144ceff962e8bf1589122c3e65".to_lowercase()); // Dotdot
        excluded.insert("0xa5407eae9ba41422680e2e00537571bcc53efbfd".to_lowercase()); // Curve
        excluded.insert("0xce16f69375520ab01377ce7b88f5ba8c48f8d666".to_lowercase()); // Squid
        excluded.insert("0x4c36388be6f416a29c8d8eee81c771ce6be14b18".to_lowercase()); // GMX
        excluded.insert("0x5954ab967bc958940b7eb73ee84797dc8a2be580".to_lowercase()); // Aave

        Self {
            robinhood_rpc_url: robinhood_rpc_url.to_string(),
            db,
            candidate_db,
            excluded_contracts: excluded,
        }
    }

    /// Forensic dry-run: returns candidates without mutating the wallet registry.
    /// Candidates require independent swap verification before explicit ingestion.
    pub async fn trace_top_whale_clusters(&self) -> Result<Vec<SmartWallet>> {
        let mut discovered_shadows = Vec::new();

        // 1. Fetch top wallets
        let top_wallets = self
            .db
            .get_wallets_by_tier(WalletTier::TierS)
            .unwrap_or_default();
        if top_wallets.is_empty() {
            return Ok(discovered_shadows);
        }

        println!("[WHALE CLUSTER DETECTIVE 🕵️] Tracing on-chain funding clusters for {} Tier-S wallets...", top_wallets.len());

        for parent in top_wallets.iter().take(50) {
            match parent.chain {
                Chain::Robinhood => {
                    let shadows = self
                        .trace_evm_subsidiaries(parent)
                        .await
                        .unwrap_or_default();
                    discovered_shadows.extend(shadows);
                }
                Chain::Solana => {
                    let shadows = self
                        .trace_solana_subsidiaries(parent)
                        .await
                        .unwrap_or_default();
                    discovered_shadows.extend(shadows);
                }
                Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
                    // Non-Robinhood EVM chains are not traced until chain-specific RPC is wired.
                }
            }
        }

        println!("[WHALE CLUSTER DETECTIVE ✅] Forensic trace complete. Discovered & linked {} shadow wallets.", discovered_shadows.len());
        Ok(discovered_shadows)
    }

    async fn trace_evm_subsidiaries(&self, parent: &SmartWallet) -> Result<Vec<SmartWallet>> {
        let rpc = RobinhoodRpc::new(&self.robinhood_rpc_url)?;
        let latest = rpc.block_number().await?;
        let latest_n = u64::from_str_radix(latest.trim_start_matches("0x"), 16)?;
        let from = latest_n.saturating_sub(2_000);
        let logs = rpc
            .get_wallet_transfer_logs(
                &format!("0x{from:x}"),
                &format!("0x{latest_n:x}"),
                &parent.address,
            )
            .await?;

        let mut subsidiaries = Vec::new();
        let mut seen = HashSet::new();
        for log in logs {
            let topics = match log.get("topics").and_then(Value::as_array) {
                Some(topics) if topics.len() >= 3 => topics,
                _ => continue,
            };
            let from_topic = topics[1].as_str().unwrap_or("");
            let to_topic = topics[2].as_str().unwrap_or("");
            if from_topic.len() != 66 || to_topic.len() != 66 {
                continue;
            }
            let sender = format!("0x{}", &from_topic[26..]);
            let recipient = format!("0x{}", &to_topic[26..]);
            if !sender.eq_ignore_ascii_case(&parent.address)
                || recipient.eq_ignore_ascii_case(&parent.address)
                || self.excluded_contracts.contains(&recipient.to_lowercase())
                || !seen.insert(recipient.to_lowercase())
            {
                continue;
            }
            if let Ok(Some(_)) = self.db.get_wallet_by_address(&recipient) {
                continue;
            }
            let tx_hash = log.get("transactionHash").and_then(Value::as_str);
            let block = log.get("blockNumber").and_then(Value::as_str);
            let suffix = recipient[2..6].to_lowercase();
            let now = Utc::now();
            let shadow_wallet = SmartWallet {
                id: None,
                rank: parent.rank + 50,
                name: format!("{} Shadow #{}", parent.name, subsidiaries.len() + 1),
                handle: format!("{}_sub_{}", parent.handle.trim_start_matches('@'), suffix),
                chain: parent.chain,
                address: recipient,
                tier: WalletTier::TierB,
                pnl_24h_usd: 0.0,
                win_rate_pct: 0.0,
                total_trades: 0,
                source: "CLUSTER_FORENSIC_CANDIDATE".to_string(),
                reputation_score: 0.0,
                execution_track: crate::models::ExecutionTrack::FastLane,
                is_active: false,
                last_seen_at: Some(now),
                updated_at: now,
            };
            self.candidate_db.upsert_candidate(
                &shadow_wallet,
                Some(&parent.address),
                None,
                "ERC20_TRANSFER_LINK",
                tx_hash,
                block,
            )?;
            subsidiaries.push(shadow_wallet);
        }
        Ok(subsidiaries)
    }

    async fn trace_solana_subsidiaries(&self, _parent: &SmartWallet) -> Result<Vec<SmartWallet>> {
        // A parent signature alone cannot identify a subsidiary address. Keep this path
        // fail-closed until token-transfer and swap instructions are decoded together.
        Ok(Vec::new())
    }
}
