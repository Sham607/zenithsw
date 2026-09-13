/// Robinhood route-aware executor.
///
/// Urutan probe per token:
///   1. V3 direct  : WETH→token, USDG→token  (semua fee tier)
///   2. V3 two-hop : WETH→USDG→token, USDG→WETH→token
///   3. V4 direct  : native/WETH pool dari Initialize logs
///
/// Route dengan amountOut terbesar yang dipilih.
/// Calldata dikodekan ke router yang sesuai venue:
///   - V3 route → SwapRouter02 via multicall(deadline, [exactInputSingle|exactInput])
///   - V4 route → Robinhood Universal Router execute(V4_SWAP)
///
/// DRY_RUN=true : quote → calldata → eth_call → estimateGas, tanpa sign/broadcast.
/// Broadcast tetap fail-closed hingga canary gate diaktifkan secara eksplisit.
use crate::chains::robinhood::{
    RobinhoodRpc, UniversalRouterV4Swap, V4InitializedPool, V4QuoteExactInputSingle,
    USDG, UNIVERSAL_ROUTER, V3_SWAP_ROUTER_02, WETH9,
};
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

const MIN_GAS_RESERVE_WEI: u128 = 2_000_000_000_000_000; // 0.002 ETH
const V3_FEE_TIERS: &[u32] = &[100, 500, 3000, 10000];
const SLIPPAGE_BPS: u128 = 50; // 0.5%
const DEADLINE_SECS: u64 = 120;

// ── Domaine internal ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum RouteKind {
    /// V3 single-hop: tokenIn → tokenOut via SwapRouter02.exactInputSingle
    V3Single { token_in: String, token_out: String, fee: u32, amount_out: u128 },
    /// V3 two-hop: tokenIn → mid → tokenOut via SwapRouter02.exactInput(encoded_path)
    V3TwoHop { token_in: String, mid: String, token_out: String, fee_a: u32, fee_b: u32, amount_out: u128 },
    /// V4 pool via Robinhood Universal Router
    V4Pool { pool: V4InitializedPool, zero_for_one: bool, amount_out: u128 },
}

impl RouteKind {
    fn amount_out(&self) -> u128 {
        match self {
            Self::V3Single { amount_out, .. } => *amount_out,
            Self::V3TwoHop { amount_out, .. } => *amount_out,
            Self::V4Pool { amount_out, .. } => *amount_out,
        }
    }
}

// ── EvmExecutor ───────────────────────────────────────────────────────────────

pub struct EvmExecutor {
    config: Config,
    #[allow(dead_code)]
    db: Arc<SmartWalletDb>,
}

