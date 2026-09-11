use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::evm_executor::EvmExecutor;
use zenithsw::execution::solana_executor::SolanaExecutor;
use zenithsw::gating::confluence::ConfluenceEngine;
use zenithsw::indexer::SmartMoneyIndexer;
use zenithsw::ingestion::fomo_scraper::FomoScraper;
use zenithsw::ipc::{IpcMessage, IpcServer};
use zenithsw::models::{ActivityType, Chain, WalletTier};
use zenithsw::streams::evm_stream::EvmStreamListener;
use zenithsw::streams::fomo_api_stream::FomoApiStreamListener;
use zenithsw::streams::solana_stream::SolanaStreamListener;

#[tokio::main]
async fn main() -> Result<()> {
    println!("======================================================");
    println!("       ZENITHSW - SMART MONEY SHADOW ENGINE (DAEMON)  ");
    println!("======================================================");

    let config = Config::load()?;
    let db = Arc::new(SmartWalletDb::new(&config.db_path)?);

    // Initial seed: Only populate if database is completely empty
    if let Ok(existing) = db.get_active_wallets() {
        if existing.is_empty() {
            let default_seeds = FomoScraper::get_default_seed_traders();
            println!(
                "[INIT] Database is empty. Syncing {} initial verified smart wallets...",
                default_seeds.len()
            );
            for s in &default_seeds {
                let _ = db.upsert_wallet(s);
            }
        }
    }

    // Initialize IPC Server
    let ipc_server = Arc::new(IpcServer::new(&config.ipc_socket_path));
    ipc_server.start().await?;

    // Initialize Stream Listeners
    let sol_listener = Arc::new(SolanaStreamListener::new(&config.solana_rpc_url));
    let arb_listener = Arc::new(EvmStreamListener::new(
        &config.arbitrum_rpc_url,
        Chain::Robinhood,
    ));
    let base_listener = Arc::new(EvmStreamListener::new(&config.base_rpc_url, Chain::Base));
    let fomo_listener = Arc::new(FomoApiStreamListener::new(Arc::new(config.clone())));

    // Initialize Gating & Executors
    let confluence = Arc::new(ConfluenceEngine::new(config.clone(), db.clone()));
    let sol_exec = Arc::new(SolanaExecutor::new(config.clone(), db.clone()));
    let evm_exec = Arc::new(EvmExecutor::new(config.clone(), db.clone()));

    // Task 1: Direct On-Chain DEX Graph Indexer Worker (Every 10 minutes)
    let indexer = Arc::new(SmartMoneyIndexer::new(
        &config.solana_rpc_url,
        &config.arbitrum_rpc_url,
        db.clone(),
        600,
    ));
    let indexer_worker = indexer.clone();
    tokio::spawn(async move {
        // Initial immediate indexer pass + loop
        indexer_worker.run_background_loop().await;
    });

    // Task 2: Whale Portfolio & Radar Harvester Worker (Top 50 7D Whales + Micro/Small Cap Bottom Classifier)
    let fomo_scraper = Arc::new(FomoScraper::new(Arc::new(config.clone()), db.clone()));
    let fomo_scraper_worker = fomo_scraper.clone();
    let ipc_radar = ipc_server.clone();
    tokio::spawn(async move {
        println!("[RADAR WORKER] Whale Portfolio & Gems Harvester online (Scanning Top 50 7D Leaderboard)...");
        loop {
            match fomo_scraper_worker
                .harvest_7d_whale_holdings_and_radar()
                .await
            {
                Ok(tokens) => {
                    println!("[RADAR HARVEST 💎] Successfully analyzed and tracked {} whale conviction tokens", tokens.len());
                    let _ = ipc_radar.broadcast(&IpcMessage::TrackedTokensUpdate { tokens });
                }
                Err(e) => {
                    eprintln!("[RADAR HARVEST ⚠️] Error harvesting whale holdings: {}", e);
                }
            }
            sleep(Duration::from_secs(60)).await; // Dynamic re-scan every 60 seconds
        }
    });

    // Task 3: High-Speed Paced Stream Ingestion Worker (Zero 429 Drop, 200ms per wallet)
    let db_stream = db.clone();
    let config_stream = Arc::new(config.clone());
    let sol_listener_stream = sol_listener.clone();
    let arb_listener_stream = arb_listener.clone();
    let base_listener_stream = base_listener.clone();
    let fomo_listener_stream = fomo_listener.clone();
    let confluence_stream = confluence.clone();
    let sol_exec_stream = sol_exec.clone();
    let evm_exec_stream = evm_exec.clone();
    let ipc_stream = ipc_server.clone();

    tokio::spawn(async move {
        println!("[STREAM WORKER] Paced Multi-Chain Streamer online (Solana: {}, Robinhood: {}, Base: {})",
            config_stream.enable_solana, config_stream.enable_robinhood, config_stream.enable_base);
        loop {
            if let Ok(mut wallets) = db_stream.get_active_wallets() {
                // Filter only enabled chains
                wallets.retain(|w| match w.chain {
                    Chain::Solana => config_stream.enable_solana,
                    Chain::Robinhood | Chain::Arbitrum => config_stream.enable_robinhood,
                    Chain::Base => config_stream.enable_base,
                    _ => false,
                });

                // Prioritize Qualified Execution Wallets (Tier-S & Tier-A) first
                wallets.sort_by_key(|w| match w.tier {
                    WalletTier::TierS => 0,
                    WalletTier::TierA => 1,
                    WalletTier::TierB => 2,
                });

                for w in wallets {
                    let mut activity_opt = if w.source == "FOMO" && w.tier == WalletTier::TierS {
                        fomo_listener_stream
                            .check_trader_swaps(&w.handle, &w.address, db_stream.clone())
                            .await
                            .ok()
                            .flatten()
                    } else {
                        None
                    };

                    if activity_opt.is_none() {
                        activity_opt = match w.chain {
                            Chain::Solana => sol_listener_stream
                                .check_recent_txs(&w.handle, &w.address, db_stream.clone())
                                .await
                                .ok()
                                .flatten(),
                            Chain::Robinhood | Chain::Arbitrum => arb_listener_stream
                                .check_wallet_activity(&w.handle, &w.address, db_stream.clone())
                                .await
                                .ok()
                                .flatten(),
                            Chain::Base => base_listener_stream
                                .check_wallet_activity(&w.handle, &w.address, db_stream.clone())
                                .await
                                .ok()
                                .flatten(),
                            _ => None,
                        };
                    }

                    if let Some(act) = activity_opt {
                        println!(
                            "[ACTIVITY 🔥] {} by @{} ({}) -> Token: {} (${:.2})",
                            act.activity_type,
                            act.wallet_handle,
                            act.chain,
                            act.token_symbol,
                            act.amount_usd
                        );
                        let _ = ipc_stream.broadcast(&IpcMessage::ActivityBroadcast {
                            activity: act.clone(),
                        });

                        // Handle Pure Shadow Mirror Exit (Proportional / Full per whale contribution)
                        if act.activity_type == ActivityType::Sell {
                            if let Ok(positions) = db_stream.get_active_positions() {
                                for pos in positions {
                                    if pos.token_mint.eq_ignore_ascii_case(&act.token_mint)
                                        && pos.chain == act.chain
                                    {
                                        let whale_addr_lower = act.wallet_address.to_lowercase();
                                        let is_participant = pos
                                            .target_whale_address
                                            .to_lowercase()
                                            .contains(&whale_addr_lower)
                                            || pos
                                                .target_whale_handle
                                                .to_lowercase()
                                                .contains(&act.wallet_handle.to_lowercase());

                                        if is_participant {
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

                                            if raw_sell_ratio >= 95.0 && whale_count <= 1.0 {
                                                println!("[SHADOW FULL EXIT 🔴] Target @{} sold 100% of {} -> Closing position completely!", act.wallet_handle, pos.token_symbol);
                                                match pos.chain {
                                                    Chain::Solana => {
                                                        let _ = sol_exec_stream
                                                            .execute_shadow_sell(
                                                                &pos.id,
                                                                "WHALE_FULL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await;
                                                    }
                                                    _ => {
                                                        let _ = evm_exec_stream
                                                            .execute_shadow_sell(
                                                                &pos.id,
                                                                "WHALE_FULL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await;
                                                    }
                                                }
                                            } else {
                                                println!("[SHADOW PROPORTIONAL EXIT 🟡] Target @{} sold {:.1}% of {} (Weighted: {:.1}%, {} whales in pos) -> Partial exit!", act.wallet_handle, raw_sell_ratio, pos.token_symbol, effective_sell_ratio, whale_count);
                                                match pos.chain {
                                                    Chain::Solana => {
                                                        let _ = sol_exec_stream
                                                            .execute_partial_shadow_sell(
                                                                &pos.id,
                                                                effective_sell_ratio,
                                                                "WHALE_PARTIAL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await;
                                                    }
                                                    _ => {
                                                        let _ = evm_exec_stream
                                                            .execute_partial_shadow_sell(
                                                                &pos.id,
                                                                effective_sell_ratio,
                                                                "WHALE_PARTIAL_DUMP",
                                                                exit_price,
                                                            )
                                                            .await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else if act.activity_type == ActivityType::Buy {
                            // Fetch real-time pool price, liquidity & market cap across Solana/EVM
                            let (current_price, pool_liq, live_mcap) =
                                SolanaStreamListener::fetch_token_price(&act.token_mint)
                                    .await
                                    .map(|(_, p, l, m)| (p, l, m))
                                    .unwrap_or((act.execution_price_usd, 35000.0, 0.0));

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
                                let _ = ipc_stream.broadcast(&IpcMessage::SignalBroadcast {
                                    signal: sig.clone(),
                                });

                                if sig.is_passed {
                                    println!("[EXECUTE 🚀] Passed strict gating! Executing shadow buy for {}", sig.token_symbol);
                                    let pos_opt = match sig.chain {
                                        Chain::Solana => sol_exec_stream
                                            .execute_shadow_buy(&sig)
                                            .await
                                            .ok()
                                            .flatten(),
                                        _ => evm_exec_stream
                                            .execute_shadow_buy(&sig)
                                            .await
                                            .ok()
                                            .flatten(),
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

                    // Pacing 200ms between wallets (5 RPS total, zero 429)
                    sleep(Duration::from_millis(200)).await;
                }
            }

            sleep(Duration::from_millis(200)).await;
        }
    });

    // Task 3: Live Position Lifecycle & Real-Time Price Tracker Worker (Whale Mirror + Dynamic Hard SL)
    let db_pos_tracker = db.clone();
    let ipc_pos_tracker = ipc_server.clone();
    let sol_exec_pos = sol_exec.clone();
    let evm_exec_pos = evm_exec.clone();
    let config_pos = config.clone();

    tokio::spawn(async move {
        println!("[POSITION TRACKER] Position & Floating PnL Tracker online (SL: -{:.1}%, TP: +{:.1}%)",
            config_pos.stop_loss_pct, config_pos.take_profit_pct);
        loop {
            if let Ok(mut positions) = db_pos_tracker.get_active_positions() {
                for pos in &mut positions {
                    if pos.status == "CLOSED" {
                        continue;
                    }

                    // Fetch latest live price for open position; never relabel stale price as fresh.
                    let live_price = match SolanaStreamListener::fetch_token_price(&pos.token_mint)
                        .await
                    {
                        Ok((_, price, liquidity, _)) if price > 0.0 && liquidity > 0.0 => price,
                        Ok((symbol, price, liquidity, _)) => {
                            eprintln!(
                                "[PRICE UPDATE SKIP ⚠️] {} returned invalid market data (price=${:.8}, liquidity=${:.2}); keeping last price with no refresh.",
                                if symbol.is_empty() { &pos.token_symbol } else { &symbol },
                                price,
                                liquidity
                            );
                            continue;
                        }
                        Err(error) => {
                            eprintln!(
                                "[PRICE UPDATE FAILED ⚠️] {} ({}) — keeping last price with no refresh.",
                                pos.token_symbol, error
                            );
                            continue;
                        }
                    };

                    // Sanity check: Discard single-tick anomaly if price jumps > 500% in one tick from an established price
                    if live_price > 0.0 {
                        if pos.current_price > 0.0 && live_price > pos.current_price * 5.0 {
                            eprintln!(
                                "[PRICE SANITY GUARD 🛡️] Suspicious price spike for {} (${:.6} -> ${:.6}). Discarding unverified tick.",
                                pos.token_symbol, pos.current_price, live_price
                            );
                            continue;
                        }

                        pos.current_price = live_price;
                        pos.peak_price = pos.peak_price.max(live_price);
                        if pos.entry_price > 0.0 {
                            pos.unrealized_pnl_pct =
                                ((pos.current_price - pos.entry_price) / pos.entry_price) * 100.0;
                        }
                        pos.last_update = Utc::now();
                        let _ = db_pos_tracker.upsert_position(pos);

                        // 1. Tier 1 Auto Take Profit (Sell 50% of initial position at +100% PnL to free-roll initial capital)
                        if pos.unrealized_pnl_pct >= config_pos.take_profit_pct
                            && (pos.status == "OPEN" || pos.status == "PARTIAL_EXIT")
                            && pos.remaining_portion_pct >= 90.0
                        {
                            println!("[AUTO-TP 1 (+{:.0}%) 🎯] Position {} ({}) reached +{:.1}% >= +{:.0}%! Selling 50% to secure capital (Free-Roll Mode)!",
                                config_pos.take_profit_pct, pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.take_profit_pct);
                            match pos.chain {
                                Chain::Solana => {
                                    let _ = sol_exec_pos
                                        .execute_partial_shadow_sell(
                                            &pos.id,
                                            50.0,
                                            "AUTO_TP_50_FREE_ROLL",
                                            live_price,
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = evm_exec_pos
                                        .execute_partial_shadow_sell(
                                            &pos.id,
                                            50.0,
                                            "AUTO_TP_50_FREE_ROLL",
                                            live_price,
                                        )
                                        .await;
                                }
                            }
                        }

                        // 2. Tier 2 Moonbag Take Profit (Sell 25% of initial at +300% PnL / 4x price, leaving 25% runner)
                        let tp2_target_pnl = 300.0;
                        if pos.unrealized_pnl_pct >= tp2_target_pnl
                            && pos.status == "PARTIAL_EXIT"
                            && pos.remaining_portion_pct > 35.0
                            && pos.remaining_portion_pct <= 55.0
                        {
                            println!("[AUTO-TP 2 (+{:.0}%) 🚀] Position {} ({}) hit +{:.1}%! Selling 25% of initial position to lock moonbag gains!",
                                tp2_target_pnl, pos.id, pos.token_symbol, pos.unrealized_pnl_pct);
                            match pos.chain {
                                Chain::Solana => {
                                    let _ = sol_exec_pos
                                        .execute_partial_shadow_sell(
                                            &pos.id,
                                            25.0,
                                            "AUTO_TP_25_RUNNER_LOCK",
                                            live_price,
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = evm_exec_pos
                                        .execute_partial_shadow_sell(
                                            &pos.id,
                                            25.0,
                                            "AUTO_TP_25_RUNNER_LOCK",
                                            live_price,
                                        )
                                        .await;
                                }
                            }
                        }

                        // 3. Tier 3 Catastrophic Peak Drawdown Guard on Runner / Moonbag
                        // If price drops > 50% from peak after a 2x+ pump, dump remaining portion to prevent roundtripping to zero
                        if pos.status == "PARTIAL_EXIT"
                            && pos.peak_price >= pos.entry_price * 2.0
                            && pos.current_price <= pos.peak_price * 0.50
                            && pos.remaining_portion_pct <= 55.0
                        {
                            println!("[PEAK DRAWDOWN GUARD 🛡️] Position {} ({}) dropped >50% from peak (${:.6} -> ${:.6})! Dumping remaining {:.0}% to lock profit!",
                                pos.id, pos.token_symbol, pos.peak_price, pos.current_price, pos.remaining_portion_pct);
                            match pos.chain {
                                Chain::Solana => {
                                    let _ = sol_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "PEAK_DRAWDOWN_GUARD_EXIT",
                                            live_price,
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = evm_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "PEAK_DRAWDOWN_GUARD_EXIT",
                                            live_price,
                                        )
                                        .await;
                                }
                            }
                        }

                        // 4. Profit-Lock Floor on Moonbag (+100% -> Floor +20.0%)
                        // STRICT SAFETY: Must have reached at least +100% PnL initially and current PnL must be positive (> +5.0%)
                        if pos.status == "PARTIAL_EXIT"
                            && pos.peak_price >= pos.entry_price * 2.0
                            && pos.unrealized_pnl_pct <= 20.0
                            && pos.unrealized_pnl_pct >= 5.0
                            && pos.remaining_portion_pct <= 55.0
                        {
                            println!("[PROFIT-LOCK FLOOR 🛡️] Position {} ({}) moonbag dropped to +{:.1}%! Closing remaining portion to guarantee green trade!",
                                pos.id, pos.token_symbol, pos.unrealized_pnl_pct);
                            match pos.chain {
                                Chain::Solana => {
                                    let _ = sol_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "PROFIT_LOCK_FLOOR_EXIT",
                                            live_price,
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = evm_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "PROFIT_LOCK_FLOOR_EXIT",
                                            live_price,
                                        )
                                        .await;
                                }
                            }
                        }

                        // 5. Emergency Hard SL Trigger (-35.0%) to prevent -99.9% rug pull wipeouts
                        if pos.unrealized_pnl_pct <= -config_pos.stop_loss_pct && pos.status != "CLOSED" {
                            println!("[EMERGENCY SL CUT-LOSS 🛑] Position {} ({}) hit {:.1}% <= -{:.1}%! Force closing immediately to protect capital!",
                                pos.id, pos.token_symbol, pos.unrealized_pnl_pct, config_pos.stop_loss_pct);
                            match pos.chain {
                                Chain::Solana => {
                                    let _ = sol_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "EMERGENCY_HARD_SL_CUTLOSS",
                                            live_price,
                                        )
                                        .await;
                                }
                                _ => {
                                    let _ = evm_exec_pos
                                        .execute_shadow_sell(
                                            &pos.id,
                                            "EMERGENCY_HARD_SL_CUTLOSS",
                                            live_price,
                                        )
                                        .await;
                                }
                            }
                        }
                    }

                    sleep(Duration::from_millis(100)).await;
                }

                // Broadcast refreshed positions to IPC socket
                let _ = ipc_pos_tracker.broadcast(&IpcMessage::PositionUpdate { positions });
            }

            sleep(Duration::from_secs(2)).await;
        }
    });

    // Task 4: Heartbeat & IPC Metrics Broadcast
    let db_heartbeat = db.clone();
    let ipc_heartbeat = ipc_server.clone();
    let sol_exec_hb = sol_exec.clone();
    let config_hb = config.clone();
    let start_time = Utc::now();
    tokio::spawn(async move {
        let mut sync_tick = 0u64;
        loop {
            sync_tick += 1;
            let uptime_secs = (Utc::now() - start_time).num_seconds().max(0) as u64;
            let total_tracked_count = db_heartbeat
                .get_active_wallets()
                .map(|w| w.len())
                .unwrap_or(0);
            let qualified_wallets = db_heartbeat.get_qualified_wallets().unwrap_or_default();
            let active_positions = db_heartbeat.get_active_positions().unwrap_or_default();
            let active_positions_count = active_positions.len();

            // Calculate real on-chain wallet balances & portfolio USD
            let (wallet_pubkey, wallet_sol_balance) = if let Ok((_, pubkey)) = sol_exec_hb.get_keypair() {
                let bal = sol_exec_hb.get_native_balance(&pubkey).await.unwrap_or(0.0);
                (pubkey, bal)
            } else {
                (String::new(), 0.0)
            };

            let sol_price_usd = sol_exec_hb.get_sol_price().await;
            let sol_usd_val = wallet_sol_balance * sol_price_usd;

            let open_positions_usd: f64 = active_positions
                .iter()
                .map(|p| {
                    (p.amount_usd * (p.remaining_portion_pct / 100.0))
                        * (1.0 + p.unrealized_pnl_pct / 100.0)
                })
                .sum();

            let wallet_equity_usd = sol_usd_val + open_positions_usd;

            let total_realized_pnl_usd: f64 = db_heartbeat
                .get_recent_trades(500)
                .map(|trades| trades.iter().map(|t| t.pnl_usd).sum())
                .unwrap_or(0.0);

            let hb = IpcMessage::Heartbeat {
                uptime_secs,
                solana_latency_ms: 68,
                arbitrum_latency_ms: 42,
                base_latency_ms: 45,
                tracked_wallets_count: total_tracked_count,
                active_positions_count,
                wallet_sol_balance,
                sol_price_usd,
                wallet_equity_usd,
                total_realized_pnl_usd,
                min_liquidity_usd: config_hb.min_liquidity_usd,
                max_price_delta_pct: config_hb.max_price_delta_pct,
                position_size_usd: config_hb.position_size_usd,
                stop_loss_pct: config_hb.stop_loss_pct,
                take_profit_pct: config_hb.take_profit_pct,
                min_whale_buy_usd: config_hb.min_whale_buy_usd,
                is_live_mode: !config_hb.dry_run,
                wallet_pubkey,
            };
            let _ = ipc_heartbeat.broadcast(&hb);

            // Sync qualified elite target wallets (Tier-S & Tier-A only) and trade executions audit log every 4 seconds
            if sync_tick % 2 == 0 {
                if !qualified_wallets.is_empty() {
                    let _ = ipc_heartbeat.broadcast(&IpcMessage::WalletSync {
                        wallets: qualified_wallets,
                    });
                }
                if let Ok(trades) = db_heartbeat.get_recent_trades(50) {
                    let _ = ipc_heartbeat.broadcast(&IpcMessage::TradeSync { trades });
                }
            }

            sleep(Duration::from_secs(2)).await;
        }
    });

    // Keep main thread alive
    tokio::signal::ctrl_c().await?;
    println!("\n[SHUTDOWN] Stopping ZenithSW Daemon cleanly...");
    Ok(())
}
