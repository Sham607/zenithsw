use anyhow::Result;
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::ipc::IpcServer;
use zenithsw::radar_runtime::RadarRuntime;

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let db = Arc::new(SmartWalletDb::new_with_wallet_path(
        &config.trading_db_path,
        &config.wallet_db_path,
    )?);
    let ipc = Arc::new(IpcServer::new(&config.ipc_socket_path));
    ipc.start().await?;
    let runtime = RadarRuntime::new(config, db.clone(), ipc)?;
    let report = runtime.run_once().await?;
    let projections = db.get_radar_projections(50)?;
    println!(
        "RADAR_RUNTIME_OK wallets={} evidence={} markets={} projections={} db_projections={}",
        report.qualified_wallets,
        report.evidence,
        report.market_snapshots,
        report.projections,
        projections.len()
    );
    for projection in projections {
        println!(
            "RADAR_OUTPUT chain={} token={} mcap={} liquidity={} price={:?} qualified={} candidates={} clusters={} accumulators={} status={:?}",
            projection.chain,
            projection.token_address,
            projection.market_cap_usd,
            projection.liquidity_usd,
            projection.price_usd,
            projection.qualified_holder_count,
            projection.candidate_holder_count,
            projection.independent_cluster_count,
            projection.direct_accumulator_count,
            projection.status
        );
    }
    Ok(())
}
