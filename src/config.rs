use anyhow::{Context, Result};
use dotenvy::dotenv;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Solana settings
    pub solana_rpc_url: String,
    pub solana_ws_url: String,
    pub solana_private_key: Option<String>,

    // Robinhood Chain settings
    pub robinhood_rpc_url: String,
    pub robinhood_ws_url: String,
    pub robinhood_api_key: Option<String>,
    pub evm_private_key: Option<String>,

    // FOMO Auth & Ingestion settings
    pub fomo_auth_token: Option<String>,
    pub fomo_sync_interval_sec: u64,
    pub stream_poll_interval_ms: u64,

    // Gating parameters
    pub max_price_delta_pct: f64, // Max +5.0% price impact / slippage delta
    pub min_liquidity_usd: f64,   // Min $30,000 liquidity in pool
    pub position_size_usd: f64,   // $15 default position size
    pub min_whale_buy_usd: f64,   // Min $250 whale order size to filter dust transactions
    pub max_price_impact_pct: f64, // Max 15.0% price impact on pool to prevent buying local tops
    pub min_market_cap_usd: f64,  // Min $50k market cap
    pub max_market_cap_usd: f64,  // Max $20M market cap
    pub trailing_stop_pct: f64,   // 10% trailing stop
    pub stop_loss_pct: f64,       // 15% stop loss
    pub take_profit_pct: f64,     // 100% take profit trigger (50% partial exit)
    pub shadow_exit_enabled: bool, // Mirror sell when whale dumps
    pub confluence_mode: String,  // "TIER_S_OR_DUAL_A"
    pub dry_run: bool,
    /// Live Robinhood gate tambahan; tidak cukup sendiri untuk broadcast.
    pub live_robinhood_enabled: bool,

    // Chains enabled
    pub enable_solana: bool,
    pub enable_robinhood: bool,
    pub enable_base: bool,

    // Persistence & IPC
    pub trading_db_path: String,
    pub wallet_db_path: String,
    pub candidate_db_path: String,
    pub ipc_socket_path: String,
}

fn required(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("{name} wajib diatur"))?;
    if value.trim().is_empty() {
        anyhow::bail!("{name} tidak boleh kosong");
    }
    Ok(value)
}

