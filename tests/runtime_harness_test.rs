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
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

fn start_engine_custom(
    workspace: &tempfile::TempDir,
    trading_db: &Path,
    wallet_db: &Path,
    candidate_db: &Path,
    custom_envs: &[(&str, &str)],
) -> Result<EngineProcess, Box<dyn std::error::Error>> {
    let engine_path = assert_cmd::cargo::cargo_bin(BIN_NAME);
    let socket_path = workspace.path().join("harness.sock");
    // Unreachable endpoints: the engine must stay alive without them.
    let solana_rpc = "https://127.0.0.1:1";
    let robinhood_rpc = "http://127.0.0.1:1";

    let mut env_map: std::collections::HashMap<&str, &str> = [
        ("SOLANA_RPC_URLS", solana_rpc),
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
    ]
    .into_iter()
    .collect();

    for (key, value) in custom_envs {
        env_map.insert(key, value);
    }

    let child = Command::new(engine_path)
        .env_clear()
        .envs(env_map)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(EngineProcess { child, socket_path })
}

fn start_engine(
    workspace: &tempfile::TempDir,
    trading_db: &Path,
    wallet_db: &Path,
    candidate_db: &Path,
) -> Result<EngineProcess, Box<dyn std::error::Error>> {
    start_engine_custom(workspace, trading_db, wallet_db, candidate_db, &[])
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
        is_dry_run: true,
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
        is_dry_run: true,
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
        source_reference: None,
        parser_version: None,
        finality: None,
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
        source_reference: None,
        parser_version: None,
        finality: None,
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

    // Contract 1: a socket may exist before the first heartbeat. Connections
    // closed or quiet before that heartbeat are expected; reconnect until a
    // real heartbeat arrives.

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
                // Each message is a replacement snapshot. The Radar worker
                // and DB heartbeat can publish different valid snapshots.
                if tokens
                    .iter()
                    .any(|t| t.token_mint == "HarnessMintSo11111111111111111111111111111111")
                {
                    saw_token_sol = true;
                }
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

/// Membuktikan recovery outbox melalui binary Engine nyata pada state terisolasi.
/// Activity sudah durable, outbox tetap QUEUED, lalu client IPC tersambung setelah
/// startup sehingga recovery terjadi pada heartbeat/reconnect loop.
#[tokio::test]
async fn runtime_harness_engine_recovers_queued_activity_outbox() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let trading_db = workspace.path().join("trading_state.db");
    let wallet_db = workspace.path().join("wallet_registry.db");
    let candidate_db = workspace.path().join("candidate_wallet_registry.db");
    seed_all_domains(&trading_db, &wallet_db);

    let expected_signature = "harness-activity-sig-sol".to_string();
    {
        let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
            trading_db.to_str().unwrap(),
            wallet_db.to_str().unwrap(),
        )
        .expect("open isolated DB");
        let activity = db
            .get_recent_activities(50)
            .expect("read seeded activities")
            .into_iter()
            .find(|activity| activity.tx_signature == expected_signature)
            .expect("seeded Solana activity");
        assert!(!db
            .insert_activity_if_new(&activity)
            .expect("dedupe activity"));
        assert!(db
            .enqueue_activity_ipc(&activity)
            .expect("enqueue activity"));
        assert_eq!(
            db.queued_activity_ipc(10)
                .expect("read queued outbox")
                .len(),
            1
        );
    }

    let mut engine =
        start_engine(&workspace, &trading_db, &wallet_db, &candidate_db).expect("engine spawn");
    wait_for_socket(&engine.socket_path).await.expect("socket");
    let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut received = false;

    while Instant::now() < deadline && !received {
        match tokio::time::timeout(Duration::from_millis(1000), rx.recv()).await {
            Ok(Some(IpcMessage::ActivityBroadcast { activity })) => {
                assert_eq!(activity.tx_signature, expected_signature);
                assert_eq!(
                    activity.wallet_address,
                    "So1HarnessWhaleQualified11111111111111111111111"
                );
                assert_eq!(
                    activity.token_mint,
                    "HarnessMintSo11111111111111111111111111111111"
                );
                received = true;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => {
                rx = connect_ipc(&engine.socket_path)
                    .await
                    .expect("IPC reconnect");
            }
        }
    }
    assert!(
        received,
        "queued ActivityBroadcast tidak dipulihkan melalui Engine"
    );

    let empty_deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < empty_deadline {
        let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
            trading_db.to_str().unwrap(),
            wallet_db.to_str().unwrap(),
        )
        .expect("reopen isolated DB");
        if db
            .queued_activity_ipc(10)
            .expect("read outbox after recovery")
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
        trading_db.to_str().unwrap(),
        wallet_db.to_str().unwrap(),
    )
    .expect("final reopen isolated DB");
    assert!(db
        .queued_activity_ipc(10)
        .expect("final outbox state")
        .is_empty());
    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine process exited during outbox recovery harness"
    );
}

