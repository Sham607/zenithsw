//! M9 runtime synchronization harness.
//!
//! Starts the real engine process with an isolated temporary DB/socket and
//! DRY_RUN=true, connects the IPC client, and asserts the documented
//! Engine → IPC → DB synchronization contract:
//!
//! - IPC is reachable, but no message is delivered before a real heartbeat.
//!   The server closes pre-heartbeat connections instead of inventing data.
//! - Heartbeat carries a monotonic `heartbeat_seq` from the engine loop.
//! - `is_live_mode` stays false because DRY_RUN=true.
//! - WalletSync, when delivered, only contains Tier-S/Tier-A qualified wallets.
//!
//! The harness never signs or broadcasts anything: all chains are disabled and
//! the engine only needs the DB and the IPC socket to run.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use zenithsw::ipc::{IpcClient, IpcMessage};
use zenithsw::models::WalletTier;

const BIN_NAME: &str = "zenithsw-engine";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(2500);
const HEARTBEAT_BUDGET: Duration = Duration::from_secs(60);

struct EngineProcess {
    child: Child,
    socket_path: PathBuf,
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket_path);
        let _ = std::fs::remove_file(format!("{}.lock", self.socket_path.display()));
    }
}

fn start_engine(
    workspace: &tempfile::TempDir,
) -> Result<EngineProcess, Box<dyn std::error::Error>> {
    let engine_path = assert_cmd::cargo::cargo_bin(BIN_NAME);
    let socket_path = workspace.path().join("harness.sock");
    let trading_db = workspace.path().join("trading_state.db");
    let wallet_db = workspace.path().join("wallet_registry.db");
    let candidate_db = workspace.path().join("candidate_wallet_registry.db");
    // Unreachable endpoints: the engine must stay alive without them.
    let solana_rpc = "https://127.0.0.1:1";
    let robinhood_rpc = "http://127.0.0.1:1";

    let child = Command::new(engine_path)
        .env_clear()
        .envs([
            ("SOLANA_RPC_URL", solana_rpc),
            ("SOLANA_WS_URL", "ws://127.0.0.1:1"),
            ("ROBINHOOD_RPC_URL", robinhood_rpc),
            ("ROBINHOOD_WS_URL", "ws://127.0.0.1:1"),
            ("DRY_RUN", "true"),
            ("ENABLE_SOLANA", "false"),
            ("ENABLE_ROBINHOOD", "false"),
            ("ENABLE_BASE", "false"),
            ("TRADING_DB_PATH", trading_db.to_str().unwrap()),
            ("WALLET_DB_PATH", wallet_db.to_str().unwrap()),
            ("CANDIDATE_DB_PATH", candidate_db.to_str().unwrap()),
            ("IPC_SOCKET_PATH", socket_path.to_str().unwrap()),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(EngineProcess { child, socket_path })
}

async fn wait_for_socket(socket_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if socket_path.exists() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("engine socket did not appear in time".into())
}

async fn connect_ipc(
    socket_path: &Path,
) -> Result<tokio::sync::mpsc::Receiver<IpcMessage>, Box<dyn std::error::Error>> {
    let (tx, rx) = tokio::sync::mpsc::channel(4096);
    let client = IpcClient::new(socket_path.to_str().unwrap());
    tokio::spawn(async move {
        let _ = client
            .connect_and_listen(
                || {},
                move |msg| {
                    // try_send: the callback runs inside a tokio task, blocking
                    // sends are forbidden; a full channel is not a harness failure.
                    let _ = tx.try_send(msg);
                },
            )
            .await;
    });
    Ok(rx)
}

#[tokio::test]
async fn runtime_harness_engine_ipc_db_synchronization() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let mut engine = start_engine(&workspace).expect("engine spawn");

    // The engine process must stay alive: an early exit means fail-closed
    // config or DB failure, which this harness is not allowed to mask.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine process exited before reaching the IPC loop"
    );

    wait_for_socket(&engine.socket_path).await.expect("socket");

    // Contract 1: connection alone must not deliver any message (no synthetic
    // heartbeat). The server may close the connection before the first real
    // heartbeat — that is the documented behavior, not a failure.
    let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
    let first = tokio::time::timeout(CONNECT_TIMEOUT, rx.recv()).await;
    match first {
        Err(_) | Ok(None) => {}
        Ok(Some(_)) => panic!("IPC delivered a message before the engine heartbeat"),
    }
    drop(rx);

    // Contract 2: reconnect like the real TUI does until a real heartbeat
    // arrives, then require a strictly increasing `heartbeat_seq` and
    // DRY_RUN semantics on every heartbeat received.
    let deadline = Instant::now() + HEARTBEAT_BUDGET;
    let mut last_seq: Option<u64> = None;
    let mut monotonic_proven = false;
    while Instant::now() < deadline && !monotonic_proven {
        let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
        loop {
            match tokio::time::timeout(CONNECT_TIMEOUT, rx.recv()).await {
                Ok(Some(IpcMessage::Heartbeat {
                    heartbeat_seq,
                    is_live_mode,
                    ..
                })) => {
                    assert!(
                        !is_live_mode,
                        "engine must run with DRY_RUN=true; is_live_mode must stay false"
                    );
                    if let Some(previous_seq) = last_seq.replace(heartbeat_seq) {
                        assert!(
                            heartbeat_seq > previous_seq,
                            "heartbeat_seq must increase: {previous_seq} -> {heartbeat_seq}"
                        );
                        monotonic_proven = true;
                        break;
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => break, // closed before/after heartbeat; reconnect
                Err(_) => break,   // quiet window; reconnect
            }
        }
        drop(rx);
        // Give the engine a beat before the next reconnect attempt; also lets
        // the first heartbeat land so the reconnect receives `latest_heartbeat`.
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        monotonic_proven,
        "no strictly increasing heartbeat pair within budget"
    );

    // Contract 3: observe one sync window; any WalletSync must contain only
    // Tier-S/Tier-A qualified wallets (DB tier contract), any heartbeat must
    // keep dry-run semantics.
    let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
    let window = Instant::now() + Duration::from_secs(6);
    while Instant::now() < window {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(IpcMessage::WalletSync { wallets })) => {
                for wallet in &wallets {
                    assert!(
                        matches!(wallet.tier, WalletTier::TierS | WalletTier::TierA),
                        "WalletSync delivered non-qualified tier: {:?}",
                        wallet.tier
                    );
                }
            }
            Ok(Some(IpcMessage::Heartbeat { is_live_mode, .. })) => {
                assert!(!is_live_mode, "heartbeat claimed live mode during DRY_RUN");
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => break,
        }
    }

    // The engine must still be alive at the end (no crash mid-harness).
    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine process exited during the harness run"
    );
}
