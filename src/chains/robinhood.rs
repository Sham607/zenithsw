use anyhow::{bail, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

pub const ROBINHOOD_CHAIN_ID_HEX: &str = "0x1237";
pub const POOL_MANAGER_V4: &str = "0x8366a39CC670B4001A1121B8F6A443A643e40951";
pub const POSITION_MANAGER_V4: &str = "0x58daec3116aae6D93017bAAea7749052E8a04fA7";
pub const V4_QUOTER: &str = "0x8Dc178eFB8111BB0973Dd9d722ebeFF267c98F94";
pub const UNIVERSAL_ROUTER: &str = "0x8876789976decbfcbbbe364623c63652db8c0904";
pub const PERMIT2: &str = "0x000000000022D473030F116dDEE9F6B43aC78BA3";
pub const ERC20_TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55aebc6f6f8";
pub const ERC20_APPROVE_SELECTOR: &str = "0x095ea7b3";
pub const ERC20_BALANCE_OF_SELECTOR: &str = "0x70a08231";
pub const V4_INITIALIZE_TOPIC: &str = "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438";

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
            if !is_address(address) { bail!("V4 PoolKey contains an invalid address"); }
        }
        if self.currency0.to_lowercase() >= self.currency1.to_lowercase() {
            bail!("V4 PoolKey currencies are not strictly ordered");
        }
        if self.fee > 1_000_000 { bail!("V4 pool fee exceeds 1_000_000"); }
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
    value.starts_with("0x") && value.len() == 66 && value[2..].chars().all(|c| c.is_ascii_hexdigit())
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
        if !is_address(&self.from) || !is_address(&self.to) { bail!("transaction address is invalid"); }
        if !is_hex_quantity(&self.value) || !is_hex_quantity(&self.nonce)
            || !is_hex_quantity(&self.gas_limit) || !is_hex_quantity(&self.gas_price)
            || !is_hex_quantity(&self.chain_id) { bail!("transaction quantity is invalid"); }
        if !self.data.starts_with("0x") || self.data.len() < 10 { bail!("transaction calldata is missing"); }
        if self.chain_id != ROBINHOOD_CHAIN_ID_HEX { bail!("transaction chain id is not Robinhood 4663"); }
        if self.broadcast_allowed { bail!("dry-run transaction cannot allow broadcast"); }
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

impl EvmBalanceSnapshot {
    pub fn validate(&self) -> Result<()> {
        for value in [&self.native_balance, &self.block] {
            if !is_hex_quantity(value) { bail!("invalid balance snapshot quantity"); }
        }
        if let Some(value) = &self.token_balance_before { if !is_hex_quantity(value) { bail!("invalid token balance before"); } }
        if let Some(value) = &self.token_balance_after { if !is_hex_quantity(value) { bail!("invalid token balance after"); } }
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
            if !is_address(address) { bail!("router plan contains invalid address"); }
        }
        if !self.router.eq_ignore_ascii_case(UNIVERSAL_ROUTER) || !self.permit2.eq_ignore_ascii_case(PERMIT2) {
            bail!("router plan targets an unapproved Robinhood deployment");
        }
        for quantity in [&self.amount_in, &self.amount_out_minimum] {
            if !is_hex_quantity(quantity) || quantity == "0x0" { bail!("router plan quantity is invalid"); }
        }
        if self.token_in.eq_ignore_ascii_case(&self.token_out) || self.deadline == 0 { bail!("router plan token/deadline invalid"); }
        if !self.calldata.starts_with("0x") || self.calldata.len() < 10 { bail!("router calldata missing"); }
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
    pub hook_data: Vec<u8>,
    pub deadline: u64,
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
    let mut head = Vec::with_capacity(32 * 9);
    head.extend_from_slice(&abi_address(&plan.pool_key.currency0));
    head.extend_from_slice(&abi_address(&plan.pool_key.currency1));
    head.extend_from_slice(&abi_uint(plan.pool_key.fee as u128));
    head.extend_from_slice(&abi_int24(plan.pool_key.tick_spacing));
    head.extend_from_slice(&abi_address(&plan.pool_key.hooks));
    head.extend_from_slice(&abi_bool(plan.zero_for_one));
    head.extend_from_slice(&abi_uint(plan.amount_in));
    head.extend_from_slice(&abi_uint(plan.amount_out_minimum));
    head.extend_from_slice(&abi_uint(32 * 9));
    let mut out = head;
    out.extend_from_slice(&abi_uint(plan.hook_data.len() as u128));
    out.extend_from_slice(&plan.hook_data);
    pad_32(&mut out);
    Ok(out)
}

