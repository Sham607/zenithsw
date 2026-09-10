pub mod scoring;
pub mod winner_profiler;

use std::sync::Arc;
use tokio::time::{sleep, Duration};
use crate::db::SmartWalletDb;
use winner_profiler::WinnerProfiler;

pub struct SmartMoneyIndexer {
    profiler: WinnerProfiler,
    interval_secs: u64,
}

impl SmartMoneyIndexer {
    pub fn new(helius_rpc_url: &str, arbitrum_rpc_url: &str, db: Arc<SmartWalletDb>, interval_secs: u64) -> Self {
        Self {
            profiler: WinnerProfiler::new(helius_rpc_url, arbitrum_rpc_url, db),
            interval_secs,
        }
    }

    pub async fn run_background_loop(&self) {
        println!("[INDEXER 🌐] On-Chain DEX Graph Indexer initialized (Interval: {}s)", self.interval_secs);
        loop {
            // Run indexing pass
            match self.profiler.discover_active_smart_wallets().await {
                Ok(new_wallets) => {
                    if !new_wallets.is_empty() {
                        println!("[INDEXER 🎯] Discovered & added {} new active smart wallet(s) directly on-chain!", new_wallets.len());
                    }
                }
                Err(e) => {
                    eprintln!("[INDEXER ⚠️] Indexing pass error: {}", e);
                }
            }
            sleep(Duration::from_secs(self.interval_secs)).await;
        }
    }
}
