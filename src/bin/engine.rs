use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use futures_util::stream::{self, StreamExt};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct InstanceLock {
    path: PathBuf,
}

impl InstanceLock {
    fn acquire(socket_path: &str) -> Result<Self> {
        let path = PathBuf::from(format!("{}.lock", socket_path));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                file.sync_all()?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing_pid = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| value.trim().parse::<u32>().ok());
                if existing_pid
                    .filter(|pid| PathBuf::from(format!("/proc/{pid}")).exists())
                    .is_some()
                {
                    bail!("another ZenithSW engine instance is already running");
                }
                fs::remove_file(&path)?;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)?;
                writeln!(file, "{}", std::process::id())?;
                file.sync_all()?;
                Ok(Self { path })
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
use tokio::time::sleep;

use zenithsw::chains::robinhood::{validate_receipt_transaction_hash, RobinhoodRpc};
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::dex_adapters::{
    drain_activity_ipc_outbox, resolve_solana_activity_metadata, DexVenue,
    NormalizedActivityMaterializationContext,
};
use zenithsw::execution::evm_executor::EvmExecutor;
use zenithsw::execution::solana_executor::{CachedRealizableQuote, SolanaExecutor};
use zenithsw::gating::confluence::ConfluenceEngine;
use zenithsw::indexer::SmartMoneyIndexer;
use zenithsw::ipc::{IpcMessage, IpcServer};
use zenithsw::models::{
    ActivityType, Chain, ShadowPosition, SmartWallet, WalletActivity, WalletTier,
};
use zenithsw::radar_rpc::{
    poll_and_persist_solana_activity_read_only_for_venue, ReadOnlyRpcClient,
    SolanaActivityPersistenceContext,
};
use zenithsw::radar_runtime::RadarRuntime;
use zenithsw::streams::evm_stream::EvmStreamListener;
use zenithsw::streams::solana_stream::SolanaStreamListener;

