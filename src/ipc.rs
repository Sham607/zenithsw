use crate::models::{
    ShadowPosition, ShadowSignal, SmartWallet, TrackedToken, TradeRecord, WalletActivity,
};
use crate::radar::RadarProjection;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    Heartbeat {
        uptime_secs: u64,
        #[serde(default)]
        heartbeat_seq: u64,
        solana_latency_ms: u64,
        arbitrum_latency_ms: u64,
        base_latency_ms: u64,
        tracked_wallets_count: usize,
        active_positions_count: usize,
        #[serde(default)]
        wallet_sol_balance: f64,
        #[serde(default)]
        sol_price_usd: f64,
        #[serde(default)]
        wallet_equity_usd: f64,
        #[serde(default)]
        total_realized_pnl_usd: f64,
        #[serde(default)]
        min_liquidity_usd: f64,
        #[serde(default)]
        max_price_delta_pct: f64,
        #[serde(default)]
        position_size_usd: f64,
        #[serde(default)]
        stop_loss_pct: f64,
        #[serde(default)]
        take_profit_pct: f64,
        #[serde(default)]
        shadow_exit_enabled: bool,
        #[serde(default)]
        min_whale_buy_usd: f64,
        #[serde(default)]
        is_live_mode: bool,
        #[serde(default)]
        wallet_pubkey: String,
        #[serde(default)]
        heartbeat_timestamp_unix: i64,
        #[serde(default)]
        solana_rpc_requests: u64,
        #[serde(default)]
        solana_rpc_successes: u64,
        #[serde(default)]
        solana_rpc_rate_limited: u64,
        #[serde(default)]
        solana_rpc_failures: u64,
        #[serde(default)]
        robinhood_rpc_requests: u64,
        #[serde(default)]
        robinhood_rpc_successes: u64,
        #[serde(default)]
        robinhood_rpc_rate_limited: u64,
        #[serde(default)]
        robinhood_rpc_failures: u64,
        #[serde(default)]
        qualified_wallets_count: usize,
        #[serde(default)]
        polled_wallets_count: usize,
        #[serde(default)]
        global_scanned_wallets_count: usize,
        #[serde(default)]
        candidate_wallets_count: usize,
    },
    WalletSync {
        wallets: Vec<SmartWallet>,
    },
    TrackedTokensUpdate {
        tokens: Vec<TrackedToken>,
    },
    RadarSync {
        projections: Vec<RadarProjection>,
    },
    ActivityBroadcast {
        activity: WalletActivity,
    },
    ActivitySync {
        activities: Vec<WalletActivity>,
    },
    SignalBroadcast {
        signal: ShadowSignal,
    },
    SignalSync {
        signals: Vec<ShadowSignal>,
    },
    PositionUpdate {
        positions: Vec<ShadowPosition>,
    },
    TradeSync {
        trades: Vec<TradeRecord>,
    },
    EngineLog {
        level: String,
        message: String,
        timestamp: String,
    },
}

pub struct IpcServer {
    socket_path: String,
    tx: broadcast::Sender<String>,
    latest_heartbeat: Arc<Mutex<Option<IpcMessage>>>,
}

impl IpcServer {
    pub fn new(socket_path: &str) -> Self {
        let (tx, _) = broadcast::channel(2000);
        Self {
            socket_path: socket_path.to_string(),
            tx,
            latest_heartbeat: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_sender(&self) -> broadcast::Sender<String> {
        self.tx.clone()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }

    pub async fn start(&self) -> Result<()> {
        if Path::new(&self.socket_path).exists() {
            let _ = tokio::fs::remove_file(&self.socket_path).await;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        let tx = self.tx.clone();
        let latest_hb = self.latest_heartbeat.clone();

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut rx = tx.subscribe();
                let latest_hb_inner = latest_hb.clone();
                tokio::spawn(async move {
                    let (_, mut write_half) = stream.into_split();

                    // Kirim heartbeat terbaru jika sudah ada. Jangan drop client bila belum ada
                    // heartbeat pertama, melainkan teruskan streaming broadcast.
                    let initial_hb = latest_hb_inner.lock().ok().and_then(|guard| guard.clone());
                    if let Some(initial_hb) = initial_hb {
                        if let Ok(json) = serde_json::to_string(&initial_hb) {
                            let _ = write_half.write_all(json.as_bytes()).await;
                            let _ = write_half.write_all(b"\n").await;
                            let _ = write_half.flush().await;
                        }
                    }

                    loop {
                        match rx.recv().await {
                            Ok(msg_str) => {
                                if write_half.write_all(msg_str.as_bytes()).await.is_err() {
                                    break;
                                }
                                if write_half.write_all(b"\n").await.is_err() {
                                    break;
                                }
                                let _ = write_half.flush().await;
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                eprintln!("[IPC LAG] Client lagged by {n} messages; catching up to latest broadcast stream");
                                continue;
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                break;
                            }
                        }
                    }
                });
            }
        });

        Ok(())
    }

    pub fn broadcast(&self, msg: &IpcMessage) -> Result<()> {
        if matches!(msg, IpcMessage::Heartbeat { .. }) {
            if let Ok(mut guard) = self.latest_heartbeat.lock() {
                *guard = Some(msg.clone());
            }
        }
        let json_str = serde_json::to_string(msg)?;
        let _ = self.tx.send(json_str);
        Ok(())
    }
}

pub struct IpcClient {
    socket_path: String,
}

impl IpcClient {
    pub fn new(socket_path: &str) -> Self {
        Self {
            socket_path: socket_path.to_string(),
        }
    }

    pub async fn connect_and_listen<F, C>(&self, mut on_connect: C, mut callback: F) -> Result<()>
    where
        C: FnMut() + Send + 'static,
        F: FnMut(IpcMessage) + Send + 'static,
    {
        let stream = UnixStream::connect(&self.socket_path).await?;
        on_connect();

        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(msg) = serde_json::from_str::<IpcMessage>(&line) {
                callback(msg);
            }
        }

        Ok(())
    }

    async fn connect_and_listen_once<F, C>(&self, mut on_connect: C, mut callback: F) -> Result<()>
    where
        C: FnMut() + Send,
        F: FnMut(IpcMessage) + Send,
    {
        let stream = UnixStream::connect(&self.socket_path).await?;
        on_connect();
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(msg) = serde_json::from_str::<IpcMessage>(&line) {
                callback(msg);
            }
        }
        Ok(())
    }

    /// Mencoba koneksi ulang dengan batas attempt eksplisit.
    /// EOF setelah koneksi valid dianggap disconnect dan akan di-retry.
    pub async fn connect_and_listen_with_retry<F, C>(
        &self,
        max_attempts: usize,
        retry_delay: std::time::Duration,
        mut on_connect: C,
        mut callback: F,
    ) -> Result<()>
    where
        C: FnMut() + Send + 'static,
        F: FnMut(IpcMessage) + Send + 'static,
    {
        if max_attempts == 0 {
            anyhow::bail!("max_attempts IPC harus lebih besar dari nol");
        }
        let mut last_error = None;
        for attempt in 0..max_attempts {
            match self
                .connect_and_listen_once(&mut on_connect, &mut callback)
                .await
            {
                Ok(()) => last_error = None,
                Err(error) => last_error = Some(error),
            }
            if attempt + 1 < max_attempts {
                tokio::time::sleep(retry_delay).await;
            }
        }
        if let Some(error) = last_error {
            Err(error)
        } else {
            Ok(())
        }
    }
}
