//! Testable IPC snapshot state machine shared by the real TUI and the
//! Engine → IPC → TUI integration harness.
//!
//! The real TUI process (`src/main.rs`) and the harness both call
//! `apply_ipc_message`, so tests exercise the exact production handler:
//! chain filtering rejects out-of-scope chains, snapshot messages replace
//! previous state atomically, and low-latency event messages append.

use crate::config::Config;
use crate::ipc::IpcMessage;
use crate::models::Chain;
use crate::tui::App;

/// Chain filter shared by every snapshot/event handler branch.
fn chain_enabled(chain: Chain, config: &Config) -> bool {
    match chain {
        Chain::Solana => config.enable_solana,
        Chain::Robinhood => config.enable_robinhood,
        Chain::Base => config.enable_base,
        Chain::Arbitrum | Chain::Ethereum => false,
    }
}

/// Applies one IPC message to the TUI state with production semantics:
/// - `Heartbeat` updates connection/freshness/scalar fields;
/// - snapshot messages replace state after chain filtering (empty stays empty);
/// - `ActivityBroadcast`/`SignalBroadcast` append with a 100-row cap.
pub fn apply_ipc_message(app: &mut App, msg: IpcMessage, config: &Config) {
    match msg {
        IpcMessage::Heartbeat {
            uptime_secs,
            heartbeat_seq,
            solana_latency_ms,
            arbitrum_latency_ms,
            base_latency_ms,
            wallet_sol_balance,
            sol_price_usd,
            wallet_equity_usd,
            total_realized_pnl_usd,
            min_liquidity_usd,
            max_price_delta_pct,
            position_size_usd,
            stop_loss_pct,
            take_profit_pct,
            shadow_exit_enabled,
            min_whale_buy_usd,
            is_live_mode,
            wallet_pubkey,
            heartbeat_timestamp_unix,
            solana_rpc_requests,
            solana_rpc_successes,
            solana_rpc_rate_limited,
            solana_rpc_failures,
            robinhood_rpc_requests,
            robinhood_rpc_successes,
            robinhood_rpc_rate_limited,
            robinhood_rpc_failures,
            qualified_wallets_count,
            polled_wallets_count,
            global_scanned_wallets_count,
            candidate_wallets_count,
            tracked_wallets_count: _,
            active_positions_count: _,
        } => {
            app.is_connected_ipc = true;
            app.heartbeat_received_at = Some(std::time::Instant::now());
            app.heartbeat_timestamp_unix = heartbeat_timestamp_unix;
            app.heartbeat_seq = heartbeat_seq;
            app.uptime_secs = uptime_secs;
            app.solana_latency_ms = solana_latency_ms;
            app.arbitrum_latency_ms = arbitrum_latency_ms;
            app.base_latency_ms = base_latency_ms;
            app.wallet_sol_balance = wallet_sol_balance;
            app.sol_price_usd = sol_price_usd;
            app.wallet_equity_usd = wallet_equity_usd;
            app.total_realized_pnl_usd = total_realized_pnl_usd;
            app.min_liquidity_usd = min_liquidity_usd;
            app.max_price_delta_pct = max_price_delta_pct;
            app.position_size_usd = position_size_usd;
            app.stop_loss_pct = stop_loss_pct;
            app.take_profit_pct = take_profit_pct;
            app.shadow_exit_enabled = shadow_exit_enabled;
            app.min_whale_buy_usd = min_whale_buy_usd;
            app.is_live_mode = is_live_mode;
            app.wallet_pubkey = wallet_pubkey;
            app.solana_rpc_requests = solana_rpc_requests;
            app.solana_rpc_successes = solana_rpc_successes;
            app.solana_rpc_rate_limited = solana_rpc_rate_limited;
            app.solana_rpc_failures = solana_rpc_failures;
            app.robinhood_rpc_requests = robinhood_rpc_requests;
            app.robinhood_rpc_successes = robinhood_rpc_successes;
            app.robinhood_rpc_rate_limited = robinhood_rpc_rate_limited;
            app.robinhood_rpc_failures = robinhood_rpc_failures;
            app.qualified_wallets_count = qualified_wallets_count;
            app.polled_wallets_count = polled_wallets_count;
            app.global_scanned_wallets_count = global_scanned_wallets_count;
            app.candidate_wallets_count = candidate_wallets_count;
        }
        IpcMessage::WalletSync { mut wallets } => {
            wallets.retain(|wallet| chain_enabled(wallet.chain, config));
            app.wallets = wallets;
        }
        IpcMessage::TrackedTokensUpdate { mut tokens } => {
            tokens.retain(|token| chain_enabled(token.chain, config));
            app.tracked_tokens = tokens;
        }
        IpcMessage::RadarSync { mut projections } => {
            projections.retain(|projection| chain_enabled(projection.chain, config));
            app.radar_projections = projections;
            if app.radar_selected_index >= app.radar_projections.len()
                && !app.radar_projections.is_empty()
            {
                app.radar_selected_index = app.radar_projections.len() - 1;
            }
        }
        IpcMessage::ActivityBroadcast { activity } => {
            if chain_enabled(activity.chain, config) {
                app.activities.insert(0, activity);
                if app.activities.len() > 100 {
                    app.activities.pop();
                }
            }
        }
        IpcMessage::ActivitySync { mut activities } => {
            activities.retain(|activity| chain_enabled(activity.chain, config));
            app.activities = activities;
        }
        IpcMessage::SignalBroadcast { signal } => {
            if chain_enabled(signal.chain, config) {
                app.signals.insert(0, signal);
                if app.signals.len() > 100 {
                    app.signals.pop();
                }
            }
        }
        IpcMessage::SignalSync { mut signals } => {
            signals.retain(|signal| chain_enabled(signal.chain, config));
            app.signals = signals;
        }
        IpcMessage::PositionUpdate { mut positions } => {
            positions.retain(|position| chain_enabled(position.chain, config));
            app.positions = positions;
        }
        IpcMessage::TradeSync { mut trades } => {
            trades.retain(|trade| chain_enabled(trade.chain, config));
            app.trades = trades;
        }
        IpcMessage::EngineLog { message, .. } => {
            app.add_log(message);
        }
    }
}
