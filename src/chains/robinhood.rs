use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ROBINHOOD_CHAIN_ID_HEX: &str = "0x1237";
pub const POOL_MANAGER_V4: &str = "0x8366a39CC670B4001A1121B8F6A443A643e40951";
pub const POSITION_MANAGER_V4: &str = "0x58daec3116aae6D93017bAAea7749052E8a04fA7";
pub const V4_QUOTER: &str = "0x8Dc178eFB8111BB0973Dd9d722ebeFF267c98F94";
pub const UNIVERSAL_ROUTER: &str = "0x8876789976decbfcbbbe364623c63652db8c0904";
pub const PERMIT2: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";
pub const V3_FACTORY: &str = "0x1f7d7550b1b028f7571e69a784071f0205fd2efa";
pub const V3_QUOTER_V2: &str = "0x33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7";
pub const V3_SWAP_ROUTER_02: &str = "0xcaf681a66d020601342297493863e78c959e5cb2";
pub const USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
pub const STATE_VIEW: &str = "0xf3334192d15450cdd385c8b70e03f9a6bd9e673b";
pub const WETH9: &str = "0x0Bd7D308f8E1639FAb988df18A8011F41EAcAD73";
pub const ERC20_TRANSFER_TOPIC: &str =
    "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
pub const ERC20_APPROVE_SELECTOR: &str = "0x095ea7b3";
pub const ERC20_BALANCE_OF_SELECTOR: &str = "0x70a08231";
pub const CHAINLINK_LATEST_ROUND_DATA_SELECTOR: &str = "0xfeaf968c";
pub const CHAINLINK_DECIMALS_SELECTOR: &str = "0x313ce567";
pub const ORACLE_PAUSED_SELECTOR: &str = "0x7706ba52";
pub const UI_MULTIPLIER_SELECTOR: &str = "0xa60bf13d";
pub const PERMIT2_ALLOWANCE_SELECTOR: &str = "0xcc53287f";
pub const PERMIT2_DOMAIN_NAME: &str = "Permit2";
pub const ROBINHOOD_ASSETS_API: &str = "https://api.robinhood.com/rhj/assets";
pub const V4_INITIALIZE_TOPIC: &str =
    "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438";
