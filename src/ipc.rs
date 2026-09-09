use crate::models::{ShadowPosition, ShadowSignal, SmartWallet, WalletActivity};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
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
    ActivityBroadcast {
        activity: WalletActivity,
    },
    SignalBroadcast {
        signal: ShadowSignal,
    },
    PositionUpdate {
        positions: Vec<ShadowPosition>,
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
}

impl IpcServer {
    pub fn new(socket_path: &str) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            socket_path: socket_path.to_string(),
            tx,
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

        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let mut rx = tx.subscribe();
                tokio::spawn(async move {
                    let (_, mut write_half) = stream.into_split();
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

    pub async fn connect_and_listen<F>(&self, mut callback: F) -> Result<()>
    where
        F: FnMut(IpcMessage) + Send + 'static,
    {
        let stream = UnixStream::connect(&self.socket_path).await?;
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
