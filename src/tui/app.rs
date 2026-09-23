use crate::models::{
    ShadowPosition, ShadowSignal, SmartWallet, TrackedToken, TradeRecord, WalletActivity,
};
use std::time::Instant;

pub struct App {
    pub current_tab: usize,
    pub wallets: Vec<SmartWallet>,
    pub tracked_tokens: Vec<TrackedToken>,
    pub radar_projections: Vec<crate::radar::RadarProjection>,
    pub activities: Vec<WalletActivity>,
    pub signals: Vec<ShadowSignal>,
    pub positions: Vec<ShadowPosition>,
    pub trades: Vec<TradeRecord>,
    pub logs: Vec<String>,
    pub solana_latency_ms: u64,
    pub arbitrum_latency_ms: u64,
    pub base_latency_ms: u64,
    pub is_connected_ipc: bool,
    pub should_quit: bool,

    // Real On-Chain Wallet Equity & Portfolio
    pub wallet_sol_balance: f64,
    pub sol_price_usd: f64,
    pub wallet_equity_usd: f64,
    pub total_realized_pnl_usd: f64,

    // Live Synchronized Parameters (from Engine Config)
    pub min_liquidity_usd: f64,
    pub max_price_delta_pct: f64,
    pub position_size_usd: f64,
    pub stop_loss_pct: f64,
    pub take_profit_pct: f64,
    pub shadow_exit_enabled: bool,
    pub min_whale_buy_usd: f64,
    pub is_live_mode: bool,
    pub wallet_pubkey: String,
    pub uptime_secs: u64,
    pub heartbeat_timestamp_unix: i64,
    pub heartbeat_received_at: Option<Instant>,
    pub heartbeat_seq: u64,
    pub solana_rpc_requests: u64,
    pub solana_rpc_successes: u64,
    pub solana_rpc_rate_limited: u64,
    pub solana_rpc_failures: u64,
    pub robinhood_rpc_requests: u64,
    pub robinhood_rpc_successes: u64,
    pub robinhood_rpc_rate_limited: u64,
    pub robinhood_rpc_failures: u64,
    pub qualified_wallets_count: usize,
    pub polled_wallets_count: usize,
    pub global_scanned_wallets_count: usize,
    pub candidate_wallets_count: usize,
    pub radar_selected_index: usize,
    pub clipboard_notice: Option<(String, Instant)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn heartbeat_is_fresh(&self) -> bool {
        self.is_connected_ipc
            && self
                .heartbeat_received_at
                .map(|received| received.elapsed().as_secs() <= 10)
                .unwrap_or(false)
    }

    pub fn new() -> Self {
        Self {
            current_tab: 0,
            wallets: Vec::new(),
            tracked_tokens: Vec::new(),
            radar_projections: Vec::new(),
            activities: Vec::new(),
            signals: Vec::new(),
            positions: Vec::new(),
            trades: Vec::new(),
            logs: Vec::new(),
            solana_latency_ms: 0,
            arbitrum_latency_ms: 0,
            base_latency_ms: 0,
            is_connected_ipc: false,
            should_quit: false,
            wallet_sol_balance: 0.0,
            sol_price_usd: 0.0,
            wallet_equity_usd: 0.0,
            total_realized_pnl_usd: 0.0,
            min_liquidity_usd: 0.0,
            max_price_delta_pct: 0.0,
            position_size_usd: 0.0,
            stop_loss_pct: 0.0,
            take_profit_pct: 0.0,
            shadow_exit_enabled: true,
            min_whale_buy_usd: 0.0,
            is_live_mode: false,
            wallet_pubkey: String::new(),
            uptime_secs: 0,
            heartbeat_timestamp_unix: 0,
            heartbeat_received_at: None,
            heartbeat_seq: 0,
            solana_rpc_requests: 0,
            solana_rpc_successes: 0,
            solana_rpc_rate_limited: 0,
            solana_rpc_failures: 0,
            robinhood_rpc_requests: 0,
            robinhood_rpc_successes: 0,
            robinhood_rpc_rate_limited: 0,
            robinhood_rpc_failures: 0,
            qualified_wallets_count: 0,
            polled_wallets_count: 0,
            global_scanned_wallets_count: 0,
            candidate_wallets_count: 0,
            radar_selected_index: 0,
            clipboard_notice: None,
        }
    }

    pub fn radar_select_next(&mut self) {
        if !self.radar_projections.is_empty() {
            if self.radar_selected_index + 1 < self.radar_projections.len() {
                self.radar_selected_index += 1;
            } else {
                self.radar_selected_index = 0;
            }
        }
    }

    pub fn radar_select_prev(&mut self) {
        if !self.radar_projections.is_empty() {
            if self.radar_selected_index == 0 {
                self.radar_selected_index = self.radar_projections.len().saturating_sub(1);
            } else {
                self.radar_selected_index -= 1;
            }
        }
    }

    pub fn selected_radar_projection(&self) -> Option<&crate::radar::RadarProjection> {
        if self.radar_projections.is_empty() {
            None
        } else {
            self.radar_projections
                .get(self.radar_selected_index)
                .or_else(|| self.radar_projections.first())
        }
    }

    pub fn next_tab(&mut self) {
        self.current_tab = (self.current_tab + 1) % 5;
    }

    pub fn prev_tab(&mut self) {
        if self.current_tab == 0 {
            self.current_tab = 4;
        } else {
            self.current_tab -= 1;
        }
    }

    pub fn add_log(&mut self, log: String) {
        self.logs.push(log);
        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }
}