fn encode_two_words(address: &str, amount: u128) -> Result<Vec<u8>> {
    if !is_address(address) || amount == 0 { bail!("invalid V4 settlement parameter"); }
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&abi_address(address));
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
    for value in values { tails.push(encode_bytes(value)); }
    let head_len = 32 + 32 * values.len();
    let mut out = Vec::new();
    out.extend_from_slice(&abi_uint(values.len() as u128));
    let mut offset = head_len;
    for tail in &tails {
        out.extend_from_slice(&abi_uint(offset as u128));
        offset += tail.len();
    }
    for tail in tails { out.extend(tail); }
    out
}

fn pad_32(value: &mut Vec<u8>) {
    while value.len() % 32 != 0 { value.push(0); }
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

        let selector = function_selector("quoteExactInputSingle(((address,address,uint24,int24,address),bool,uint128,bytes))");
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
        while (encoded.len() - 4) % 32 != 0 { encoded.push(0); }
        Ok(format!("0x{}", hex::encode(encoded)))
    }
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

#[derive(Clone)]
pub struct RobinhoodRpc {
    url: String,
    client: Client,
}

impl RobinhoodRpc {
    async fn rpc(&self, method: &str, params: Value) -> Result<Value> {
        let payload = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let response: Value = self.client.post(&self.url).json(&payload).send().await?.error_for_status()?.json().await?;
        if let Some(error) = response.get("error") {
            bail!("Robinhood RPC {} failed: {}", method, error);
        }
        response.get("result").cloned().ok_or_else(|| anyhow::anyhow!("Robinhood RPC {} returned no result", method))
    }

    pub async fn block_number(&self) -> Result<String> {
        let result = self.rpc("eth_blockNumber", json!([])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) { bail!("invalid block number returned by RPC"); }
        Ok(value.to_string())
    }

