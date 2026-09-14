pub mod cluster_detective;
pub mod scoring;
pub mod winner_profiler;

use crate::db::{CandidateWalletDb, SmartWalletDb};
use cluster_detective::WhaleClusterDetective;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use winner_profiler::WinnerProfiler;

pub struct SmartMoneyIndexer {
    profiler: WinnerProfiler,
    detective: WhaleClusterDetective,
    interval_secs: u64,
}

impl SmartMoneyIndexer {
    pub fn new(
        helius_rpc_url: &str,
        robinhood_rpc_url: &str,
        db: Arc<SmartWalletDb>,
        candidate_db: Arc<CandidateWalletDb>,
        interval_secs: u64,
    ) -> Self {
        Self {
            profiler: WinnerProfiler::new(helius_rpc_url, robinhood_rpc_url, db.clone(), candidate_db.clone()),
            detective: WhaleClusterDetective::new(helius_rpc_url, robinhood_rpc_url, db, candidate_db),
            interval_secs,
        }
    }

    pub async fn run_background_loop(&self) {
        println!("[INDEXER 🌐] On-Chain DEX Graph Indexer & Whale Cluster Detective initialized (Interval: {}s)", self.interval_secs);

        // Initial Cluster Forensic Scan
        if let Ok(shadows) = self.detective.trace_top_whale_clusters().await {
            if !shadows.is_empty() {
                println!("[INDEXER 🕵️] Initial Cluster Detective pass uncovered {} subsidiary whale wallets!", shadows.len());
            }
        }

        loop {
            // Run indexing pass
            match self.profiler.discover_active_smart_wallets().await {
                Ok(new_wallets) => {
                    if !new_wallets.is_empty() {
                        println!("[INDEXER 🎯] Discovered {} on-chain wallet candidate(s); candidates remain MONITOR_ONLY pending independent audit.", new_wallets.len());
                    }
                }
                Err(e) => {
                    eprintln!("[INDEXER ⚠️] Indexing pass error: {}", e);
                }
            }

            // Run cluster detective pass every 3 cycles
            if let Ok(shadows) = self.detective.trace_top_whale_clusters().await {
                if !shadows.is_empty() {
                    println!("[INDEXER 🕵️] Discovered {} subsidiary wallet candidate(s) via funding trace; candidates remain MONITOR_ONLY.", shadows.len());
                }
            }

            sleep(Duration::from_secs(self.interval_secs)).await;
        }
    }
}
