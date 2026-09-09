use crate::models::{ShadowPosition, ShadowSignal, SmartWallet, TradeRecord, WalletActivity};

pub struct App {
    pub current_tab: usize,
    pub wallets: Vec<SmartWallet>,
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
}

impl App {
    pub fn new() -> Self {
        Self {
            current_tab: 0,
            wallets: Vec::new(),
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
        }
    }

    pub fn next_tab(&mut self) {
        self.current_tab = (self.current_tab + 1) % 4;
    }

    pub fn prev_tab(&mut self) {
        if self.current_tab == 0 {
            self.current_tab = 3;
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