fn parse_bool(name: &str, default: bool) -> Result<bool> {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => anyhow::bail!("{name} harus bernilai true atau false"),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn parse_value<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    match env::var(name) {
        Ok(value) => value
            .trim()
            .parse()
            .map_err(|error| anyhow::anyhow!("{name} tidak valid: {error}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

fn absolute_path(name: &str, default: &str) -> Result<String> {
    let raw = env::var(name).unwrap_or_else(|_| default.to_string());
    if raw.trim().is_empty() {
        anyhow::bail!("{name} tidak boleh kosong");
    }
    let path = PathBuf::from(raw);
    if path.as_os_str() == ":memory:" {
        return Ok(":memory:".to_string());
    }
    let path = if path.is_absolute() {
        path
    } else {
        env::current_dir()?.join(path)
    };
    Ok(path.to_string_lossy().into_owned())
}

fn validate_url(name: &str, value: &str, schemes: &[&str]) -> Result<()> {
    if value.trim() != value || value.chars().any(char::is_whitespace) {
        anyhow::bail!("{name} tidak boleh mengandung whitespace");
    }
    let scheme = schemes
        .iter()
        .find(|scheme| value.starts_with(**scheme))
        .ok_or_else(|| anyhow::anyhow!("{name} memakai skema URL yang tidak didukung"))?;
    let authority = value[scheme.len()..]
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if authority.is_empty() || authority.contains(char::is_whitespace) {
        anyhow::bail!("{name} harus memiliki host yang valid");
    }
    Ok(())
}

fn optional_positive_u64(name: &str) -> Result<()> {
    if let Ok(value) = env::var(name) {
        let parsed = value
            .trim()
            .parse::<u64>()
            .map_err(|error| anyhow::anyhow!("{name} tidak valid: {error}"))?;
        if parsed == 0 {
            anyhow::bail!("{name} harus lebih besar dari nol");
        }
    }
    Ok(())
}

fn optional_hex_address(name: &str) -> Result<()> {
    if let Ok(value) = env::var(name) {
        let value = value.trim();
        if value.len() != 42
            || !value.starts_with("0x")
            || !value[2..]
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            anyhow::bail!("{name} harus berupa alamat EVM 20-byte");
        }
    }
    Ok(())
}

fn validate_optional_runtime_env() -> Result<()> {
    optional_positive_u64("ROBINHOOD_SCAN_BLOCKS")?;
    optional_positive_u64("ROBINHOOD_SCAN_CHUNK")?;
    optional_positive_u64("ROBINHOOD_TRADE_AMOUNT_WEI")?;
    optional_hex_address("ROBINHOOD_WALLET_ADDRESS")?;
    optional_hex_address("ROBINHOOD_TOKEN_ADDRESS")?;
    if let Ok(value) = env::var("ROBINHOOD_SCAN_CHUNK") {
        let parsed = value
            .trim()
            .parse::<u64>()
            .map_err(|error| anyhow::anyhow!("ROBINHOOD_SCAN_CHUNK tidak valid: {error}"))?;
        if parsed < 100 {
            anyhow::bail!("ROBINHOOD_SCAN_CHUNK minimal 100");
        }
    }
    Ok(())
}

impl Config {
    pub fn load() -> Result<Self> {
        dotenv().ok();
        let solana_rpc_url = required("SOLANA_RPC_URLS").or_else(|_| required("SOLANA_RPC_URL"))?;
        if solana_rpc_url.split(',').any(|url| url.trim().is_empty()) {
            anyhow::bail!("SOLANA_RPC_URLS memiliki endpoint kosong");
        }
        for url in solana_rpc_url.split(',') {
            validate_url("SOLANA_RPC_URLS", url.trim(), &["http://", "https://"])?;
        }
        let solana_ws_url = required("SOLANA_WS_URL")?;
        validate_url("SOLANA_WS_URL", &solana_ws_url, &["ws://", "wss://"])?;
        let robinhood_rpc_url = required("ROBINHOOD_RPC_URL")?;
        validate_url(
            "ROBINHOOD_RPC_URL",
            &robinhood_rpc_url,
            &["http://", "https://"],
        )?;
        let robinhood_ws_url = required("ROBINHOOD_WS_URL")?;
        validate_url("ROBINHOOD_WS_URL", &robinhood_ws_url, &["ws://", "wss://"])?;

        let fomo_sync_interval_sec = parse_value("FOMO_SYNC_INTERVAL_SEC", 5u64)?;
        let stream_poll_interval_ms = parse_value("STREAM_POLL_INTERVAL_MS", 500u64)?;
        if fomo_sync_interval_sec == 0 {
            anyhow::bail!("FOMO_SYNC_INTERVAL_SEC harus lebih besar dari nol");
        }
        if stream_poll_interval_ms == 0 {
            anyhow::bail!("STREAM_POLL_INTERVAL_MS harus lebih besar dari nol");
        }
        let max_price_delta_pct = parse_value("MAX_PRICE_DELTA_PCT", 5.0f64)?;
        let min_liquidity_usd = parse_value("MIN_LIQUIDITY_USD", 15000.0f64)?;
        let position_size_usd = parse_value("POSITION_SIZE_USD", 10.0f64)?;
        let min_whale_buy_usd = parse_value("MIN_WHALE_BUY_USD", 50.0f64)?;
        let max_price_impact_pct = parse_value("MAX_PRICE_IMPACT_PCT", 15.0f64)?;
        let min_market_cap_usd = parse_value("MIN_MARKET_CAP_USD", 50000.0f64)?;
        let max_market_cap_usd = parse_value("MAX_MARKET_CAP_USD", 20000000.0f64)?;
        let trailing_stop_pct = parse_value("TRAILING_STOP_PCT", 10.0f64)?;
        let stop_loss_pct = parse_value("STOP_LOSS_PCT", 15.0f64)?;
        let take_profit_pct = parse_value("TAKE_PROFIT_PCT", 100.0f64)?;
        for (name, value) in [
            ("MAX_PRICE_DELTA_PCT", max_price_delta_pct),
            ("MIN_LIQUIDITY_USD", min_liquidity_usd),
            ("POSITION_SIZE_USD", position_size_usd),
            ("MIN_WHALE_BUY_USD", min_whale_buy_usd),
            ("MAX_PRICE_IMPACT_PCT", max_price_impact_pct),
            ("MIN_MARKET_CAP_USD", min_market_cap_usd),
            ("MAX_MARKET_CAP_USD", max_market_cap_usd),
            ("TRAILING_STOP_PCT", trailing_stop_pct),
            ("STOP_LOSS_PCT", stop_loss_pct),
            ("TAKE_PROFIT_PCT", take_profit_pct),
        ] {
            if !value.is_finite() || value <= 0.0 {
                anyhow::bail!("{name} harus finite dan lebih besar dari nol");
            }
        }
        if min_market_cap_usd > max_market_cap_usd {
            anyhow::bail!("MIN_MARKET_CAP_USD tidak boleh melebihi MAX_MARKET_CAP_USD");
        }

        let dry_run = parse_bool("DRY_RUN", true)?;
        let live_robinhood_enabled = parse_bool("LIVE_ROBINHOOD_ENABLED", false)?;
        let enable_solana = parse_bool("ENABLE_SOLANA", true)?;
        let enable_robinhood = parse_bool("ENABLE_ROBINHOOD", false)?;
        let enable_base = parse_bool("ENABLE_BASE", false)?;
        if enable_base {
            anyhow::bail!("ENABLE_BASE=true ditolak: hanya Solana dan Robinhood yang didukung");
        }
        if live_robinhood_enabled && dry_run {
            anyhow::bail!("LIVE_ROBINHOOD_ENABLED=true tidak kompatibel dengan DRY_RUN=true");
        }

        let config = Self {
            solana_rpc_url,
            solana_ws_url,
            solana_private_key: env::var("SOLANA_PRIVATE_KEY").ok(),
            robinhood_rpc_url,
            robinhood_ws_url,
            robinhood_api_key: env::var("ROBINHOOD_BLOCKSCOUT_API_KEY")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            evm_private_key: env::var("ROBINHOOD_PRIVATE_KEY")
                .ok()
                .or_else(|| env::var("EVM_PRIVATE_KEY").ok()),
            fomo_auth_token: env::var("FOMO_AUTH_TOKEN")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            fomo_sync_interval_sec,
            stream_poll_interval_ms,
            max_price_delta_pct,
            min_liquidity_usd,
            position_size_usd,
            min_whale_buy_usd,
            max_price_impact_pct,
            min_market_cap_usd,
            max_market_cap_usd,
            trailing_stop_pct,
            stop_loss_pct,
            take_profit_pct,
            shadow_exit_enabled: parse_bool("SHADOW_EXIT_ENABLED", true)?,
            confluence_mode: env::var("CONFLUENCE_MODE")
                .unwrap_or_else(|_| "TIER_S_OR_DUAL_A".to_string()),
            dry_run,
            live_robinhood_enabled,
            enable_solana,
            enable_robinhood,
            enable_base,
            trading_db_path: absolute_path("TRADING_DB_PATH", "data/trading_state.db")?,
            wallet_db_path: absolute_path("WALLET_DB_PATH", "data/wallet_registry.db")?,
            candidate_db_path: absolute_path(
                "CANDIDATE_DB_PATH",
                "data/candidate_wallet_registry.db",
            )?,
            ipc_socket_path: absolute_path("IPC_SOCKET_PATH", "/tmp/zenithsw.sock")?,
        };
        config.validate_paths()?;
        validate_optional_runtime_env()?;
        Ok(config)
    }

    fn validate_paths(&self) -> Result<()> {
        for (name, value) in [
            ("TRADING_DB_PATH", &self.trading_db_path),
            ("WALLET_DB_PATH", &self.wallet_db_path),
            ("CANDIDATE_DB_PATH", &self.candidate_db_path),
        ] {
            if value != ":memory:" && Path::new(value).parent().is_none() {
                anyhow::bail!("{name} tidak memiliki parent directory valid");
            }
        }
        let db_paths = [
            &self.trading_db_path,
            &self.wallet_db_path,
            &self.candidate_db_path,
        ];
        if db_paths
            .iter()
            .filter(|path| **path != ":memory:")
            .collect::<std::collections::HashSet<_>>()
            .len()
            != db_paths.iter().filter(|path| **path != ":memory:").count()
        {
            anyhow::bail!("TRADING_DB_PATH, WALLET_DB_PATH, dan CANDIDATE_DB_PATH harus berbeda");
        }
        if self.ipc_socket_path == ":memory:" {
            anyhow::bail!("IPC_SOCKET_PATH tidak boleh :memory:");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_boolean_without_fallback() {
        std::env::set_var("CONFIG_TEST_BOOL", "ture");
        let result = parse_bool("CONFIG_TEST_BOOL", true);
        std::env::remove_var("CONFIG_TEST_BOOL");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_number_without_fallback() {
        std::env::set_var("CONFIG_TEST_NUMBER", "not-a-number");
        let result = parse_value::<u64>("CONFIG_TEST_NUMBER", 5);
        std::env::remove_var("CONFIG_TEST_NUMBER");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_url_shape() {
        assert!(validate_url("RPC", "https://", &["https://"]).is_err());
        assert!(validate_url("RPC", "ftp://node", &["https://"]).is_err());
        assert!(validate_url("RPC", "https://node.example/rpc", &["https://"]).is_ok());
        assert!(validate_url("RPC", " https://node.example/rpc", &["https://"]).is_err());
        assert!(validate_url("RPC", "https://node.example/rpc path", &["https://"]).is_err());
    }

    #[test]
    fn converts_relative_path_to_absolute() {
        std::env::remove_var("CONFIG_TEST_PATH");
        let path = absolute_path("CONFIG_TEST_PATH", "data/state.db").unwrap();
        assert!(Path::new(&path).is_absolute());
    }

    #[test]
    fn rejects_zero_runtime_intervals() {
        std::env::set_var("FOMO_SYNC_INTERVAL_SEC", "0");
        let result = Config::load();
        std::env::remove_var("FOMO_SYNC_INTERVAL_SEC");
        assert!(result.is_err());

        std::env::set_var("STREAM_POLL_INTERVAL_MS", "0");
        let result = Config::load();
        std::env::remove_var("STREAM_POLL_INTERVAL_MS");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_invalid_scan_chunk_without_fallback() {
        std::env::set_var("ROBINHOOD_SCAN_CHUNK", "not-a-number");
        let result = validate_optional_runtime_env();
        std::env::remove_var("ROBINHOOD_SCAN_CHUNK");
        assert!(result.is_err());
    }
}
