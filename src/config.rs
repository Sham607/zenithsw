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
    pub solana_execution_rpc_url: Option<String>,
    pub solana_portfolio_rpc_url: Option<String>,
    pub solana_private_key: Option<String>,

    // Robinhood Chain settings
    pub robinhood_rpc_url: String,
    pub robinhood_ws_url: String,
    pub robinhood_api_key: Option<String>,
    pub evm_private_key: Option<String>,

    // Stream settings
    pub stream_poll_interval_ms: u64,

    // Gating parameters
    pub max_price_delta_pct: f64, // Max +5.0% price impact / slippage delta
    pub min_liquidity_usd: f64,   // Min $30,000 liquidity in pool
    pub position_size_usd: f64,   // $15 default position size
    pub min_whale_buy_usd: f64,   // Min $250 whale order size to filter dust transactions
    pub max_token_exposure_usd: f64, // Max $50 aggregated exposure per token mint
    pub max_price_impact_pct: f64, // Max 15.0% price impact on pool to prevent buying local tops
    pub price_sanity_spike_mult: f64, // Max single-tick price multiplier (e.g. 5.0x / 500%)
    /// Umur maksimum harga sebelum TUI menandai stale (detik). Bukan pemicu jual.
    pub stale_price_max_secs: u64,
    /// Umur minimum pool sebelum layak dibeli (detik). 0 = tidak dibatasi.
    pub min_pool_age_secs: u64,
    pub min_market_cap_usd: f64,              // Min $50k market cap
    pub max_market_cap_usd: f64,              // Max $20M market cap
    pub confluence_window_secs: i64,          // Window for confluence counting (e.g. 300s)
    pub tier_b_min_confluence_buyers: u32,    // Minimum buyers for Tier-B solo trigger (e.g. 2)
    pub trailing_stop_pct: f64,               // 10% trailing stop
    pub trailing_stop_trigger_pct: f64,       // Activation threshold for trailing stop (e.g. 35%)
    pub stop_loss_pct: f64,                   // 15% stop loss
    pub take_profit_pct: f64,                 // 100% take profit trigger (50% partial exit)
    pub take_profit_2_pct: f64,               // Tier 2 take profit (e.g. 300% / 4x)
    pub tp1_portion_pct: f64,                 // Portion % to exit at TP1 (e.g. 50%)
    pub tp2_portion_pct: f64,                 // Portion % to exit at TP2 (e.g. 25%)
    pub runner_min_pump_mult: f64, // Minimum pump multiplier from entry to arm runner protections (e.g. 2.0x)
    pub profit_lock_min_current_pnl_pct: f64, // Minimum current PnL % to trigger profit-lock floor exit (e.g. 5%)
    pub liquidity_drain_usd: f64,             // Floor for emergency exit on liquidity collapse
    pub peak_drawdown_pct: f64,               // Max drop % from peak for moonbag exit (e.g. 50%)
    pub profit_lock_floor_pct: f64,           // Minimum positive profit floor on moonbag (e.g. 20%)
    pub whale_full_dump_pct: f64,             // Whale sell % triggering 100% mirror dump (e.g. 95%)

    // On-Chain Execution Parameters
    pub solana_buy_slippage_bps: u16, // Jupiter buy slippage bps (e.g. 100 bps)
    pub solana_tp_slippage_bps: u16,  // Jupiter TP sell slippage bps (e.g. 250 bps)
    pub solana_trailing_slippage_bps: u16, // Jupiter trailing sell slippage bps (e.g. 300 bps)
    pub solana_sl_slippage_bps: u16,  // Jupiter emergency hard SL slippage bps (e.g. 500 bps)
    pub solana_priority_fee_max_lamports: u64, // Batas atas priority fee Jupiter (level veryHigh)
    /// TTL cache kuotasi exit Jupiter (ms). Dipakai untuk monitoring; SL selalu pakai quote segar.
    pub exit_quote_ttl_ms: u64,
    pub evm_slippage_bps: u128,    // EVM swap slippage bps (e.g. 50 bps)
    pub evm_gas_reserve_wei: u128, // EVM minimum gas reserve in wei (e.g. 0.002 ETH)
    pub evm_deadline_secs: u64,    // EVM swap deadline in seconds (e.g. 120s)

    // Radar Parameters
    pub radar_min_holding_value_usd: f64, // Min holding USD to qualify for radar projection (e.g. 100.0)
    pub radar_prune_minutes: i64,         // Radar stale projection prune cutoff minutes (e.g. 30)

    pub quarantine_base_minutes: u64, // Base cooldown duration for failed tokens
    pub quarantine_max_minutes: u64,  // Maximum cooldown cap for failed tokens
    pub demote_consecutive_losses: u32, // Number of consecutive losses to demote whale
    pub demote_catastrophic_loss_pct: f64, // Single-trade loss % to trigger demotion
    pub copytrade_allowed_whales: Vec<String>, // Whitelist substring pattern for copytrade execution (default empty = allow all)
    pub shadow_exit_enabled: bool,             // Mirror sell when whale dumps
    pub bot_take_profit_pct: f64, // Bot autonomous 100% TP (when shadow_exit_enabled=false)
    pub bot_stop_loss_pct: f64,   // Bot autonomous 100% SL (when shadow_exit_enabled=false)
    pub bot_trailing_stop_trigger_pct: f64, // Bot autonomous trailing trigger
    pub bot_trailing_stop_pct: f64, // Bot autonomous trailing drop %
    pub confluence_mode: String,  // "TIER_S_OR_DUAL_A"
    pub dry_run: bool,
    /// Jeda trading: engine tetap memantau & SL proteksi jalan, tetapi BUY ditolak.
    pub trading_paused: bool,
    pub sim_initial_balance: f64, // Virtual cash balance in dry_run mode
    /// Live Robinhood gate tambahan; tidak cukup sendiri untuk broadcast.
    pub live_robinhood_enabled: bool,

    // Chains enabled
    pub enable_solana: bool,
    pub enable_robinhood: bool,
    pub enable_base: bool,
    /// Collector DEX canonical tetap opt-in; metadata runtime wajib lengkap.
    pub enable_solana_dex_worker: bool,

    // Persistence & IPC

    // Metadata scope untuk collector DEX Solana. Tidak ada fallback sintetis.
    pub solana_dex_token_address: Option<String>,
    pub solana_dex_quote_asset: Option<String>,
    pub solana_dex_token_symbol: Option<String>,
    pub solana_dex_token_decimals: Option<u8>,
    pub solana_dex_quote_decimals: Option<u8>,
    pub solana_dex_quote_price_usd: Option<f64>,
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