#[tokio::main]
async fn main() -> Result<()> {
    println!("======================================================");
    println!("       ZENITHSW - SMART MONEY SHADOW ENGINE (DAEMON)  ");
    println!("======================================================");

    let config = Config::load()?;
    let polled_wallets_count = Arc::new(AtomicUsize::new(0));
    let global_scanned_wallets_count = Arc::new(AtomicUsize::new(0));
    let robinhood_primary_rpc = config
        .robinhood_rpc_url
        .split(',')
        .map(str::trim)
        .find(|url| !url.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ROBINHOOD_RPC_URL tidak memiliki endpoint valid"))?
        .to_string();
    let mut single_rpc_config = config.clone();
    single_rpc_config.robinhood_rpc_url = robinhood_primary_rpc.clone();
    let _instance_lock = InstanceLock::acquire(&config.ipc_socket_path)?;
    let db = Arc::new(SmartWalletDb::new_with_wallet_path(
        &config.trading_db_path,
        &config.wallet_db_path,
    )?);
    let candidate_db = Arc::new(zenithsw::db::CandidateWalletDb::open(
        &config.candidate_db_path,
    )?);

    // wallet_registry.db is authoritative and read-only for engine startup.
    // Empty or unavailable qualified scope must remain empty, not be seeded.

    // Initialize IPC Server
    let ipc_server = Arc::new(IpcServer::new(&config.ipc_socket_path));
    ipc_server.start().await?;

    // Pulihkan delivery activity tertunda sebelum worker lain dimulai.
    // Batas eksplisit menjaga startup tetap bounded; item sisanya tetap QUEUED.
    const STARTUP_OUTBOX_DRAIN_LIMIT: usize = 128;
    if ipc_server.receiver_count() > 0 {
        let recovered = drain_activity_ipc_outbox(&db, &ipc_server, STARTUP_OUTBOX_DRAIN_LIMIT)
            .map_err(|error| anyhow::anyhow!("startup outbox recovery gagal: {error}"))?;
        println!("[IPC OUTBOX] startup recovered={recovered} limit={STARTUP_OUTBOX_DRAIN_LIMIT}");
    } else {
        println!("[IPC OUTBOX] startup menunggu client untuk recovery");
    }

    let radar_runtime = Arc::new(RadarRuntime::new(
        config.clone(),
        db.clone(),
        ipc_server.clone(),
    )?);
    let radar_worker = radar_runtime.clone();
    tokio::spawn(async move {
        radar_worker.run_loop().await;
    });

    // Canonical Solana DEX collector worker (opt-in via ENABLE_SOLANA_DEX_WORKER).
    // When enabled, it supersedes legacy Solana stream/WS polling to eliminate duplicate activity.
    if config.enable_solana_dex_worker {
        let dex_client = Arc::new(ReadOnlyRpcClient::new(&config.solana_rpc_url)?);
        let dex_db = db.clone();
        let dex_ipc = ipc_server.clone();
        let dex_config = config.clone();
        tokio::spawn(async move {
            println!(
                "[DEX WORKER] Canonical Solana DEX collector online (Venue: SolanaRaydiumAmm)"
            );
            let poll_interval = Duration::from_millis(dex_config.stream_poll_interval_ms.max(2000));
            let mut interval = tokio::time::interval(poll_interval);
            loop {
                interval.tick().await;
                let addresses = match dex_db.get_qualified_scan_addresses() {
                    Ok(addresses) => addresses
                        .into_iter()
                        .filter(|address| address.chain == Chain::Solana)
                        .collect::<Vec<_>>(),
                    Err(error) => {
                        eprintln!("[DEX WORKER SCOPE ERROR] qualified wallet scope gagal: {error}");
                        continue;
                    }
                };
                for address in addresses {
                    let (wallet, metadata) = match resolve_solana_activity_metadata(
                        &dex_db,
                        &dex_config,
                        &address.address,
                        Utc::now(),
                    ) {
                        Ok(resolved) => resolved,
                        Err(error) => {
                            eprintln!(
                                "[DEX WORKER METADATA ERROR] address={} error={error}",
                                address.address
                            );
                            continue;
                        }
                    };
                    let materialization = NormalizedActivityMaterializationContext {
                        wallet: &wallet,
                        token_symbol: &metadata.token_symbol,
                        token_decimals: metadata.token_decimals,
                        quote_decimals: metadata.quote_decimals,
                        quote_price_usd: metadata.quote_price_usd,
                    };
                    let context = SolanaActivityPersistenceContext {
                        client: &dex_client,
                        db: &dex_db,
                        ipc: &dex_ipc,
                        address: &address,
                        token_address: &metadata.token_address,
                        quote_asset: &metadata.quote_asset,
                        cursor: None,
                        history_limit: 10,
                        max_batch: 10,
                        materialization,
                        scope_key: &address.address,
                    };
                    match poll_and_persist_solana_activity_read_only_for_venue(
                        context,
                        DexVenue::SolanaRaydiumAmm,
                    )
                    .await
                    {
                        Ok(report) => {
                            if report.persisted_activities > 0 {
                                println!(
                                    "[DEX WORKER ACTIVITY] address={} persisted={} cursor_advanced={}",
                                    address.address,
                                    report.persisted_activities,
                                    report.cursor_advanced
                                );
                            }
                        }
                        Err(error) => {
                            eprintln!(
                                "[DEX WORKER POLL ERROR] address={} error={error}",
                                address.address
                            );
                        }
                    }
                }
            }
        });
    }

    // Initialize Stream Listeners (Streamer murni pakai pool RPC publik, proteksi kuota Helius)
    let sol_listener = Arc::new(SolanaStreamListener::new(&config.solana_rpc_url, None));
    let sol_listener_hb = Arc::clone(&sol_listener);
    let arb_listener = Arc::new(EvmStreamListener::new(
        &config.robinhood_rpc_url,
        Chain::Robinhood,
    ));

    // Radar event detector Solana: read-only logsSubscribe dengan reconnect.
    // Ditangguhkan bila canonical DEX worker aktif untuk mencegah duplikasi event.
    if config.enable_solana && !config.enable_solana_dex_worker {
        let sol_ws_listener = sol_listener.clone();
        let sol_ws_db = db.clone();
        let sol_ws_url = config.solana_ws_url.clone();
        tokio::spawn(async move {
            let mut ws_failures = 0u32;
            loop {
                let wallets = match sol_ws_db.get_qualified_scan_addresses() {
                    Ok(addresses) => addresses
                        .into_iter()
                        .filter(|address| address.chain == Chain::Solana)
                        .collect(),
                    Err(error) => {
                        eprintln!("[SOLANA WS SCOPE ERROR] qualified wallet scope gagal: {error}");
                        Vec::new()
                    }
                };
                match sol_ws_listener
                    .run_websocket_monitor(&sol_ws_url, wallets, sol_ws_db.clone())
                    .await
                {
                    Ok(()) => ws_failures = 0,
                    Err(error) => {
                        ws_failures = ws_failures.saturating_add(1);
                        let error_text = error.to_string();
                        let provider_rate_limited = error_text.contains("429")
                            || error_text
                                .to_ascii_lowercase()
                                .contains("too many requests");
                        let backoff_secs =
                            2u64.saturating_mul(2u64.saturating_pow(ws_failures.min(5)));
                        eprintln!(
                            "[SOLANA WS ERROR] {error}; reconnect dalam {}s",
                            backoff_secs.min(60)
                        );
                        if provider_rate_limited || ws_failures >= 5 {
                            eprintln!(
                                "[SOLANA WS DISABLED] provider={} consecutive_failures={}; gunakan bounded HTTP polling",
                                if provider_rate_limited { "rate_limited" } else { "unstable" },
                                ws_failures
                            );
                            break;
                        }
                        sleep(Duration::from_secs(backoff_secs.min(60))).await;
                    }
                }
            }
        });
    }

    if config.enable_robinhood {
        arb_listener.validate_robinhood_chain().await?;
        let robinhood_rpc = RobinhoodRpc::new(&robinhood_primary_rpc)?;
        robinhood_rpc.chain_id().await?;
        robinhood_rpc.validate_deployments().await?;
        println!("[ROBINHOOD RPC ✅] Chain ID 4663 and official Uniswap V4 deployments validated");
    }

    // Set configurable demote thresholds
    db.set_demote_thresholds(
        config.demote_consecutive_losses,
        config.demote_catastrophic_loss_pct,
    );

    // Initialize Gating & Executors
    let confluence = Arc::new(ConfluenceEngine::new(config.clone(), db.clone()));
    let sol_exec = Arc::new(SolanaExecutor::new(config.clone(), db.clone()));
    let evm_exec = Arc::new(EvmExecutor::new(config.clone(), db.clone()));

    // Recover only existing transaction hashes/signatures; never rebroadcast after restart.
    let pending_intents = db.get_pending_execution_intents()?;
    for (intent_id, chain, _action, _token_mint, _wallet, state, tx_hash, _signature) in
        pending_intents
    {
        match chain.as_str() {
            // Intent Solana diselesaikan oleh resolver berbasis bukti on-chain di bawah.
            "SOLANA" => {}
            "ROBINHOOD" => {
                if let Some(tx_hash) = tx_hash {
                    let rpc = RobinhoodRpc::new(&config.robinhood_rpc_url)?;
                    let receipt = rpc.transaction_receipt(&tx_hash).await?;
                    let next_state = match receipt {
                        Some(receipt) => {
                            if let Err(error) =
                                validate_receipt_transaction_hash(&tx_hash, &receipt)
                            {
                                eprintln!("[INTENT RECOVERY] receipt mismatch: {error}");
                                "UNKNOWN"
                            } else if receipt.get("status").and_then(serde_json::Value::as_str)
                                == Some("0x1")
                            {
                                "CONFIRMED"
                            } else {
                                "FAILED"
                            }
                        }
                        None => "PENDING_RECONCILIATION",
                    };
                    if let Err(error) =
                        db.update_execution_intent(&intent_id, next_state, Some(&tx_hash), None)
                    {
                        eprintln!("[INTENT RECOVERY ERROR] update ROBINHOOD intent gagal: {error}");
                    }
                    println!(
                        "[INTENT RECOVERY] chain=ROBINHOOD state={} previous_state={}",
                        next_state, state
                    );
                } else {
                    if let Err(error) =
                        db.update_execution_intent(&intent_id, "UNKNOWN", None, None)
                    {
                        eprintln!(
                            "[INTENT RECOVERY ERROR] update ROBINHOOD failed intent gagal: {error}"
                        );
                    }
                    println!(
                        "[INTENT RECOVERY] chain=ROBINHOOD state=FAILED reason=missing_tx_hash"
                    );
                }
            }
            _ => {
                if let Err(error) = db.update_execution_intent(&intent_id, "UNKNOWN", None, None) {
                    eprintln!(
                        "[INTENT RECOVERY ERROR] update unsupported-chain intent gagal: {error}"
                    );
                }
                println!("[INTENT RECOVERY] state=FAILED reason=unsupported_chain");
            }
        }
    }

    // Resolver intent Solana: FAILED hanya bila gagal on-chain / blockhash expired.
    if config.enable_solana {
        match sol_exec.resolve_pending_intents(0).await {
            Ok(count) => println!("[INTENT RECOVERY] chain=SOLANA resolved={count}"),
            Err(error) => eprintln!("[INTENT RECOVERY ERROR] chain=SOLANA error={error}"),
        }
        let resolver_exec = sol_exec.clone();
        tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(30)).await;
                if let Err(error) = resolver_exec.resolve_pending_intents(150).await {
                    eprintln!("[INTENT RESOLVER ERROR] chain=SOLANA error={error}");
                }
            }
        });
    }

    // Startup Physical On-Chain Position Reconciliation & Orphan Recovery (Zero-Guessing)
    let wallet_pubkey_mismatch = Arc::new(AtomicBool::new(false));
    if config.enable_solana {
        let current_wallet_pubkey = sol_exec.get_wallet_pubkey();
        if let Some(ref current_pk) = current_wallet_pubkey {
            if let Ok(active_positions) = db.get_active_positions() {
                for pos in &active_positions {
                    if let Some(ref owner) = pos.owner_pubkey {
                        if !owner.is_empty() && owner != current_pk {
                            eprintln!(
                                "[WALLET MISMATCH GUARD ⚠️] Posisi {} ({}) milik wallet {}, wallet aktif {}. Posisi tidak akan dimonitor oleh instans ini.",
                                pos.id, pos.token_symbol, owner, current_pk
                            );
                            wallet_pubkey_mismatch.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
        }

        match sol_exec.reconcile_startup_positions().await {
            Ok((ghost_closed, orphans_adopted)) => {
                println!(
                    "[STARTUP AUDIT ✅] Solana physical reconciliation complete: ghost_closed={} orphans_adopted={}",
                    ghost_closed, orphans_adopted
                );
            }
            Err(error) => {
                eprintln!(
                    "[STARTUP AUDIT ERROR ⚠️] Solana physical reconciliation failed: {error}"
                );
            }
        }
    }

    // Task 1: Direct On-Chain DEX Graph Indexer Worker (Every 10 minutes)
    let indexer = Arc::new(SmartMoneyIndexer::new(
        &config.solana_rpc_url,
        &robinhood_primary_rpc,
        db.clone(),
        candidate_db.clone(),
        600,
    ));
    let indexer_worker = indexer.clone();
    tokio::spawn(async move {
        // Initial immediate indexer pass + loop
        indexer_worker.run_background_loop().await;
    });

    // Legacy FOMO Radar worker intentionally disabled. Gems Radar output must
    // come only from the read-only on-chain pipeline; until its runtime
    // collector is wired, the authoritative tracked-token snapshot is empty.

    // Task 3: High-Speed Paced Stream Ingestion Worker (Zero 429 Drop, 200ms per wallet)
    let db_stream = db.clone();
    let config_stream = Arc::new(config.clone());
    let sol_listener_stream = sol_listener.clone();
    let arb_listener_stream = arb_listener.clone();
    let confluence_stream = confluence.clone();
    let sol_exec_stream = sol_exec.clone();
    let evm_exec_stream = evm_exec.clone();
    let ipc_stream = ipc_server.clone();
    let polled_counter = polled_wallets_count.clone();
    let global_scanned_counter = global_scanned_wallets_count.clone();

    tokio::spawn(async move {
        println!("[STREAM WORKER] Paced Multi-Chain Streamer online (Solana: {}, Robinhood: {}, Base: {})",
            config_stream.enable_solana, config_stream.enable_robinhood, config_stream.enable_base);
        let mut last_robinhood_scan = Instant::now() - Duration::from_secs(30);
        loop {
            if let Ok(mut wallets) = db_stream.get_active_wallets() {
                // Filter only enabled chains
                wallets.retain(|w| {
                    let chain_enabled = match w.chain {
                        Chain::Solana => {
                            config_stream.enable_solana && !config_stream.enable_solana_dex_worker
                        }
                        Chain::Robinhood => config_stream.enable_robinhood,
                        Chain::Base => config_stream.enable_base,
                        _ => false,
                    };
                    // Kandidat discovery/monitor-only tidak boleh menghabiskan kuota RPC.
                    let qualified = matches!(w.tier, WalletTier::TierS | WalletTier::TierA)
                        && !matches!(
                            w.source.as_str(),
                            "ONCHAIN_INDEXER_CANDIDATE" | "CLUSTER_FORENSIC_CANDIDATE"
                        );
                    chain_enabled && qualified && w.chain != Chain::Robinhood
                });

                // Robinhood uses one bounded global log scan, not one eth_getLogs per wallet.
                let robinhood_global_results = if last_robinhood_scan.elapsed()
                    >= Duration::from_secs(15)
                {
                    last_robinhood_scan = Instant::now();
                    let robinhood_lookup: std::collections::HashMap<String, SmartWallet> =
                        match db_stream.get_active_wallets() {
                            Ok(wallets) => wallets
                                .into_iter()
                                .filter(|w| {
                                    w.chain == Chain::Robinhood
                                        && w.is_active
                                        && matches!(w.tier, WalletTier::TierS | WalletTier::TierA)
                                })
                                .map(|w| (w.address.to_lowercase(), w))
                                .collect(),
                            Err(error) => {
                                eprintln!("[SCANNER DB ERROR] Robinhood wallet scope unavailable: {error}");
                                std::collections::HashMap::new()
                            }
                        };
                    global_scanned_counter.store(robinhood_lookup.len(), Ordering::Relaxed);
                    let robinhood_pairs: Vec<(String, String)> = robinhood_lookup
                        .values()
                        .map(|w| (w.handle.clone(), w.address.clone()))
                        .collect();
                    let scan_result = arb_listener_stream
                        .scan_global_wallet_activity(&robinhood_pairs, db_stream.clone())
                        .await;
                    if let Err(error) = &scan_result {
                        eprintln!("[SCANNER RPC ERROR] Robinhood global scan failed: {error}");
                    }
                    scan_result
                        .unwrap_or_else(|_| Vec::new())
                        .into_iter()
                        .filter_map(|activity| {
                            robinhood_lookup
                                .get(&activity.wallet_address.to_lowercase())
                                .cloned()
                                .map(|wallet| (activity, wallet))
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };

                // Prioritize Qualified Execution Wallets (Tier-S & Tier-A) first
                wallets.sort_by_key(|w| match w.tier {
                    WalletTier::TierS => 0,
                    WalletTier::TierA => 1,
                    WalletTier::TierB => 2,
                });

                // Concurrently poll wallet activities across high-speed RPC (8 concurrent workers)
                polled_counter.store(wallets.len(), Ordering::Relaxed);
                let mut activity_results: Vec<(WalletActivity, SmartWallet)> =
                    stream::iter(wallets.into_iter())
                        .map(|w| {
                            let sol_listener = sol_listener_stream.clone();
                            let db = db_stream.clone();
                            async move {
                                let activity_opt = match w.chain {
                                        Chain::Solana => match sol_listener
                                            .check_recent_txs(&w.handle, &w.address, db.clone())
                                            .await
                                        {
                                            Ok(activity) => activity,
                                            Err(error) => {
                                                eprintln!("[POLL ERROR] Solana wallet={} address={} error={error}", w.handle, w.address);
                                                None
                                            }
                                        },
                                        // Robinhood diproses oleh global scanner di atas.
                                        Chain::Robinhood | Chain::Arbitrum => None,
                                        _ => None,
                                    };
                                activity_opt.map(|act| (act, w))
                            }
                        })
                        .buffer_unordered(8)
                        .filter_map(|res| async move { res })
                        .collect()
                        .await;
                activity_results.extend(robinhood_global_results);

                for (act, w) in activity_results {
                    println!(
                        "[ACTIVITY 🔥] {} by @{} ({}) -> Token: {} (${:.2})",
                        act.activity_type,
                        act.wallet_handle,
                        act.chain,
                        act.token_symbol,
                        act.amount_usd
                    );
                    if let Err(error) = ipc_stream.broadcast(&IpcMessage::ActivityBroadcast {
                        activity: act.clone(),
                    }) {
                        eprintln!("[IPC ERROR] ActivityBroadcast gagal: {error}");
                    }

                    // Handle Pure Shadow Mirror Exit (Proportional / Full per whale contribution)
                    if act.activity_type == ActivityType::Sell {
                        match db_stream.get_active_positions() {
                            Ok(positions) => {
                                for pos in positions {
                                    if pos.token_mint.eq_ignore_ascii_case(&act.token_mint)
                                        && pos.chain == act.chain
                                    {
                                        let whale_addr_lower =
                                            act.wallet_address.trim().to_lowercase();
                                        let handle_lower = act
                                            .wallet_handle
                                            .trim()
                                            .trim_start_matches('@')
                                            .to_lowercase();
                                        let is_participant =
                                            pos.target_whale_address.split(',').map(str::trim).any(
                                                |address| {
                                                    address.to_lowercase() == whale_addr_lower
                                                },
                                            ) || pos
                                                .target_whale_handle
                                                .split(',')
                                                .map(|handle| {
                                                    handle
                                                        .trim()
                                                        .trim_start_matches('@')
                                                        .to_lowercase()
                                                })
                                                .any(|handle| handle == handle_lower);

                                        if config_stream.shadow_exit_enabled && is_participant {
                                            let exit_price = if act.execution_price_usd > 0.0 {
                                                act.execution_price_usd
                                            } else {
                                                pos.current_price
                                            };
                                            let raw_sell_ratio =
                                                act.sell_ratio_pct.clamp(1.0, 100.0);

                                            // Count participating whales to compute weighted exit ratio
                                            let whale_count =
                                                pos.target_whale_address.split(',').count().max(1)
                                                    as f64;
                                            let whale_weight = 1.0 / whale_count;
                                            let effective_sell_ratio =
                                                (raw_sell_ratio * whale_weight).clamp(1.0, 100.0);

                                            if raw_sell_ratio >= config_stream.whale_full_dump_pct
                                                && whale_count <= 1.0
                                            {
                                                println!("[SHADOW FULL EXIT 🔴] Target @{} sold {:.0}% (>= {:.0}%) of {} -> Closing position completely!", act.wallet_handle, raw_sell_ratio, config_stream.whale_full_dump_pct, pos.token_symbol);
                                                match pos.chain {
                                                    Chain::Solana => {
                                                        if let Err(error) = sol_exec_stream
                                                            .execute_shadow_sell(
                                                                &pos.id,
                                                                "WHALE_FULL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await
                                                        {
                                                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=WHALE_FULL_DUMP error={error}", pos.id);
                                                        }
                                                    }
                                                    _ => {
                                                        if let Err(error) = evm_exec_stream
                                                            .execute_shadow_sell(
                                                                &pos.id,
                                                                "WHALE_FULL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await
                                                        {
                                                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=WHALE_FULL_DUMP error={error}", pos.id);
                                                        }
                                                    }
                                                }
                                            } else {
                                                println!("[SHADOW PROPORTIONAL EXIT 🟡] Target @{} sold {:.1}% of {} (Weighted: {:.1}%, {} whales in pos) -> Partial exit!", act.wallet_handle, raw_sell_ratio, pos.token_symbol, effective_sell_ratio, whale_count);
                                                match pos.chain {
                                                    Chain::Solana => {
                                                        if let Err(error) = sol_exec_stream
                                                            .execute_partial_shadow_sell(
                                                                &pos.id,
                                                                effective_sell_ratio,
                                                                "WHALE_PARTIAL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await
                                                        {
                                                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=WHALE_PARTIAL_DUMP error={error}", pos.id);
                                                        }
                                                    }
                                                    _ => {
                                                        if let Err(error) = evm_exec_stream
                                                            .execute_partial_shadow_sell(
                                                                &pos.id,
                                                                effective_sell_ratio,
                                                                "WHALE_PARTIAL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await
                                                        {
                                                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=WHALE_PARTIAL_DUMP error={error}", pos.id);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                eprintln!("[MIRROR DB ERROR] Active positions unavailable for sell activity: {error}");
                            }
                        }
                    } else if act.activity_type == ActivityType::Buy {
                        // Fetch real-time pool price, liquidity & market cap across Solana/EVM
                        let market_data = match act.chain {
                                Chain::Solana => SolanaStreamListener::fetch_token_price(&act.token_mint).await,
                                Chain::Robinhood => Err(anyhow::anyhow!("Robinhood market data requires a verified on-chain quote; Solana price resolver is not permitted")),
                                _ => Err(anyhow::anyhow!("unsupported chain market data source")),
                            };
                        let (current_price, pool_liq, live_mcap) = match market_data {
                            Ok((symbol, price, liquidity, market_cap))
                                if !symbol.is_empty()
                                    && price.is_finite()
                                    && price > 0.0
                                    && liquidity.is_finite()
                                    && liquidity > 0.0
                                    && market_cap.is_finite()
                                    && market_cap > 0.0 =>
                            {
                                (price, liquidity, market_cap)
                            }
                            Ok((symbol, price, liquidity, market_cap)) => {
                                eprintln!(
                                        "[ENTRY DATA REJECTED ⚠️] {} returned invalid market data (symbol={}, price={}, liquidity={}, mcap={})",
                                        act.token_mint, symbol, price, liquidity, market_cap
                                    );
                                continue;
                            }
                            Err(error) => {
                                eprintln!(
                                    "[ENTRY DATA FAILED ⚠️] {} ({}) — rejecting entry",
                                    act.token_mint, error
                                );
                                continue;
                            }
                        };

                        let mut act_to_eval = act.clone();
                        if act_to_eval.execution_price_usd <= 0.0 && current_price > 0.0 {
                            act_to_eval.execution_price_usd = current_price;
                        }

                        // Strict Gating & Confluence Evaluation with Live Market Cap
                        if let Ok(sig) = confluence_stream.evaluate_activity_with_mcap(
                            &act_to_eval,
                            &w,
                            current_price,
                            pool_liq,
                            live_mcap,
                        ) {
                            if let Err(error) = ipc_stream.broadcast(&IpcMessage::SignalBroadcast {
                                signal: sig.clone(),
                            }) {
                                eprintln!("[IPC ERROR] SignalBroadcast gagal: {error}");
                            }

                            if sig.is_passed {
                                if config_stream.trading_paused {
                                    println!(
                                        "[TRADING PAUSED ⏸️] TRADING_PAUSED=true. BUY baru dilewati untuk {} ({}). Posisi aktif tetap diawasi untuk exit.",
                                        sig.token_symbol, sig.token_mint
                                    );
                                    continue;
                                }
                                println!("[EXECUTE 🚀] Passed strict gating! Executing shadow buy for {}", sig.token_symbol);
                                let pos_opt = match sig.chain {
                                    Chain::Solana => {
                                        match sol_exec_stream.execute_shadow_buy(&sig).await {
                                            Ok(position) => {
                                                confluence
                                                    .cooldowns
                                                    .clear_cooldown(&sig.token_mint);
                                                position
                                            }
                                            Err(error) => {
                                                let dur = confluence
                                                    .cooldowns
                                                    .record_failure(&sig.token_mint);
                                                eprintln!("[EXECUTION ERROR] Solana shadow buy token={} error={} -> Quarantined for {:?}", sig.token_mint, error, dur);
                                                None
                                            }
                                        }
                                    }
                                    _ => match evm_exec_stream.execute_shadow_buy(&sig).await {
                                        Ok(position) => {
                                            confluence.cooldowns.clear_cooldown(&sig.token_mint);
                                            position
                                        }
                                        Err(error) => {
                                            let dur = confluence
                                                .cooldowns
                                                .record_failure(&sig.token_mint);
                                            eprintln!("[EXECUTION ERROR] EVM shadow buy token={} error={} -> Quarantined for {:?}", sig.token_mint, error, dur);
                                            None
                                        }
                                    },
                                };

                                if let Some(new_pos) = pos_opt {
                                    println!("[POSITION OPENED ✅] ID: {}, Token: {}, Entry: ${:.6}, Amount: ${:.2}", new_pos.id, new_pos.token_symbol, new_pos.entry_price, new_pos.amount_usd);
                                }
                            } else {
                                println!(
                                    "[GATING REJECT ❌] Token: {} rejected: {:?}",
                                    sig.token_symbol, sig.rejection_reason
                                );
                            }
                        }
                    }
                }
            }

            sleep(Duration::from_millis(config_stream.stream_poll_interval_ms)).await;
        }
    });

    // Task 3: Live Position Lifecycle & Real-Time Price Tracker Worker (Whale Mirror + Dynamic Hard SL)
    let db_pos_tracker = db.clone();
    let ipc_pos_tracker = ipc_server.clone();
    let sol_exec_pos = sol_exec.clone();
    let evm_exec_pos = evm_exec.clone();
    let config_pos = config.clone();

    tokio::spawn(async move {
        if config_pos.shadow_exit_enabled {
            println!(
                "[POSITION TRACKER] Position Tracker online (Mode: WHALE SHADOW, SL: -{:.1}%, TP: +{:.1}%)",
                config_pos.stop_loss_pct, config_pos.take_profit_pct
            );
        } else {
            println!(
                "[POSITION TRACKER] Position Tracker online (Mode: BOT AUTONOMOUS 100%, SL: -{:.1}%, TP: +{:.1}%)",
                config_pos.bot_stop_loss_pct, config_pos.bot_take_profit_pct
            );
        }
        loop {
            if let Ok(positions) = db_pos_tracker.get_active_positions() {
                let eval_futures = positions.into_iter().map(|pos| {
                    let db = db_pos_tracker.clone();
                    let sol_exec = sol_exec_pos.clone();
                    let evm_exec = evm_exec_pos.clone();
                    let config = Arc::new(config_pos.clone());
                    evaluate_single_position(pos, db, sol_exec, evm_exec, config)
                });
                let refreshed_positions = futures_util::future::join_all(eval_futures).await;

                // Broadcast refreshed positions to IPC socket
                if let Err(error) = ipc_pos_tracker.broadcast(&IpcMessage::PositionUpdate {
                    positions: refreshed_positions,
                }) {
                    eprintln!("[IPC ERROR] PositionUpdate gagal: {error}");
                }
            }

            sleep(Duration::from_secs(1)).await;
        }
    });

    // Task 4: Heartbeat & IPC Metrics Broadcast
    let db_heartbeat = db.clone();
    let ipc_heartbeat = ipc_server.clone();
    let sol_exec_hb = sol_exec.clone();
    let arb_listener_hb = arb_listener.clone();
    let config_hb = config.clone();
    let polled_counter_hb = polled_wallets_count.clone();
    let global_scanned_counter_hb = global_scanned_wallets_count.clone();
    let candidate_counter = Arc::new(AtomicUsize::new(0));
    let candidate_counter_hb = candidate_counter.clone();
    let candidate_db_hb = candidate_db.clone();
    let robinhood_rpc_hb = if config.enable_robinhood {
        Some(RobinhoodRpc::new(&robinhood_primary_rpc)?)
    } else {
        None
    };

    // Shared metrics state for Heartbeat to prevent RPC network latency spikes
    // from blocking or delaying the 2-second IPC Heartbeat pulse.
    #[derive(Default, Clone)]
    struct HeartbeatMetrics {
        wallet_pubkey: String,
        wallet_sol_balance: f64,
        sol_price_usd: f64,
        solana_latency_ms: u64,
        robinhood_latency_ms: u64,
        total_tokens_usd: f64,
    }

    let shared_metrics = Arc::new(tokio::sync::RwLock::new(HeartbeatMetrics::default()));
    let shared_metrics_worker = shared_metrics.clone();
    let sol_exec_metrics = sol_exec_hb.clone();
    let robinhood_rpc_metrics = robinhood_rpc_hb.clone();

    // Background worker for async on-chain balance & latency polling
    tokio::spawn(async move {
        let mut last_known_sol_balance = 0.0;
        loop {
            let solana_latency_start = Instant::now();
            let (wallet_pubkey, wallet_sol_balance) =
                if let Ok((_, pubkey)) = sol_exec_metrics.get_keypair() {
                    let bal = match sol_exec_metrics.get_native_balance(&pubkey).await {
                        Ok(b) => {
                            last_known_sol_balance = b;
                            b
                        }
                        Err(_) => last_known_sol_balance,
                    };
                    (pubkey, bal)
                } else {
                    (String::new(), 0.0)
                };
            let solana_latency_ms =
                u64::try_from(solana_latency_start.elapsed().as_millis()).unwrap_or(u64::MAX);

            let sol_price_usd = sol_exec_metrics.get_sol_price().await.unwrap_or(0.0);

            let onchain_tokens = if wallet_pubkey.is_empty() || config_hb.dry_run {
                Vec::new()
            } else {
                sol_exec_metrics
                    .get_all_wallet_token_balances(&wallet_pubkey)
                    .await
            };

            let mut total_tokens_usd = 0.0;
            if !config_hb.dry_run {
                for (mint, amt) in onchain_tokens {
                    match SolanaStreamListener::fetch_token_price(&mint).await {
                        Ok((_, price, _, _)) if price > 0.0 => {
                            total_tokens_usd += amt * price;
                        }
                        Ok((_, price, _, _)) => {
                            eprintln!("[HEARTBEAT PRICE SKIP] mint={mint} price={price} — tidak dihitung ke equity");
                        }
                        Err(error) => {
                            eprintln!("[HEARTBEAT PRICE FAILED] mint={mint} error={error}");
                        }
                    }
                }
            }

            let robinhood_latency_start = Instant::now();
            let robinhood_latency_result = match robinhood_rpc_metrics.as_ref() {
                Some(rpc) => Some(rpc.chain_id().await),
                None => None,
            };
            let robinhood_latency_ms = match robinhood_latency_result {
                Some(Ok(_)) => {
                    u64::try_from(robinhood_latency_start.elapsed().as_millis()).unwrap_or(u64::MAX)
                }
                _ => 0,
            };

            {
                let mut lock = shared_metrics_worker.write().await;
                *lock = HeartbeatMetrics {
                    wallet_pubkey,
                    wallet_sol_balance,
                    sol_price_usd,
                    solana_latency_ms,
                    robinhood_latency_ms,
                    total_tokens_usd,
                };
            }

            sleep(Duration::from_secs(15)).await;
        }
    });

    let start_time = Utc::now();
    tokio::spawn(async move {
        loop {
            if let Ok(count) = candidate_db_hb.count() {
                if let Ok(count) = usize::try_from(count) {
                    candidate_counter.store(count, Ordering::Relaxed);
                }
            }
            sleep(Duration::from_secs(5)).await;
        }
    });
    let wallet_pubkey_mismatch_hb = Arc::clone(&wallet_pubkey_mismatch);
    tokio::spawn(async move {
        let mut sync_tick = 0u64;
        loop {
            sync_tick += 1;
            let uptime_secs = (Utc::now() - start_time).num_seconds().max(0) as u64;
            let total_tracked_count = match db_heartbeat.get_active_wallets() {
                Ok(wallets) => wallets.len(),
                Err(error) => {
                    eprintln!("[HEARTBEAT DB ERROR] Active wallet count unavailable: {error}");
                    0
                }
            };
            let qualified_wallets = match db_heartbeat.get_qualified_wallets() {
                Ok(wallets) => wallets,
                Err(error) => {
                    eprintln!("[HEARTBEAT DB ERROR] Qualified wallet scope unavailable: {error}");
                    Vec::new()
                }
            };
            let active_positions = match db_heartbeat.get_active_positions() {
                Ok(positions) => positions,
                Err(error) => {
                    eprintln!("[HEARTBEAT DB ERROR] Active positions unavailable: {error}");
                    Vec::new()
                }
            };
            let active_positions_count = active_positions.len();

            // Read asynchronously cached metrics without blocking the 2s heartbeat cycle
            let metrics = {
                let lock = shared_metrics.read().await;
                lock.clone()
            };
            let wallet_pubkey = metrics.wallet_pubkey;
            let wallet_sol_balance = metrics.wallet_sol_balance;
            let sol_price_usd = metrics.sol_price_usd;
            let sol_usd_val = wallet_sol_balance * sol_price_usd;
            let solana_latency_ms = metrics.solana_latency_ms;
            let robinhood_latency_ms = metrics.robinhood_latency_ms;
            let total_tokens_usd = metrics.total_tokens_usd;

            let open_positions_usd: f64 = if config_hb.dry_run {
                active_positions
                    .iter()
                    .map(|p| {
                        (p.amount_usd * (p.remaining_portion_pct / 100.0))
                            * (1.0 + p.unrealized_pnl_pct / 100.0)
                    })
                    .sum()
            } else if total_tokens_usd > 0.0 {
                total_tokens_usd
            } else {
                active_positions
                    .iter()
                    .map(|p| {
                        (p.amount_usd * (p.remaining_portion_pct / 100.0))
                            * (1.0 + p.unrealized_pnl_pct / 100.0)
                    })
                    .sum()
            };

            let wallet_equity_usd = sol_usd_val + open_positions_usd;

            let total_realized_pnl_usd: Option<f64> = match db_heartbeat.get_recent_trades(500) {
                Ok(trades) => Some(trades.iter().map(|t| t.pnl_usd).sum()),
                Err(error) => {
                    eprintln!("[HEARTBEAT DB ERROR] Realized PnL unavailable: {error}");
                    None
                }
            };

            let (rpc_requests, rpc_successes, rpc_rate_limited, rpc_failures) =
                arb_listener_hb.health_snapshot();
            if sync_tick.is_multiple_of(10) {
                println!(
                    "[RPC HEALTH] chain=ROBINHOOD requests={} successes={} rate_limited={} failures={}",
                    rpc_requests, rpc_successes, rpc_rate_limited, rpc_failures
                );
            }

            let (ws_subscribed_wallets, ws_expected_wallets) =
                sol_listener_hb.ws_subscription_status();
            let stale_positions_count = active_positions
                .iter()
                .filter(|p| {
                    p.price_source == "STALE"
                        || (Utc::now() - p.price_updated_at).num_seconds().max(0) as u64
                            >= config_hb.stale_price_max_secs
                })
                .count();
            let trading_paused = config_hb.trading_paused;
            let wallet_pubkey_mismatch = wallet_pubkey_mismatch_hb.load(Ordering::Relaxed);

            let hb = IpcMessage::Heartbeat {
                uptime_secs,
                heartbeat_seq: sync_tick,
                // Latensi diukur dari request Solana/Robinhood aktual; 0 berarti unavailable.
                solana_latency_ms,
                arbitrum_latency_ms: robinhood_latency_ms,
                base_latency_ms: 0,
                tracked_wallets_count: total_tracked_count,
                active_positions_count,
                wallet_sol_balance,
                sol_price_usd,
                wallet_equity_usd,
                total_realized_pnl_usd,
                min_liquidity_usd: config_hb.min_liquidity_usd,
                max_price_delta_pct: config_hb.max_price_delta_pct,
                position_size_usd: config_hb.position_size_usd,
                stop_loss_pct: if config_hb.shadow_exit_enabled {
                    config_hb.stop_loss_pct
                } else {
                    config_hb.bot_stop_loss_pct
                },
                take_profit_pct: if config_hb.shadow_exit_enabled {
                    config_hb.take_profit_pct
                } else {
                    config_hb.bot_take_profit_pct
                },
                shadow_exit_enabled: config_hb.shadow_exit_enabled,
                min_whale_buy_usd: config_hb.min_whale_buy_usd,
                is_live_mode: !config_hb.dry_run,
                wallet_pubkey,
                heartbeat_timestamp_unix: Utc::now().timestamp(),
                solana_rpc_requests: sol_exec_hb.rpc_health_snapshot().0,
                solana_rpc_successes: sol_exec_hb.rpc_health_snapshot().1,
                solana_rpc_rate_limited: sol_exec_hb.rpc_health_snapshot().2,
                solana_rpc_failures: sol_exec_hb.rpc_health_snapshot().3,
                robinhood_rpc_requests: rpc_requests,
                robinhood_rpc_successes: rpc_successes,
                robinhood_rpc_rate_limited: rpc_rate_limited,
                robinhood_rpc_failures: rpc_failures,
                qualified_wallets_count: qualified_wallets.len(),
                polled_wallets_count: polled_counter_hb.load(Ordering::Relaxed),
                global_scanned_wallets_count: global_scanned_counter_hb.load(Ordering::Relaxed),
                candidate_wallets_count: candidate_counter_hb.load(Ordering::Relaxed),
                ws_subscribed_wallets,
                ws_expected_wallets,
                stale_positions_count,
                trading_paused,
                wallet_pubkey_mismatch,
            };
            if let Err(error) = ipc_heartbeat.broadcast(&hb) {
                eprintln!("[IPC ERROR] Heartbeat gagal: {error}");
            }

            // Sync qualified elite target wallets (Tier-S & Tier-A only) and trade executions audit log every 4 seconds
            if sync_tick.is_multiple_of(2) {
                if !qualified_wallets.is_empty() {
                    if let Err(error) = ipc_heartbeat.broadcast(&IpcMessage::WalletSync {
                        wallets: qualified_wallets,
                    }) {
                        eprintln!("[IPC ERROR] WalletSync gagal: {error}");
                    }
                }
                match db_heartbeat.get_recent_trades(50) {
                    Ok(trades) => {
                        if let Err(error) =
                            ipc_heartbeat.broadcast(&IpcMessage::TradeSync { trades })
                        {
                            eprintln!("[IPC ERROR] TradeSync gagal: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[HEARTBEAT DB ERROR] Recent trades unavailable: {error}");
                    }
                }
                match db_heartbeat.get_recent_activities(50) {
                    Ok(activities) => {
                        if let Err(error) =
                            ipc_heartbeat.broadcast(&IpcMessage::ActivitySync { activities })
                        {
                            eprintln!("[IPC ERROR] ActivitySync gagal: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[HEARTBEAT DB ERROR] Recent activities unavailable: {error}");
                    }
                }
                match db_heartbeat.get_recent_signals(50) {
                    Ok(signals) => {
                        if let Err(error) =
                            ipc_heartbeat.broadcast(&IpcMessage::SignalSync { signals })
                        {
                            eprintln!("[IPC ERROR] SignalSync gagal: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[HEARTBEAT DB ERROR] Recent signals unavailable: {error}");
                    }
                }
                // Tracked tokens are also a DB-backed replacement snapshot so the
                // TUI never depends on radar-harvest events to refresh this domain.
                match db_heartbeat.get_tracked_tokens(50) {
                    Ok(tokens) => {
                        if let Err(error) =
                            ipc_heartbeat.broadcast(&IpcMessage::TrackedTokensUpdate { tokens })
                        {
                            eprintln!("[IPC ERROR] TrackedTokensSync gagal: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[HEARTBEAT DB ERROR] Tracked tokens unavailable: {error}");
                    }
                }
                match db_heartbeat.get_radar_projections(50) {
                    Ok(projections) => {
                        if let Err(error) =
                            ipc_heartbeat.broadcast(&IpcMessage::RadarSync { projections })
                        {
                            eprintln!("[IPC ERROR] RadarSync gagal: {error}");
                        }
                    }
                    Err(error) => {
                        eprintln!("[HEARTBEAT DB ERROR] Radar projections unavailable: {error}");
                    }
                }
                // Reconnect recovery: hanya drain bila client sudah terhubung.
                // Item tetap QUEUED bila belum ada consumer IPC.
                const RECONNECT_OUTBOX_DRAIN_LIMIT: usize = 128;
                if ipc_heartbeat.receiver_count() > 0 {
                    if let Err(error) = drain_activity_ipc_outbox(
                        &db_heartbeat,
                        &ipc_heartbeat,
                        RECONNECT_OUTBOX_DRAIN_LIMIT,
                    ) {
                        eprintln!("[IPC OUTBOX ERROR] reconnect recovery gagal: {error}");
                    }
                }
            }

            sleep(Duration::from_secs(2)).await;
        }
    });

    // Task 5: Database Self-Maintenance Worker (Runs every 1 hour)
    let db_maintenance = db.clone();
    tokio::spawn(async move {
        // Run initial maintenance 10s after daemon boots
        sleep(Duration::from_secs(10)).await;
        loop {
            match db_maintenance.run_database_maintenance() {
                Ok(report) => {
                    println!(
                        "[DB MAINTENANCE ✅] Pruned: evidence={} stale_holdings={} snapshots={} | WAL checkpoint & optimize OK",
                        report.deleted_evidence,
                        report.deleted_stale_holdings,
                        report.deleted_snapshots
                    );
                }
                Err(error) => {
                    eprintln!("[DB MAINTENANCE ERROR ⚠️] Database maintenance gagal: {error}");
                }
            }
            sleep(Duration::from_secs(3600)).await;
        }
    });

    // Keep main thread alive
    tokio::signal::ctrl_c().await?;
    println!("\n[SHUTDOWN] Stopping ZenithSW Daemon cleanly...");
    Ok(())
}

/// Jumlah raw token posisi + decimals mint untuk kuotasi exit.
/// Live = saldo on-chain riil; dry-run = amount_native × 10^decimals.
/// Error bila saldo/decimals tidak terbukti — tidak ada nilai pengganti.
async fn position_exit_amount_raw(
    pos: &ShadowPosition,
    sol_exec: &SolanaExecutor,
) -> Result<(u64, u8)> {
    let decimals = sol_exec.get_token_decimals(&pos.token_mint).await?;
    let amount_raw = if pos.is_dry_run {
        (pos.amount_native * 10f64.powi(i32::from(decimals))).round() as u64
    } else {
        sol_exec.get_own_token_balance(&pos.token_mint).await?.0
    };
    Ok((amount_raw, decimals))
}

/// Harga realizable per token (USD) dari kuotasi Jupiter; error bila harga SOL belum terverifikasi.
async fn realizable_price_usd(quote: &CachedRealizableQuote, decimals: u8) -> Result<f64> {
    let sol_usd = SolanaStreamListener::get_cached_sol_price_usd()
        .await
        .ok_or_else(|| anyhow!("harga SOL/USD belum terverifikasi"))?;
    let price = quote.calculate_realizable_price_usd(sol_usd, decimals);
    if price.is_finite() && price > 0.0 {
        Ok(price)
    } else {
        bail!("harga realizable tidak valid ({price})")
    }
}

/// Harga realizable fresh (tanpa cache) untuk jumlah token riil posisi.
async fn fresh_realizable_price(pos: &ShadowPosition, sol_exec: &SolanaExecutor) -> Result<f64> {
    let (amount_raw, decimals) = position_exit_amount_raw(pos, sol_exec).await?;
    let quote = sol_exec
        .get_fresh_exit_quote(&pos.token_mint, amount_raw)
        .await?;
    realizable_price_usd(&quote, decimals).await
}

/// Harga posisi = harga terbukti terendah (konservatif). Keduanya tidak ada = None (STALE).
fn select_position_price(
    dex_price: Option<f64>,
    realizable_price: Option<f64>,
) -> Option<(f64, &'static str)> {
    match (dex_price, realizable_price) {
        (Some(dex), Some(realizable)) if realizable < dex => Some((realizable, "JUPITER")),
        (Some(dex), _) => Some((dex, "DEXSCREENER")),
        (None, Some(realizable)) => Some((realizable, "JUPITER")),
        (None, None) => None,
    }
}

async fn evaluate_single_position(
    mut pos: ShadowPosition,
    db_pos_tracker: Arc<SmartWalletDb>,
    sol_exec_pos: Arc<SolanaExecutor>,
    evm_exec_pos: Arc<EvmExecutor>,
    config_pos: Arc<Config>,
) -> ShadowPosition {
    if pos.status == "CLOSED" {
        return pos;
    }

    // 1. Cek kepemilikan posisi: engine hanya mengelola posisi milik wallet aktif
    let active_pubkey = sol_exec_pos.get_wallet_pubkey();
    if !pos.is_dry_run && pos.chain == Chain::Solana {
        if let Some(ref owner) = pos.owner_pubkey {
            if let Some(ref active) = active_pubkey {
                if owner != active {
                    // Milik wallet lain. Lewati agar tidak disentuh atau ditutup oleh wallet aktif yang berbeda.
                    return pos;
                }
            }
        }
    }

    // 2. External exit detection: Jika posisi berstatus aktif di DB tetapi saldo token on-chain 0,
    // rekonsiliasi manual exit seketika dan catat ke trade_history via MANUAL_EXIT_EXTERNAL.
    if !pos.is_dry_run && pos.chain == Chain::Solana {
        if let Ok((raw_bal, _)) = sol_exec_pos.get_own_token_balance(&pos.token_mint).await {
            if raw_bal == 0 {
                println!(
                    "[EXTERNAL EXIT DETECTED 🔄] Token {} ({}) saldo on-chain 0. Mencari bukti transaksi jual di riwayat on-chain untuk posisi {}.",
                    pos.token_symbol, pos.token_mint, pos.id
                );
                if let Err(e) = sol_exec_pos.reconcile_external_exit(&pos).await {
                    eprintln!(
                        "[EXTERNAL EXIT HOLD ⚠️] Posisi {} ditahan, dicoba lagi siklus berikutnya: {e}",
                        pos.id
                    );
                }
                return pos;
            }
        }
    }

    // Fetch latest live price & pool liquidity; never relabel stale price as fresh.
    let price_result = match pos.chain {
                        Chain::Solana => SolanaStreamListener::fetch_token_price(&pos.token_mint).await,
                        Chain::Robinhood => Err(anyhow::anyhow!("Robinhood position price requires a verified on-chain quote; Solana resolver is not permitted")),
                        _ => Err(anyhow::anyhow!("unsupported chain position price source")),
                    };
    // DexScreener gagal ≠ posisi buta: kuotasi Jupiter tetap dievaluasi di bawah (A3).
    let dex_market = match price_result {
        Ok((_, price, liquidity, _))
            if price.is_finite() && price > 0.0 && liquidity.is_finite() && liquidity > 0.0 =>
        {
            Some((price, liquidity))
        }
        Ok((symbol, price, liquidity, _)) => {
            eprintln!(
                                "[PRICE UPDATE SKIP ⚠️] {} returned invalid market data (price=${:.8}, liquidity=${:.2}); lanjut ke kuotasi Jupiter.",
                                if symbol.is_empty() { &pos.token_symbol } else { &symbol },
                                price,
                                liquidity
                            );
            None
        }
        Err(error) => {
            eprintln!(
                "[PRICE UPDATE FAILED ⚠️] {} ({}) — DexScreener gagal; lanjut ke kuotasi Jupiter.",
                pos.token_symbol, error
            );
            None
        }
    };
    // Tanpa data DexScreener, likuiditas tidak diketahui (0) sehingga drain guard tidak terpicu palsu.
    let live_liquidity = dex_market.map_or(0.0, |(_, liquidity)| liquidity);

    // Realizable Jupiter Quote integration:
    // Verifikasi harga realizable on-chain via Jupiter swap quote.
    // Jika harga realizable lebih rendah daripada DexScreener (akibat slippage/price impact/pool tipis),
    // wajib prioritaskan harga realizable on-chain sebagai ground truth untuk trailing stop & stop-loss!
    let mut realizable_price: Option<f64> = None;
    if pos.chain == Chain::Solana {
        // Saldo/decimals tidak terbukti: kuotasi realizable dilewati (tanpa sampel/nilai default).
        // Harga DexScreener tetap dipakai sehingga SL masih dievaluasi; recheck SL fail-closed.
        let realizable = match position_exit_amount_raw(&pos, &sol_exec_pos).await {
            Ok((token_amount_raw, decimals)) => sol_exec_pos
                .get_realizable_exit_quote(&pos.token_mint, token_amount_raw)
                .await
                .map(|quote| (quote, decimals)),
            Err(error) => {
                eprintln!(
                    "[REALIZABLE ORACLE SKIP ⚠️] {} ({}) saldo/decimals tidak terbukti: {error}",
                    pos.token_symbol, pos.token_mint
                );
                Err(error)
            }
        };

        if let Ok((quote, decimals)) = realizable {
            match realizable_price_usd(&quote, decimals).await {
                Ok(calc_price) => {
                    if dex_market.is_none_or(|(dex_price, _)| calc_price < dex_price) {
                        println!(
                                        "[REALIZABLE ORACLE 🎯] {} DexScreener={} vs Realizable=${:.8} (impact: {:.1}%, out: {:.4} SOL). Menggunakan realizable price.",
                                        pos.token_symbol,
                                        dex_market.map_or("N/A".to_string(), |(dex_price, _)| format!("${dex_price:.8}")),
                                        calc_price, quote.price_impact_pct, quote.estimated_out_sol
                                    );
                    }
                    realizable_price = Some(calc_price);
                }
                Err(error) => eprintln!(
                    "[REALIZABLE ORACLE SKIP ⚠️] {} harga realizable tidak terbukti: {error}",
                    pos.token_symbol
                ),
            }
        }
    }

    let Some((live_price, price_source)) =
        select_position_price(dex_market.map(|(price, _)| price), realizable_price)
    else {
        eprintln!(
            "[PRICE STALE ⚠️] {} ({}) DexScreener & Jupiter sama-sama tidak tersedia; tanpa keputusan siklus ini.",
            pos.token_symbol, pos.token_mint
        );
        pos.price_source = "STALE".to_string();
        return pos;
    };

    // Sanity check: Discard single-tick anomaly if price jumps > configured multiplier in one tick from an established price
    if live_price > 0.0 {
        if pos.current_price > 0.0
            && live_price > pos.current_price * config_pos.price_sanity_spike_mult
        {
            eprintln!(
                                "[PRICE SANITY GUARD 🛡️] Suspicious price spike for {} (${:.6} -> ${:.6}). Discarding unverified tick.",
                                pos.token_symbol, pos.current_price, live_price
                            );
            return pos;
        }

        // Downward sanity check: Warn on extreme single-tick drop (>50% drop in one tick).
        // Hanya untuk harga dari DexScreener: anjlok versi kuotasi Jupiter tidak boleh diveto
        // DexScreener (A2); recheck SL tetap memakai kuotasi Jupiter fresh.
        if price_source == "DEXSCREENER"
            && pos.current_price > 0.0
            && live_price < pos.current_price * 0.50
        {
            eprintln!(
                                "[PRICE SANITY GUARD 🛡️] Extreme price drop for {} (${:.6} -> ${:.6}, -{:.1}% in single tick). Debouncing 1500ms with fresh oracle recheck...",
                                pos.token_symbol,
                                pos.current_price,
                                live_price,
                                ((pos.current_price - live_price) / pos.current_price) * 100.0
                            );
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let fresh_check = match pos.chain {
                Chain::Solana => SolanaStreamListener::fetch_token_price_fresh(&pos.token_mint)
                    .await
                    .ok(),
                _ => None,
            };
            if let Some((_, fresh_p, fresh_liq, _)) = fresh_check {
                if fresh_p >= pos.current_price * 0.50
                    && fresh_liq >= config_pos.liquidity_drain_usd
                {
                    println!(
                                        "[PRICE SANITY GUARD 🛡️] False price crash rejected for {} (${:.6} recovered vs glitched ${:.6}). Tick discarded!",
                                        pos.token_symbol, fresh_p, live_price
                                    );
                    return pos;
                }
            }
        }

        pos.current_price = live_price;
        pos.price_source = price_source.to_string();
        pos.price_updated_at = Utc::now();
        pos.peak_price = pos.peak_price.max(live_price);
        if pos.entry_price > 0.0 {
            pos.unrealized_pnl_pct =
                ((pos.current_price - pos.entry_price) / pos.entry_price) * 100.0;
        }
        pos.last_update = Utc::now();

        // Realizable Jupiter Quote check for Solana positions nearing TP (+10%) or SL (-20%)
        if pos.chain == Chain::Solana
            && (pos.unrealized_pnl_pct >= 10.0 || pos.unrealized_pnl_pct <= -20.0)
        {
            let quote = match position_exit_amount_raw(&pos, &sol_exec_pos).await {
                Ok((raw_amt, _)) => {
                    sol_exec_pos
                        .get_realizable_exit_quote(&pos.token_mint, raw_amt)
                        .await
                }
                Err(error) => Err(error),
            };
            if let Ok(quote) = quote {
                if quote.price_impact_pct > 15.0 {
                    eprintln!(
                                        "[REALIZABLE EXIT GUARD ⚠️] Token {} Jupiter price impact is severe ({:.1}%). Realizable out: {:.4} SOL",
                                        pos.token_symbol, quote.price_impact_pct, quote.estimated_out_sol
                                    );
                }
            }
        }

        // Persist every valid tick so current_price, PnL, and last_update
        // remain recoverable after a restart, even when the price is flat.
        // The TUI receives the same fresh snapshot below via IPC.
        if let Err(error) = db_pos_tracker.upsert_position(&pos) {
            eprintln!(
                "[POSITION DB ERROR] Gagal menyimpan posisi {}: {error}",
                pos.id
            );
            return pos;
        }

        // Liquidity Drain Guard (Rug-pull trap protection: exit immediately if pool liquidity collapses below floor)
        if live_liquidity > 0.0
            && live_liquidity < config_pos.liquidity_drain_usd
            && (pos.status == "OPEN" || pos.status == "PARTIAL_EXIT")
        {
            println!(
                                "[LIQUIDITY DRAIN ⏳] Position {} ({}) reported pool liquidity ${:.0} (<${:.0}). Debouncing 1500ms to verify on-chain before emergency dump...",
                                pos.id, pos.token_symbol, live_liquidity, config_pos.liquidity_drain_usd
                            );
            tokio::time::sleep(Duration::from_millis(1500)).await;

            let is_verified_drain = match pos.chain {
                Chain::Solana => {
                    let fresh_market =
                        SolanaStreamListener::fetch_token_price_fresh(&pos.token_mint)
                            .await
                            .ok();

                    let dexscreener_recovered = fresh_market
                        .as_ref()
                        .map(|(_, _, fresh_liq, _)| *fresh_liq >= config_pos.liquidity_drain_usd)
                        .unwrap_or(false);

                    if dexscreener_recovered {
                        let fresh_liq = fresh_market.unwrap().2;
                        println!(
                                            "[LIQUIDITY DRAIN REJECTED 🛡️] {} liquidity recovered to ${:.0} on recheck. False alarm discarded!",
                                            pos.token_symbol, fresh_liq
                                        );
                        false
                    } else {
                        if fresh_market.is_none() {
                            println!(
                                                "[LIQUIDITY DRAIN WARNING ⚠️] {} DexScreener recheck unavailable/timed out. Verifying via Jupiter route...",
                                                pos.token_symbol
                                            );
                        }
                        // Secondary check: rute Jupiter untuk jumlah token riil posisi (bukan sampel)
                        let drain_quote = match position_exit_amount_raw(&pos, &sol_exec_pos).await
                        {
                            Ok((raw_amt, _)) => {
                                sol_exec_pos
                                    .get_fresh_exit_quote(&pos.token_mint, raw_amt)
                                    .await
                            }
                            Err(error) => Err(error),
                        };
                        match drain_quote {
                            Ok(q) if q.estimated_out_lamports > 0 && q.price_impact_pct < 50.0 => {
                                println!(
                                                    "[LIQUIDITY DRAIN REJECTED 🛡️] {} Jupiter route verified active (impact: {:.1}%, out: {:.4} SOL). False alarm discarded!",
                                                    pos.token_symbol, q.price_impact_pct, q.estimated_out_sol
                                                );
                                false
                            }
                            Ok(q) => {
                                println!(
                                                    "[LIQUIDITY DRAIN CONFIRMED 🚨] {} Jupiter route severely degraded (impact: {:.1}%, out: {:.4} SOL). Triggering emergency exit!",
                                                    pos.token_symbol, q.price_impact_pct, q.estimated_out_sol
                                                );
                                true
                            }
                            Err(err) => {
                                let err_msg = err.to_string();
                                let lower = err_msg.to_lowercase();
                                let is_terminal = lower.contains("could_not_find_any_route")
                                    || lower.contains("could not find any route")
                                    || lower.contains("token_not_tradable")
                                    || lower.contains("not tradable")
                                    || lower.contains("not_enough_liquidity")
                                    || lower.contains("not enough liquidity")
                                    || lower.contains("route_not_found");

                                if is_terminal {
                                    println!(
                                                        "[LIQUIDITY DRAIN CONFIRMED 🚨] {} Jupiter route collapsed on-chain ({}). Triggering emergency exit!",
                                                        pos.token_symbol, err_msg
                                                    );
                                    true
                                } else {
                                    eprintln!(
                                                        "[LIQUIDITY DRAIN SUSPENDED ⚠️] {} route verification failed with network/transient error: {}. Position held safely for next cycle.",
                                                        pos.token_symbol, err_msg
                                                    );
                                    false
                                }
                            }
                        }
                    }
                }
                _ => true,
            };

            if !is_verified_drain {
                return pos;
            }

            println!(
                                "[LIQUIDITY DRAIN GUARD 🚨] Confirmed pool liquidity collapse for {} (<${:.0})! Emergency dumping to avoid rug-pull trap!",
                                pos.token_symbol, config_pos.liquidity_drain_usd
                            );
            match pos.chain {
                Chain::Solana => {
                    if let Err(error) = sol_exec_pos
                        .execute_shadow_sell(&pos.id, "LIQUIDITY_DRAIN_EXIT", live_price)
                        .await
                    {
                        eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=LIQUIDITY_DRAIN_EXIT error={error}", pos.id);
                    }
                }
                _ => {
                    if let Err(error) = evm_exec_pos
                        .execute_shadow_sell(&pos.id, "LIQUIDITY_DRAIN_EXIT", live_price)
                        .await
                    {
                        eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=LIQUIDITY_DRAIN_EXIT error={error}", pos.id);
                    }
                }
            }
            return pos;
        }

        if config_pos.shadow_exit_enabled {
            // =========================================================================
            // MODE A: WHALE SHADOW & RUNNER PROTOCOL (Configured Multi-Tier Exit + Moonbag)
            // =========================================================================

            // 1. Tier 1 Auto Take Profit (Sell configured % of initial position at TP1 PnL to free-roll initial capital)
            if pos.unrealized_pnl_pct >= config_pos.take_profit_pct
                && (pos.status == "OPEN" || pos.status == "PARTIAL_EXIT")
                && pos.remaining_portion_pct >= 90.0
            {
                println!("[AUTO-TP 1 (+{:.0}%) 🎯] Position {} ({}) reached +{:.1}% >= +{:.0}%! Selling {:.0}% to secure capital (Free-Roll Mode)!",
                                    config_pos.take_profit_pct, pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.take_profit_pct, config_pos.tp1_portion_pct);
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_partial_shadow_sell(
                                &pos.id,
                                config_pos.tp1_portion_pct,
                                "AUTO_TP_50_FREE_ROLL",
                                live_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=AUTO_TP_50_FREE_ROLL error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_partial_shadow_sell(
                                &pos.id,
                                config_pos.tp1_portion_pct,
                                "AUTO_TP_50_FREE_ROLL",
                                live_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=AUTO_TP_50_FREE_ROLL error={error}", pos.id);
                        }
                    }
                }
            }

            // 2. Tier 2 Moonbag Take Profit (Sell configured % of initial at configured TP2 PnL, leaving runner)
            let tp2_target_pnl = config_pos.take_profit_2_pct;
            let remaining_after_tp1 = 100.0 - config_pos.tp1_portion_pct;
            let moonbag_portion = remaining_after_tp1 - config_pos.tp2_portion_pct;
            if pos.unrealized_pnl_pct >= tp2_target_pnl
                && pos.status == "PARTIAL_EXIT"
                && pos.remaining_portion_pct > (moonbag_portion + 5.0)
                && pos.remaining_portion_pct <= (remaining_after_tp1 + 5.0)
            {
                println!("[AUTO-TP 2 (+{:.0}%) 🚀] Position {} ({}) hit +{:.1}%! Selling {:.0}% of initial position to lock moonbag gains!",
                                    tp2_target_pnl, pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.tp2_portion_pct);
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_partial_shadow_sell(
                                &pos.id,
                                config_pos.tp2_portion_pct,
                                "AUTO_TP_25_RUNNER_LOCK",
                                live_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=AUTO_TP_25_RUNNER_LOCK error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_partial_shadow_sell(
                                &pos.id,
                                config_pos.tp2_portion_pct,
                                "AUTO_TP_25_RUNNER_LOCK",
                                live_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=AUTO_TP_25_RUNNER_LOCK error={error}", pos.id);
                        }
                    }
                }
            }

            // 3. Tier 3 Catastrophic Peak Drawdown Guard on Runner / Moonbag
            // If price drops beyond configured % from peak after a configured multiplier pump, dump remaining portion to prevent roundtripping to zero
            let peak_drawdown_ratio = 1.0 - (config_pos.peak_drawdown_pct / 100.0);
            if pos.status == "PARTIAL_EXIT"
                && pos.peak_price >= pos.entry_price * config_pos.runner_min_pump_mult
                && pos.current_price <= pos.peak_price * peak_drawdown_ratio
                && pos.remaining_portion_pct <= (remaining_after_tp1 + 5.0)
            {
                println!("[PEAK DRAWDOWN GUARD 🛡️] Position {} ({}) dropped >{:.0}% from peak (${:.6} -> ${:.6})! Dumping remaining {:.0}% to lock profit!",
                                    pos.id, pos.token_symbol, config_pos.peak_drawdown_pct, pos.peak_price, pos.current_price, pos.remaining_portion_pct);
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_shadow_sell(&pos.id, "PEAK_DRAWDOWN_GUARD_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=PEAK_DRAWDOWN_GUARD_EXIT error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_shadow_sell(&pos.id, "PEAK_DRAWDOWN_GUARD_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=PEAK_DRAWDOWN_GUARD_EXIT error={error}", pos.id);
                        }
                    }
                }
            }

            // 4. Profit-Lock Floor on Moonbag (locked once profit reaches the configured floor)
            // STRICT SAFETY: Must have reached at least configured pump multiplier initially and current PnL must be positive (>= profit_lock_min_current_pnl_pct)
            if pos.status == "PARTIAL_EXIT"
                && pos.peak_price >= pos.entry_price * config_pos.runner_min_pump_mult
                && pos.unrealized_pnl_pct <= config_pos.profit_lock_floor_pct
                && pos.unrealized_pnl_pct >= config_pos.profit_lock_min_current_pnl_pct
                && pos.remaining_portion_pct <= (remaining_after_tp1 + 5.0)
            {
                println!("[PROFIT-LOCK FLOOR 🛡️] Position {} ({}) moonbag dropped to +{:.1}% (<= +{:.0}%)! Closing remaining portion to guarantee green trade!",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.profit_lock_floor_pct);
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_shadow_sell(&pos.id, "PROFIT_LOCK_FLOOR_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=PROFIT_LOCK_FLOOR_EXIT error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_shadow_sell(&pos.id, "PROFIT_LOCK_FLOOR_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=PROFIT_LOCK_FLOOR_EXIT error={error}", pos.id);
                        }
                    }
                }
            }

            // 5. Pre-TP Conditional Trailing Stop Guard (Capital Preservation)
            // Activates when peak gain reaches trailing_stop_trigger_pct (e.g. +35%),
            // and dumps 100% if current price drops >= trailing_stop_pct (e.g. 15%) from peak.
            let peak_gain_pct = if pos.entry_price > 0.0 {
                ((pos.peak_price - pos.entry_price) / pos.entry_price) * 100.0
            } else {
                0.0
            };

            if (pos.status == "OPEN" || pos.remaining_portion_pct >= 90.0)
                && peak_gain_pct >= config_pos.trailing_stop_trigger_pct
                && pos.status != "CLOSED"
            {
                let trailing_drop_threshold = 1.0 - (config_pos.trailing_stop_pct / 100.0);
                let trailing_exit_price = pos.peak_price * trailing_drop_threshold;

                if pos.current_price <= trailing_exit_price {
                    println!(
                                        "[TRAILING STOP GUARD 🛡️] Position {} ({}) peak was +{:.1}% (${:.6}), dropped >{:.0}% from peak to ${:.6} (<= ${:.6})! Dumping 100% to lock profit!",
                                        pos.id, pos.token_symbol, peak_gain_pct, pos.peak_price, config_pos.trailing_stop_pct, pos.current_price, trailing_exit_price
                                    );
                    match pos.chain {
                        Chain::Solana => {
                            if let Err(error) = sol_exec_pos
                                .execute_shadow_sell(&pos.id, "TRAILING_STOP_EXIT", live_price)
                                .await
                            {
                                eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=TRAILING_STOP_EXIT error={error}", pos.id);
                            }
                        }
                        _ => {
                            if let Err(error) = evm_exec_pos
                                .execute_shadow_sell(&pos.id, "TRAILING_STOP_EXIT", live_price)
                                .await
                            {
                                eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=TRAILING_STOP_EXIT error={error}", pos.id);
                            }
                        }
                    }
                    return pos;
                }
            }

            // 6. Emergency Hard Stop Loss Cut-Loss (Disaster Protection Floor)
            let hard_sl_trigger = pos.unrealized_pnl_pct <= -config_pos.stop_loss_pct;
            if hard_sl_trigger && pos.status != "CLOSED" {
                println!(
                                    "[SL CONFIRMATION ⏳] Position {} ({}) hit {:.1}% <= -{:.1}%. Awaiting 1200ms debounce to filter flash wicks...",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.stop_loss_pct
                                );
                sleep(Duration::from_millis(1200)).await;

                if pos.chain == Chain::Solana {
                    // Solana exit oracle ground truth: Jupiter realizable exit quote (NO DEXSCREENER VETO!)
                    // Hanya harga terbukti (saldo riil + decimals + harga SOL) yang boleh membatalkan SL.
                    match fresh_realizable_price(&pos, &sol_exec_pos).await {
                        Ok(fresh_p) => {
                            if pos.entry_price > 0.0 {
                                let fresh_pnl =
                                    ((fresh_p - pos.entry_price) / pos.entry_price) * 100.0;
                                pos.current_price = fresh_p;
                                pos.unrealized_pnl_pct = fresh_pnl;
                                pos.price_source = "JUPITER".to_string();
                                pos.price_updated_at = Utc::now();
                                let _ = db_pos_tracker.upsert_position(&pos);

                                if fresh_pnl > -config_pos.stop_loss_pct {
                                    println!(
                                                        "[SL FLASH-WICK REJECTED 🛡️] Position {} ({}) Jupiter realizable price recovered to ${:.6} ({:.1}% > -{:.1}%). Cut-loss cancelled!",
                                                        pos.id, pos.token_symbol, fresh_p, fresh_pnl, config_pos.stop_loss_pct
                                                    );
                                    return pos;
                                }
                            }
                        }
                        Err(quote_err) => {
                            // Saldo/decimals/harga SOL/quote tidak terbukti -> FAIL-CLOSED!
                            // Dilarang membatalkan SL berdasarkan DexScreener!
                            eprintln!(
                                                "[SL RECHECK FAIL-CLOSED ⚠️] Token {} harga realizable fresh tidak terbukti ({quote_err}). Proceeding with EMERGENCY CUT-LOSS to protect capital!",
                                                pos.token_symbol
                                            );
                        }
                    }
                } else {
                    // For non-Solana (e.g. EVM)
                    let recheck_price: Option<f64> = match pos.chain {
                        Chain::Robinhood => None,
                        _ => None,
                    };
                    if let Some(fresh_p) = recheck_price {
                        if fresh_p > 0.0 && pos.entry_price > 0.0 {
                            let fresh_pnl = ((fresh_p - pos.entry_price) / pos.entry_price) * 100.0;
                            pos.current_price = fresh_p;
                            pos.unrealized_pnl_pct = fresh_pnl;
                            pos.price_updated_at = Utc::now();
                            let _ = db_pos_tracker.upsert_position(&pos);
                            if fresh_pnl > -config_pos.stop_loss_pct {
                                return pos;
                            }
                        }
                    }
                }

                println!("[EMERGENCY SL CUT-LOSS 🛑] Position {} ({}) confirmed breach at {:.1}% <= -{:.1}%! Force closing immediately to protect capital!",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.stop_loss_pct);
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_shadow_sell(
                                &pos.id,
                                "EMERGENCY_HARD_SL_CUTLOSS",
                                pos.current_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=EMERGENCY_HARD_SL_CUTLOSS error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_shadow_sell(
                                &pos.id,
                                "EMERGENCY_HARD_SL_CUTLOSS",
                                pos.current_price,
                            )
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=EMERGENCY_HARD_SL_CUTLOSS error={error}", pos.id);
                        }
                    }
                }
                return pos;
            }
        } else {
            // =========================================================================
            // MODE B: PURE BOT AUTONOMOUS EXIT (Mode FALSE — All-or-Nothing / Clean Scalp)
            // =========================================================================

            // 1. Bot Trailing Stop Guard (Capital Preservation)
            let peak_gain_pct = if pos.entry_price > 0.0 {
                ((pos.peak_price - pos.entry_price) / pos.entry_price) * 100.0
            } else {
                0.0
            };

            if pos.status != "CLOSED" && peak_gain_pct >= config_pos.bot_trailing_stop_trigger_pct {
                let trailing_drop_threshold = 1.0 - (config_pos.bot_trailing_stop_pct / 100.0);
                let trailing_exit_price = pos.peak_price * trailing_drop_threshold;

                if pos.current_price <= trailing_exit_price {
                    println!(
                                        "[BOT TRAILING STOP 🛡️] Position {} ({}) peak was +{:.1}% (${:.6}), dropped >{:.0}% from peak to ${:.6} (<= ${:.6})! Dumping 100% to lock profit!",
                                        pos.id, pos.token_symbol, peak_gain_pct, pos.peak_price, config_pos.bot_trailing_stop_pct, pos.current_price, trailing_exit_price
                                    );
                    match pos.chain {
                        Chain::Solana => {
                            if let Err(error) = sol_exec_pos
                                .execute_shadow_sell(&pos.id, "BOT_TRAILING_STOP_EXIT", live_price)
                                .await
                            {
                                eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=BOT_TRAILING_STOP_EXIT error={error}", pos.id);
                            }
                        }
                        _ => {
                            if let Err(error) = evm_exec_pos
                                .execute_shadow_sell(&pos.id, "BOT_TRAILING_STOP_EXIT", live_price)
                                .await
                            {
                                eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=BOT_TRAILING_STOP_EXIT error={error}", pos.id);
                            }
                        }
                    }
                    return pos;
                }
            }

            // 2. Bot Full Take Profit (100% dump, no partials)
            if pos.status != "CLOSED" && pos.unrealized_pnl_pct >= config_pos.bot_take_profit_pct {
                println!(
                                    "[BOT AUTO-TP 🎯] Position {} ({}) hit +{:.1}% >= +{:.0}%! Selling 100% full position!",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.bot_take_profit_pct
                                );
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_shadow_sell(&pos.id, "BOT_FULL_TP_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=BOT_FULL_TP_EXIT error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_shadow_sell(&pos.id, "BOT_FULL_TP_EXIT", live_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=BOT_FULL_TP_EXIT error={error}", pos.id);
                        }
                    }
                }
                return pos;
            }

            // 3. Bot Full Hard Stop Loss (100% cut loss)
            if pos.status != "CLOSED" && pos.unrealized_pnl_pct <= -config_pos.bot_stop_loss_pct {
                println!(
                                    "[SL CONFIRMATION ⏳] Position {} ({}) hit {:.1}% <= -{:.1}%. Awaiting 1200ms debounce to filter flash wicks...",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.bot_stop_loss_pct
                                );
                sleep(Duration::from_millis(1200)).await;

                // Re-verify with fresh Jupiter realizable quote for Solana (bypass DexScreener to prevent stale veto)
                if pos.chain == Chain::Solana {
                    // Hanya harga terbukti (saldo riil + decimals + harga SOL) yang boleh membatalkan SL.
                    match fresh_realizable_price(&pos, &sol_exec_pos).await {
                        Ok(fresh_p) => {
                            if pos.entry_price > 0.0 {
                                let fresh_pnl =
                                    ((fresh_p - pos.entry_price) / pos.entry_price) * 100.0;
                                pos.current_price = fresh_p;
                                pos.price_source = "JUPITER".to_string();
                                pos.price_updated_at = Utc::now();
                                pos.unrealized_pnl_pct = fresh_pnl;
                                let _ = db_pos_tracker.upsert_position(&pos);

                                if fresh_pnl > -config_pos.bot_stop_loss_pct {
                                    println!(
                                                        "[SL RECHECK RECOVERED 🛡️] Position {} ({}) Jupiter realizable price recovered to ${:.6} ({:.1}% > -{:.1}%). Cut-loss cancelled!",
                                                        pos.id, pos.token_symbol, fresh_p, fresh_pnl, config_pos.bot_stop_loss_pct
                                                    );
                                    return pos;
                                }
                            }
                        }
                        Err(err) => {
                            // Fail-closed: Jika gagal mendapatkan fresh quote Jupiter saat SL terpicu,
                            // eksekusi tetap dilanjutkan untuk melindungi modal (fail-closed exit).
                            eprintln!(
                                                "[SL RECHECK FAIL-CLOSED ⚠️] Harga realizable fresh tidak terbukti untuk {} ({err}). Tetap lanjut cut-loss demi proteksi modal!",
                                                pos.token_symbol
                                            );
                        }
                    }
                }

                println!(
                                    "[BOT EMERGENCY SL 🛑] Position {} ({}) confirmed breach at {:.1}% <= -{:.1}%! Force closing 100% immediately!",
                                    pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.bot_stop_loss_pct
                                );
                match pos.chain {
                    Chain::Solana => {
                        if let Err(error) = sol_exec_pos
                            .execute_shadow_sell(&pos.id, "BOT_HARD_SL_CUTLOSS", pos.current_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] Solana pos={} reason=BOT_HARD_SL_CUTLOSS error={error}", pos.id);
                        }
                    }
                    _ => {
                        if let Err(error) = evm_exec_pos
                            .execute_shadow_sell(&pos.id, "BOT_HARD_SL_CUTLOSS", pos.current_price)
                            .await
                        {
                            eprintln!("[EXIT EXECUTION ERROR] EVM pos={} reason=BOT_HARD_SL_CUTLOSS error={error}", pos.id);
                        }
                    }
                }
                return pos;
            }
        }
    }

    pos
}

#[cfg(test)]
mod position_price_tests {
    use super::select_position_price;

    #[test]
    fn dexscreener_failure_falls_back_to_jupiter_and_never_blind() {
        // DexScreener gagal: Jupiter tetap dipakai (posisi tidak buta).
        assert_eq!(select_position_price(None, Some(0.5)), Some((0.5, "JUPITER")));
        // Keduanya ada: harga terendah (konservatif) menang.
        assert_eq!(select_position_price(Some(1.0), Some(0.4)), Some((0.4, "JUPITER")));
        assert_eq!(select_position_price(Some(1.0), Some(1.2)), Some((1.0, "DEXSCREENER")));
        // Jupiter gagal: DexScreener tetap dipakai.
        assert_eq!(select_position_price(Some(1.0), None), Some((1.0, "DEXSCREENER")));
        // Keduanya gagal: STALE, tanpa harga karangan.
        assert_eq!(select_position_price(None, None), None);
    }
}