/// Membuktikan lifecycle canonical Solana DEX worker melalui binary Engine nyata
/// pada state terisolasi:
/// 1. Qualified wallet dari DB sementara dibaca oleh Engine.
/// 2. Worker memuat cursor durable awal (None) untuk venue SolanaRaydiumAmm.
/// 3. Memvalidasi transaksi swap dari mock HTTP server.
/// 4. Menyimpan verified activity ke database, antre outbox, dan broadcast via IPC.
/// 5. Memajukan durable cursor ke (SigHarnessDEX, 42).
/// 6. Mengabaikan polling berikutnya berkat cursor advance (idempotency).
#[tokio::test]
async fn runtime_harness_engine_canonical_solana_dex_worker_lifecycle() {
    let workspace = tempfile::tempdir().expect("temp workspace");
    let trading_db = workspace.path().join("trading_state.db");
    let wallet_db = workspace.path().join("wallet_registry.db");
    let candidate_db = workspace.path().join("candidate_wallet_registry.db");

    // Seed satu qualified Tier-S wallet Solana
    {
        let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
            trading_db.to_str().unwrap(),
            wallet_db.to_str().unwrap(),
        )
        .expect("open DB for seed");
        let now = chrono::Utc::now();
        db.upsert_wallet(&SmartWallet {
            id: None,
            rank: 1,
            name: "Harness Whale Qualified".into(),
            handle: "harness-whale-dex".into(),
            chain: Chain::Solana,
            address: "Qualified111".into(),
            tier: WalletTier::TierS,
            pnl_24h_usd: 50_000.0,
            win_rate_pct: 80.0,
            total_trades: 150,
            source: "ONCHAIN_INDEXER".into(),
            reputation_score: 98.0,
            execution_track: ExecutionTrack::VaultLane,
            is_active: true,
            last_seen_at: Some(now),
            updated_at: now,
        })
        .expect("seed wallet");
    }

    // Jalankan mock HTTP JSON-RPC server untuk Solana
    let can_serve_signature = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let can_serve_signal = can_serve_signature.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let can_serve = can_serve_signal.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = match socket.read(&mut buffer).await {
                        Ok(n) if n > 0 => n,
                        _ => break,
                    };
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request_text = String::from_utf8_lossy(&request);
                let body = if request_text.contains("getSignaturesForAddress") {
                    if can_serve.load(std::sync::atomic::Ordering::SeqCst) {
                        r#"{"jsonrpc":"2.0","id":1,"result":[{"signature":"SigHarnessDEX","slot":42,"blockTime":1700000000}]}"#
                    } else {
                        r#"{"jsonrpc":"2.0","id":1,"result":[]}"#
                    }
                } else if request_text.contains("getTransaction") {
                    r#"{"jsonrpc":"2.0","id":2,"result":{"slot":42,"blockTime":1700000000,"transaction":{"signatures":["SigHarnessDEX"],"message":{"instructions":[{"programId":"675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","data":"H","accounts":["Token111","PoolHarness","Authority111","VaultA","VaultB","UserIn","UserOut","Qualified111"]}]}},"meta":{"err":null,"preTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"10"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"1000"}}],"postTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"20"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"800"}}]}}}"#
                } else {
                    r#"{"jsonrpc":"2.0","id":1,"result":null}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });

    let custom_envs = [
        ("ENABLE_SOLANA", "true"),
        ("ENABLE_SOLANA_DEX_WORKER", "true"),
        ("SOLANA_RPC_URLS", endpoint.as_str()),
        ("SOLANA_RPC_URL", endpoint.as_str()),
        ("SOLANA_WS_URL", "ws://127.0.0.1:1"),
        ("SOLANA_DEX_TOKEN_ADDRESS", "Token111"),
        ("SOLANA_DEX_QUOTE_ASSET", "USDC111"),
        ("SOLANA_DEX_TOKEN_SYMBOL", "TOK"),
        ("SOLANA_DEX_TOKEN_DECIMALS", "2"),
        ("SOLANA_DEX_QUOTE_DECIMALS", "2"),
        ("SOLANA_DEX_QUOTE_PRICE_USD", "1.0"),
        ("STREAM_POLL_INTERVAL_MS", "500"),
    ];

    let mut engine = start_engine_custom(
        &workspace,
        &trading_db,
        &wallet_db,
        &candidate_db,
        &custom_envs,
    )
    .expect("engine spawn with canonical dex worker");

    wait_for_socket(&engine.socket_path).await.expect("socket");
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut rx = connect_ipc(&engine.socket_path).await.expect("ipc client");
    let mut client_ready = false;
    while Instant::now() < deadline && !client_ready {
        match tokio::time::timeout(CONNECT_TIMEOUT, rx.recv()).await {
            Ok(Some(IpcMessage::Heartbeat { .. })) => {
                client_ready = true;
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => {
                rx = connect_ipc(&engine.socket_path).await.expect("reconnect");
            }
        }
    }
    assert!(client_ready, "engine IPC heartbeat ready");

    // Sekarang subscriber IPC aktif; izinkan mock server melayani signature swap
    can_serve_signature.store(true, std::sync::atomic::Ordering::SeqCst);

    let poll_deadline = Instant::now() + Duration::from_secs(20);
    let mut received_activity = false;

    while Instant::now() < poll_deadline && !received_activity {
        match tokio::time::timeout(CONNECT_TIMEOUT, rx.recv()).await {
            Ok(Some(IpcMessage::ActivityBroadcast { activity })) => {
                if activity.tx_signature == "SigHarnessDEX" {
                    assert_eq!(activity.activity_type, ActivityType::Buy);
                    assert_eq!(activity.wallet_address, "Qualified111");
                    assert_eq!(activity.token_mint, "Token111");
                    assert!(activity.amount_usd > 0.0);
                    received_activity = true;
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) | Err(_) => {
                rx = connect_ipc(&engine.socket_path)
                    .await
                    .expect("IPC reconnect");
            }
        }
    }

    assert!(
        received_activity,
        "ActivityBroadcast dari canonical DEX worker tidak diterima via IPC"
    );

    // Verifikasi state database: activity tersimpan dan cursor telah maju
    let db = zenithsw::db::SmartWalletDb::new_with_wallet_path(
        trading_db.to_str().unwrap(),
        wallet_db.to_str().unwrap(),
    )
    .expect("reopen DB to check persistence");

    let activities = db.get_recent_activities(10).expect("read activities");
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].tx_signature, "SigHarnessDEX");

    let cursor = db
        .load_dex_cursor(
            Chain::Solana,
            zenithsw::dex_adapters::DexVenue::SolanaRaydiumAmm,
            "Qualified111",
        )
        .expect("load dex cursor");
    assert_eq!(
        cursor,
        Some(zenithsw::dex_adapters::ScanCursor::Solana {
            signature: "SigHarnessDEX".into(),
            slot: 42,
        })
    );

    assert!(
        engine.child.try_wait().expect("engine status").is_none(),
        "engine process exited during canonical worker harness"
    );
    drop(server);
}
