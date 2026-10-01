use crate::config::Config;
use crate::db::{deduplicate_scan_addresses, SmartWalletDb, WalletScanAddress};
use crate::ipc::{IpcMessage, IpcServer};
use crate::models::Chain;
use crate::radar::{parse_market_snapshot, RadarEvidenceKind, RadarPolicy};
use crate::radar_pipeline::run_read_only_pipeline;
use crate::radar_rpc::{build_read_only_rpc_client, collect_solana_holdings_read_only};
use anyhow::{Context, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

const RADAR_INTERVAL_SECS: u64 = 120;
const MAX_MARKET_TOKENS_PER_CYCLE: usize = 50;
const DEXSCREENER_URL: &str = "https://api.dexscreener.com/latest/dex/tokens/";

type MarketCacheEntry = (crate::radar::RadarMarketSnapshot, Instant);
type MarketCacheMap = HashMap<(Chain, String), MarketCacheEntry>;

pub struct RadarRuntime {
    config: Config,
    db: Arc<SmartWalletDb>,
    ipc: Arc<IpcServer>,
    market_client: Client,
    dead_tokens: Arc<RwLock<HashMap<String, Instant>>>,
    holding_cursor: AtomicUsize,
    market_cache: Arc<RwLock<MarketCacheMap>>,
}

impl RadarRuntime {
    pub fn new(config: Config, db: Arc<SmartWalletDb>, ipc: Arc<IpcServer>) -> Result<Self> {
        Ok(Self {
            config,
            db,
            ipc,
            market_client: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                .build()?,
            dead_tokens: Arc::new(RwLock::new(HashMap::new())),
            holding_cursor: AtomicUsize::new(0),
            market_cache: Arc::new(RwLock::new(HashMap::new())),
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
        let alias_map = self.db.get_wallet_alias_map().unwrap_or_default();
        let mut evidence: Vec<crate::radar::OnChainTokenEvidence> = evidence
            .into_iter()
            .map(|mut item| {
                if let Some(alias) = alias_map.get(&item.wallet_address.trim().to_ascii_lowercase())
                {
                    item.cluster_id = Some(alias.clone());
                }
                item
            })
            .collect();

        // Opsi B: Ambil bukti transaksi beli baru (DirectSwap) >= $100 USD dalam 24 jam terakhir
        let buy_cutoff = Utc::now() - chrono::Duration::hours(24);
        if let Ok(recent_buys) = self.db.get_recent_buy_evidence(buy_cutoff, 100.0) {
            for mut buy in recent_buys {
                if let Some(alias) = alias_map.get(&buy.wallet_address.trim().to_ascii_lowercase())
                {
                    buy.cluster_id = Some(alias.clone());
                }
                evidence.push(buy);
            }
        }
        self.db.insert_radar_evidence_batch(&evidence)?;

        // Prioritaskan kandidat pemindaian pasar berdasarkan relevansi sinyal:
        // 1. DirectSwap (pembelian paus aktif 24 jam terakhir)
        // 2. Konfluensi (jumlah dompet paus unik)
        // 3. Kebaruan observasi (observed_at terbaru)
        struct TokenScanCandidate {
            token_address: String,
            has_direct_swap: bool,
            wallet_count: usize,
            latest_observed_at: chrono::DateTime<Utc>,
        }

        let mut token_stats: HashMap<String, (bool, HashSet<String>, chrono::DateTime<Utc>)> =
            HashMap::new();

        for item in &evidence {
            let entry = token_stats
                .entry(item.token_address.clone())
                .or_insert_with(|| (false, HashSet::new(), item.observed_at));
            if item.kind == RadarEvidenceKind::DirectSwap {
                entry.0 = true;
            }
            entry
                .1
                .insert(item.wallet_address.trim().to_ascii_lowercase());
            if item.observed_at > entry.2 {
                entry.2 = item.observed_at;
            }
        }

        let candidates: Vec<TokenScanCandidate> = token_stats
            .into_iter()
            .map(
                |(token_address, (has_direct_swap, wallets, latest_observed_at))| {
                    TokenScanCandidate {
                        token_address,
                        has_direct_swap,
                        wallet_count: wallets.len(),
                        latest_observed_at,
                    }
                },
            )
            .collect();

        let dead_cutoff = Duration::from_secs(3600);
        let dead_snapshot = {
            let dead = self.dead_tokens.read().await;
            dead.clone()
        };

        let mut direct_candidates = Vec::new();
        let mut holding_candidates = Vec::new();

        for candidate in candidates {
            if candidate.has_direct_swap {
                direct_candidates.push(candidate);
            } else {
                let is_dead = dead_snapshot
                    .get(&candidate.token_address)
                    .map(|t| t.elapsed() < dead_cutoff)
                    .unwrap_or(false);
                if !is_dead {
                    holding_candidates.push(candidate);
                }
            }
        }

        direct_candidates.sort_by(|a, b| b.latest_observed_at.cmp(&a.latest_observed_at));
        holding_candidates.sort_by(|a, b| {
            b.wallet_count
                .cmp(&a.wallet_count)
                .then_with(|| b.latest_observed_at.cmp(&a.latest_observed_at))
        });

        let mut tokens: Vec<String> = Vec::new();
        for c in direct_candidates {
            tokens.push(c.token_address);
        }

        let remaining_slots = MAX_MARKET_TOKENS_PER_CYCLE.saturating_sub(tokens.len());
        if remaining_slots > 0 && !holding_candidates.is_empty() {
            let cursor = self
                .holding_cursor
                .fetch_add(remaining_slots, Ordering::Relaxed);
            let start = cursor % holding_candidates.len();
            for i in 0..remaining_slots.min(holding_candidates.len()) {
                let idx = (start + i) % holding_candidates.len();
                tokens.push(holding_candidates[idx].token_address.clone());
            }
        }

        for token in tokens {
            sleep(Duration::from_millis(50)).await;
            let response = match self
                .market_client
                .get(format!("{DEXSCREENER_URL}{token}"))
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(error) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} request error: {error}");
                    continue;
                }
            };
            if !response.status().is_success() {
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    self.dead_tokens
                        .write()
                        .await
                        .insert(token.clone(), Instant::now());
                }
                continue;
            }
            let body: Value = match response.json().await {
                Ok(val) => val,
                Err(error) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} json parse error: {error}");
                    continue;
                }
            };
            let observed_at = Utc::now();
            match parse_market_snapshot(Chain::Solana, &token, &body, observed_at) {
                Ok(Some(snapshot)) => {
                    self.db
                        .insert_radar_market_snapshot(&snapshot, &token, Chain::Solana)?;
                    let mut cache = self.market_cache.write().await;
                    cache.insert((Chain::Solana, token.clone()), (snapshot, Instant::now()));
                }
                Ok(None) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} tidak memiliki pair Solana valid");
                    self.dead_tokens
                        .write()
                        .await
                        .insert(token.clone(), Instant::now());
                }
                Err(error) => {
                    eprintln!("[RADAR MARKET SKIP] token={token} response invalid: {error}");
                    if error.to_string().contains("missing pairs") {
                        self.dead_tokens
                            .write()
                            .await
                            .insert(token.clone(), Instant::now());
                    }
                }
            }
        }

        let mut markets = HashMap::new();
        {
            let mut cache = self.market_cache.write().await;
            cache.retain(|_, (_, instant)| instant.elapsed() < Duration::from_secs(1800));
            for (k, (snap, _)) in cache.iter() {
                markets.insert(k.clone(), snap.clone());
            }
        }

        // Tier-2: GeckoTerminal OHLCV collection for top qualified tokens
        let mut priority_tokens: Vec<(String, String, usize, f64)> = Vec::new();
        for ((chain, token), snapshot) in &markets {
            if *chain != Chain::Solana {
                continue;
            }
            if let Some(pool_addr) = &snapshot.pool_address {
                if snapshot.liquidity_usd >= 10_000.0 {
                    let q_count = evidence
                        .iter()
                        .filter(|e| {
                            e.chain == Chain::Solana
                                && e.token_address.eq_ignore_ascii_case(token)
                                && e.qualified_wallet
                        })
                        .map(|e| e.wallet_address.trim().to_ascii_lowercase())
                        .collect::<HashSet<_>>()
                        .len();
                    if q_count > 0 {
                        priority_tokens.push((
                            token.clone(),
                            pool_addr.clone(),
                            q_count,
                            snapshot.liquidity_usd,
                        ));
                    }
                }
            }
        }
        priority_tokens.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.3.total_cmp(&a.3)));
        priority_tokens.truncate(15);

        for (token, pool_addr, _, _) in priority_tokens {
            let gecko_url = format!(
                "https://api.geckoterminal.com/api/v2/networks/solana/pools/{}/ohlcv/hour?aggregate=1&limit=24",
                pool_addr
            );
            sleep(Duration::from_millis(500)).await;
            let resp = match self.market_client.get(&gecko_url).send().await {
                Ok(r) => r,
                Err(error) => {
                    eprintln!("[RADAR GECKO OHLCV SKIP] token={token} request error: {error}");
                    continue;
                }
            };
            if !resp.status().is_success() {
                continue;
            }
            let body: Value = match resp.json().await {
                Ok(val) => val,
                Err(error) => {
                    eprintln!("[RADAR GECKO OHLCV SKIP] token={token} json error: {error}");
                    continue;
                }
            };
            if let Ok(bars) = crate::radar::parse_gecko_ohlcv(&body) {
                let (structure, action) = crate::radar::compute_market_structure_and_action(&bars);
                if let Some(snap) = markets.get_mut(&(Chain::Solana, token.clone())) {
                    snap.market_structure = structure.clone();
                    snap.price_action = action.clone();
                    let _ = self
                        .db
                        .insert_radar_market_snapshot(snap, &token, Chain::Solana);
                }
                let mut cache = self.market_cache.write().await;
                if let Some(cached) = cache.get_mut(&(Chain::Solana, token.clone())) {
                    cached.0.market_structure = structure;
                    cached.0.price_action = action;
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
                min_holding_value_usd: self.config.radar_min_holding_value_usd,
            },
        )?;
        let observed_at = Utc::now();
        for projection in &output.projections {
            self.db.insert_radar_projection(projection, observed_at)?;
        }
        let prune_cutoff = observed_at - chrono::Duration::minutes(self.config.radar_prune_minutes);
        let _ = self.db.prune_stale_radar_projections(prune_cutoff);
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
