use anyhow::Result;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;

fn main() -> Result<()> {
    let config = Config::load()?;
    let _db = SmartWalletDb::new_with_wallet_path(&config.trading_db_path, &config.wallet_db_path)?;
    println!("RADAR_MIGRATION_OK");
    println!("trading_db_path={}", config.trading_db_path);
    println!("wallet_db_path={}", config.wallet_db_path);
    Ok(())
}
