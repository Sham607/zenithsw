use anyhow::Result;
use zenithsw::config::Config;

fn main() -> Result<()> {
    let config = Config::load()?;
    println!("CONFIG_OK");
    println!("dry_run={}", config.dry_run);
    println!("live_robinhood_enabled={}", config.live_robinhood_enabled);
    println!("enable_solana={}", config.enable_solana);
    println!("enable_robinhood={}", config.enable_robinhood);
    println!("enable_base={}", config.enable_base);
    println!(
        "solana_rpc_endpoint_count={}",
        config
            .solana_rpc_url
            .split(',')
            .filter(|url| !url.trim().is_empty())
            .count()
    );
    println!("trading_db_path={}", config.trading_db_path);
    println!("wallet_db_path={}", config.wallet_db_path);
    println!("candidate_db_path={}", config.candidate_db_path);
    println!("ipc_socket_path={}", config.ipc_socket_path);
    Ok(())
}