fn optional_env(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn optional_positive_u8(name: &str) -> Result<Option<u8>> {
    match env::var(name) {
        Ok(value) => {
            let parsed = value
                .trim()
                .parse::<u8>()
                .map_err(|error| anyhow::anyhow!("{name} tidak valid: {error}"))?;
            if parsed == 0 {
                anyhow::bail!("{name} harus lebih besar dari nol");
            }
            Ok(Some(parsed))
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn optional_positive_f64(name: &str) -> Result<Option<f64>> {
    match env::var(name) {
        Ok(value) => {
            let parsed = value
                .trim()
                .parse::<f64>()
                .map_err(|error| anyhow::anyhow!("{name} tidak valid: {error}"))?;
            if !parsed.is_finite() || parsed <= 0.0 {
                anyhow::bail!("{name} harus finite dan lebih besar dari nol");
            }
            Ok(Some(parsed))
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn optional_positive_u64(name: &str) -> Result<()> {
    if let Ok(value) = env::var(name) {
        let value = value.trim();
        if value.is_empty() {
            return Ok(());
        }
        let parsed = value
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
        if value.is_empty() {
            return Ok(());
        }
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
        let solana_execution_rpc_url = env::var("SOLANA_EXECUTION_RPC_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(ref url) = solana_execution_rpc_url {
            validate_url("SOLANA_EXECUTION_RPC_URL", url, &["http://", "https://"])?;
        }
        let solana_portfolio_rpc_url = env::var("SOLANA_PORTFOLIO_RPC_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(ref url) = solana_portfolio_rpc_url {
            validate_url("SOLANA_PORTFOLIO_RPC_URL", url, &["http://", "https://"])?;
        }
        let robinhood_rpc_url = required("ROBINHOOD_RPC_URL")?;
        validate_url(
            "ROBINHOOD_RPC_URL",
            &robinhood_rpc_url,
            &["http://", "https://"],
        )?;
        let robinhood_ws_url = required("ROBINHOOD_WS_URL")?;
        validate_url("ROBINHOOD_WS_URL", &robinhood_ws_url, &["ws://", "wss://"])?;

        let stream_poll_interval_ms = parse_value("STREAM_POLL_INTERVAL_MS", 500u64)?;
        if stream_poll_interval_ms == 0 {
            anyhow::bail!("STREAM_POLL_INTERVAL_MS harus lebih besar dari nol");
        }
        let max_price_delta_pct = parse_value("MAX_PRICE_DELTA_PCT", 5.0f64)?;
        let min_liquidity_usd = parse_value("MIN_LIQUIDITY_USD", 15000.0f64)?;
        let position_size_usd = parse_value("POSITION_SIZE_USD", 10.0f64)?;
        let min_whale_buy_usd = parse_value("MIN_WHALE_BUY_USD", 50.0f64)?;
        let max_token_exposure_usd = parse_value("MAX_TOKEN_EXPOSURE_USD", 50.0f64)?;
        let max_price_impact_pct = parse_value("MAX_PRICE_IMPACT_PCT", 15.0f64)?;
        let price_sanity_spike_mult = parse_value("PRICE_SANITY_SPIKE_MULT", 5.0f64)?;
        let stale_price_max_secs = parse_value("STALE_PRICE_MAX_SECS", 5u64)?;
        if stale_price_max_secs == 0 {
            anyhow::bail!("STALE_PRICE_MAX_SECS harus lebih besar dari 0");
        }
        let min_pool_age_secs = parse_value("MIN_POOL_AGE_SECS", 0u64)?;
        let min_market_cap_usd = parse_value("MIN_MARKET_CAP_USD", 50000.0f64)?;
        let max_market_cap_usd = parse_value("MAX_MARKET_CAP_USD", 20000000.0f64)?;
        let confluence_window_secs = parse_value("CONFLUENCE_WINDOW_SECS", 300i64)?;
        let tier_b_min_confluence_buyers = parse_value("TIER_B_MIN_CONFLUENCE_BUYERS", 2u32)?;
        let trailing_stop_pct = parse_value("TRAILING_STOP_PCT", 10.0f64)?;
        let trailing_stop_trigger_pct = parse_value("TRAILING_STOP_TRIGGER_PCT", 35.0f64)?;
        let stop_loss_pct = parse_value("STOP_LOSS_PCT", 15.0f64)?;
        let take_profit_pct = parse_value("TAKE_PROFIT_PCT", 100.0f64)?;
        let take_profit_2_pct = parse_value("TAKE_PROFIT_2_PCT", 300.0f64)?;
        let tp1_portion_pct = parse_value("TP1_PORTION_PCT", 50.0f64)?;
        let tp2_portion_pct = parse_value("TP2_PORTION_PCT", 25.0f64)?;
        let runner_min_pump_mult = parse_value("RUNNER_MIN_PUMP_MULT", 2.0f64)?;
        let profit_lock_min_current_pnl_pct =
            parse_value("PROFIT_LOCK_MIN_CURRENT_PNL_PCT", 5.0f64)?;
        let liquidity_drain_usd = parse_value("LIQUIDITY_DRAIN_USD", 10000.0f64)?;
        let peak_drawdown_pct = parse_value("PEAK_DRAWDOWN_PCT", 50.0f64)?;
        let profit_lock_floor_pct = parse_value("PROFIT_LOCK_FLOOR_PCT", 20.0f64)?;
        let whale_full_dump_pct = parse_value("WHALE_FULL_DUMP_PCT", 95.0f64)?;
        let solana_buy_slippage_bps = parse_value("SOLANA_BUY_SLIPPAGE_BPS", 100u16)?;
        let solana_tp_slippage_bps = parse_value("SOLANA_TP_SLIPPAGE_BPS", 250u16)?;
        let solana_trailing_slippage_bps = parse_value("SOLANA_TRAILING_SLIPPAGE_BPS", 300u16)?;
        let solana_sl_slippage_bps = parse_value("SOLANA_SL_SLIPPAGE_BPS", 500u16)?;
        let solana_priority_fee_max_lamports =
            parse_value("SOLANA_PRIORITY_FEE_MAX_LAMPORTS", 100_000u64)?;
        if solana_priority_fee_max_lamports == 0 || solana_priority_fee_max_lamports > 5_000_000 {
            anyhow::bail!("SOLANA_PRIORITY_FEE_MAX_LAMPORTS harus 1..=5000000");
        }
        let exit_quote_ttl_ms = parse_value("EXIT_QUOTE_TTL_MS", 2_000u64)?;
        if !(100..=60_000).contains(&exit_quote_ttl_ms) {
            anyhow::bail!("EXIT_QUOTE_TTL_MS harus 100..=60000");
        }
        let evm_slippage_bps = parse_value("EVM_SLIPPAGE_BPS", 50u128)?;
        let evm_gas_reserve_wei = parse_value("EVM_GAS_RESERVE_WEI", 2_000_000_000_000_000u128)?;
        let evm_deadline_secs = parse_value("EVM_DEADLINE_SECS", 120u64)?;
        let radar_min_holding_value_usd = parse_value("RADAR_MIN_HOLDING_VALUE_USD", 100.0f64)?;
        let radar_prune_minutes = parse_value("RADAR_PRUNE_MINUTES", 30i64)?;
        let bot_take_profit_pct = parse_value("BOT_TAKE_PROFIT_PCT", 50.0f64)?;
        let bot_stop_loss_pct = parse_value("BOT_STOP_LOSS_PCT", 20.0f64)?;
        let bot_trailing_stop_trigger_pct = parse_value("BOT_TRAILING_STOP_TRIGGER_PCT", 30.0f64)?;
        let bot_trailing_stop_pct = parse_value("BOT_TRAILING_STOP_PCT", 10.0f64)?;
        let quarantine_base_minutes = parse_value("QUARANTINE_BASE_MINUTES", 15u64)?;
        let quarantine_max_minutes = parse_value("QUARANTINE_MAX_MINUTES", 240u64)?;
        let demote_consecutive_losses = parse_value("DEMOTE_CONSECUTIVE_LOSSES", 2u32)?;
        let demote_catastrophic_loss_pct = parse_value("DEMOTE_CATASTROPHIC_LOSS_PCT", 30.0f64)?;
        let copytrade_allowed_whales = env::var("COPYTRADE_ALLOWED_WHALES")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect::<Vec<String>>();

        for (name, value) in [
            ("MAX_PRICE_DELTA_PCT", max_price_delta_pct),
            ("MIN_LIQUIDITY_USD", min_liquidity_usd),
            ("POSITION_SIZE_USD", position_size_usd),
            ("MIN_WHALE_BUY_USD", min_whale_buy_usd),
            ("MAX_TOKEN_EXPOSURE_USD", max_token_exposure_usd),
            ("MAX_PRICE_IMPACT_PCT", max_price_impact_pct),
            ("PRICE_SANITY_SPIKE_MULT", price_sanity_spike_mult),
            ("MIN_MARKET_CAP_USD", min_market_cap_usd),
            ("MAX_MARKET_CAP_USD", max_market_cap_usd),
            ("TRAILING_STOP_PCT", trailing_stop_pct),
            ("TRAILING_STOP_TRIGGER_PCT", trailing_stop_trigger_pct),
            ("STOP_LOSS_PCT", stop_loss_pct),
            ("TAKE_PROFIT_PCT", take_profit_pct),
            ("TAKE_PROFIT_2_PCT", take_profit_2_pct),
            ("TP1_PORTION_PCT", tp1_portion_pct),
            ("TP2_PORTION_PCT", tp2_portion_pct),
            ("RUNNER_MIN_PUMP_MULT", runner_min_pump_mult),
            (
                "PROFIT_LOCK_MIN_CURRENT_PNL_PCT",
                profit_lock_min_current_pnl_pct,
            ),
            ("LIQUIDITY_DRAIN_USD", liquidity_drain_usd),
            ("PEAK_DRAWDOWN_PCT", peak_drawdown_pct),
            ("PROFIT_LOCK_FLOOR_PCT", profit_lock_floor_pct),
            ("WHALE_FULL_DUMP_PCT", whale_full_dump_pct),
            ("RADAR_MIN_HOLDING_VALUE_USD", radar_min_holding_value_usd),
            ("BOT_TAKE_PROFIT_PCT", bot_take_profit_pct),
            ("BOT_STOP_LOSS_PCT", bot_stop_loss_pct),
            (
                "BOT_TRAILING_STOP_TRIGGER_PCT",
                bot_trailing_stop_trigger_pct,
            ),
            ("BOT_TRAILING_STOP_PCT", bot_trailing_stop_pct),
            ("DEMOTE_CATASTROPHIC_LOSS_PCT", demote_catastrophic_loss_pct),
        ] {
            if !value.is_finite() || value <= 0.0 {
                anyhow::bail!("{name} harus finite dan lebih besar dari nol");
            }
        }
        if tp1_portion_pct >= 100.0 {
            anyhow::bail!("TP1_PORTION_PCT harus kurang dari 100");
        }
        if tp2_portion_pct >= 100.0 {
            anyhow::bail!("TP2_PORTION_PCT harus kurang dari 100");
        }
        if tp1_portion_pct + tp2_portion_pct >= 100.0 {
            anyhow::bail!(
                "TP1_PORTION_PCT + TP2_PORTION_PCT harus kurang dari 100 untuk menyisakan moonbag"
            );
        }
        if runner_min_pump_mult <= 1.0 {
            anyhow::bail!("RUNNER_MIN_PUMP_MULT harus lebih besar dari 1.0");
        }
        if confluence_window_secs <= 0 {
            anyhow::bail!("CONFLUENCE_WINDOW_SECS harus lebih besar dari nol");
        }
        if tier_b_min_confluence_buyers == 0 {
            anyhow::bail!("TIER_B_MIN_CONFLUENCE_BUYERS harus lebih besar dari nol");
        }
        if evm_deadline_secs == 0 {
            anyhow::bail!("EVM_DEADLINE_SECS harus lebih besar dari nol");
        }
        if radar_prune_minutes <= 0 {
            anyhow::bail!("RADAR_PRUNE_MINUTES harus lebih besar dari nol");
        }
        if quarantine_base_minutes == 0 {
            anyhow::bail!("QUARANTINE_BASE_MINUTES harus lebih besar dari nol");
        }
        if quarantine_max_minutes == 0 {
            anyhow::bail!("QUARANTINE_MAX_MINUTES harus lebih besar dari nol");
        }
        if quarantine_base_minutes > quarantine_max_minutes {
            anyhow::bail!("QUARANTINE_BASE_MINUTES tidak boleh melebihi QUARANTINE_MAX_MINUTES");
        }
        if demote_consecutive_losses == 0 {
            anyhow::bail!("DEMOTE_CONSECUTIVE_LOSSES harus lebih besar dari nol");
        }
        if peak_drawdown_pct > 100.0 {
            anyhow::bail!("PEAK_DRAWDOWN_PCT tidak boleh melebihi 100.0");
        }
        if trailing_stop_pct > 100.0 {
            anyhow::bail!("TRAILING_STOP_PCT tidak boleh melebihi 100.0");
        }
        if bot_stop_loss_pct > 100.0 {
            anyhow::bail!("BOT_STOP_LOSS_PCT tidak boleh melebihi 100.0");
        }
        if bot_trailing_stop_pct > 100.0 {
            anyhow::bail!("BOT_TRAILING_STOP_PCT tidak boleh melebihi 100.0");
        }
        if whale_full_dump_pct > 100.0 {
            anyhow::bail!("WHALE_FULL_DUMP_PCT tidak boleh melebihi 100.0");
        }
        if min_market_cap_usd > max_market_cap_usd {
            anyhow::bail!("MIN_MARKET_CAP_USD tidak boleh melebihi MAX_MARKET_CAP_USD");
        }

        let dry_run = parse_bool("DRY_RUN", true)?;
        let trading_paused = parse_bool("TRADING_PAUSED", false)?;
        let sim_initial_balance = parse_value("SIM_INITIAL_BALANCE", 100.0f64)?;
        if sim_initial_balance <= 0.0 {
            anyhow::bail!("SIM_INITIAL_BALANCE harus lebih besar dari 0");
        }
        let live_robinhood_enabled = parse_bool("LIVE_ROBINHOOD_ENABLED", false)?;
        let enable_solana = parse_bool("ENABLE_SOLANA", true)?;
        let enable_robinhood = parse_bool("ENABLE_ROBINHOOD", false)?;
        let enable_base = parse_bool("ENABLE_BASE", false)?;
        let enable_solana_dex_worker = parse_bool("ENABLE_SOLANA_DEX_WORKER", false)?;
        let solana_dex_token_address = optional_env("SOLANA_DEX_TOKEN_ADDRESS");
        let solana_dex_quote_asset = optional_env("SOLANA_DEX_QUOTE_ASSET");
        let solana_dex_token_symbol = optional_env("SOLANA_DEX_TOKEN_SYMBOL");
        let solana_dex_token_decimals = optional_positive_u8("SOLANA_DEX_TOKEN_DECIMALS")?;
        let solana_dex_quote_decimals = optional_positive_u8("SOLANA_DEX_QUOTE_DECIMALS")?;
        let solana_dex_quote_price_usd = optional_positive_f64("SOLANA_DEX_QUOTE_PRICE_USD")?;
        if enable_solana_dex_worker
            && (!enable_solana
                || solana_dex_token_address.is_none()
                || solana_dex_quote_asset.is_none()
                || solana_dex_token_symbol.is_none()
                || solana_dex_token_decimals.is_none()
                || solana_dex_quote_decimals.is_none()
                || solana_dex_quote_price_usd.is_none())
        {
            anyhow::bail!("ENABLE_SOLANA_DEX_WORKER membutuhkan metadata token/quote lengkap");
        }
        if enable_base {
            anyhow::bail!("ENABLE_BASE=true ditolak: hanya Solana dan Robinhood yang didukung");
        }
        if live_robinhood_enabled && dry_run {
            anyhow::bail!("LIVE_ROBINHOOD_ENABLED=true tidak kompatibel dengan DRY_RUN=true");
        }

        let config = Self {
            solana_rpc_url,
            solana_ws_url,
            solana_execution_rpc_url,
            solana_portfolio_rpc_url,
            solana_private_key: env::var("SOLANA_PRIVATE_KEY").ok(),
            robinhood_rpc_url,
            robinhood_ws_url,
            robinhood_api_key: env::var("ROBINHOOD_BLOCKSCOUT_API_KEY")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            evm_private_key: env::var("ROBINHOOD_PRIVATE_KEY")
                .ok()
                .or_else(|| env::var("EVM_PRIVATE_KEY").ok()),
            stream_poll_interval_ms,
            max_price_delta_pct,
            min_liquidity_usd,
            position_size_usd,
            min_whale_buy_usd,
            max_token_exposure_usd,
            max_price_impact_pct,
            price_sanity_spike_mult,
            stale_price_max_secs,
            min_pool_age_secs,
            min_market_cap_usd,
            max_market_cap_usd,
            confluence_window_secs,
            tier_b_min_confluence_buyers,
            trailing_stop_pct,
            trailing_stop_trigger_pct,
            stop_loss_pct,
            take_profit_pct,
            take_profit_2_pct,
            tp1_portion_pct,
            tp2_portion_pct,
            runner_min_pump_mult,
            profit_lock_min_current_pnl_pct,
            liquidity_drain_usd,
            peak_drawdown_pct,
            profit_lock_floor_pct,
            whale_full_dump_pct,
            solana_buy_slippage_bps,
            solana_tp_slippage_bps,
            solana_trailing_slippage_bps,
            solana_sl_slippage_bps,
            solana_priority_fee_max_lamports,
            exit_quote_ttl_ms,
            evm_slippage_bps,
            evm_gas_reserve_wei,
            evm_deadline_secs,
            radar_min_holding_value_usd,
            radar_prune_minutes,
            quarantine_base_minutes,
            quarantine_max_minutes,
            demote_consecutive_losses,
            demote_catastrophic_loss_pct,
            copytrade_allowed_whales,
            shadow_exit_enabled: parse_bool("SHADOW_EXIT_ENABLED", true)?,
            bot_take_profit_pct,
            bot_stop_loss_pct,
            bot_trailing_stop_trigger_pct,
            bot_trailing_stop_pct,
            confluence_mode: env::var("CONFLUENCE_MODE")
                .unwrap_or_else(|_| "TIER_S_OR_DUAL_A".to_string()),
            dry_run,
            trading_paused,
            sim_initial_balance,
            live_robinhood_enabled,
            enable_solana,
            enable_robinhood,
            enable_base,
            enable_solana_dex_worker,
            solana_dex_token_address,
            solana_dex_quote_asset,
            solana_dex_token_symbol,
            solana_dex_token_decimals,
            solana_dex_quote_decimals,
            solana_dex_quote_price_usd,
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