pub const PONS_V2_LAUNCH_ROUTER: &str = "0xe33e9e479df8802cb0866d5d05258bec4cf62948";
pub const PONS_V2_GRADUATION_EXECUTOR: &str = "0xc7819b64a1daecd7ec19856d026cb14efbd89046";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RobinhoodVenue {
    PonsV2Curve,
    PonsV1V3,
    PonsV2GraduatedV4,
    UniswapV3,
    UniswapV4,
    Unknown,
    TransferOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedRobinhoodReceipt {
    pub venue: RobinhoodVenue,
    pub success: bool,
    pub has_transfer: bool,
    pub token_contracts: Vec<String>,
}

/// Memastikan receipt yang diterima memang milik transaction hash intent.
/// Replacement transaction harus melewati reconciliation eksplisit; tidak
/// boleh dianggap confirmed hanya karena receipt lain berhasil.
pub fn validate_receipt_transaction_hash(expected_tx_hash: &str, receipt: &Value) -> Result<()> {
    if !is_tx_hash(expected_tx_hash) {
        bail!("expected transaction hash invalid");
    }
    let actual = receipt
        .get("transactionHash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("receipt transactionHash missing"))?;
    if !is_tx_hash(actual) || !actual.eq_ignore_ascii_case(expected_tx_hash) {
        bail!("receipt transaction hash mismatch");
    }
    Ok(())
}

fn is_tx_hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Classifies only evidence present in a successful receipt. A token transfer
/// without a known venue remains TransferOnly and must not create a BUY signal.
pub fn classify_robinhood_receipt(
    receipt: &Value,
    wallet: &str,
) -> Result<ClassifiedRobinhoodReceipt> {
    if !is_address(wallet) {
        bail!("invalid wallet address");
    }
    let success = receipt.get("status").and_then(Value::as_str) == Some("0x1");
    if !success {
        return Ok(ClassifiedRobinhoodReceipt {
            venue: RobinhoodVenue::Unknown,
            success: false,
            has_transfer: false,
            token_contracts: Vec::new(),
        });
    }
    let logs = receipt
        .get("logs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("receipt logs missing"))?;
    let mut has_transfer = false;
    let mut contracts = Vec::new();
    let mut venue = match receipt.get("to").and_then(Value::as_str).unwrap_or("") {
        address if address.eq_ignore_ascii_case(PONS_V2_LAUNCH_ROUTER) => {
            RobinhoodVenue::PonsV2Curve
        }
        address if address.eq_ignore_ascii_case(PONS_V2_GRADUATION_EXECUTOR) => {
            RobinhoodVenue::PonsV2GraduatedV4
        }
        address if address.eq_ignore_ascii_case(V3_SWAP_ROUTER_02) => RobinhoodVenue::UniswapV3,
        address if address.eq_ignore_ascii_case(UNIVERSAL_ROUTER) => {
            RobinhoodVenue::PonsV2GraduatedV4
        }
        _ => RobinhoodVenue::TransferOnly,
    };
    for log in logs {
        let address = log.get("address").and_then(Value::as_str).unwrap_or("");
        let topics = log
            .get("topics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let topic0 = topics.first().and_then(Value::as_str).unwrap_or("");
        if topic0.eq_ignore_ascii_case(ERC20_TRANSFER_TOPIC) {
            has_transfer = true;
            if is_address(address)
                && !contracts
                    .iter()
                    .any(|v: &String| v.eq_ignore_ascii_case(address))
            {
                contracts.push(address.to_string());
            }
        }
        if address.eq_ignore_ascii_case(PONS_V2_LAUNCH_ROUTER) {
            venue = RobinhoodVenue::PonsV2Curve;
        } else if address.eq_ignore_ascii_case(PONS_V2_GRADUATION_EXECUTOR) {
            venue = RobinhoodVenue::PonsV2GraduatedV4;
        } else if address.eq_ignore_ascii_case(UNIVERSAL_ROUTER) && topic0.starts_with("0x40e9cecb")
        {
            venue = RobinhoodVenue::UniswapV4;
        } else if address.eq_ignore_ascii_case(V3_SWAP_ROUTER_02) {
            venue = RobinhoodVenue::UniswapV3;
        } else if address.eq_ignore_ascii_case(POOL_MANAGER_V4) && topic0.starts_with("0x40e9cecb")
        {
            venue = RobinhoodVenue::UniswapV4;
        }
    }
    if !has_transfer {
        venue = RobinhoodVenue::Unknown;
    }
    Ok(ClassifiedRobinhoodReceipt {
        venue,
        success: true,
        has_transfer,
        token_contracts: contracts,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4InitializedPool {
    pub pool_id: String,
    pub pool_key: V4PoolKey,
    pub sqrt_price_x96: String,
    pub tick: i32,
}

impl V4PoolKey {
    pub fn validate(&self) -> Result<()> {
        for address in [&self.currency0, &self.currency1, &self.hooks] {
            if !is_address(address) {
                bail!("V4 PoolKey contains an invalid address");
            }
        }
        if self.currency0.to_lowercase() >= self.currency1.to_lowercase() {
            bail!("V4 PoolKey currencies are not strictly ordered");
        }
        if self.fee > 1_000_000 {
            bail!("V4 pool fee exceeds 1_000_000");
        }
        if self.tick_spacing < -8_388_608 || self.tick_spacing > 8_388_607 {
            bail!("V4 tick spacing does not fit int24");
        }
        Ok(())
    }
}

impl V4InitializedPool {
    pub fn validate(&self) -> Result<()> {
        if !is_hex_bytes32(&self.pool_id) || !is_hex_quantity(&self.sqrt_price_x96) {
            bail!("invalid V4 Initialize pool identity/state");
        }
        self.pool_key.validate()
    }
}

fn is_hex_bytes32(value: &str) -> bool {
    value.starts_with("0x")
        && value.len() == 66
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmTransactionPlan {
    pub from: String,
    pub to: String,
    pub data: String,
    pub value: String,
    pub nonce: String,
    pub gas_limit: String,
    pub gas_price: String,
    pub chain_id: String,
    pub broadcast_allowed: bool,
}

impl EvmTransactionPlan {
    pub fn validate_dry_run(&self) -> Result<()> {
        if !is_address(&self.from) || !is_address(&self.to) {
            bail!("transaction address is invalid");
        }
        if !is_hex_quantity(&self.value)
            || !is_hex_quantity(&self.nonce)
            || !is_hex_quantity(&self.gas_limit)
            || !is_hex_quantity(&self.gas_price)
            || !is_hex_quantity(&self.chain_id)
        {
            bail!("transaction quantity is invalid");
        }
        if !self.data.starts_with("0x") || self.data.len() < 10 {
            bail!("transaction calldata is missing");
        }
        if self.chain_id != ROBINHOOD_CHAIN_ID_HEX {
            bail!("transaction chain id is not Robinhood 4663");
        }
        if self.broadcast_allowed {
            bail!("dry-run transaction cannot allow broadcast");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmBalanceSnapshot {
    pub native_balance: String,
    pub token_balance_before: Option<String>,
    pub token_balance_after: Option<String>,
    pub block: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permit2AllowanceSnapshot {
    pub owner: String,
    pub token: String,
    pub spender: String,
    pub amount: u128,
    pub expiration: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permit2Eip712Request {
    pub owner: String,
    pub token: String,
    pub spender: String,
    pub amount: u128,
    pub expiration: u64,
    pub nonce: u64,
    pub sig_deadline: u64,
    pub chain_id: u64,
    pub verifying_contract: String,
}

impl Permit2Eip712Request {
    pub fn validate(&self, now_unix: u64) -> Result<()> {
        for address in [
            &self.owner,
            &self.token,
            &self.spender,
            &self.verifying_contract,
        ] {
            if !is_address(address) {
                bail!("Permit2 EIP-712 address invalid");
            }
        }
        if self.chain_id != 4663 || !self.verifying_contract.eq_ignore_ascii_case(PERMIT2) {
            bail!("Permit2 EIP-712 domain chain/verifying contract invalid");
        }
        if self.amount == 0 || self.expiration <= now_unix || self.sig_deadline <= now_unix {
            bail!("Permit2 EIP-712 amount or deadline invalid");
        }
        Ok(())
    }
}

impl Permit2AllowanceSnapshot {
    pub fn validate(&self, required_amount: u128, now_unix: u64) -> Result<()> {
        if !is_address(&self.owner) || !is_address(&self.token) || !is_address(&self.spender) {
            bail!("Permit2 allowance address invalid");
        }
        if self.amount < required_amount {
            bail!("Permit2 allowance amount insufficient");
        }
        if self.expiration <= now_unix {
            bail!("Permit2 allowance expired");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainlinkPriceSnapshot {
    pub feed: String,
    pub answer: u128,
    pub decimals: u8,
    pub updated_at: u64,
    pub observed_at: u64,
    pub heartbeat_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodTradingCapabilities {
    pub fractional_tradability: Option<String>,
    pub all_day_tradability: Option<String>,
    pub extended_hours_fractional_tradability: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodAssetMetadata {
    pub uid: String,
    pub symbol: String,
    pub contract_address: String,
    pub chain_id: u64,
    pub current_multiplier: String,
    pub status: String,
    pub trading_capabilities: Option<RobinhoodTradingCapabilities>,
}

impl RobinhoodAssetMetadata {
    pub fn from_api_value(value: &Value) -> Result<Self> {
        let uid = value.get("id").and_then(Value::as_str).unwrap_or("");
        let symbol = value
            .get("tokenSymbol")
            .and_then(Value::as_str)
            .unwrap_or("");
        let multiplier = value
            .get("currentMultiplier")
            .and_then(Value::as_str)
            .unwrap_or("");
        let status = value.get("status").and_then(Value::as_str).unwrap_or("");
        let deployment = value
            .get("deployments")
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .find(|item| item.get("chainId").and_then(Value::as_u64) == Some(4663))
            })
            .ok_or_else(|| anyhow::anyhow!("asset has no Robinhood mainnet deployment"))?;
        let contract_address = deployment
            .get("contractAddress")
            .and_then(Value::as_str)
            .unwrap_or("");
        if !is_hex_bytes32(uid)
            || symbol.is_empty()
            || !is_address(contract_address)
            || multiplier.is_empty()
            || status.is_empty()
        {
            bail!("Robinhood asset metadata is incomplete");
        }
        let capabilities =
            value
                .get("tradingCapabilities")
                .map(|caps| RobinhoodTradingCapabilities {
                    fractional_tradability: caps
                        .get("fractionalTradability")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    all_day_tradability: caps
                        .get("allDayTradability")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    extended_hours_fractional_tradability: caps
                        .get("extendedHoursFractionalTradability")
                        .and_then(Value::as_bool),
                });
        Ok(Self {
            uid: uid.to_string(),
            symbol: symbol.to_string(),
            contract_address: contract_address.to_string(),
            chain_id: 4663,
            current_multiplier: multiplier.to_string(),
            status: status.to_string(),
            trading_capabilities: capabilities,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SequencerUptimeSnapshot {
    pub feed: String,
    pub status: u64,
    pub started_at: u64,
    pub updated_at: u64,
    pub observed_at: u64,
    pub heartbeat_secs: u64,
    pub grace_period_secs: u64,
}

impl SequencerUptimeSnapshot {
    pub fn validate(&self) -> Result<()> {
        if !is_address(&self.feed)
            || self.status > 1
            || self.started_at == 0
            || self.updated_at == 0
        {
            bail!("sequencer uptime snapshot has invalid feed, status, or timestamp");
        }
        if self.heartbeat_secs == 0 || self.grace_period_secs == 0 {
            bail!("sequencer uptime heartbeat/grace period must be non-zero");
        }
        if self.updated_at > self.observed_at || self.started_at > self.observed_at {
            bail!("sequencer uptime timestamp is in the future");
        }
        if self.observed_at - self.updated_at > self.heartbeat_secs {
            bail!("sequencer uptime feed is stale");
        }
        if self.status != 0 {
            bail!("Robinhood sequencer is down");
        }
        if self.observed_at - self.started_at < self.grace_period_secs {
            bail!("Robinhood sequencer grace period is not over");
        }
        Ok(())
    }
}

impl ChainlinkPriceSnapshot {
    pub fn validate(&self) -> Result<()> {
        if !is_address(&self.feed) || self.answer == 0 || self.updated_at == 0 {
            bail!("Chainlink price snapshot has invalid feed, answer, or timestamp");
        }
        if self.decimals > 36 || self.heartbeat_secs == 0 {
            bail!("Chainlink price snapshot decimals/heartbeat invalid");
        }
        if self.updated_at > self.observed_at {
            bail!("Chainlink price timestamp is in the future");
        }
        if self.observed_at - self.updated_at > self.heartbeat_secs {
            bail!("Chainlink price feed is stale");
        }
        Ok(())
    }
}

impl EvmBalanceSnapshot {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.native_balance, &self.block] {
            if !is_hex_quantity(value) {
                bail!("invalid balance snapshot quantity");
            }
        }
        if let Some(value) = &self.token_balance_before {
            if !is_hex_quantity(value) {
                bail!("invalid token balance before");
            }
        }
        if let Some(value) = &self.token_balance_after {
            if !is_hex_quantity(value) {
                bail!("invalid token balance after");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniversalRouterV4Plan {
    pub router: String,
    pub permit2: String,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: String,
    pub amount_out_minimum: String,
    pub deadline: u64,
    pub calldata: String,
}

impl UniversalRouterV4Plan {
    pub fn validate(&self) -> Result<()> {
        for address in [&self.router, &self.permit2, &self.token_in, &self.token_out] {
            if !is_address(address) {
                bail!("router plan contains invalid address");
            }
        }
        if !self.router.eq_ignore_ascii_case(UNIVERSAL_ROUTER)
            || !self.permit2.eq_ignore_ascii_case(PERMIT2)
        {
            bail!("router plan targets an unapproved Robinhood deployment");
        }
        for quantity in [&self.amount_in, &self.amount_out_minimum] {
            if !is_hex_quantity(quantity) || quantity == "0x0" {
                bail!("router plan quantity is invalid");
            }
        }
        if self.token_in.eq_ignore_ascii_case(&self.token_out) || self.deadline == 0 {
            bail!("router plan token/deadline invalid");
        }
        if !self.calldata.starts_with("0x") || self.calldata.len() < 10 {
            bail!("router calldata missing");
        }
        Ok(())
    }
}

/// Parameter single-hop V4 yang dipakai oleh Universal Router.
/// Nilai integer disimpan sebagai u128/i32 karena seluruh batas ABI di sini
/// dapat direpresentasikan tanpa floating point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniversalRouterV4Swap {
    pub pool_key: V4PoolKey,
    pub zero_for_one: bool,
    pub amount_in: u128,
    pub amount_out_minimum: u128,
    pub min_hop_price_x36: u128,
    pub hook_data: Vec<u8>,
    pub deadline: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4PathKey {
    pub intermediate_currency: String,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: String,
    pub hook_data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniversalRouterV4MultiHopSwap {
    pub currency_in: String,
    pub recipient: String,
    pub path: Vec<V4PathKey>,
    pub amount_in: u128,
    pub amount_out_minimum: u128,
    pub deadline: u64,
}

impl UniversalRouterV4MultiHopSwap {
    pub fn encode_calldata(&self) -> Result<String> {
        if !is_address(&self.currency_in)
            || self.path.is_empty()
            || self.amount_in == 0
            || self.amount_out_minimum == 0
            || self.deadline == 0
        {
            bail!("invalid V4 multi-hop parameters");
        }
        for key in &self.path {
            if !is_address(&key.intermediate_currency)
                || !is_address(&key.hooks)
                || key.fee > 1_000_000
                || key.tick_spacing < -8_388_608
                || key.tick_spacing > 8_388_607
            {
                bail!("invalid V4 PathKey");
            }
        }
        let swap_params = encode_v4_exact_input_multi(self)?;
        let settle = encode_currency_amount_payer(&self.currency_in, self.amount_in, true)?;
        let output = self.path.last().unwrap().intermediate_currency.clone();
        let take =
            encode_currency_recipient_amount(&output, &self.recipient, self.amount_out_minimum)?;
        let input = encode_v4_router_input(&[0x07, 0x0b, 0x0e], &[swap_params, settle, take]);
        let body = encode_execute_args(&[0x10], &[input], self.deadline);
        let mut out = function_selector("execute(bytes,bytes[],uint256)").to_vec();
        out.extend(body);
        Ok(format!("0x{}", hex::encode(out)))
    }
}

fn encode_v4_quote_params(
    currency_in: &str,
    path: &[V4PathKey],
    amount_in: u128,
) -> Result<Vec<u8>> {
    if !is_address(currency_in) || path.is_empty() || amount_in == 0 {
        bail!("invalid V4 quote parameters");
    }
    let mut tails = Vec::new();
    for key in path {
        let mut item = Vec::new();
        item.extend_from_slice(&abi_address(&key.intermediate_currency));
        item.extend_from_slice(&abi_uint(key.fee as u128));
        item.extend_from_slice(&abi_int24(key.tick_spacing));
        item.extend_from_slice(&abi_address(&key.hooks));
        item.extend_from_slice(&abi_uint(160));
        item.extend_from_slice(&abi_uint(key.hook_data.len() as u128));
        item.extend_from_slice(&key.hook_data);
        pad_32(&mut item);
        tails.push(item);
    }
    let array_head_len = 32 + 32 * tails.len();
    let mut array = abi_uint(tails.len() as u128).to_vec();
    let mut offset = array_head_len;
    for tail in &tails {
        array.extend_from_slice(&abi_uint(offset as u128));
        offset += tail.len();
    }
    for tail in tails {
        array.extend(tail);
    }
    let mut out = Vec::new();
    out.extend_from_slice(&abi_address(currency_in));
    out.extend_from_slice(&abi_uint(96));
    out.extend_from_slice(&abi_uint(amount_in));
    out.extend(array);
    Ok(out)
}

fn encode_v4_exact_input_multi(plan: &UniversalRouterV4MultiHopSwap) -> Result<Vec<u8>> {
    let mut path_tails = Vec::new();
    for key in &plan.path {
        let mut item = Vec::new();
        item.extend_from_slice(&abi_address(&key.intermediate_currency));
        item.extend_from_slice(&abi_uint(key.fee as u128));
        item.extend_from_slice(&abi_int24(key.tick_spacing));
        item.extend_from_slice(&abi_address(&key.hooks));
        item.extend_from_slice(&abi_uint(160));
        item.extend_from_slice(&abi_uint(key.hook_data.len() as u128));
        item.extend_from_slice(&key.hook_data);
        pad_32(&mut item);
        path_tails.push(item);
    }
    let path_head_len = 32 + 32 * path_tails.len();
    let mut path = Vec::new();
    path.extend_from_slice(&abi_uint(path_tails.len() as u128));
    let mut offset = path_head_len;
    for tail in &path_tails {
        path.extend_from_slice(&abi_uint(offset as u128));
        offset += tail.len();
    }
    for tail in path_tails {
        path.extend(tail);
    }
    let min_prices = vec![0u8; 32];
    let mut out = Vec::new();
    out.extend_from_slice(&abi_address(&plan.currency_in));
    out.extend_from_slice(&abi_uint(160));
    out.extend_from_slice(&abi_uint((160 + path.len()) as u128));
    out.extend_from_slice(&abi_uint(plan.amount_in));
    out.extend_from_slice(&abi_uint(plan.amount_out_minimum));
    out.extend(path);
    out.extend(min_prices);
    Ok(out)
}

impl UniversalRouterV4Swap {
    /// Encode `execute(bytes,bytes[],uint256)` dengan command V4_SWAP (0x10)
    /// dan actions SWAP_EXACT_IN_SINGLE, SETTLE_ALL, TAKE_ALL.
    ///
    /// Fungsi ini hanya membangun calldata; validasi kontrak dan eth_call tetap
    /// wajib dilakukan oleh executor sebelum signing/broadcast.
    pub fn encode_calldata(&self) -> Result<String> {
        self.pool_key.validate()?;
        if self.amount_in == 0 || self.amount_out_minimum == 0 || self.deadline == 0 {
            bail!("V4 router amount/deadline must be non-zero");
        }
        let swap_params = encode_v4_exact_input_single(self)?;
        let (input_currency, output_currency) = if self.zero_for_one {
            (&self.pool_key.currency0, &self.pool_key.currency1)
        } else {
            (&self.pool_key.currency1, &self.pool_key.currency0)
        };
        let settle_params = encode_two_words(input_currency, self.amount_in)?;
        let take_params = encode_two_words(output_currency, self.amount_out_minimum)?;
        let actions = vec![0x06u8, 0x0cu8, 0x0fu8];
        let v4_input = encode_v4_router_input(&actions, &[swap_params, settle_params, take_params]);
        let commands = vec![0x10u8];
        let inputs = vec![v4_input];
        let selector = function_selector("execute(bytes,bytes[],uint256)");
        let body = encode_execute_args(&commands, &inputs, self.deadline);
        let mut out = selector.to_vec();
        out.extend(body);
        Ok(format!("0x{}", hex::encode(out)))
    }
}

fn encode_v4_exact_input_single(plan: &UniversalRouterV4Swap) -> Result<Vec<u8>> {
    // Robinhood Universal Router V4 requires minHopPriceX36 between amountOutMinimum and hookData.
    let mut head = Vec::with_capacity(32 * 12);
    head.extend_from_slice(&abi_uint(32));
    head.extend_from_slice(&abi_address(&plan.pool_key.currency0));
    head.extend_from_slice(&abi_address(&plan.pool_key.currency1));
    head.extend_from_slice(&abi_uint(plan.pool_key.fee as u128));
    head.extend_from_slice(&abi_int24(plan.pool_key.tick_spacing));
    head.extend_from_slice(&abi_address(&plan.pool_key.hooks));
    head.extend_from_slice(&abi_bool(plan.zero_for_one));
    head.extend_from_slice(&abi_uint(plan.amount_in));
    head.extend_from_slice(&abi_uint(plan.amount_out_minimum));
    head.extend_from_slice(&abi_uint(plan.min_hop_price_x36));
    head.extend_from_slice(&abi_uint(10 * 32));
    head.extend_from_slice(&abi_uint(plan.hook_data.len() as u128));
    let mut out = head;
    out.extend_from_slice(&plan.hook_data);
    pad_32(&mut out);
    Ok(out)
}

fn encode_two_words(address: &str, amount: u128) -> Result<Vec<u8>> {
    if !is_address(address) || amount == 0 {
        bail!("invalid V4 settlement parameter");
    }
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&abi_address(address));
    out.extend_from_slice(&abi_uint(amount));
    Ok(out)
}

fn encode_currency_amount_payer(
    address: &str,
    amount: u128,
    payer_is_user: bool,
) -> Result<Vec<u8>> {
    if !is_address(address) || amount == 0 {
        bail!("invalid V4 settle parameter");
    }
    let mut out = encode_two_words(address, amount)?;
    out.extend_from_slice(&abi_bool(payer_is_user));
    Ok(out)
}

fn encode_currency_recipient_amount(
    address: &str,
    recipient: &str,
    amount: u128,
) -> Result<Vec<u8>> {
    if !is_address(address) || !is_address(recipient) || amount == 0 {
        bail!("invalid V4 take parameter");
    }
    let mut out = encode_two_words(address, amount)?;
    out[32..64].copy_from_slice(&abi_address(recipient));
    out.extend_from_slice(&abi_uint(amount));
    Ok(out)
}

fn encode_v4_router_input(actions: &[u8], params: &[Vec<u8>]) -> Vec<u8> {
    let actions_tail = encode_bytes(actions);
    let params_tail = encode_bytes_array(params);
    let mut out = Vec::new();
    out.extend_from_slice(&abi_uint(64));
    out.extend_from_slice(&abi_uint((64 + actions_tail.len()) as u128));
    out.extend(actions_tail);
    out.extend(params_tail);
    out
}

fn encode_execute_args(commands: &[u8], inputs: &[Vec<u8>], deadline: u64) -> Vec<u8> {
    let commands_tail = encode_bytes(commands);
    let inputs_tail = encode_bytes_array(inputs);
    let mut out = Vec::new();
    out.extend_from_slice(&abi_uint(96));
    out.extend_from_slice(&abi_uint((96 + commands_tail.len()) as u128));
    out.extend_from_slice(&abi_uint(deadline as u128));
    out.extend(commands_tail);
    out.extend(inputs_tail);
    out
}

fn encode_bytes(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + value.len() + 31);
    out.extend_from_slice(&abi_uint(value.len() as u128));
    out.extend_from_slice(value);
    pad_32(&mut out);
    out
}

fn encode_bytes_array(values: &[Vec<u8>]) -> Vec<u8> {
    let mut tails = Vec::with_capacity(values.len());
    for value in values {
        tails.push(encode_bytes(value));
    }
    let head_len = 32 + 32 * values.len();
    let mut out = Vec::new();
    out.extend_from_slice(&abi_uint(values.len() as u128));
    let mut offset = head_len;
    for tail in &tails {
        out.extend_from_slice(&abi_uint(offset as u128));
        offset += tail.len();
    }
    for tail in tails {
        out.extend(tail);
    }
    out
}

fn pad_32(value: &mut Vec<u8>) {
    while !value.len().is_multiple_of(32) {
        value.push(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4PoolKey {
    pub currency0: String,
    pub currency1: String,
    pub fee: u32,
    pub tick_spacing: i32,
    pub hooks: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V4QuoteExactInputSingle {
    pub pool_key: V4PoolKey,
    pub zero_for_one: bool,
    pub exact_amount: u128,
    pub hook_data: Vec<u8>,
}

impl V4QuoteExactInputSingle {
    /// Encodes the canonical IV4Quoter quoteExactInputSingle call.
    /// PoolKey is static; hookData is dynamic and its offset is relative to the tuple start.
    pub fn encode_calldata(&self) -> Result<String> {
        for address in [
            self.pool_key.currency0.as_str(),
            self.pool_key.currency1.as_str(),
            self.pool_key.hooks.as_str(),
        ] {
            if !is_address(address) {
                bail!("V4 PoolKey contains an invalid address");
            }
        }
        if self.pool_key.fee > 1_000_000 {
            bail!("V4 pool fee exceeds 1_000_000");
        }
        if self.pool_key.tick_spacing < -8_388_608 || self.pool_key.tick_spacing > 8_388_607 {
            bail!("V4 tick spacing does not fit int24");
        }

        let selector = function_selector(
            "quoteExactInputSingle(((address,address,uint24,int24,address),bool,uint128,bytes))",
        );
        let mut encoded = Vec::with_capacity(4 + 32 * 8 + self.hook_data.len());
        encoded.extend_from_slice(&selector);
        // The single struct argument is a dynamic tuple because hookData is dynamic.
        // Its head starts at the first 32-byte word after the selector.
        encoded.extend_from_slice(&abi_uint(32));
        encoded.extend_from_slice(&abi_address(&self.pool_key.currency0));
        encoded.extend_from_slice(&abi_address(&self.pool_key.currency1));
        encoded.extend_from_slice(&abi_uint(self.pool_key.fee as u128));
        encoded.extend_from_slice(&abi_int24(self.pool_key.tick_spacing));
        encoded.extend_from_slice(&abi_address(&self.pool_key.hooks));
        encoded.extend_from_slice(&abi_bool(self.zero_for_one));
        encoded.extend_from_slice(&abi_uint(self.exact_amount));
        // Tuple head: PoolKey(5) + zeroForOne(1) + exactAmount(1) + hookData offset(1) = 8 words.
        // hookData offset is relative to tuple start.
        encoded.extend_from_slice(&abi_uint(256));
        // ABI tail dipad relatif terhadap selector, bukan mem-pad selector.
        encoded.extend_from_slice(&abi_uint(self.hook_data.len() as u128));
        encoded.extend_from_slice(&self.hook_data);
        while (encoded.len() - 4) % 32 != 0 {
            encoded.push(0);
        }
        Ok(format!("0x{}", hex::encode(encoded)))
    }
}

/// Computes the canonical Uniswap V4 PoolId: keccak256(abi.encode(PoolKey)).
pub fn pool_id_for_key(pool_key: &V4PoolKey) -> Result<[u8; 32]> {
    pool_key.validate()?;
    let mut encoded = Vec::with_capacity(32 * 5);
    encoded.extend_from_slice(&abi_address(&pool_key.currency0));
    encoded.extend_from_slice(&abi_address(&pool_key.currency1));
    encoded.extend_from_slice(&abi_uint(pool_key.fee as u128));
    encoded.extend_from_slice(&abi_int24(pool_key.tick_spacing));
    encoded.extend_from_slice(&abi_address(&pool_key.hooks));
    Ok(Keccak256::digest(encoded).into())
}

fn function_selector(signature: &str) -> [u8; 4] {
    use sha3::{Digest, Keccak256};
    let digest = Keccak256::digest(signature.as_bytes());
    [digest[0], digest[1], digest[2], digest[3]]
}

fn abi_address(address: &str) -> [u8; 32] {
    let mut word = [0u8; 32];
    let bytes = hex::decode(&address[2..]).expect("validated EVM address");
    word[12..].copy_from_slice(&bytes);
    word
}

fn abi_uint(value: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn abi_int24(value: i32) -> [u8; 32] {
    let mut word = [if value < 0 { 0xff } else { 0 }; 32];
    let raw = (value as i64).to_be_bytes();
    word[24..].copy_from_slice(&raw);
    word
}

fn abi_bool(value: bool) -> [u8; 32] {
    abi_uint(if value { 1 } else { 0 })
}

fn parse_first_abi_word(value: &str) -> Result<u128> {
    let raw = value
        .strip_prefix("0x")
        .ok_or_else(|| anyhow::anyhow!("invalid ABI output"))?;
    if raw.len() < 64 {
        bail!("ABI output too short");
    }
    u128::from_str_radix(&raw[..64], 16).context("ABI amount overflow")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodQuoteRequest {
    pub token_in: String,
    pub token_out: String,
    pub amount_in: String,
    pub quoted_at_block: String,
    pub expires_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodQuote {
    pub token_in: String,
    pub token_out: String,
    pub amount_in: String,
    pub amount_out: String,
    pub quoted_at_block: String,
    pub expires_at_unix: u64,
}

impl RobinhoodQuote {
    pub fn validate(&self, now_unix: u64) -> Result<()> {
        if !is_address(&self.token_in) || !is_address(&self.token_out) {
            bail!("quote token address is invalid");
        }
        if self.token_in.eq_ignore_ascii_case(&self.token_out) {
            bail!("quote token_in and token_out must differ");
        }
        if !is_hex_quantity(&self.amount_in) || !is_hex_quantity(&self.amount_out) {
            bail!("quote amount is not a valid integer quantity");
        }
        if self.amount_out == "0x0" || self.amount_out == "0x" {
            bail!("quote amount_out is zero");
        }
        if self.quoted_at_block == "0x0" || self.quoted_at_block.is_empty() {
            bail!("quote block is missing");
        }
        if self.expires_at_unix <= now_unix {
            bail!("quote expired");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSwap {
    pub tx_hash: String,
    pub block_number: String,
    pub router: String,
    pub pool: String,
    pub token_contracts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvmSwapReconciliation {
    pub token_in: String,
    pub token_out: String,
    pub input_balance_before: String,
    pub input_balance_after: String,
    pub output_balance_before: String,
    pub output_balance_after: String,
    pub actual_amount_in: u128,
    pub actual_amount_out: u128,
    pub minimum_amount_out: u128,
}

impl EvmSwapReconciliation {
    pub fn validate_buy(&self) -> Result<()> {
        for address in [&self.token_in, &self.token_out] {
            if !is_address(address) {
                bail!("swap reconciliation token address invalid");
            }
        }
        if self.token_in.eq_ignore_ascii_case(&self.token_out) {
            bail!("swap reconciliation token addresses must differ");
        }
        let input_before = parse_quantity_u128(&self.input_balance_before)?;
        let input_after = parse_quantity_u128(&self.input_balance_after)?;
        let output_before = parse_quantity_u128(&self.output_balance_before)?;
        let output_after = parse_quantity_u128(&self.output_balance_after)?;
        let input_delta = input_before
            .checked_sub(input_after)
            .ok_or_else(|| anyhow::anyhow!("input balance increased during BUY"))?;
        let output_delta = output_after
            .checked_sub(output_before)
            .ok_or_else(|| anyhow::anyhow!("output balance decreased during BUY"))?;
        if input_delta != self.actual_amount_in {
            bail!("input balance delta does not match actual fill");
        }
        if output_delta != self.actual_amount_out {
            bail!("output balance delta does not match actual fill");
        }
        if self.actual_amount_out < self.minimum_amount_out {
            bail!("actual output is below minimum amount");
        }
        if self.actual_amount_in == 0 || self.actual_amount_out == 0 {
            bail!("actual swap amounts must be non-zero");
        }
        Ok(())
    }

    /// Returns actual output-per-input scaled by 1e18 after reconciliation.
    pub fn fill_price_x18(&self) -> Result<u128> {
        self.validate_buy()?;
        self.actual_amount_out
            .checked_mul(1_000_000_000_000_000_000u128)
            .and_then(|value| value.checked_div(self.actual_amount_in))
            .ok_or_else(|| anyhow::anyhow!("fill price overflow or zero input"))
    }
}

#[derive(Clone)]
pub struct RobinhoodRpc {
    urls: Vec<String>,
    rpc_index: Arc<AtomicUsize>,
    cooldowns: Arc<Mutex<Vec<Option<Instant>>>>,
    client: Client,
}

impl RobinhoodRpc {
    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let payload = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let count = self.urls.len();
        if count == 0 {
            bail!("Robinhood RPC pool kosong");
        }
        let start = self.rpc_index.fetch_add(1, Ordering::Relaxed) % count;
        let mut last_error = String::new();
        for offset in 0..count {
            let index = (start + offset) % count;
            let available = self.cooldowns.lock().unwrap()[index]
                .map(|until| until <= Instant::now())
                .unwrap_or(true);
            if !available {
                continue;
            }
            let response = match self
                .client
                .post(&self.urls[index])
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    last_error = format!("endpoint {index} transport: {error}");
                    self.cooldowns.lock().unwrap()[index] =
                        Some(Instant::now() + Duration::from_secs(15));
                    continue;
                }
            };
            if response.status().as_u16() == 429 || response.status().is_server_error() {
                last_error = format!("endpoint {index} HTTP {}", response.status());
                self.cooldowns.lock().unwrap()[index] =
                    Some(Instant::now() + Duration::from_secs(30));
                continue;
            }
            if !response.status().is_success() {
                last_error = format!("endpoint {index} HTTP {}", response.status());
                continue;
            }
            let response: Value = response.json().await?;
            if let Some(error) = response.get("error") {
                last_error = format!("endpoint {index} RPC error: {error}");
                continue;
            }
            return response
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Robinhood RPC {method} returned no result"));
        }
        bail!("Robinhood RPC {method} failed after pool rotation: {last_error}")
    }

    /// Reads official Robinhood Stock Token metadata without using it as an execution price source.
    pub async fn asset_metadata(&self, contract_address: &str) -> Result<RobinhoodAssetMetadata> {
        if !is_address(contract_address) {
            bail!("invalid Robinhood asset contract address");
        }
        let response = self
            .client
            .get(ROBINHOOD_ASSETS_API)
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        let assets = body
            .get("assets")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Robinhood assets API response missing assets"))?;
        for asset in assets {
            let metadata = match RobinhoodAssetMetadata::from_api_value(asset) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            if metadata
                .contract_address
                .eq_ignore_ascii_case(contract_address)
            {
                return Ok(metadata);
            }
        }
        bail!("Robinhood asset contract is not present in official assets API")
    }

    pub async fn block_number(&self) -> Result<String> {
        let result = self.rpc("eth_blockNumber", json!([])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid block number returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn get_logs(&self, from_block: &str, to_block: &str) -> Result<Vec<Value>> {
        self.get_logs_for_token(from_block, to_block, None).await
    }

    /// Reads ERC-20 Transfer logs involving one wallet using standard eth_getLogs.
    /// This avoids provider-specific enhanced APIs and keeps the range bounded.
    pub async fn get_wallet_transfer_logs(
        &self,
        from_block: &str,
        to_block: &str,
        wallet: &str,
    ) -> Result<Vec<Value>> {
        if !is_hex_quantity(from_block) || !is_hex_quantity(to_block) || !is_address(wallet) {
            bail!("invalid wallet transfer log query");
        }
        let wallet_topic = format!("0x{}{}", "0".repeat(24), wallet[2..].to_lowercase());
        let mut logs = Vec::new();
        for topics in [
            json!([ERC20_TRANSFER_TOPIC, wallet_topic, Value::Null]),
            json!([ERC20_TRANSFER_TOPIC, Value::Null, wallet_topic]),
        ] {
            let result = self
                .rpc(
                    "eth_getLogs",
                    json!([{"fromBlock": from_block, "toBlock": to_block, "topics": topics}]),
                )
                .await?;
            let values = result
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid wallet transfer logs result"))?;
            logs.extend(values.iter().cloned());
        }
        logs.sort_by_key(|a| a.to_string());
        logs.dedup_by(|a, b| a == b);
        Ok(logs)
    }

    /// Reads ERC-20 Transfer logs for one token contract in a bounded block range.
    pub async fn get_token_transfer_logs(
        &self,
        from_block: &str,
        to_block: &str,
        token: &str,
    ) -> Result<Vec<Value>> {
        if !is_hex_quantity(from_block) || !is_hex_quantity(to_block) || !is_address(token) {
            bail!("invalid token transfer log query");
        }
        let result = self
            .rpc(
                "eth_getLogs",
                json!([{
                    "address": token,
                    "fromBlock": from_block,
                    "toBlock": to_block,
                    "topics": [ERC20_TRANSFER_TOPIC]
                }]),
            )
            .await?;
        let logs = result
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("invalid token transfer logs result"))?;
        Ok(logs.clone())
    }

    /// Mengambil Initialize logs dari PoolManager, opsional difilter oleh currency.
    /// Filter memakai topic indexed yang sudah dipad 32-byte; hasil parsial/error tidak
    /// pernah diperlakukan sebagai bukti bahwa pool tidak ada.
    pub async fn get_logs_for_token(
        &self,
        from_block: &str,
        to_block: &str,
        token: Option<&str>,
    ) -> Result<Vec<Value>> {
        if !is_hex_quantity(from_block) || !is_hex_quantity(to_block) {
            bail!("invalid log block range");
        }
        let token_topic = if let Some(address) = token {
            if !is_address(address) {
                bail!("invalid token address for pool discovery");
            }
            Some(format!(
                "0x{}{}",
                "0".repeat(24),
                address[2..].to_lowercase()
            ))
        } else {
            None
        };
        let topic_indexes: Vec<usize> = if token_topic.is_some() {
            vec![2, 3]
        } else {
            vec![0]
        };
        let mut all_logs = Vec::new();
        for topic_index in topic_indexes {
            let topics = match token_topic {
                Some(ref topic) => {
                    let mut filters = vec![Value::Null, Value::Null, Value::Null, Value::Null];
                    filters[0] = json!(V4_INITIALIZE_TOPIC);
                    filters[topic_index] = json!(topic);
                    Value::Array(filters)
                }
                None => json!([V4_INITIALIZE_TOPIC]),
            };
            let result = self
                .rpc(
                    "eth_getLogs",
                    json!([{
                        "address": POOL_MANAGER_V4,
                        "fromBlock": from_block,
                        "toBlock": to_block,
                        "topics": topics
                    }]),
                )
                .await?;
            let logs = result
                .as_array()
                .ok_or_else(|| anyhow::anyhow!("invalid eth_getLogs result"))?;
            all_logs.extend(logs.iter().cloned());
        }
        all_logs.sort_by_key(|a| a.to_string());
        all_logs.dedup_by(|a, b| a == b);
        Ok(all_logs)
    }

    pub fn parse_initialize_log(log: &Value) -> Result<V4InitializedPool> {
        let topics = log
            .get("topics")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("Initialize log topics missing"))?;
        if topics.len() != 4
            || topics
                .first()
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase()
                != V4_INITIALIZE_TOPIC
        {
            bail!("not a V4 Initialize log");
        }
        let pool_id = topics[1].as_str().unwrap_or("").to_string();
        let currency0 = format!(
            "0x{}",
            &topics[2].as_str().unwrap_or("").trim_start_matches("0x")[24..]
        );
        let currency1 = format!(
            "0x{}",
            &topics[3].as_str().unwrap_or("").trim_start_matches("0x")[24..]
        );
        let data = log.get("data").and_then(Value::as_str).unwrap_or("");
        let words = data.strip_prefix("0x").unwrap_or("");
        if words.len() < 192 || !words.len().is_multiple_of(64) {
            bail!("Initialize log data malformed");
        }
        let word = |index: usize| -> &str { &words[index * 64..(index + 1) * 64] };
        let fee = u32::from_str_radix(word(0), 16)?;
        let tick_raw = i32::from_str_radix(&word(1)[56..], 16)?;
        let tick_spacing = if tick_raw & 0x0080_0000 != 0 {
            tick_raw | !0x00ff_ffff
        } else {
            tick_raw
        };
        let hooks = format!("0x{}", &word(2)[24..]);
        let sqrt_price_x96 = format!("0x{}", word(3).trim_start_matches('0'));
        let tick = i32::from_str_radix(&word(4)[56..], 16)?;
        let pool = V4InitializedPool {
            pool_id,
            pool_key: V4PoolKey {
                currency0,
                currency1,
                fee,
                tick_spacing,
                hooks,
            },
            sqrt_price_x96,
            tick,
        };
        pool.validate()?;
        Ok(pool)
    }

    pub async fn get_balance(&self, address: &str, tag: &str) -> Result<String> {
        if !is_address(address) || tag.trim().is_empty() {
            bail!("invalid balance query");
        }
        let result = self.rpc("eth_getBalance", json!([address, tag])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid balance returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn get_transaction_count(&self, address: &str, tag: &str) -> Result<String> {
        if !is_address(address) || tag.trim().is_empty() {
            bail!("invalid nonce query");
        }
        let result = self
            .rpc("eth_getTransactionCount", json!([address, tag]))
            .await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid nonce returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn gas_price(&self) -> Result<String> {
        let result = self.rpc("eth_gasPrice", json!([])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid gas price returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn estimate_gas(&self, tx: &Value) -> Result<String> {
        if !tx.is_object() {
            bail!("gas estimate transaction must be an object");
        }
        let result = self.rpc("eth_estimateGas", json!([tx, "latest"])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid gas estimate returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn eth_call(&self, tx: &Value) -> Result<String> {
        if !tx.is_object() {
            bail!("eth_call transaction must be an object");
        }
        let result = self.rpc("eth_call", json!([tx, "latest"])).await?;
        let value = result.as_str().unwrap_or("");
        if value.is_empty() || value == "0x" {
            bail!("eth_call returned empty output");
        }
        Ok(value.to_string())
    }

    /// Reads and validates the Chainlink L2 sequencer uptime feed.
    /// The feed address and timing policy must come from a verified manifest.
    pub async fn sequencer_uptime_snapshot(
        &self,
        feed: &str,
        observed_at: u64,
        heartbeat_secs: u64,
        grace_period_secs: u64,
    ) -> Result<SequencerUptimeSnapshot> {
        if !is_address(feed) {
            bail!("invalid sequencer uptime feed address");
        }
        let raw = self
            .eth_call(&json!({"to": feed, "data": CHAINLINK_LATEST_ROUND_DATA_SELECTOR}))
            .await?;
        let status = parse_abi_u64_word(&raw, 1)?;
        let started_at = parse_abi_u64_word(&raw, 2)?;
        let updated_at = parse_abi_u64_word(&raw, 3)?;
        let snapshot = SequencerUptimeSnapshot {
            feed: feed.to_string(),
            status,
            started_at,
            updated_at,
            observed_at,
            heartbeat_secs,
            grace_period_secs,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    /// Reads and validates a Chainlink AggregatorV3 price snapshot on Robinhood.
    /// Feed address, heartbeat, and observation time are caller-supplied metadata;
    /// no off-chain price or hardcoded feed address is used.
    pub async fn chainlink_price_snapshot(
        &self,
        feed: &str,
        observed_at: u64,
        heartbeat_secs: u64,
    ) -> Result<ChainlinkPriceSnapshot> {
        if !is_address(feed) {
            bail!("invalid Chainlink feed address");
        }
        if heartbeat_secs == 0 {
            bail!("Chainlink heartbeat must be non-zero");
        }
        let raw = self
            .eth_call(&json!({"to": feed, "data": CHAINLINK_LATEST_ROUND_DATA_SELECTOR}))
            .await?;
        let answer = parse_positive_int256_word(&raw, 1)?;
        let updated_at = parse_abi_u64_word(&raw, 3)?;
        let decimals_raw = self
            .eth_call(&json!({"to": feed, "data": CHAINLINK_DECIMALS_SELECTOR}))
            .await?;
        let decimals = parse_abi_u64_word(&decimals_raw, 0)?;
        let snapshot = ChainlinkPriceSnapshot {
            feed: feed.to_string(),
            answer,
            decimals: u8::try_from(decimals)
                .map_err(|_| anyhow::anyhow!("Chainlink decimals exceed u8"))?,
            updated_at,
            observed_at,
            heartbeat_secs,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub async fn token_oracle_paused(&self, token: &str) -> Result<bool> {
        if !is_address(token) {
            bail!("invalid token address for oracle pause");
        }
        let raw = self
            .eth_call(&json!({"to": token, "data": ORACLE_PAUSED_SELECTOR}))
            .await?;
        match parse_abi_u64_word(&raw, 0)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => bail!("oraclePaused returned non-boolean value"),
        }
    }

    pub async fn token_ui_multiplier(&self, token: &str) -> Result<u128> {
        if !is_address(token) {
            bail!("invalid token address for UI multiplier");
        }
        let raw = self
            .eth_call(&json!({"to": token, "data": UI_MULTIPLIER_SELECTOR}))
            .await?;
        let multiplier = parse_abi_u128_word(&raw, 0)?;
        if multiplier == 0 {
            bail!("token uiMultiplier is zero");
        }
        Ok(multiplier)
    }

    pub async fn transaction_receipt(&self, tx_hash: &str) -> Result<Option<Value>> {
        if !tx_hash.starts_with("0x")
            || tx_hash.len() != 66
            || !tx_hash[2..].chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!("invalid transaction hash");
        }
        let result = self
            .rpc("eth_getTransactionReceipt", json!([tx_hash]))
            .await?;
        match result {
            Value::Null => Ok(None),
            receipt if receipt.is_object() => Ok(Some(receipt)),
            _ => bail!("invalid receipt response"),
        }
    }

    /// Polls a receipt with a bounded attempt count; timeout remains UNKNOWN.
    pub async fn wait_for_transaction_receipt(
        &self,
        tx_hash: &str,
        max_attempts: usize,
        interval: Duration,
    ) -> Result<Option<Value>> {
        if max_attempts == 0 {
            bail!("receipt polling max_attempts must be non-zero");
        }
        for attempt in 0..max_attempts {
            if let Some(receipt) = self.transaction_receipt(tx_hash).await? {
                return Ok(Some(receipt));
            }
            if attempt + 1 < max_attempts {
                tokio::time::sleep(interval).await;
            }
        }
        Ok(None)
    }

    pub async fn token_balance_of(&self, token: &str, owner: &str) -> Result<String> {
        self.token_balance_of_at(token, owner, "latest").await
    }

    pub async fn token_balance_of_at(
        &self,
        token: &str,
        owner: &str,
        block_tag: &str,
    ) -> Result<String> {
        if !is_address(token) || !is_address(owner) || block_tag.trim().is_empty() {
            bail!("invalid token balance address or block tag");
        }
        let mut data = function_selector("balanceOf(address)").to_vec();
        data.extend_from_slice(&abi_address(owner));
        let result = self
            .rpc(
                "eth_call",
                json!([{"to": token, "data": format!("0x{}", hex::encode(data))}, block_tag]),
            )
            .await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) {
            bail!("invalid token balance returned by RPC");
        }
        Ok(value.to_string())
    }

    pub async fn token_allowance(&self, token: &str, owner: &str, spender: &str) -> Result<String> {
        if !is_address(token) || !is_address(owner) || !is_address(spender) {
            bail!("invalid allowance address");
        }
        let mut data = function_selector("allowance(address,address)").to_vec();
        data.extend_from_slice(&abi_address(owner));
        data.extend_from_slice(&abi_address(spender));
        self.eth_call(&json!({"to": token, "data": format!("0x{}", hex::encode(data))}))
            .await
    }

    /// Reads Permit2 allowance(owner, token, spender) => amount, expiration, nonce.
    pub async fn permit2_allowance(
        &self,
        owner: &str,
        token: &str,
        spender: &str,
    ) -> Result<Permit2AllowanceSnapshot> {
        if !is_address(owner) || !is_address(token) || !is_address(spender) {
            bail!("invalid Permit2 allowance address");
        }
        let mut data = function_selector("allowance(address,address,address)").to_vec();
        data.extend_from_slice(&abi_address(owner));
        data.extend_from_slice(&abi_address(token));
        data.extend_from_slice(&abi_address(spender));
        let raw = self
            .eth_call(&json!({"to": PERMIT2, "data": format!("0x{}", hex::encode(data))}))
            .await?;
        Ok(Permit2AllowanceSnapshot {
            owner: owner.to_string(),
            token: token.to_string(),
            spender: spender.to_string(),
            amount: parse_abi_u128_word(&raw, 0)?,
            expiration: parse_abi_u64_word(&raw, 1)?,
            nonce: parse_abi_u64_word(&raw, 2)?,
        })
    }

    /// Quotes a Uniswap V3 single hop using the official Robinhood QuoterV2.
    /// The tuple is static: do not prepend a dynamic outer offset.
    /// Quotes a V4 exact-input multi-hop path through the verified V4Quoter.
    pub async fn v4_quote_exact_input(
        &self,
        currency_in: &str,
        path: &[V4PathKey],
        amount_in: u128,
    ) -> Result<u128> {
        if !is_address(currency_in) || path.is_empty() || amount_in == 0 {
            bail!("invalid V4 multi-hop quote");
        }
        let mut data = function_selector(
            "quoteExactInput((address,(address,uint24,int24,address,bytes)[],uint128))",
        )
        .to_vec();
        let encoded = encode_v4_quote_params(currency_in, path, amount_in)?;
        data.extend_from_slice(&abi_uint(32));
        data.extend(encoded);
        let result = self
            .eth_call(&json!({"to": V4_QUOTER, "data": format!("0x{}", hex::encode(data))}))
            .await?;
        parse_first_abi_word(&result)
    }

    pub async fn v3_quote_exact_input_single(
        &self,
        token_in: &str,
        token_out: &str,
        amount_in: u128,
        fee: u32,
    ) -> Result<u128> {
        for address in [token_in, token_out] {
            if !is_address(address) {
                bail!("invalid V3 quote token address");
            }
        }
        if amount_in == 0 || fee > 1_000_000 {
            bail!("invalid V3 quote amount or fee");
        }
        let mut data =
            function_selector("quoteExactInputSingle((address,address,uint256,uint24,uint160))")
                .to_vec();
        data.extend_from_slice(&abi_address(token_in));
        data.extend_from_slice(&abi_address(token_out));
        data.extend_from_slice(&abi_uint(amount_in));
        data.extend_from_slice(&abi_uint(fee as u128));
        data.extend_from_slice(&abi_uint(0));
        let result = self
            .eth_call(&json!({"to": V3_QUOTER_V2, "data": format!("0x{}", hex::encode(data))}))
            .await?;
        parse_first_abi_word(&result)
    }

    /// Quotes a Uniswap V3 multi-hop via encoded path using QuoterV2.quoteExactInput.
    pub async fn v3_quote_exact_input(
        &self,
        encoded_path_hex: &str,
        amount_in: u128,
    ) -> Result<u128> {
        if amount_in == 0 {
            bail!("amount_in harus > 0");
        }
        let path_bytes = hex::decode(encoded_path_hex.trim_start_matches("0x"))
            .context("encoded_path bukan hex yang valid")?;
        // quoteExactInput(bytes path, uint256 amountIn)
        // path is dynamic bytes, amountIn is uint256
        // calldata: selector + offset_path(32) + amountIn(32) + path_len(32) + path_data
        let sel = function_selector("quoteExactInput(bytes,uint256)");
        let mut data = sel.to_vec();
        data.extend_from_slice(&abi_uint(64u128)); // offset of path = 2*32
        data.extend_from_slice(&abi_uint(amount_in));
        data.extend_from_slice(&abi_uint(path_bytes.len() as u128));
        data.extend_from_slice(&path_bytes);
        while !data.len().is_multiple_of(32) {
            data.push(0);
        }
        let result = self
            .eth_call(&json!({"to": V3_QUOTER_V2, "data": format!("0x{}", hex::encode(data))}))
            .await?;
        parse_first_abi_word(&result)
    }

    /// Returns the V3 pool address for a token pair and fee tier.
    pub async fn v3_pool(&self, token_a: &str, token_b: &str, fee: u32) -> Result<String> {
        for address in [token_a, token_b] {
            if !is_address(address) {
                bail!("invalid V3 pool token address");
            }
        }
        let mut data = function_selector("getPool(address,address,uint24)").to_vec();
        data.extend_from_slice(&abi_address(token_a));
        data.extend_from_slice(&abi_address(token_b));
        data.extend_from_slice(&abi_uint(fee as u128));
        let result = self
            .eth_call(&json!({"to": V3_FACTORY, "data": format!("0x{}", hex::encode(data))}))
            .await?;
        let raw = result
            .strip_prefix("0x")
            .ok_or_else(|| anyhow::anyhow!("invalid V3 pool output"))?;
        if raw.len() < 64 {
            bail!("V3 factory output too short");
        }
        let address = format!("0x{}", &raw[24..64]);
        if address == "0x0000000000000000000000000000000000000000" {
            bail!("V3 pool does not exist");
        }
        Ok(address)
    }

    /// Reads the V4 PoolManager slot0 state for a pool ID at the current latest block.
    pub async fn pool_slot0(&self, pool_id: &str) -> Result<String> {
        if !is_hex_bytes32(pool_id) {
            bail!("invalid V4 pool id");
        }
        let mut data = function_selector("getSlot0(bytes32)").to_vec();
        data.extend_from_slice(&hex::decode(&pool_id[2..])?);
        self.eth_call(&json!({"to": POOL_MANAGER_V4, "data": format!("0x{}", hex::encode(data))}))
            .await
    }

    /// Broadcasts one already-signed transaction through the RPC pool.
    /// The caller owns the live authorization and must reconcile the returned hash.
    pub async fn send_raw_transaction(&self, raw_tx: &str) -> Result<String> {
        if !raw_tx.starts_with("0x") || raw_tx.len() <= 2 {
            bail!("signed raw transaction is empty or malformed");
        }
        let result = self.rpc("eth_sendRawTransaction", json!([raw_tx])).await?;
        let hash = result.as_str().unwrap_or("");
        if !hash.starts_with("0x") || hash.len() != 66 {
            bail!("RPC returned invalid transaction hash");
        }
        Ok(hash.to_string())
    }

    pub fn new(url: &str) -> Result<Self> {
        let urls: Vec<String> = url
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if urls.is_empty() {
            bail!("Robinhood RPC URL is empty");
        }
        Ok(Self {
            cooldowns: Arc::new(Mutex::new(vec![None; urls.len()])),
            rpc_index: Arc::new(AtomicUsize::new(0)),
            urls,
            client: Client::builder().timeout(Duration::from_secs(8)).build()?,
        })
    }

    pub async fn chain_id(&self) -> Result<String> {
        let result = self.rpc("eth_chainId", json!([])).await?;
        let chain_id = result.as_str().unwrap_or("");
        if chain_id != ROBINHOOD_CHAIN_ID_HEX {
            bail!(
                "wrong Robinhood chain id: expected {}, got {}",
                ROBINHOOD_CHAIN_ID_HEX,
                chain_id
            );
        }
        Ok(chain_id.to_string())
    }

    pub async fn validate_deployments(&self) -> Result<()> {
        for address in [
            POOL_MANAGER_V4,
            POSITION_MANAGER_V4,
            V4_QUOTER,
            UNIVERSAL_ROUTER,
            PERMIT2,
        ] {
            let result = self.rpc("eth_getCode", json!([address, "latest"])).await?;
            let code = result.as_str().unwrap_or("0x");
            if code == "0x" || code.is_empty() {
                bail!(
                    "official Robinhood deployment has no contract code: {}",
                    address
                );
            }
        }
        Ok(())
    }

    /// Performs a read-only eth_call against the verified V4Quoter.
    /// ABI calldata is supplied by the caller only after venue-specific encoding is verified.
    pub async fn eth_call_quoter(&self, calldata: &str) -> Result<String> {
        if !calldata.starts_with("0x") || calldata.len() <= 2 {
            bail!("quoter calldata is empty or malformed");
        }
        let block_tag = self.block_number().await?;
        let result = self
            .rpc(
                "eth_call",
                json!([{"to": V4_QUOTER, "data": calldata}, block_tag]),
            )
            .await?;
        let result = result.as_str().unwrap_or("");
        if result.is_empty() || result == "0x" {
            bail!("Robinhood V4Quoter returned empty output");
        }
        Ok(result.to_string())
    }

    pub fn parse_verified_swap(receipt: &Value, swap_topics: &[&str]) -> Result<VerifiedSwap> {
        let status = receipt.get("status").and_then(Value::as_str).unwrap_or("");
        if status != "0x1" {
            bail!("transaction receipt is not successful");
        }
        let tx_hash = receipt
            .get("transactionHash")
            .and_then(Value::as_str)
            .unwrap_or("");
        let block_number = receipt
            .get("blockNumber")
            .and_then(Value::as_str)
            .unwrap_or("");
        let router = receipt.get("to").and_then(Value::as_str).unwrap_or("");
        if tx_hash.is_empty() || block_number.is_empty() || router.is_empty() {
            bail!("receipt missing transactionHash, blockNumber, or router");
        }
        let logs = receipt
            .get("logs")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("receipt missing logs"))?;
        let mut has_transfer = false;
        let mut has_swap = false;
        let mut pool = String::new();
        let mut token_contracts: Vec<String> = Vec::new();
        for log in logs {
            let address = log
                .get("address")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let topics = log
                .get("topics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let topic0 = topics
                .first()
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            if topic0 == ERC20_TRANSFER_TOPIC {
                has_transfer = true;
                if !address.is_empty()
                    && !token_contracts
                        .iter()
                        .any(|v| v.eq_ignore_ascii_case(&address))
                {
                    token_contracts.push(address.clone());
                }
            }
            if swap_topics
                .iter()
                .any(|topic| topic.eq_ignore_ascii_case(&topic0))
            {
                has_swap = true;
                pool = address;
            }
        }
        if !has_transfer || !has_swap || pool.is_empty() {
            bail!("receipt does not contain a verified swap event");
        }
        Ok(VerifiedSwap {
            tx_hash: tx_hash.to_string(),
            block_number: block_number.to_string(),
            router: router.to_string(),
            pool,
            token_contracts,
        })
    }
}

fn abi_word(value: &str, index: usize) -> Result<&str> {
    let raw = value
        .strip_prefix("0x")
        .ok_or_else(|| anyhow::anyhow!("ABI output missing 0x prefix"))?;
    let start = index
        .checked_mul(64)
        .ok_or_else(|| anyhow::anyhow!("ABI word offset overflow"))?;
    let end = start
        .checked_add(64)
        .ok_or_else(|| anyhow::anyhow!("ABI word end overflow"))?;
    raw.get(start..end)
        .ok_or_else(|| anyhow::anyhow!("ABI output missing word {index}"))
}

fn parse_abi_u128_word(value: &str, index: usize) -> Result<u128> {
    let word = abi_word(value, index)?;
    if word[..32].chars().any(|character| character != '0') {
        bail!("ABI word {index} exceeds u128");
    }
    Ok(u128::from_str_radix(&word[32..], 16)?)
}

fn parse_abi_u64_word(value: &str, index: usize) -> Result<u64> {
    let word = abi_word(value, index)?;
    if word[..48].chars().any(|character| character != '0') {
        bail!("ABI word {index} exceeds u64");
    }
    Ok(u64::from_str_radix(&word[48..], 16)?)
}

fn parse_positive_int256_word(value: &str, index: usize) -> Result<u128> {
    let word = abi_word(value, index)?;
    if word.as_bytes()[0] >= b'8' {
        bail!("Chainlink answer is negative");
    }
    parse_abi_u128_word(value, index)
}

fn is_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn is_hex_quantity(value: &str) -> bool {
    value.starts_with("0x") && value.len() > 2 && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_quantity_u128(value: &str) -> Result<u128> {
    if !is_hex_quantity(value) {
        bail!("invalid hexadecimal quantity");
    }
    Ok(u128::from_str_radix(&value[2..], 16)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt(status: &str, swap: bool) -> Value {
        let mut logs = vec![json!({"address":"0xtoken","topics":[ERC20_TRANSFER_TOPIC]})];
        if swap {
            logs.push(json!({"address":"0xpool","topics":["0xswap"]}));
        }
        json!({"status":status,"transactionHash":"0xtx","blockNumber":"0x10","to":"0xrouter","logs":logs})
    }

    #[test]
    fn receipt_transaction_hash_mismatch_is_rejected() {
        let expected = format!("0x{}", "11".repeat(32));
        let actual = format!("0x{}", "22".repeat(32));
        let receipt = json!({"transactionHash": actual});
        assert!(validate_receipt_transaction_hash(&expected, &receipt).is_err());
    }

    #[test]
    fn receipt_transaction_hash_match_is_accepted_case_insensitively() {
        let expected = format!("0x{}", "ab".repeat(32));
        let receipt = json!({"transactionHash": format!("0x{}", expected[2..].to_uppercase())});
        assert!(validate_receipt_transaction_hash(&expected, &receipt).is_ok());
    }

    #[test]
    fn receipt_missing_transaction_hash_is_rejected() {
        let expected = format!("0x{}", "11".repeat(32));
        assert!(validate_receipt_transaction_hash(&expected, &json!({})).is_err());
    }

    fn quote(expires_at_unix: u64) -> RobinhoodQuote {
        RobinhoodQuote {
            token_in: "0x0000000000000000000000000000000000000001".to_string(),
            token_out: "0x0000000000000000000000000000000000000002".to_string(),
            amount_in: "0x10".to_string(),
            amount_out: "0x20".to_string(),
            quoted_at_block: "0x10".to_string(),
            expires_at_unix,
        }
    }

    #[test]
    fn permit2_eip712_request_accepts_verified_domain() {
        let request = Permit2Eip712Request {
            owner: "0x0000000000000000000000000000000000000001".to_string(),
            token: "0x0000000000000000000000000000000000000002".to_string(),
            spender: UNIVERSAL_ROUTER.to_string(),
            amount: 100,
            expiration: 2_000,
            nonce: 7,
            sig_deadline: 1_500,
            chain_id: 4663,
            verifying_contract: PERMIT2.to_string(),
        };
        assert!(request.validate(1_000).is_ok());
    }

    #[test]
    fn permit2_eip712_request_rejects_wrong_domain_or_expiry() {
        let mut request = Permit2Eip712Request {
            owner: "0x0000000000000000000000000000000000000001".to_string(),
            token: "0x0000000000000000000000000000000000000002".to_string(),
            spender: UNIVERSAL_ROUTER.to_string(),
            amount: 100,
            expiration: 2_000,
            nonce: 7,
            sig_deadline: 1_500,
            chain_id: 1,
            verifying_contract: PERMIT2.to_string(),
        };
        assert!(request.validate(1_000).is_err());
        request.chain_id = 4663;
        request.verifying_contract = "0x0000000000000000000000000000000000000003".to_string();
        assert!(request.validate(1_000).is_err());
        request.verifying_contract = PERMIT2.to_string();
        request.sig_deadline = 1_000;
        assert!(request.validate(1_000).is_err());
    }

    #[test]
    fn parses_official_asset_metadata_for_robinhood_deployment() {
        let asset = json!({
            "id": "0x0000000000000000000000000000000000000000000000000000000000000001",
            "tokenSymbol": "TEST",
            "deployments": [{"contractAddress": "0x0000000000000000000000000000000000000002", "chainId": 4663}],
            "currentMultiplier": "1.000000000000000000",
            "status": "ASSET_STATUS_ACTIVE",
            "tradingCapabilities": {
                "fractionalTradability": "tradable",
                "allDayTradability": "untradable",
                "extendedHoursFractionalTradability": false
            }
        });
        let metadata = RobinhoodAssetMetadata::from_api_value(&asset).unwrap();
        assert_eq!(metadata.chain_id, 4663);
        assert_eq!(metadata.symbol, "TEST");
        assert_eq!(
            metadata.contract_address,
            "0x0000000000000000000000000000000000000002"
        );
        assert_eq!(
            metadata
                .trading_capabilities
                .unwrap()
                .all_day_tradability
                .as_deref(),
            Some("untradable")
        );
    }

    #[test]
    fn rejects_asset_metadata_without_robinhood_deployment() {
        let asset = json!({
            "id": "0x0000000000000000000000000000000000000000000000000000000000000001",
            "tokenSymbol": "TEST",
            "deployments": [{"contractAddress": "0x0000000000000000000000000000000000000002", "chainId": 46630}],
            "currentMultiplier": "1.000000000000000000",
            "status": "ASSET_STATUS_ACTIVE"
        });
        assert!(RobinhoodAssetMetadata::from_api_value(&asset).is_err());
    }

    #[test]
    fn sequencer_uptime_snapshot_accepts_up_after_grace_period() {
        let snapshot = SequencerUptimeSnapshot {
            feed: "0x0000000000000000000000000000000000000001".to_string(),
            status: 0,
            started_at: 600,
            updated_at: 950,
            observed_at: 1_000,
            heartbeat_secs: 300,
            grace_period_secs: 300,
        };
        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn sequencer_uptime_snapshot_rejects_down_stale_and_grace_period() {
        let base = SequencerUptimeSnapshot {
            feed: "0x0000000000000000000000000000000000000001".to_string(),
            status: 1,
            started_at: 1,
            updated_at: 900,
            observed_at: 1_000,
            heartbeat_secs: 300,
            grace_period_secs: 300,
        };
        assert!(base.validate().is_err());
        assert!(SequencerUptimeSnapshot {
            updated_at: 1,
            status: 0,
            ..base.clone()
        }
        .validate()
        .is_err());
        assert!(SequencerUptimeSnapshot {
            started_at: 800,
            status: 0,
            ..base
        }
        .validate()
        .is_err());
    }

    #[test]
    fn chainlink_snapshot_accepts_fresh_positive_answer() {
        let snapshot = ChainlinkPriceSnapshot {
            feed: "0x0000000000000000000000000000000000000001".to_string(),
            answer: 123_456_789,
            decimals: 8,
            updated_at: 900,
            observed_at: 1_000,
            heartbeat_secs: 300,
        };
        assert!(snapshot.validate().is_ok());
    }

    #[test]
    fn chainlink_snapshot_rejects_stale_or_future_data() {
        let stale = ChainlinkPriceSnapshot {
            feed: "0x0000000000000000000000000000000000000001".to_string(),
            answer: 1,
            decimals: 8,
            updated_at: 1,
            observed_at: 1_000,
            heartbeat_secs: 300,
        };
        assert!(stale.validate().is_err());
        let future = ChainlinkPriceSnapshot {
            updated_at: 1_001,
            ..stale
        };
        assert!(future.validate().is_err());
    }

    #[test]
    fn chainlink_abi_decoding_rejects_negative_and_short_words() {
        let negative = format!("0x{}", "f".repeat(64 * 5));
        assert!(parse_positive_int256_word(&negative, 1).is_err());
        assert!(parse_abi_u64_word("0x00", 0).is_err());
    }

    #[test]
    fn transfer_only_is_rejected() {
        assert!(RobinhoodRpc::parse_verified_swap(&receipt("0x1", false), &["0xswap"]).is_err());
    }

    #[test]
    fn classifier_keeps_unknown_transfer_fail_closed() {
        let receipt = json!({
            "status": "0x1",
            "logs": [{
                "address": "0x0000000000000000000000000000000000000009",
                "topics": [ERC20_TRANSFER_TOPIC]
            }]
        });
        let classified =
            classify_robinhood_receipt(&receipt, "0x0000000000000000000000000000000000000001")
                .unwrap();
        assert_eq!(classified.venue, RobinhoodVenue::TransferOnly);
        assert!(classified.has_transfer);
    }

    #[test]
    fn classifier_identifies_pons_v2_curve_router() {
        let receipt = json!({
            "status": "0x1",
            "logs": [
                {"address": PONS_V2_LAUNCH_ROUTER, "topics": []},
                {"address": "0x0000000000000000000000000000000000000009", "topics": [ERC20_TRANSFER_TOPIC]}
            ]
        });
        let classified =
            classify_robinhood_receipt(&receipt, "0x0000000000000000000000000000000000000001")
                .unwrap();
        assert_eq!(classified.venue, RobinhoodVenue::PonsV2Curve);
    }
    #[test]
    fn failed_receipt_is_rejected() {
        assert!(RobinhoodRpc::parse_verified_swap(&receipt("0x0", true), &["0xswap"]).is_err());
    }
    #[test]
    fn successful_swap_is_accepted() {
        let parsed = RobinhoodRpc::parse_verified_swap(&receipt("0x1", true), &["0xswap"]).unwrap();
        assert_eq!(parsed.tx_hash, "0xtx");
        assert_eq!(parsed.pool, "0xpool");
    }
    #[test]
    fn pool_key_calldata_has_outer_tuple_offset() {
        let request = V4QuoteExactInputSingle {
            pool_key: V4PoolKey {
                currency0: "0x0000000000000000000000000000000000000001".to_string(),
                currency1: "0x0000000000000000000000000000000000000002".to_string(),
                fee: 3000,
                tick_spacing: 60,
                hooks: "0x0000000000000000000000000000000000000003".to_string(),
            },
            zero_for_one: true,
            exact_amount: 1000,
            hook_data: vec![],
        };
        let data = request.encode_calldata().unwrap();
        assert!(data.starts_with("0x"));
        assert_eq!((data.len() - 2 - 8) % 64, 0);
    }

    #[test]
    fn universal_router_reverse_direction_targets_input_and_output_currencies() {
        let swap = UniversalRouterV4Swap {
            pool_key: V4PoolKey {
                currency0: "0x0000000000000000000000000000000000000001".to_string(),
                currency1: "0x0000000000000000000000000000000000000002".to_string(),
                fee: 3000,
                tick_spacing: 60,
                hooks: "0x0000000000000000000000000000000000000003".to_string(),
            },
            zero_for_one: false,
            amount_in: 1_000,
            amount_out_minimum: 900,
            min_hop_price_x36: 0,
            hook_data: Vec::new(),
            deadline: 1_900_000_000,
        };
        let calldata = swap.encode_calldata().unwrap();
        let input_word = format!("{:0>64}", "2");
        let output_word = format!("{:0>64}", "1");
        assert!(calldata.contains(&input_word));
        assert!(calldata.contains(&output_word));
    }

    #[test]
    fn universal_router_v4_calldata_is_deterministic_and_targets_execute() {
        let swap = UniversalRouterV4Swap {
            pool_key: V4PoolKey {
                currency0: "0x0000000000000000000000000000000000000001".to_string(),
                currency1: "0x0000000000000000000000000000000000000002".to_string(),
                fee: 3000,
                tick_spacing: 60,
                hooks: "0x0000000000000000000000000000000000000003".to_string(),
            },
            zero_for_one: true,
            amount_in: 1_000,
            amount_out_minimum: 900,
            min_hop_price_x36: 0,
            hook_data: Vec::new(),
            deadline: 1_900_000_000,
        };
        let calldata = swap.encode_calldata().unwrap();
        let selector = hex::encode(function_selector("execute(bytes,bytes[],uint256)"));
        assert!(calldata.starts_with(&format!("0x{}", selector)));
        assert_eq!((calldata.len() - 2 - 8) % 64, 0);
        assert!(calldata.len() > 10);
    }

    #[test]
    fn transaction_plan_rejects_broadcast_and_wrong_chain() {
        let mut plan = EvmTransactionPlan {
            from: "0x0000000000000000000000000000000000000001".to_string(),
            to: UNIVERSAL_ROUTER.to_string(),
            data: "0x12345678".to_string(),
            value: "0x0".to_string(),
            nonce: "0x1".to_string(),
            gas_limit: "0x5208".to_string(),
            gas_price: "0x1".to_string(),
            chain_id: ROBINHOOD_CHAIN_ID_HEX.to_string(),
            broadcast_allowed: false,
        };
        assert!(plan.validate_dry_run().is_ok());
        plan.broadcast_allowed = true;
        assert!(plan.validate_dry_run().is_err());
        plan.broadcast_allowed = false;
        plan.chain_id = "0x1".to_string();
        assert!(plan.validate_dry_run().is_err());
    }

    #[test]
    fn router_plan_rejects_unapproved_targets() {
        let plan = UniversalRouterV4Plan {
            router: "0x0000000000000000000000000000000000000001".to_string(),
            permit2: PERMIT2.to_string(),
            token_in: "0x0000000000000000000000000000000000000002".to_string(),
            token_out: "0x0000000000000000000000000000000000000003".to_string(),
            amount_in: "0x1".to_string(),
            amount_out_minimum: "0x1".to_string(),
            deadline: 1,
            calldata: "0x12345678".to_string(),
        };
        assert!(plan.validate().is_err());
    }

    #[test]
    fn buy_reconciliation_calculates_integer_fill_price() {
        let reconciliation = EvmSwapReconciliation {
            token_in: "0x0000000000000000000000000000000000000001".to_string(),
            token_out: "0x0000000000000000000000000000000000000002".to_string(),
            input_balance_before: "0x64".to_string(),
            input_balance_after: "0x32".to_string(),
            output_balance_before: "0xa".to_string(),
            output_balance_after: "0x78".to_string(),
            actual_amount_in: 50,
            actual_amount_out: 110,
            minimum_amount_out: 100,
        };
        assert_eq!(
            reconciliation.fill_price_x18().unwrap(),
            2_200_000_000_000_000_000
        );
    }

    #[test]
    fn buy_reconciliation_accepts_matching_balance_deltas() {
        let reconciliation = EvmSwapReconciliation {
            token_in: "0x0000000000000000000000000000000000000001".to_string(),
            token_out: "0x0000000000000000000000000000000000000002".to_string(),
            input_balance_before: "0x64".to_string(),
            input_balance_after: "0x32".to_string(),
            output_balance_before: "0xa".to_string(),
            output_balance_after: "0x78".to_string(),
            actual_amount_in: 50,
            actual_amount_out: 110,
            minimum_amount_out: 100,
        };
        assert!(reconciliation.validate_buy().is_ok());
    }

    #[test]
    fn buy_reconciliation_rejects_mismatch_and_slippage() {
        let reconciliation = EvmSwapReconciliation {
            token_in: "0x0000000000000000000000000000000000000001".to_string(),
            token_out: "0x0000000000000000000000000000000000000002".to_string(),
            input_balance_before: "0x64".to_string(),
            input_balance_after: "0x32".to_string(),
            output_balance_before: "0xa".to_string(),
            output_balance_after: "0x32".to_string(),
            actual_amount_in: 50,
            actual_amount_out: 40,
            minimum_amount_out: 100,
        };
        assert!(reconciliation.validate_buy().is_err());
    }

    #[test]
    fn balance_snapshot_rejects_invalid_values() {
        let snapshot = EvmBalanceSnapshot {
            native_balance: "0x1".to_string(),
            token_balance_before: Some("not_hex".to_string()),
            token_balance_after: Some("0x2".to_string()),
            block: "0x10".to_string(),
        };
        assert!(snapshot.validate().is_err());
    }

    #[test]
    fn valid_quote_is_accepted() {
        assert!(quote(200).validate(100).is_ok());
    }
    #[test]
    fn expired_quote_is_rejected() {
        assert!(quote(100).validate(100).is_err());
    }
    #[test]
    fn zero_output_quote_is_rejected() {
        let mut q = quote(200);
        q.amount_out = "0x0".to_string();
        assert!(q.validate(100).is_err());
    }
}
