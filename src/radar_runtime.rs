use crate::config::Config;
use crate::db::{deduplicate_scan_addresses, SmartWalletDb, WalletScanAddress};
use crate::ipc::{IpcMessage, IpcServer};
use crate::models::Chain;
use crate::radar::{parse_market_snapshot, RadarPolicy};
use crate::radar_pipeline::run_read_only_pipeline;
use crate::radar_rpc::{build_read_only_rpc_client, collect_solana_holdings_read_only};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

const RADAR_INTERVAL_SECS: u64 = 600;
const MAX_MARKET_TOKENS_PER_CYCLE: usize = 50;
const DEXSCREENER_URL: &str = "https://api.dexscreener.com/latest/dex/tokens/";

pub struct RadarRuntime {
    config: Config,
    db: Arc<SmartWalletDb>,
    ipc: Arc<IpcServer>,
    market_client: Client,
}

impl RadarRuntime {
    pub fn new(config: Config, db: Arc<SmartWalletDb>, ipc: Arc<IpcServer>) -> Result<Self> {
        Ok(Self {
            config,
            db,
            ipc,
            market_client: Client::builder().timeout(Duration::from_secs(10)).build()?,
        })
    }

    /// Menjalankan satu siklus observasi Radar tanpa signing, broadcast, atau executor.
    pub async fn run_once(&self) -> Result<RadarRunReport> {
        let addresses = deduplicate_scan_addresses(self.db.get_qualified_scan_addresses()?);
        let solana_addresses: Vec<WalletScanAddress> = addresses
            .into_iter()
            .filter(|address| address.chain == Chain::Solana)
            .collect();
        if solana_addresses.is_empty() {
            self.ipc
                .broadcast(&IpcMessage::RadarSync {
                    projections: vec![],
                })
                .context("RadarSync empty gagal")?;
            return Ok(RadarRunReport::empty());
        }

        let rpc = build_read_only_rpc_client(&self.config.solana_rpc_url)?;
        let evidence = collect_solana_holdings_read_only(&rpc, &solana_addresses)
            .await
            .context("collector holdings Solana gagal")?;
        for item in &evidence {
            self.db.insert_radar_evidence(item)?;
        }

        let mut tokens: Vec<String> = evidence
            .iter()
            .map(|item| item.token_address.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        tokens.sort();
        tokens.truncate(MAX_MARKET_TOKENS_PER_CYCLE);
        let mut markets = HashMap::new();
        for token in tokens {
            let response = self
                .market_client
                .get(format!("{DEXSCREENER_URL}{token}"))
                .send()
                .await
                .with_context(|| format!("market request gagal token={token}"))?;
            if !response.status().is_success() {
                continue;
            }
            let body: Value = response.json().await?;
            let observed_at = Utc::now();
            match parse_market_snapshot(Chain::Solana, &token, &body, observed_at) {
                Ok(Some(snapshot)) => {
                    self.db
                        .insert_radar_market_snapshot(&snapshot, &token, Chain::Solana)?;
                    markets.insert((Chain::Solana, token), snapshot);
                }
                Ok(None) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} tidak memiliki pair Solana valid");
                }
                Err(error) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} response invalid: {error}");
                }
            }
        }

        let output = run_read_only_pipeline(
            &evidence,
            &[],
            &[],
            &markets,
            RadarPolicy {
                min_market_cap_usd: self.config.min_market_cap_usd,
                max_market_cap_usd: self.config.max_market_cap_usd,
                min_liquidity_usd: self.config.min_liquidity_usd,
            },
        )?;
        let observed_at = Utc::now();
        for projection in &output.projections {
            self.db.insert_radar_projection(projection, observed_at)?;
        }
        self.ipc.broadcast(&IpcMessage::RadarSync {
            projections: output.projections.clone(),
        })?;
        Ok(RadarRunReport {
            qualified_wallets: solana_addresses.len(),
            evidence: evidence.len(),
            market_snapshots: markets.len(),
            projections: output.projections.len(),
        })
    }

    pub async fn run_loop(self: Arc<Self>) {
        loop {
            match self.run_once().await {
                Ok(report) => println!(
                    "[RADAR ON-CHAIN] wallets={} evidence={} markets={} projections={}",
                    report.qualified_wallets,
                    report.evidence,
                    report.market_snapshots,
                    report.projections
                ),
                Err(error) => eprintln!("[RADAR ON-CHAIN ERROR] {error}"),
            }
            sleep(Duration::from_secs(RADAR_INTERVAL_SECS)).await;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RadarRunReport {
    pub qualified_wallets: usize,
    pub evidence: usize,
    pub market_snapshots: usize,
    pub projections: usize,
}

impl RadarRunReport {
    fn empty() -> Self {
        Self::default()
    }
}