impl EvmExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self { Self { config, db } }

    fn rpc(&self) -> Result<RobinhoodRpc> { RobinhoodRpc::new(&self.config.robinhood_rpc_url) }

    fn wallet_address(&self) -> Result<String> {
        if let Ok(address) = std::env::var("ROBINHOOD_WALLET_ADDRESS") {
            if is_evm_address(&address) { return Ok(address); }
            bail!("ROBINHOOD_WALLET_ADDRESS invalid");
        }
        let raw = self.config.evm_private_key.as_deref()
            .ok_or_else(|| anyhow!("evm_private_key belum dikonfigurasi"))?;
        let bytes = hex::decode(raw.trim_start_matches("0x"))
            .context("invalid EVM private key encoding")?;
        let secret = SecretKey::from_slice(&bytes).context("invalid EVM private key")?;
        let point = secret.public_key().to_encoded_point(false);
        let hash = Keccak256::digest(&point.as_bytes()[1..]);
        Ok(format!("0x{}", hex::encode(&hash[12..])))
    }

    fn trade_amount_wei(&self) -> Result<u128> {
        let raw = std::env::var("ROBINHOOD_TRADE_AMOUNT_WEI")
            .context("ROBINHOOD_TRADE_AMOUNT_WEI wajib diatur")?;
        let v = raw.parse::<u128>().context("ROBINHOOD_TRADE_AMOUNT_WEI bukan integer")?;
        if v == 0 { bail!("trade amount harus > 0"); }
        Ok(v)
    }

    // ── Route discovery ───────────────────────────────────────────────────────

    /// Probe semua route yang tersedia: V3 direct, V3 two-hop, V4 pool.
    /// Return semua kandidat yang berhasil quote; caller memilih yang terbaik.
    async fn probe_routes(&self, rpc: &RobinhoodRpc, token: &str, amount_in: u128)
        -> Vec<RouteKind>
    {
        let mut routes: Vec<RouteKind> = Vec::new();
        let weth = WETH9.to_lowercase();
        let usdg = USDG.to_lowercase();
        let tok  = token.to_lowercase();

        // — V3 direct: WETH→token, USDG→token —
        for (tin, label) in [(&weth[..], "WETH"), (&usdg[..], "USDG")] {
            if tin == tok { continue; }
            for &fee in V3_FEE_TIERS {
                if let Ok(out) = rpc.v3_quote_exact_input_single(tin, &tok, amount_in, fee).await {
                    if out > 0 {
                        println!("[ROUTE] V3 direct {label}→token fee={fee} amountOut={out}");
                        routes.push(RouteKind::V3Single {
                            token_in: tin.to_string(), token_out: tok.clone(), fee, amount_out: out,
                        });
                    }
                }
            }
        }

        // — V3 two-hop: WETH→USDG→token, USDG→WETH→token —
        let two_hops: &[(&str, &str, &str)] = &[
            (&weth, &usdg, &tok),
            (&usdg, &weth, &tok),
        ];
        for &(tin, mid, tout) in two_hops {
            if tin == tok || mid == tok { continue; }
            for &fee_a in &[500u32, 3000] {
                for &fee_b in &[500u32, 3000] {
                    // encode path: tokenIn(20) fee_a(3) mid(20) fee_b(3) tokenOut(20)
                    let encoded = encode_v3_path(&[tin, mid, tout], &[fee_a, fee_b]);
                    if let Ok(out) = rpc.v3_quote_exact_input(&encoded, amount_in).await {
                        if out > 0 {
                            println!("[ROUTE] V3 two-hop {tin}→{mid}→{tout} fees={fee_a}/{fee_b} amountOut={out}");
                            routes.push(RouteKind::V3TwoHop {
                                token_in: tin.to_string(),
                                mid: mid.to_string(),
                                token_out: tout.to_string(),
                                fee_a, fee_b, amount_out: out,
                            });
                        }
                    }
                }
            }
        }

        // — V4: probe semua PoolKey dari Initialize logs —
        if let Ok(pools) = self.v4_pools_for_token(rpc, token).await {
            for pool in pools {
                let native_is_c0 = pool.pool_key.currency0 == "0x0000000000000000000000000000000000000000";
                let native_is_c1 = pool.pool_key.currency1 == "0x0000000000000000000000000000000000000000";
                let weth_is_c0   = pool.pool_key.currency0.eq_ignore_ascii_case(WETH9);
                let weth_is_c1   = pool.pool_key.currency1.eq_ignore_ascii_case(WETH9);
                let usdg_is_c0   = pool.pool_key.currency0.eq_ignore_ascii_case(USDG);
                let usdg_is_c1   = pool.pool_key.currency1.eq_ignore_ascii_case(USDG);
                let zero_for_one = native_is_c0 || weth_is_c0 || usdg_is_c0;
                // skip pool yang tidak mengandung native/WETH/USDG sebagai pair
                if !native_is_c0 && !native_is_c1 && !weth_is_c0 && !weth_is_c1 && !usdg_is_c0 && !usdg_is_c1 {
                    continue;
                }
                let req = V4QuoteExactInputSingle {
                    pool_key: pool.pool_key.clone(),
                    zero_for_one,
                    exact_amount: amount_in,
                    hook_data: Vec::new(),
                };
                if let Ok(cd) = req.encode_calldata() {
                    if let Ok(raw) = rpc.eth_call_quoter(&cd).await {
                        if let Ok(out) = parse_first_word(&raw) {
                            if out > 0 {
                                println!("[ROUTE] V4 pool_id={} fee={} hook={} amountOut={out}", pool.pool_id, pool.pool_key.fee, pool.pool_key.hooks);
                                routes.push(RouteKind::V4Pool { pool, zero_for_one, amount_out: out });
                            }
                        }
                    }
                }
            }
        }

        routes
    }

    /// Ambil semua V4 PoolKey yang mengandung token ini dari Initialize logs.
    async fn v4_pools_for_token(&self, rpc: &RobinhoodRpc, token: &str) -> Result<Vec<V4InitializedPool>> {
        let latest = rpc.block_number().await?;
        let latest_n = parse_hex_quantity(&latest)?;
        let scan = std::env::var("ROBINHOOD_SCAN_BLOCKS").ok()
            .and_then(|v| v.parse::<u64>().ok()).unwrap_or(100_000).min(300_000);
        let chunk = std::env::var("ROBINHOOD_SCAN_CHUNK").ok()
            .and_then(|v| v.parse::<u64>().ok()).unwrap_or(2_000).clamp(100, 5_000);
        let latest_n = u64::try_from(latest_n).context("block number overflow")?;
        let from = latest_n.saturating_sub(scan);
        let mut pools = Vec::new();
        for start in (from..=latest_n).step_by(chunk as usize) {
            let end = (start + chunk - 1).min(latest_n);
            if let Ok(logs) = rpc.get_logs_for_token(&format!("0x{start:x}"), &format!("0x{end:x}"), Some(token)).await {
                for log in logs {
                    if let Ok(p) = RobinhoodRpc::parse_initialize_log(&log) { pools.push(p); }
                }
            }
        }
        Ok(pools)
    }

    // ── Calldata builder ──────────────────────────────────────────────────────

    fn build_v3_single_calldata(
        &self, token_in: &str, token_out: &str, fee: u32,
        amount_in: u128, amount_out_min: u128, recipient: &str, deadline: u64,
    ) -> Result<String> {
        // SwapRouter02.exactInputSingle(params) — inner call, wrapped in multicall(deadline,[data])
        let sel_single = keccak_selector("exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))");
        let mut inner = sel_single.to_vec();
        inner.extend_from_slice(&abi_address(token_in));
        inner.extend_from_slice(&abi_address(token_out));
        inner.extend_from_slice(&abi_uint(fee as u128));
        inner.extend_from_slice(&abi_address(recipient));
        inner.extend_from_slice(&abi_uint(amount_in));
        inner.extend_from_slice(&abi_uint(amount_out_min));
        inner.extend_from_slice(&abi_uint(0u128)); // sqrtPriceLimitX96
        wrap_in_multicall(deadline, &inner)
    }

    fn build_v3_twohop_calldata(
        &self, token_in: &str, mid: &str, token_out: &str, fee_a: u32, fee_b: u32,
        amount_in: u128, amount_out_min: u128, recipient: &str, deadline: u64,
    ) -> Result<String> {
        // SwapRouter02.exactInput(params) — path encoded, wrapped in multicall
        let encoded_path = encode_v3_path(&[token_in, mid, token_out], &[fee_a, fee_b]);
        let sel_multi = keccak_selector("exactInput((bytes,address,uint256,uint256))");
        // path is dynamic bytes: encode as tuple with offset
        let path_bytes = encode_v3_path_bytes(&[token_in, mid, token_out], &[fee_a, fee_b]);
        let mut inner = sel_multi.to_vec();
        // tuple head: path_offset=0x80, recipient, amountIn, amountOutMinimum
        inner.extend_from_slice(&abi_uint(0x80u128)); // offset of path bytes in tuple
        inner.extend_from_slice(&abi_address(recipient));
        inner.extend_from_slice(&abi_uint(amount_in));
        inner.extend_from_slice(&abi_uint(amount_out_min));
        // path bytes
        inner.extend_from_slice(&abi_uint(path_bytes.len() as u128));
        inner.extend_from_slice(&path_bytes);
        let _ = encoded_path; // computed above for clarity
        // pad to 32
        while inner.len() % 32 != 4 { inner.push(0); } // 4 = selector len
        wrap_in_multicall(deadline, &inner)
    }

    fn build_v4_calldata(
        &self, pool: &V4InitializedPool, zero_for_one: bool,
        amount_in: u128, amount_out_min: u128, deadline: u64,
    ) -> Result<String> {
        let swap = UniversalRouterV4Swap {
            pool_key: pool.pool_key.clone(),
            zero_for_one,
            amount_in,
            amount_out_minimum: amount_out_min,
            min_hop_price_x36: 0,
            hook_data: Vec::new(),
            deadline,
        };
        swap.encode_calldata()
    }

    // ── simulate_buy ──────────────────────────────────────────────────────────

    async fn simulate_buy(&self, signal: &ShadowSignal) -> Result<()> {
        if !is_evm_address(&signal.token_mint) { bail!("token_mint bukan alamat EVM yang valid"); }
        if is_blacklisted_asset(&signal.token_mint, &signal.token_symbol) { bail!("aset diblacklist"); }

        let rpc = self.rpc()?;
        rpc.chain_id().await.context("chain ID check gagal")?;
        rpc.validate_deployments().await.context("venue deployment check gagal")?;

        let wallet = self.wallet_address()?;
        let amount_in = self.trade_amount_wei()?;
        let native_balance = parse_hex_quantity(&rpc.get_balance(&wallet, "latest").await?)?;
        if native_balance < amount_in.saturating_add(MIN_GAS_RESERVE_WEI) {
            bail!("saldo native ETH tidak cukup; butuh {} wei, punya {}", amount_in + MIN_GAS_RESERVE_WEI, native_balance);
        }

        println!("[ROBINHOOD] probe routes untuk token {} ({}), amount_in={} wei ...", signal.token_symbol, signal.token_mint, amount_in);
        let mut routes = self.probe_routes(&rpc, &signal.token_mint, amount_in).await;

        if routes.is_empty() {
            bail!("tidak ada route yang bisa mengisi swap {} di Robinhood Chain", signal.token_mint);
        }

        // pilih route dengan amountOut terbesar
        routes.sort_by(|a, b| b.amount_out().cmp(&a.amount_out()));
        let best = routes.remove(0);
        let amount_out = best.amount_out();
        let amount_out_min = amount_out.saturating_mul(10_000 - SLIPPAGE_BPS) / 10_000;
        let deadline = now_unix()?.saturating_add(DEADLINE_SECS);

        println!("[ROBINHOOD] best route: {:?}", best);
        println!("[ROBINHOOD] amountOut={amount_out} amountOutMin={amount_out_min}");

        // encode calldata berdasarkan venue
        let (calldata, router, value_wei) = match &best {
            RouteKind::V3Single { token_in, token_out, fee, .. } => {
                let cd = self.build_v3_single_calldata(token_in, token_out, *fee, amount_in, amount_out_min, &wallet, deadline)?;
                (cd, V3_SWAP_ROUTER_02.to_string(), 0u128)
            }
            RouteKind::V3TwoHop { token_in, mid, token_out, fee_a, fee_b, .. } => {
                let cd = self.build_v3_twohop_calldata(token_in, mid, token_out, *fee_a, *fee_b, amount_in, amount_out_min, &wallet, deadline)?;
                (cd, V3_SWAP_ROUTER_02.to_string(), 0u128)
            }
            RouteKind::V4Pool { pool, zero_for_one, .. } => {
                let cd = self.build_v4_calldata(pool, *zero_for_one, amount_in, amount_out_min, deadline)?;
                // V4 native pool: value = amount_in; non-native: 0 (perlu approve WETH terlebih dulu)
                let is_native_in = pool.pool_key.currency0 == "0x0000000000000000000000000000000000000000"
                    && *zero_for_one;
                let val = if is_native_in { amount_in } else { 0 };
                (cd, UNIVERSAL_ROUTER.to_string(), val)
            }
        };

        let tx = json!({
            "from": wallet,
            "to": router,
            "value": format!("0x{value_wei:x}"),
            "data": calldata,
        });

        let call_result = rpc.eth_call(&tx).await
            .context("eth_call dry-run gagal")?;
        let gas = rpc.estimate_gas(&tx).await
            .context("eth_estimateGas gagal")?;

        println!(
            "[ROBINHOOD DRY-RUN] chain=4663 token={} amount_in={} amount_out={} gas={} eth_call_bytes={} broadcast=false",
            signal.token_symbol, amount_in, amount_out, gas, call_result.len()
        );
        Ok(())
    }

    // ── Public API ────────────────────────────────────────────────────────────

    pub async fn execute_shadow_buy(&self, signal: &ShadowSignal) -> Result<Option<ShadowPosition>> {
        if signal.chain != Chain::Robinhood { bail!("EvmExecutor hanya untuk sinyal Robinhood"); }
        if !signal.is_passed { return Ok(None); }
        if self.config.dry_run {
            self.simulate_buy(signal).await?;
            return Ok(None);
        }
        if !self.config.live_robinhood_enabled {
            bail!("DRY_RUN=false tetapi LIVE_ROBINHOOD_ENABLED=false; live Robinhood tetap terkunci");
        }
        bail!("live Robinhood belum siap: signer, allowance, broadcast, receipt, dan reconciliation belum lengkap")
    }

    pub async fn execute_partial_shadow_sell(&self, _id: &str, _ratio: f64, _reason: &str, _price: f64) -> Result<()> {
        bail!("Robinhood SELL fail-closed: belum diimplementasikan")
    }

    pub async fn execute_shadow_sell(&self, _id: &str, _reason: &str, _price: f64) -> Result<()> {
        bail!("Robinhood SELL fail-closed: belum diimplementasikan")
    }
}

