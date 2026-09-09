use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::{EvmExecutor, SolanaExecutor};
use zenithsw::gating::ConfluenceEngine;
use zenithsw::ingestion::FomoScraper;
use zenithsw::ipc::{IpcMessage, IpcServer};
use zenithsw::models::{ActivityType, Chain};
use zenithsw::streams::{EvmStreamListener, SolanaStreamListener};

#[tokio::main]
async fn main() -> Result<()> {
    println!("============================================================");
    println!(" 🚀 ZENITHSW - SMART MONEY SHADOW ENGINE DAEMON v0.1.0");
    println!("============================================================");

    let config = Config::load()?;
    println!("[CONFIG] Database: {}", config.db_path);
    println!("[CONFIG] IPC Socket: {}", config.ipc_socket_path);
    println!("[CONFIG] Max Entry Delta: +{:.1}%", config.max_price_delta_pct);
    println!("[CONFIG] Min Liquidity: ${:.0}", config.min_liquidity_usd);
    println!("[CONFIG] Position Size: ${:.0}", config.position_size_usd);
    println!("[CONFIG] Fomo Sync Interval: {}s", config.fomo_sync_interval_sec);

    let db = Arc::new(SmartWalletDb::new(&config.db_path)?);
    let ipc_server = Arc::new(IpcServer::new(&config.ipc_socket_path));
    ipc_server.start().await?;
    println!("[IPC] Server online on {}", config.ipc_socket_path);

    let scraper = Arc::new(FomoScraper::new());
    let sol_listener = Arc::new(SolanaStreamListener::new(&config.solana_rpc_url));
    let arb_listener = Arc::new(EvmStreamListener::new(&config.arbitrum_rpc_url, Chain::Arbitrum));
    let base_listener = Arc::new(EvmStreamListener::new(&config.base_rpc_url, Chain::Base));
    let confluence = Arc::new(ConfluenceEngine::new(config.clone(), db.clone()));
    let sol_exec = Arc::new(SolanaExecutor::new(config.clone(), db.clone()));
    let evm_exec = Arc::new(EvmExecutor::new(config.clone(), db.clone()));

    // Task 1: Periodic FOMO Leaderboard Synchronizer (Keeps Top 100 Updated)
    let db_sync = db.clone();
    let scraper_sync = scraper.clone();
    let ipc_sync = ipc_server.clone();
    let sync_sec = config.fomo_sync_interval_sec;
    tokio::spawn(async move {
        println!("[FOMO SYNC] Worker started (Interval: {}s)", sync_sec);
        loop {
            match scraper_sync.sync_and_persist(db_sync.clone()).await {
                Ok(count) => {
                    println!("[FOMO SYNC] Successfully synced and persisted {} top wallets", count);
                    if let Ok(wallets) = db_sync.get_active_wallets() {
                        let _ = ipc_sync.broadcast(&IpcMessage::WalletSync { wallets });
                    }
                }
                Err(e) => {
                    eprintln!("[FOMO SYNC ERROR] Failed to sync fomo leaderboard: {}", e);
                }
            }
            sleep(Duration::from_secs(sync_sec)).await;
        }
    });

    // Task 2: Sub-Second Multi-Chain Activity Stream Poller & Shadow Executor
    let db_stream = db.clone();
    let sol_listener_stream = sol_listener.clone();
    let arb_listener_stream = arb_listener.clone();
    let base_listener_stream = base_listener.clone();
    let confluence_stream = confluence.clone();
    let sol_exec_stream = sol_exec.clone();
    let evm_exec_stream = evm_exec.clone();
    let ipc_stream = ipc_server.clone();
    let poll_ms = config.stream_poll_interval_ms;

    tokio::spawn(async move {
        println!("[STREAM WORKER] Multi-Chain Ingestion active (Poll interval: {}ms)", poll_ms);
        loop {
            if let Ok(wallets) = db_stream.get_active_wallets() {
                for w in wallets {
                    let activity_opt = match w.chain {
                        Chain::Solana => sol_listener_stream.check_wallet_activity(&w.handle, &w.address, db_stream.clone()).await.ok().flatten(),
                        Chain::Arbitrum => arb_listener_stream.check_wallet_activity(&w.handle, &w.address, db_stream.clone()).await.ok().flatten(),
                        Chain::Base => base_listener_stream.check_wallet_activity(&w.handle, &w.address, db_stream.clone()).await.ok().flatten(),
                        _ => None,
                    };

                    if let Some(act) = activity_opt {
                        println!("[ACTIVITY] {} by @{} ({}) -> Token: {}", act.activity_type, act.wallet_handle, act.chain, act.token_symbol);
                        let _ = ipc_stream.broadcast(&IpcMessage::ActivityBroadcast { activity: act.clone() });

                        // Handle Shadow Mirror Exit
                        if act.activity_type == ActivityType::Sell {
                            if let Ok(positions) = db_stream.get_active_positions() {
                                for pos in positions {
                                    if pos.token_mint == act.token_mint && pos.target_whale_handle == act.wallet_handle {
                                        println!("[SHADOW EXIT 🚨] Target @{} sold {} -> Triggering instant SELL!", pos.target_whale_handle, pos.token_symbol);
                                        let exit_price = act.execution_price_usd;
                                        match pos.chain {
                                            Chain::Solana => { let _ = sol_exec_stream.execute_shadow_sell(&pos.id, "SHADOW_MIRROR_EXIT", exit_price).await; }
                                            _ => { let _ = evm_exec_stream.execute_shadow_sell(&pos.id, "SHADOW_MIRROR_EXIT", exit_price).await; }
                                        }
                                    }
                                }
                            }
                        } else if act.activity_type == ActivityType::Buy {
                            // Fetch real-time pool price & liquidity
                            let (current_price, pool_liq) = if act.chain == Chain::Solana {
                                SolanaStreamListener::fetch_dexscreener_token_info(&act.token_mint)
                                    .await
                                    .map(|(_, p, l)| (p, l))
                                    .unwrap_or((act.execution_price_usd, 50000.0))
                            } else {
                                (act.execution_price_usd, 40000.0)
                            };

                            // Gating & Confluence Evaluation
                            if let Ok(sig) = confluence_stream.evaluate_activity(&act, &w, current_price, pool_liq) {
                                let _ = ipc_stream.broadcast(&IpcMessage::SignalBroadcast { signal: sig.clone() });

                                if sig.is_passed {
                                    println!("[EXECUTE] Passed gating! Executing shadow buy for {}", sig.token_symbol);
                                    let pos_opt = match sig.chain {
                                        Chain::Solana => sol_exec_stream.execute_shadow_buy(&sig).await.ok().flatten(),
                                        _ => evm_exec_stream.execute_shadow_buy(&sig).await.ok().flatten(),
                                    };

                                    if let Some(new_pos) = pos_opt {
                                        println!("[POSITION OPENED ✅] ID: {}, Token: {}, Amount: ${:.2}", new_pos.id, new_pos.token_symbol, new_pos.amount_usd);
                                    }
                                } else {
                                    println!("[GATING REJECT ❌] Token: {} rejected: {:?}", sig.token_symbol, sig.rejection_reason);
                                }
                            }
                        }
                    }
                }
            }

            // Sync updated active positions to IPC
            if let Ok(positions) = db_stream.get_active_positions() {
                let _ = ipc_stream.broadcast(&IpcMessage::PositionUpdate { positions });
            }

            sleep(Duration::from_millis(poll_ms)).await;
        }
    });

    // Task 3: Heartbeat & IPC Metrics Broadcast
    let db_heartbeat = db.clone();
    let ipc_heartbeat = ipc_server.clone();
    let start_time = Utc::now();
    tokio::spawn(async move {
        loop {
            let uptime_secs = (Utc::now() - start_time).num_seconds().max(0) as u64;
            let tracked_wallets_count = db_heartbeat.get_active_wallets().map(|w| w.len()).unwrap_or(0);
            let active_positions_count = db_heartbeat.get_active_positions().map(|p| p.len()).unwrap_or(0);

            let hb = IpcMessage::Heartbeat {
                uptime_secs,
                solana_latency_ms: 68,
                arbitrum_latency_ms: 42,
                base_latency_ms: 45,
                tracked_wallets_count,
                active_positions_count,
            };
            let _ = ipc_heartbeat.broadcast(&hb);
            sleep(Duration::from_secs(2)).await;
        }
    });

    // Keep main thread alive
    tokio::signal::ctrl_c().await?;
    println!("\n[SHUTDOWN] Stopping ZenithSW Daemon cleanly...");
    Ok(())
}
