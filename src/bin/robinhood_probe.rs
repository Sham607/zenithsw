use anyhow::Result;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::env;
use std::time::Duration;

const DEFAULT_TOKEN: &str = "0x451b42a15100c340ca12f7c66de06fac5ea2d751";
const ADDRESS_LEN: usize = 42;
const POOL_MANAGER: &str = "0x8366a39CC670B4001A1121B8F6A443A643e40951";
const INIT_TOPIC: &str = "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438";

fn address_from_key(raw: &str) -> Result<String> {
    let key = raw.trim_start_matches("0x");
    let secret = SecretKey::from_slice(&hex::decode(key)?)?;
    let point = secret.public_key().to_encoded_point(false);
    let hash = Keccak256::digest(&point.as_bytes()[1..]);
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Result<Value> {
    let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    for attempt in 0..4u64 {
        let response = client.post(url).json(&request).send().await?;
        let status = response.status();
        if status.as_u16() == 429 || status.is_server_error() {
            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
            continue;
        }
        let response: Value = response.error_for_status()?.json().await?;
        if let Some(error) = response.get("error") { anyhow::bail!("{}: {}", method, error); }
        return Ok(response["result"].clone());
    }
    anyhow::bail!("{}: RPC rate-limited after retries", method)
}

async fn token_call(client: &reqwest::Client, url: &str, token_address: &str, data: &[u8]) -> Result<Value> {
    rpc(client, url, "eth_call", json!([{"to":token_address,"data":format!("0x{}",hex::encode(data))},"latest"])).await
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let url = env::var("ROBINHOOD_RPC_URL")?;
    let token = env::var("ROBINHOOD_TOKEN_ADDRESS").ok();
    if let Some(address) = token.as_deref() {
        if address.len() != ADDRESS_LEN || !address.starts_with("0x") || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
            anyhow::bail!("ROBINHOOD_TOKEN_ADDRESS invalid");
        }
    }
    let wallet = match env::var("ROBINHOOD_WALLET_ADDRESS") {
        Ok(address) => {
            if address.len() != ADDRESS_LEN || !address.starts_with("0x") || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
                anyhow::bail!("ROBINHOOD_WALLET_ADDRESS invalid");
            }
            address
        }
        Err(_) => {
            let key = env::var("ROBINHOOD_PRIVATE_KEY").or_else(|_| env::var("EVM_PRIVATE_KEY"))?;
            address_from_key(&key)?
        }
    };
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build()?;
    let chain = rpc(&client, &url, "eth_chainId", json!([])).await?;
    let block = rpc(&client, &url, "eth_blockNumber", json!([])).await?;
    let balance = rpc(&client, &url, "eth_getBalance", json!([&wallet, "latest"])).await?;
    let nonce = rpc(&client, &url, "eth_getTransactionCount", json!([&wallet, "pending"])).await?;
    let token_address = token.as_deref().unwrap_or("");
    let code = if token_address.is_empty() { json!("0x") } else { rpc(&client, &url, "eth_getCode", json!([token_address, "latest"])).await? };
    let latest = u64::from_str_radix(block.as_str().unwrap_or("0x0").trim_start_matches("0x"), 16)?;
    let token_topic = token.as_ref().map(|value| format!("0x{:0>64}", value.trim_start_matches("0x")));
    let mut matching_pools = Vec::new();
    let mut scan_errors = Vec::new();
    let scan_blocks: u64 = env::var("ROBINHOOD_SCAN_BLOCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(50_000).min(100_000);
    let chunk_size: u64 = env::var("ROBINHOOD_SCAN_CHUNK").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000).clamp(100, 5_000);
    let scan_from = latest.saturating_sub(scan_blocks);
    for start in (scan_from..=latest).step_by(chunk_size as usize) {
        let end = (start + chunk_size - 1).min(latest);
        let topic_indexes: Vec<Option<usize>> = if token_topic.is_some() { vec![Some(2), Some(3)] } else { vec![None] };
        for topic_index in topic_indexes {
            let topics = if let Some(index) = topic_index {
                let mut filters = vec![Value::Null, Value::Null, Value::Null, Value::Null];
                filters[0] = json!(INIT_TOPIC);
                filters[index] = json!(token_topic.as_ref().unwrap());
                Value::Array(filters)
            } else {
                json!([INIT_TOPIC])
            };
            let result = rpc(&client, &url, "eth_getLogs", json!([{
                "address": POOL_MANAGER,
                "fromBlock": format!("0x{:x}", start),
                "toBlock": format!("0x{:x}", end),
                "topics": topics
            }])).await;
            match result {
                Ok(value) => if let Some(logs) = value.as_array() {
                    for log in logs {
                        let topics = log.get("topics").and_then(Value::as_array).cloned().unwrap_or_default();
                        let data = log.get("data").and_then(Value::as_str).unwrap_or("").trim_start_matches("0x");
                        if topics.len() == 4 && data.len() >= 320 {
                            let fee = u32::from_str_radix(&data[0..64], 16).unwrap_or(0);
                            let spacing_raw = u32::from_str_radix(&data[64..128], 16).unwrap_or(0);
                            let spacing = if spacing_raw & 0x0080_0000 != 0 { (spacing_raw | 0xff00_0000) as i32 } else { spacing_raw as i32 };
                            matching_pools.push(json!({
                                "pool_id": topics[1], "currency0": format!("0x{}", &topics[2].as_str().unwrap_or("")[26..]),
                                "currency1": format!("0x{}", &topics[3].as_str().unwrap_or("")[26..]),
                                "fee": fee, "tick_spacing": spacing,
                                "hooks": format!("0x{}", &data[128+24..128+64]),
                                "tx": log.get("transactionHash"), "block": log.get("blockNumber")
                            }));
                        }
                    }
                }
                Err(error) => scan_errors.push(format!("{}-{} topic{:?}: {}", start, end, topic_index, error)),
            }
        }
    }
    matching_pools.sort_by_key(|v| v.to_string());
    matching_pools.dedup_by_key(|v| v.to_string());
    let logs = Some(matching_pools);
    let decimals = match token.as_deref() {
        Some(address) => token_call(&client, &url, address, &[0x31, 0x3c, 0xe5, 0x67]).await,
        None => Err(anyhow::anyhow!("token not specified")),
    };
    let symbol = match token.as_deref() {
        Some(address) => token_call(&client, &url, address, &[0x95, 0xd8, 0x9b, 0x41]).await,
        None => Err(anyhow::anyhow!("token not specified")),
    };
    let name = match token.as_deref() {
        Some(address) => token_call(&client, &url, address, &[0x06, 0xfd, 0xde, 0x03]).await,
        None => Err(anyhow::anyhow!("token not specified")),
    };
    println!("chain_id={}", chain.as_str().unwrap_or(""));
    println!("latest_block={}", block.as_str().unwrap_or(""));
    println!("wallet_address={}", wallet);
    println!("native_balance={}", balance.as_str().unwrap_or(""));
    println!("pending_nonce={}", nonce.as_str().unwrap_or(""));
    println!("bow_contract_code_present={}", code.as_str().map(|v| v != "0x" && !v.is_empty()).unwrap_or(false));
    println!("bow_decimals={:?}", decimals.ok());
    println!("bow_symbol={:?}", symbol.ok());
    println!("bow_name={:?}", name.ok());
    println!("v4_bow_pools={}", serde_json::to_string(&logs)?);
    println!("v4_scan_errors={}", serde_json::to_string(&scan_errors)?);
    Ok(())
}