// ── Helpers V3 path encoding ──────────────────────────────────────────────────

/// Encode packed path: addr(20) fee(3) addr(20) fee(3) addr(20) sebagai hex string
fn encode_v3_path(tokens: &[&str], fees: &[u32]) -> String {
    let mut bytes: Vec<u8> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let addr = hex::decode(token.trim_start_matches("0x")).unwrap_or_default();
        bytes.extend_from_slice(&addr);
        if i < fees.len() {
            let f = fees[i];
            bytes.push((f >> 16) as u8);
            bytes.push((f >> 8) as u8);
            bytes.push(f as u8);
        }
    }
    format!("0x{}", hex::encode(bytes))
}

/// Same as encode_v3_path but returns raw bytes (untuk ABI encoding)
fn encode_v3_path_bytes(tokens: &[&str], fees: &[u32]) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let addr = hex::decode(token.trim_start_matches("0x")).unwrap_or_default();
        bytes.extend_from_slice(&addr);
        if i < fees.len() {
            let f = fees[i];
            bytes.push((f >> 16) as u8);
            bytes.push((f >> 8) as u8);
            bytes.push(f as u8);
        }
    }
    bytes
}

/// SwapRouter02 multicall(uint256 deadline, bytes[] data)
fn wrap_in_multicall(deadline: u64, inner: &[u8]) -> Result<String> {
    let sel = keccak_selector("multicall(uint256,bytes[])");
    // encode(deadline, [inner])
    // args: deadline(32) + array_offset(32) + array_len(32) + elem_offset(32) + elem_len(32) + elem_data
    let mut out = sel.to_vec();
    out.extend_from_slice(&abi_uint(deadline as u128));
    out.extend_from_slice(&abi_uint(64u128));         // offset of bytes[] = 2 * 32
    out.extend_from_slice(&abi_uint(1u128));           // array length
    out.extend_from_slice(&abi_uint(32u128));          // offset of element 0 within array
    out.extend_from_slice(&abi_uint(inner.len() as u128));
    out.extend_from_slice(inner);
    while out.len() % 32 != 0 { out.push(0); }
    Ok(format!("0x{}", hex::encode(out)))
}

