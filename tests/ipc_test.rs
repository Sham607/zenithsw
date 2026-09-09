use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zenithsw::ipc::{IpcClient, IpcMessage, IpcServer};

#[tokio::test]
async fn test_ipc_broadcast_and_client_roundtrip() {
    let socket_path = "/tmp/zenithsw_test_roundtrip.sock";
    let server = Arc::new(IpcServer::new(socket_path));
    server.start().await.unwrap();

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

    sleep(Duration::from_millis(50)).await;

    let test_hb = IpcMessage::Heartbeat {
        uptime_secs: 100,
        solana_latency_ms: 25,
        arbitrum_latency_ms: 30,
        base_latency_ms: 35,
        tracked_wallets_count: 5,
        active_positions_count: 1,
    };

    server.broadcast(&test_hb).unwrap();

    let received = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(received.is_ok());
    let msg = received.unwrap();
    assert!(msg.is_some());
}