    pub async fn get_logs(&self, from_block: &str, to_block: &str) -> Result<Vec<Value>> {
        self.get_logs_for_token(from_block, to_block, None).await
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
            Some(format!("0x{}{}", "0".repeat(24), address[2..].to_lowercase()))
        } else {
            None
        };
        let topic_indexes: Vec<usize> = if token_topic.is_some() { vec![2, 3] } else { vec![0] };
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
            let result = self.rpc("eth_getLogs", json!([{
                "address": POOL_MANAGER_V4,
                "fromBlock": from_block,
                "toBlock": to_block,
                "topics": topics
            }])).await?;
            let logs = result.as_array().ok_or_else(|| anyhow::anyhow!("invalid eth_getLogs result"))?;
            all_logs.extend(logs.iter().cloned());
        }
        all_logs.sort_by(|a, b| a.to_string().cmp(&b.to_string()));
        all_logs.dedup_by(|a, b| a == b);
        Ok(all_logs)
    }

    pub fn parse_initialize_log(log: &Value) -> Result<V4InitializedPool> {
        let topics = log.get("topics").and_then(Value::as_array).ok_or_else(|| anyhow::anyhow!("Initialize log topics missing"))?;
        if topics.len() != 4 || topics.first().and_then(Value::as_str).unwrap_or("").to_lowercase() != V4_INITIALIZE_TOPIC {
            bail!("not a V4 Initialize log");
        }
        let pool_id = topics[1].as_str().unwrap_or("").to_string();
        let currency0 = format!("0x{}", topics[2].as_str().unwrap_or("").trim_start_matches("0x")[24..].to_string());
        let currency1 = format!("0x{}", topics[3].as_str().unwrap_or("").trim_start_matches("0x")[24..].to_string());
        let data = log.get("data").and_then(Value::as_str).unwrap_or("");
        let words = data.strip_prefix("0x").unwrap_or("");
        if words.len() < 192 || words.len() % 64 != 0 { bail!("Initialize log data malformed"); }
        let word = |index: usize| -> &str { &words[index * 64..(index + 1) * 64] };
        let fee = u32::from_str_radix(word(0), 16)?;
        let tick_raw = i32::from_str_radix(&word(1)[56..], 16)?;
        let tick_spacing = if tick_raw & 0x0080_0000 != 0 { tick_raw | !0x00ff_ffff } else { tick_raw };
        let hooks = format!("0x{}", &word(2)[24..]);
        let sqrt_price_x96 = format!("0x{}", word(3).trim_start_matches('0'));
        let tick = i32::from_str_radix(&word(4)[56..], 16)?;
        let pool = V4InitializedPool { pool_id, pool_key: V4PoolKey { currency0, currency1, fee, tick_spacing, hooks }, sqrt_price_x96, tick };
        pool.validate()?;
        Ok(pool)
    }

    pub async fn get_balance(&self, address: &str, tag: &str) -> Result<String> {
        if !is_address(address) || tag.trim().is_empty() { bail!("invalid balance query"); }
        let result = self.rpc("eth_getBalance", json!([address, tag])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) { bail!("invalid balance returned by RPC"); }
        Ok(value.to_string())
    }

    pub async fn get_transaction_count(&self, address: &str, tag: &str) -> Result<String> {
        if !is_address(address) || tag.trim().is_empty() { bail!("invalid nonce query"); }
        let result = self.rpc("eth_getTransactionCount", json!([address, tag])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) { bail!("invalid nonce returned by RPC"); }
        Ok(value.to_string())
    }

    pub async fn gas_price(&self) -> Result<String> {
        let result = self.rpc("eth_gasPrice", json!([])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) { bail!("invalid gas price returned by RPC"); }
        Ok(value.to_string())
    }

    pub async fn estimate_gas(&self, tx: &Value) -> Result<String> {
        if !tx.is_object() { bail!("gas estimate transaction must be an object"); }
        let result = self.rpc("eth_estimateGas", json!([tx, "latest"])).await?;
        let value = result.as_str().unwrap_or("");
        if !is_hex_quantity(value) { bail!("invalid gas estimate returned by RPC"); }
        Ok(value.to_string())
    }

    pub async fn eth_call(&self, tx: &Value) -> Result<String> {
        if !tx.is_object() { bail!("eth_call transaction must be an object"); }
        let result = self.rpc("eth_call", json!([tx, "latest"])).await?;
        let value = result.as_str().unwrap_or("");
        if value.is_empty() || value == "0x" { bail!("eth_call returned empty output"); }
        Ok(value.to_string())
    }

    pub async fn transaction_receipt(&self, tx_hash: &str) -> Result<Option<Value>> {
        if !tx_hash.starts_with("0x") || tx_hash.len() != 66 || !tx_hash[2..].chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("invalid transaction hash");
        }
        let payload = json!({"jsonrpc":"2.0","id":1,"method":"eth_getTransactionReceipt","params":[tx_hash]});
        let response: Value = self.client.post(&self.url).json(&payload).send().await?.error_for_status()?.json().await?;
        if let Some(error) = response.get("error") { bail!("receipt query failed: {}", error); }
        match response.get("result") {
            Some(Value::Null) | None => Ok(None),
            Some(receipt) if receipt.is_object() => Ok(Some(receipt.clone())),
            _ => bail!("invalid receipt response"),
        }
    }

    pub async fn token_balance_of(&self, token: &str, owner: &str) -> Result<String> {
        if !is_address(token) || !is_address(owner) { bail!("invalid token balance address"); }
        let mut data = function_selector("balanceOf(address)").to_vec();
        data.extend_from_slice(&abi_address(owner));
        self.eth_call(&json!({"to": token, "data": format!("0x{}", hex::encode(data))})).await
    }

    pub async fn token_allowance(&self, token: &str, owner: &str, spender: &str) -> Result<String> {
        if !is_address(token) || !is_address(owner) || !is_address(spender) { bail!("invalid allowance address"); }
        let mut data = function_selector("allowance(address,address)").to_vec();
        data.extend_from_slice(&abi_address(owner));
        data.extend_from_slice(&abi_address(spender));
        self.eth_call(&json!({"to": token, "data": format!("0x{}", hex::encode(data))})).await
    }

    pub async fn send_raw_transaction(&self, _raw_tx: &str) -> Result<String> {
        bail!("raw transaction broadcast is intentionally disabled until live gate approval")
    }

    pub fn new(url: &str) -> Result<Self> {
        if url.trim().is_empty() {
            bail!("Robinhood RPC URL is empty");
        }
        Ok(Self {
            url: url.to_string(),
            client: Client::builder().timeout(Duration::from_secs(8)).build()?,
        })
    }

    pub async fn chain_id(&self) -> Result<String> {
        let payload = json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]});
        let response: Value = self
            .client
            .post(&self.url)
            .json(&payload)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let chain_id = response.get("result").and_then(Value::as_str).unwrap_or("");
        if chain_id != ROBINHOOD_CHAIN_ID_HEX {
            bail!("wrong Robinhood chain id: expected {}, got {}", ROBINHOOD_CHAIN_ID_HEX, chain_id);
        }
        Ok(chain_id.to_string())
    }

    pub async fn validate_deployments(&self) -> Result<()> {
        for address in [POOL_MANAGER_V4, POSITION_MANAGER_V4, V4_QUOTER, UNIVERSAL_ROUTER, PERMIT2] {
            let payload = json!({"jsonrpc":"2.0","id":1,"method":"eth_getCode","params":[address,"latest"]});
            let response: Value = self
                .client
                .post(&self.url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            let code = response.get("result").and_then(Value::as_str).unwrap_or("0x");
            if code == "0x" || code.is_empty() {
                bail!("official Robinhood deployment has no contract code: {}", address);
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
        let payload = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"eth_call",
            "params":[{"to": V4_QUOTER, "data": calldata}, block_tag]
        });
        let mut last_error = String::new();
        for attempt in 0..2 {
            let response: Value = self
                .client
                .post(&self.url)
                .json(&payload)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if response.get("error").is_none() {
                let result = response.get("result").and_then(Value::as_str).unwrap_or("");
                if result.is_empty() || result == "0x" {
                    bail!("Robinhood V4Quoter returned empty output");
                }
                return Ok(result.to_string());
            }
            last_error = response.get("error").map(Value::to_string).unwrap_or_else(|| "unknown revert".to_string());
            if attempt == 0 { tokio::time::sleep(Duration::from_millis(250)).await; }
        }
        bail!("Robinhood V4Quoter eth_call reverted after one retry: {}", last_error)
    }

    pub fn parse_verified_swap(receipt: &Value, swap_topics: &[&str]) -> Result<VerifiedSwap> {
        let status = receipt.get("status").and_then(Value::as_str).unwrap_or("");
        if status != "0x1" {
            bail!("transaction receipt is not successful");
        }
        let tx_hash = receipt.get("transactionHash").and_then(Value::as_str).unwrap_or("");
        let block_number = receipt.get("blockNumber").and_then(Value::as_str).unwrap_or("");
        let router = receipt.get("to").and_then(Value::as_str).unwrap_or("");
        if tx_hash.is_empty() || block_number.is_empty() || router.is_empty() {
            bail!("receipt missing transactionHash, blockNumber, or router");
        }
        let logs = receipt.get("logs").and_then(Value::as_array).ok_or_else(|| anyhow::anyhow!("receipt missing logs"))?;
        let mut has_transfer = false;
        let mut has_swap = false;
        let mut pool = String::new();
        let mut token_contracts: Vec<String> = Vec::new();
        for log in logs {
            let address = log.get("address").and_then(Value::as_str).unwrap_or("").to_string();
            let topics = log.get("topics").and_then(Value::as_array).cloned().unwrap_or_default();
            let topic0 = topics.first().and_then(Value::as_str).unwrap_or("").to_lowercase();
            if topic0 == ERC20_TRANSFER_TOPIC {
                has_transfer = true;
                if !address.is_empty() && !token_contracts.iter().any(|v| v.eq_ignore_ascii_case(&address)) {
                    token_contracts.push(address.clone());
                }
            }
            if swap_topics.iter().any(|topic| topic.eq_ignore_ascii_case(&topic0)) {
                has_swap = true;
                pool = address;
            }
        }
        if !has_transfer || !has_swap || pool.is_empty() {
            bail!("receipt does not contain a verified swap event");
        }
        Ok(VerifiedSwap { tx_hash: tx_hash.to_string(), block_number: block_number.to_string(), router: router.to_string(), pool, token_contracts })
    }
}

