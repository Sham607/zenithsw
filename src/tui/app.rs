use crate::models::{
    ShadowPosition, ShadowSignal, SmartWallet, TrackedToken, TradeRecord, WalletActivity,
};

pub struct App {
    pub current_tab: usize,
    pub wallets: Vec<SmartWallet>,
    pub tracked_tokens: Vec<TrackedToken>,
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
    pub min_whale_buy_usd: f64,
    pub is_live_mode: bool,
    pub wallet_pubkey: String,
    pub uptime_secs: u64,
}

impl App {
    pub fn new() -> Self {
        Self {
            current_tab: 0,
            wallets: Vec::new(),
            tracked_tokens: Vec::new(),
            activities: Vec::new(),
            signals: Vec::new(),
            positions: Vec::new(),
            trades: Vec::new(),
            logs: Vec::new(),
            solana_latency_ms: 45,
            arbitrum_latency_ms: 38,
            base_latency_ms: 42,
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
            min_whale_buy_usd: 0.0,
            is_live_mode: false,
            wallet_pubkey: String::new(),
            uptime_secs: 0,
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
