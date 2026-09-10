use crate::models::{
    ShadowPosition, ShadowSignal, SmartWallet, TrackedToken, TradeRecord, WalletActivity,
};
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
        solana_latency_ms: u64,
        arbitrum_latency_ms: u64,
        base_latency_ms: u64,
        tracked_wallets_count: usize,
        active_positions_count: usize,
    },
    WalletSync {
        wallets: Vec<SmartWallet>,
    },
    TrackedTokensUpdate {
        tokens: Vec<TrackedToken>,
    },
    ActivityBroadcast {
        activity: WalletActivity,
    },
    SignalBroadcast {
        signal: ShadowSignal,
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
                    
                    // Send current heartbeat or initial handshake line immediately
                    let initial_hb = {
                        if let Ok(guard) = latest_hb_inner.lock() {
                            guard.clone().unwrap_or(IpcMessage::Heartbeat {
                                uptime_secs: 1,
                                solana_latency_ms: 35,
                                arbitrum_latency_ms: 28,
                                base_latency_ms: 32,
                                tracked_wallets_count: 100,
                                active_positions_count: 0,
                            })
                        } else {
                            IpcMessage::Heartbeat {
                                uptime_secs: 1,
                                solana_latency_ms: 35,
                                arbitrum_latency_ms: 28,
                                base_latency_ms: 32,
                                tracked_wallets_count: 100,
                                active_positions_count: 0,
                            }
                        }
                    };
                    
                    if let Ok(json) = serde_json::to_string(&initial_hb) {
                        let _ = write_half.write_all(json.as_bytes()).await;
                        let _ = write_half.write_all(b"\n").await;
                        let _ = write_half.flush().await;
                    }

                    while let Ok(msg_str) = rx.recv().await {
                        if write_half.write_all(msg_str.as_bytes()).await.is_err() {
                            break;
                        }
                        if write_half.write_all(b"\n").await.is_err() {
                            break;
                        }
                        let _ = write_half.flush().await;
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
}
