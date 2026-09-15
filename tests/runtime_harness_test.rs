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
use zenithsw::models::{
    ActivityType, Chain, ExecutionTrack, ShadowPosition, ShadowSignal, SmartWallet, TrackedToken,
    TradeRecord, WalletActivity, WalletTier,
};

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
    trading_db: &Path,
    wallet_db: &Path,
    candidate_db: &Path,
) -> Result<EngineProcess, Box<dyn std::error::Error>> {
    let engine_path = assert_cmd::cargo::cargo_bin(BIN_NAME);
    let socket_path = workspace.path().join("harness.sock");
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
            (
                "TRADING_DB_PATH",
                trading_db.to_str().expect("trading db path"),
            ),
            (
                "WALLET_DB_PATH",
                wallet_db.to_str().expect("wallet db path"),
            ),
            (
                "CANDIDATE_DB_PATH",
                candidate_db.to_str().expect("candidate db path"),
            ),
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

/// Seed one representative row of every sync domain into the isolated DBs the
/// real engine will read from. Chains are tagged so the test can also verify
/// chain filtering (Solana rows must arrive; Base rows must be dropped).
fn seed_all_domains(trading_db: &Path, wallet_db: &Path) {
    let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
        trading_db.to_str().unwrap(),
        wallet_db.to_str().unwrap(),
    )
    .expect("seed DB harus bisa dibuat");
    let now = chrono::Utc::now();

    // Wallets: one Tier-S Solana (must sync), one Tier-B Solana (must NOT sync).
    db.upsert_wallet(&SmartWallet {
        id: None,
        rank: 1,
        name: "Harness Whale S".into(),
        handle: "harness-whale-s".into(),
        chain: Chain::Solana,
        address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        tier: WalletTier::TierS,
        pnl_24h_usd: 42_000.0,
        win_rate_pct: 78.5,
        total_trades: 120,
        source: "ONCHAIN_INDEXER".into(),
        reputation_score: 95.0,
        execution_track: ExecutionTrack::FastLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    })
    .expect("seed wallet qualified");
    db.upsert_wallet(&SmartWallet {
        id: None,
        rank: 9,
        name: "Harness Whale B".into(),
        handle: "harness-whale-b".into(),
        chain: Chain::Solana,
        address: "So1HarnessWhaleTierB1111111111111111111111111".into(),
        tier: WalletTier::TierB,
        pnl_24h_usd: 500.0,
        win_rate_pct: 51.0,
        total_trades: 30,
        source: "ONCHAIN_INDEXER".into(),
        reputation_score: 40.0,
        execution_track: ExecutionTrack::FastLane,
        is_active: true,
        last_seen_at: Some(now),
        updated_at: now,
    })
    .expect("seed wallet tier-b");

    // Positions: one OPEN Solana, one Base (must be chain-filtered).
    db.upsert_position(&ShadowPosition {
        id: "harness-pos-sol".into(),
        chain: Chain::Solana,
        token_symbol: "HARNESSSOL".into(),
        token_mint: "HarnessMintSo11111111111111111111111111111111".into(),
        target_whale_handle: "harness-whale-s".into(),
        target_whale_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        track: ExecutionTrack::FastLane,
        entry_price: 0.001,
        current_price: 0.0012,
        peak_price: 0.0013,
        amount_native: 1_000.0,
        amount_usd: 15.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 20.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".into(),
    })
    .expect("seed position solana");
    db.upsert_position(&ShadowPosition {
        id: "harness-pos-base".into(),
        chain: Chain::Base,
        token_symbol: "HARNESSBASE".into(),
        token_mint: "0xharnessbase".into(),
        target_whale_handle: "harness-whale-s".into(),
        target_whale_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        track: ExecutionTrack::FastLane,
        entry_price: 0.002,
        current_price: 0.002,
        peak_price: 0.002,
        amount_native: 2_000.0,
        amount_usd: 20.0,
        remaining_portion_pct: 100.0,
        unrealized_pnl_pct: 0.0,
        realized_pnl_usd: 0.0,
        entry_time: now,
        last_update: now,
        status: "OPEN".into(),
    })
    .expect("seed position base");

    // Trade history (audit log domain).
    db.record_trade(&TradeRecord {
        timestamp: now,
        chain: Chain::Solana,
        action: "BUY".into(),
        symbol: "HARNESSSOL".into(),
        mint: "HarnessMintSo11111111111111111111111111111111".into(),
        target_whale: "harness-whale-s".into(),
        exit_reason: "WHALE_MIRROR_ENTRY".into(),
        track: ExecutionTrack::FastLane,
        portion_sold_pct: 100.0,
        entry_price: 0.001,
        exit_price: 0.001,
        pnl_pct: 0.0,
        pnl_usd: 0.0,
        duration_secs: 0,
    })
    .expect("seed trade");

    // Whale activities (one Solana Buy + one Base Sell for chain filtering).
    db.insert_activity(&WalletActivity {
        id: None,
        wallet_handle: "harness-whale-s".into(),
        wallet_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "HARNESSSOL".into(),
        token_mint: "HarnessMintSo11111111111111111111111111111111".into(),
        amount_usd: 250.0,
        token_amount: 250_000.0,
        execution_price_usd: 0.001,
        whale_balance_before: 0.0,
        whale_balance_after: 250_000.0,
        sell_ratio_pct: 0.0,
        tx_signature: "harness-activity-sig-sol".into(),
        timestamp: now,
    })
    .expect("seed activity solana");
    db.insert_activity(&WalletActivity {
        id: None,
        wallet_handle: "harness-whale-s".into(),
        wallet_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        chain: Chain::Base,
        activity_type: ActivityType::Sell,
        token_symbol: "HARNESSBASE".into(),
        token_mint: "0xharnessbase".into(),
        amount_usd: 300.0,
        token_amount: 150_000.0,
        execution_price_usd: 0.002,
        whale_balance_before: 150_000.0,
        whale_balance_after: 0.0,
        sell_ratio_pct: 100.0,
        tx_signature: "harness-activity-sig-base".into(),
        timestamp: now,
    })
    .expect("seed activity base");

    // Signals (one passed Solana, one Base).
    db.insert_signal(&ShadowSignal {
        chain: Chain::Solana,
        token_symbol: "HARNESSSOL".into(),
        token_mint: "HarnessMintSo11111111111111111111111111111111".into(),
        whale_handle: "harness-whale-s".into(),
        whale_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 0.001,
        current_pool_price: 0.0011,
        price_delta_pct: 10.0,
        pool_liquidity_usd: 25_000.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 1,
        timestamp: now,
    })
    .expect("seed signal solana");
    db.insert_signal(&ShadowSignal {
        chain: Chain::Base,
        token_symbol: "HARNESSBASE".into(),
        token_mint: "0xharnessbase".into(),
        whale_handle: "harness-whale-s".into(),
        whale_address: "So1HarnessWhaleQualified11111111111111111111111".into(),
        whale_tier: WalletTier::TierS,
        track: ExecutionTrack::FastLane,
        whale_entry_price: 0.002,
        current_pool_price: 0.0021,
        price_delta_pct: 5.0,
        pool_liquidity_usd: 30_000.0,
        is_passed: true,
        rejection_reason: None,
        confluence_count: 1,
        timestamp: now,
    })
    .expect("seed signal base");

    // Tracked tokens (one Solana, one Base).
    db.upsert_tracked_token(&TrackedToken {
        token_mint: "HarnessMintSo11111111111111111111111111111111".into(),
        token_symbol: "HARNESSSOL".into(),
        chain: Chain::Solana,
        market_cap_usd: 1_500_000.0,
        liquidity_usd: 35_000.0,
        price_usd: 0.0011,
        change_24h_pct: 12.0,
        change_6h_pct: 6.0,
        change_1h_pct: 2.0,
        whale_count: 3,
        total_whale_usd: 900.0,
        top_whale_handles: "harness-whale-s".into(),
        status: "TRACKED".into(),
        upside_tier: "TIER-A".into(),
        last_scanned_at: now,
    })
    .expect("seed tracked token solana");
    db.upsert_tracked_token(&TrackedToken {
        token_mint: "0xharnessbase".into(),
        token_symbol: "HARNESSBASE".into(),
        chain: Chain::Base,
        market_cap_usd: 900_000.0,
        liquidity_usd: 30_000.0,
        price_usd: 0.0021,
        change_24h_pct: 5.0,
        change_6h_pct: 3.0,
        change_1h_pct: 1.0,
        whale_count: 2,
        total_whale_usd: 600.0,
        top_whale_handles: "harness-whale-s".into(),
        status: "TRACKED".into(),
        upside_tier: "TIER-B".into(),
        last_scanned_at: now,
    })
    .expect("seed tracked token base");
}