// ── Tiny ABI primitives ───────────────────────────────────────────────────────

fn abi_address(address: &str) -> [u8; 32] {
    let mut word = [0u8; 32];
    let b = hex::decode(address.trim_start_matches("0x")).unwrap_or_default();
    if b.len() == 20 { word[12..].copy_from_slice(&b); }
    word
}

fn abi_uint(value: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn keccak_selector(sig: &str) -> [u8; 4] {
    let digest = Keccak256::digest(sig.as_bytes());
    [digest[0], digest[1], digest[2], digest[3]]
}

fn is_evm_address(v: &str) -> bool {
    v.len() == 42 && v.starts_with("0x") && v[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_hex_quantity(v: &str) -> Result<u128> {
    u128::from_str_radix(v.strip_prefix("0x").ok_or_else(|| anyhow!("invalid hex"))?, 16)
        .context("invalid hex quantity")
}

fn parse_first_word(v: &str) -> Result<u128> {
    let raw = v.strip_prefix("0x").ok_or_else(|| anyhow!("invalid ABI output"))?;
    if raw.len() < 64 { bail!("ABI output terlalu pendek"); }
    u128::from_str_radix(&raw[..64], 16).context("ABI word overflow")
}

fn now_unix() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH).context("clock error")?.as_secs())
}
