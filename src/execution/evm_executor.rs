use crate::chains::robinhood::{RobinhoodRpc, UniversalRouterV4Swap, V4PoolKey, V4QuoteExactInputSingle, UNIVERSAL_ROUTER};
use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{is_blacklisted_asset, Chain, ShadowPosition, ShadowSignal};
use anyhow::{anyhow, bail, Context, Result};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MIN_GAS_RESERVE_WEI: u128 = 2_000_000_000_000_000;
const NATIVE_CURRENCY: &str = "0x0000000000000000000000000000000000000000";

pub struct EvmExecutor {
    config: Config,
    db: Arc<SmartWalletDb>,
}

impl EvmExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self { Self { config, db } }

    fn rpc(&self) -> Result<RobinhoodRpc> { RobinhoodRpc::new(&self.config.robinhood_rpc_url) }

    fn wallet_address(&self) -> Result<String> {
        if let Ok(address) = std::env::var("ROBINHOOD_WALLET_ADDRESS") {
            if is_address(&address) { return Ok(address); }
            bail!("ROBINHOOD_WALLET_ADDRESS invalid");
        }
        let raw = self.config.evm_private_key.as_deref().ok_or_else(|| anyhow!("Robinhood wallet address/key unavailable"))?;
        let key = raw.trim_start_matches("0x");
        let bytes = hex::decode(key).context("invalid EVM private key encoding")?;
        let secret = SecretKey::from_slice(&bytes).context("invalid EVM private key")?;
        let point = secret.public_key().to_encoded_point(false);
        let hash = Keccak256::digest(&point.as_bytes()[1..]);
        Ok(format!("0x{}", hex::encode(&hash[12..])))
    }

    fn token_amount_wei(&self) -> Result<u128> {
        let raw = std::env::var("ROBINHOOD_TRADE_AMOUNT_WEI")
            .context("ROBINHOOD_TRADE_AMOUNT_WEI wajib diatur untuk dry-run; konversi USD/native on-chain belum tersedia")?;
        let amount = raw.parse::<u128>().context("ROBINHOOD_TRADE_AMOUNT_WEI invalid")?;
        if amount == 0 { bail!("trade amount must be positive"); }
        Ok(amount)
    }

    async fn discover_pool(&self, rpc: &RobinhoodRpc, token: &str) -> Result<V4PoolKey> {
        if let Ok(block) = std::env::var("ROBINHOOD_POOL_INIT_BLOCK") {
            let logs = rpc.get_logs_for_token(&block, &block, Some(token)).await?;
            for log in logs {
                if let Ok(pool) = RobinhoodRpc::parse_initialize_log(&log) {
                    return Ok(pool.pool_key);
                }
            }
            bail!("no valid V4 PoolKey found at configured init block {}", block);
        }
        let latest = rpc.block_number().await?;
        let latest_n = parse_quantity(&latest)?;
        let scan_blocks = std::env::var("ROBINHOOD_SCAN_BLOCKS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(50_000).min(250_000);
        let chunk = std::env::var("ROBINHOOD_SCAN_CHUNK").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(2_000).clamp(100, 5_000);
        let latest_n = u64::try_from(latest_n).context("block number exceeds u64")?;
        let from = latest_n.saturating_sub(scan_blocks);
        let mut pools = Vec::new();
        for start in (from..=latest_n).step_by(chunk as usize) {
            let end = (start + chunk - 1).min(latest_n);
            let logs = rpc.get_logs_for_token(&format!("0x{start:x}"), &format!("0x{end:x}"), Some(token)).await?;
            for log in logs {
                if let Ok(pool) = RobinhoodRpc::parse_initialize_log(&log) { pools.push(pool); }
            }
        }
        pools.into_iter().next().map(|p| p.pool_key).ok_or_else(|| anyhow!("no valid V4 PoolKey found for token {} in bounded scan {}..{}", token, from, latest_n))
    }

    async fn simulate_buy(&self, signal: &ShadowSignal) -> Result<()> {
        if !is_address(&signal.token_mint) { bail!("Robinhood token_mint must be a canonical EVM address"); }
        if is_blacklisted_asset(&signal.token_mint, &signal.token_symbol) { bail!("asset is blacklisted"); }
        let rpc = self.rpc()?;
        rpc.chain_id().await?;
        rpc.validate_deployments().await?;
        let wallet = self.wallet_address()?;
        let amount_in = self.token_amount_wei()?;
        let native_balance = parse_quantity(&rpc.get_balance(&wallet, "latest").await?)?;
        if native_balance < amount_in.saturating_add(MIN_GAS_RESERVE_WEI) { bail!("insufficient native balance including gas reserve"); }
        let pool_key = self.discover_pool(&rpc, &signal.token_mint).await?;
        let native_is_currency0 = pool_key.currency0.eq_ignore_ascii_case(NATIVE_CURRENCY);
        let native_is_currency1 = pool_key.currency1.eq_ignore_ascii_case(NATIVE_CURRENCY);
        if !native_is_currency0 && !native_is_currency1 { bail!("discovered pool does not contain native currency"); }
        let zero_for_one = native_is_currency0;
        let quote_request = V4QuoteExactInputSingle { pool_key: pool_key.clone(), zero_for_one, exact_amount: amount_in, hook_data: Vec::new() };
        let quote_calldata = quote_request.encode_calldata()?;
        println!("[ROBINHOOD QUOTE] selector={} calldata_len={} pool={}→{} fee={} tick={} zero_for_one={}", &quote_calldata[0..10], quote_calldata.len(), pool_key.currency0, pool_key.currency1, pool_key.fee, pool_key.tick_spacing, zero_for_one);
        let quote_out = rpc.eth_call_quoter(&quote_calldata).await?;
        let amount_out = parse_first_word(&quote_out)?;
        if amount_out == 0 { bail!("V4 quote returned zero"); }
        let min_out = amount_out.saturating_mul(10_000u128.saturating_sub(500)) / 10_000;
        let swap = UniversalRouterV4Swap { pool_key, zero_for_one, amount_in, amount_out_minimum: min_out, hook_data: Vec::new(), deadline: now_unix()?.saturating_add(60) };
        let calldata = swap.encode_calldata()?;
        let tx = json!({"from": wallet, "to": UNIVERSAL_ROUTER, "value": format!("0x{amount_in:x}"), "data": calldata});
        let call_result = rpc.eth_call(&tx).await?;
        let gas = rpc.estimate_gas(&tx).await?;
        println!("[ROBINHOOD DRY-RUN] chain=4663 amount_in={} amount_out={} gas={} eth_call_bytes={} broadcast=false", amount_in, amount_out, gas, call_result.len());
        Ok(())
    }

    pub async fn execute_shadow_buy(&self, signal: &ShadowSignal) -> Result<Option<ShadowPosition>> {
        if signal.chain != Chain::Robinhood { bail!("EvmExecutor received a non-Robinhood signal"); }
        if !signal.is_passed { return Ok(None); }
        if !self.config.dry_run { bail!("live Robinhood execution is locked until explicit canary gate; no position persisted"); }
        self.simulate_buy(signal).await?;
        Ok(None)
    }

    pub async fn execute_partial_shadow_sell(&self, _position_id: &str, _sell_ratio_pct: f64, _exit_reason: &str, _current_price: f64) -> Result<()> {
        bail!("Robinhood SELL is fail-closed until on-chain token balance, quote, eth_call, receipt, event, and balance reconciliation are implemented")
    }

    pub async fn execute_shadow_sell(&self, _position_id: &str, _exit_reason: &str, _current_price: f64) -> Result<()> {
        bail!("Robinhood SELL is fail-closed; database position was not changed")
    }
}

fn is_address(value: &str) -> bool { value.len() == 42 && value.starts_with("0x") && value[2..].chars().all(|c| c.is_ascii_hexdigit()) }
fn parse_quantity(value: &str) -> Result<u128> { u128::from_str_radix(value.strip_prefix("0x").ok_or_else(|| anyhow!("invalid hex quantity"))?, 16).context("invalid hex quantity") }
fn parse_first_word(value: &str) -> Result<u128> { let raw = value.strip_prefix("0x").ok_or_else(|| anyhow!("invalid ABI output"))?; if raw.len() < 64 { bail!("ABI output too short"); } u128::from_str_radix(&raw[32..64], 16).context("quote amount overflow") }
fn now_unix() -> Result<u64> { Ok(SystemTime::now().duration_since(UNIX_EPOCH).context("clock before epoch")?.as_secs()) }