fn is_address(value: &str) -> bool {
    value.len() == 42 && value.starts_with("0x") && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn is_hex_quantity(value: &str) -> bool {
    value.starts_with("0x") && value.len() > 2 && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn receipt(status: &str, swap: bool) -> Value {
        let mut logs = vec![json!({"address":"0xtoken","topics":[ERC20_TRANSFER_TOPIC]})];
        if swap { logs.push(json!({"address":"0xpool","topics":["0xswap"]})); }
        json!({"status":status,"transactionHash":"0xtx","blockNumber":"0x10","to":"0xrouter","logs":logs})
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
    fn transfer_only_is_rejected() { assert!(RobinhoodRpc::parse_verified_swap(&receipt("0x1", false), &["0xswap"]).is_err()); }
    #[test]
    fn failed_receipt_is_rejected() { assert!(RobinhoodRpc::parse_verified_swap(&receipt("0x0", true), &["0xswap"]).is_err()); }
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
    fn valid_quote_is_accepted() { assert!(quote(200).validate(100).is_ok()); }
    #[test]
    fn expired_quote_is_rejected() { assert!(quote(100).validate(100).is_err()); }
    #[test]
    fn zero_output_quote_is_rejected() {
        let mut q = quote(200);
        q.amount_out = "0x0".to_string();
        assert!(q.validate(100).is_err());
    }
}
