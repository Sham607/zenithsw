use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zenithsw::ipc::{IpcClient, IpcMessage, IpcServer};

#[tokio::test]
async fn test_ipc_broadcast_and_client_roundtrip() {
    let socket_path = "/tmp/zenithsw_ipc_test.sock";
    let server = Arc::new(IpcServer::new(socket_path));
    server.start().await.unwrap();

    let client = IpcClient::new(socket_path);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(async move {
        let _ = client
            .connect_and_listen(move |msg| {
                let _ = tx.send(msg);
            })
            .await;
    });

    sleep(Duration::from_millis(50)).await;

    let heartbeat = IpcMessage::Heartbeat {
        uptime_secs: 100,
        solana_latency_ms: 55,
        arbitrum_latency_ms: 35,
        base_latency_ms: 40,
        tracked_wallets_count: 50,
        active_positions_count: 3,
    };

    server.broadcast(&heartbeat).unwrap();

    let received = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();

    match received {
        IpcMessage::Heartbeat {
            uptime_secs,
            solana_latency_ms,
            ..
        } => {
            assert_eq!(uptime_secs, 100);
            assert_eq!(solana_latency_ms, 55);
        }
        _ => panic!("Expected Heartbeat message"),
    }
}