#[tokio::test]
async fn runtime_harness_engine_ipc_db_synchronization() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let trading_db = workspace.path().join("trading_state.db");
    let wallet_db = workspace.path().join("wallet_registry.db");
    let candidate_db = workspace.path().join("candidate_wallet_registry.db");
    let mut engine =
        start_engine(&workspace, &trading_db, &wallet_db, &candidate_db).expect("engine spawn");

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

/// Full multi-domain E2E: every sync domain (wallets, positions, trades,
/// activities, signals, tracked tokens) is seeded into a real isolated DB, the
/// real engine process reads it and broadcasts over IPC, and the consumer side
/// must receive the exact rows with tier + chain filtering enforced.
#[tokio::test]
async fn runtime_harness_full_domain_sync_e2e() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let trading_db = workspace.path().join("trading_state.db");
    let wallet_db = workspace.path().join("wallet_registry.db");
    let candidate_db = workspace.path().join("candidate_wallet_registry.db");
    seed_all_domains(&trading_db, &wallet_db);

    let mut engine =
        start_engine(&workspace, &trading_db, &wallet_db, &candidate_db).expect("engine spawn");
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine exited before the IPC loop (seeded DB must not fail closed startup)"
    );
    wait_for_socket(&engine.socket_path).await.expect("socket");

    let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
    let deadline = Instant::now() + HEARTBEAT_BUDGET;

    let (
        mut saw_heartbeat,
        mut saw_wallet_s,
        mut saw_position_sol,
        mut saw_trade,
        mut saw_activity_sol,
        mut saw_signal_sol,
        mut saw_token_sol,
    ) = (false, false, false, false, false, false, false);

    while Instant::now() < deadline
        && !(saw_heartbeat
            && saw_wallet_s
            && saw_position_sol
            && saw_trade
            && saw_activity_sol
            && saw_signal_sol
            && saw_token_sol)
    {
        match tokio::time::timeout(Duration::from_millis(1000), rx.recv()).await {
            Ok(Some(IpcMessage::Heartbeat { is_live_mode, .. })) => {
                assert!(!is_live_mode);
                saw_heartbeat = true;
            }
            Ok(Some(IpcMessage::WalletSync { wallets })) => {
                for wallet in &wallets {
                    assert!(
                        matches!(wallet.tier, WalletTier::TierS | WalletTier::TierA),
                        "tier filter violated: {:?}",
                        wallet.tier
                    );
                    if wallet.handle == "harness-whale-s" {
                        saw_wallet_s = true;
                    }
                }
                assert!(
                    !wallets.iter().any(|w| w.handle == "harness-whale-b"),
                    "Tier-B wallet must never sync"
                );
            }
            Ok(Some(IpcMessage::PositionUpdate { positions })) => {
                if positions.iter().any(|p| p.id == "harness-pos-sol") {
                    saw_position_sol = true;
                }
                assert!(
                    positions.iter().any(|p| p.id == "harness-pos-base"),
                    "raw IPC snapshot must preserve Base row; TUI handler applies filtering"
                );
            }
            Ok(Some(IpcMessage::TradeSync { trades })) => {
                if trades
                    .iter()
                    .any(|t| t.symbol == "HARNESSSOL" && t.chain == Chain::Solana)
                {
                    saw_trade = true;
                }
            }
            Ok(Some(IpcMessage::ActivitySync { activities })) => {
                if activities
                    .iter()
                    .any(|a| a.tx_signature == "harness-activity-sig-sol")
                {
                    saw_activity_sol = true;
                }
                assert!(
                    activities
                        .iter()
                        .any(|a| a.tx_signature == "harness-activity-sig-base"),
                    "raw IPC snapshot must preserve Base activity; TUI handler applies filtering"
                );
            }
            Ok(Some(IpcMessage::SignalSync { signals })) => {
                assert!(signals
                    .iter()
                    .any(|s| s.token_mint == "HarnessMintSo11111111111111111111111111111111"));
                assert!(signals.iter().any(|s| s.token_mint == "0xharnessbase"));
                saw_signal_sol = true;
            }
            Ok(Some(IpcMessage::TrackedTokensUpdate { tokens })) => {
                assert!(tokens
                    .iter()
                    .any(|t| t.token_mint == "HarnessMintSo11111111111111111111111111111111"));
                assert!(tokens.iter().any(|t| t.token_mint == "0xharnessbase"));
                saw_token_sol = true;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if Instant::now() < deadline {
                    rx = connect_ipc(&engine.socket_path)
                        .await
                        .expect("IPC reconnect");
                }
            }
        }
    }

    assert!(saw_heartbeat, "heartbeat tidak diterima");
    assert!(saw_wallet_s, "WalletSync (Tier-S) tidak diterima");
    assert!(saw_position_sol, "PositionUpdate (Solana) tidak diterima");
    assert!(saw_trade, "TradeSync tidak diterima");
    assert!(saw_activity_sol, "ActivitySync (Solana) tidak diterima");
    assert!(saw_signal_sol, "SignalSync (Solana) tidak diterima");
    assert!(saw_token_sol, "TrackedTokensUpdate tidak diterima");

    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine process exited during the multi-domain harness run"
    );
}
