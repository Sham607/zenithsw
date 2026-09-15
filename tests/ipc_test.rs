use std::sync::Arc;
use std::time::Duration;
use zenithsw::ipc::{IpcClient, IpcMessage, IpcServer};

#[tokio::test]
async fn test_ipc_broadcast_and_client_roundtrip() {
    let socket_path = "/tmp/zenithsw_test_roundtrip.sock";
    let server = Arc::new(IpcServer::new(socket_path));
    server.start().await.unwrap();

    let test_hb = IpcMessage::Heartbeat {
        uptime_secs: 100,
        heartbeat_seq: 42,
        solana_latency_ms: 25,
        arbitrum_latency_ms: 30,
        base_latency_ms: 35,
        tracked_wallets_count: 5,
        active_positions_count: 1,
        wallet_sol_balance: 0.25,
        sol_price_usd: 100.0,
        wallet_equity_usd: 35.0,
        total_realized_pnl_usd: 5.0,
        min_liquidity_usd: 30000.0,
        max_price_delta_pct: 5.0,
        position_size_usd: 5.0,
        stop_loss_pct: 35.0,
        take_profit_pct: 50.0,
        min_whale_buy_usd: 250.0,
        is_live_mode: true,
        wallet_pubkey: "bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk".to_string(),
        heartbeat_timestamp_unix: 1_000,
        solana_rpc_requests: 10,
        solana_rpc_successes: 9,
        solana_rpc_rate_limited: 1,
        solana_rpc_failures: 0,
        robinhood_rpc_requests: 20,
        robinhood_rpc_successes: 18,
        robinhood_rpc_rate_limited: 2,
        robinhood_rpc_failures: 0,
        qualified_wallets_count: 5,
        polled_wallets_count: 3,
        global_scanned_wallets_count: 2,
        candidate_wallets_count: 7,
    };
    server.broadcast(&test_hb).unwrap();

    let client = IpcClient::new(socket_path);
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    tokio::spawn(async move {
        let _ = client
            .connect_and_listen(
                || {},
                move |msg| {
                    let tx = tx.clone();
                    let _ = tx.try_send(msg);
                },
            )
            .await;
    });

    let received = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(received.is_ok());
    let msg = received.unwrap();
    assert!(msg.is_some());
    match msg.unwrap() {
        IpcMessage::Heartbeat {
            uptime_secs: _,
            heartbeat_seq,
            qualified_wallets_count,
            polled_wallets_count,
            global_scanned_wallets_count,
            candidate_wallets_count,
            ..
        } => {
            assert_eq!(heartbeat_seq, 42);
            assert_eq!(qualified_wallets_count, 5);
            assert_eq!(polled_wallets_count, 3);
            assert_eq!(global_scanned_wallets_count, 2);
            assert_eq!(candidate_wallets_count, 7);
        }
        other => panic!("expected heartbeat, got {other:?}"),
    }
}

#[tokio::test]
async fn test_ipc_client_waits_for_real_heartbeat() {
    let socket_path = "/tmp/zenithsw_test_no_heartbeat.sock";
    let server = Arc::new(IpcServer::new(socket_path));
    server.start().await.unwrap();

    let client = IpcClient::new(socket_path);
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        let _ = client
            .connect_and_listen(
                || {},
                move |msg| {
                    let _ = tx.try_send(msg);
                },
            )
            .await;
    });

    let received = tokio::time::timeout(Duration::from_millis(250), rx.recv()).await;
    assert!(
        matches!(received, Err(_) | Ok(None)),
        "client menerima pesan tanpa heartbeat aktual"
    );
}
