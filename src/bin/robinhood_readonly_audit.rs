use anyhow::{bail, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use zenithsw::config::Config;

const CHAIN_ID: u64 = 4663;
const POOL_MANAGER: &str = "0x8366a39CC670B4001A1121B8F6A443A643e40951";
const QUOTER: &str = "0x8Dc178eFB8111BB0973Dd9d722ebeFF267c98F94";
const ROUTER: &str = "0x8876789976decbfcbbbe364623c63652db8c0904";

async fn call(client: &Client, endpoint: &str, method: &str, params: Value) -> Result<Value> {
    let response = client
        .post(endpoint)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?;
    let status = response.status();
    let value: Value = response.json().await?;
    if !status.is_success() {
        bail!("{method} HTTP status {status}");
    }
    if let Some(error) = value.get("error") {
        bail!(
            "{method} JSON-RPC error code={} message={}",
            error.get("code").and_then(Value::as_i64).unwrap_or(0),
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{method} result missing"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let config = Config::load()?;
    let endpoint = config
        .robinhood_rpc_url
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("ROBINHOOD_RPC_URL kosong"))?;
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;

    let chain_id = call(&client, endpoint, "eth_chainId", json!([])).await?;
    let chain_value = u64::from_str_radix(
        chain_id.as_str().unwrap_or("0x0").trim_start_matches("0x"),
        16,
    )?;
    if chain_value != CHAIN_ID {
        bail!("chain ID tidak cocok: expected={CHAIN_ID} actual={chain_value}");
    }
    let block = call(&client, endpoint, "eth_blockNumber", json!([])).await?;
    let mut code_results = Vec::new();
    for address in [POOL_MANAGER, QUOTER, ROUTER] {
        let code = call(&client, endpoint, "eth_getCode", json!([address, "latest"])).await?;
        let code_text = code.as_str().unwrap_or("");
        if code_text == "0x" || code_text.is_empty() {
            bail!("deployment bytecode missing for audited address");
        }
        code_results.push(code_text.len());
    }
    println!("ROBINHOOD_READONLY_AUDIT_OK");
    println!("chain_id={chain_value}");
    println!("latest_block={}", block.as_str().unwrap_or("invalid"));
    println!("deployments_verified={}", code_results.len());
    Ok(())
}
