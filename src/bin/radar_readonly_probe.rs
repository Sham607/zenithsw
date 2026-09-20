use anyhow::{Context, Result};
use serde_json::json;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::radar_rpc::{
    build_read_only_rpc_client, collect_solana_activity_read_only,
    collect_solana_holdings_read_only,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let db = SmartWalletDb::new_with_wallet_path(&config.trading_db_path, &config.wallet_db_path)?;
    let addresses = db
        .get_qualified_scan_addresses()
        .context("gagal membaca qualified wallet addresses")?;
    let scan = addresses
        .into_iter()
        .filter(|address| address.chain == zenithsw::models::Chain::Solana)
        .take(1)
        .collect::<Vec<_>>();
    let endpoints: Vec<&str> = config
        .solana_rpc_url
        .split(',')
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .collect();
    let mut passing_endpoint = None;
    for (index, endpoint) in endpoints.iter().enumerate() {
        let client = build_read_only_rpc_client(endpoint)?;
        let slot = client
            .call(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getSlot",
                "params": [{"commitment": "confirmed"}]
            }))
            .await;
        let health = client
            .call(&json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "getHealth",
                "params": []
            }))
            .await;
        if slot.is_err() {
            println!("endpoint_index={index} slot=error health=not_checked");
            continue;
        }
        if let Err(error) = health {
            println!("endpoint_index={index} slot=ok health_error={error}");
            continue;
        }
        match collect_solana_holdings_read_only(&client, &scan).await {
            Ok(evidence) => {
                for item in &evidence {
                    db.insert_radar_evidence(item)?;
                }
                let activity_count = if let Some(token) = evidence.first() {
                    collect_solana_activity_read_only(
                        &client,
                        &scan[0],
                        &token.token_address,
                        "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                        5,
                    )
                    .await?
                    .len()
                } else {
                    0
                };
                println!(
                    "endpoint_index={index} slot=ok health=ok holdings=ok evidence={} activity={activity_count}",
                    evidence.len()
                );
                passing_endpoint = Some((index, evidence.len(), activity_count));
                break;
            }
            Err(error) => {
                println!("endpoint_index={index} slot=ok health=ok holdings_error={error}")
            }
        }
    }
    let Some((index, evidence_count, activity_count)) = passing_endpoint else {
        anyhow::bail!("tidak ada endpoint Solana yang lulus holdings read-only");
    };
    println!("RADAR_READONLY_RPC_OK");
    println!("selected_endpoint_index={index}");
    println!("qualified_solana_wallets_available={}", scan.len());
    println!("wallets_scanned={}", scan.len());
    println!("verified_holdings_evidence={evidence_count}");
    println!("verified_activity_count={activity_count}");
    Ok(())
}
