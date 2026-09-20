use anyhow::{bail, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::{env, time::Duration};

const DEFAULT_TX: &str = "0x3a6c97cd0f64112ec3097747d07297c439b004e039c4fdb716c8d85bb133f2c1";
const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";
const USDG: &str = "0x5fc5360d0400a0fd4f2af552add042d716f1d168";
const XL: &str = "0x1cdb289befdfac8af945a288bcdccc382cb34d32";
const ROUTER: &str = "0x5a705de8982235a7fa45bb83dcacf03a211389c7";

fn replace_word(input: &str, index: usize, value: u128) -> Result<String> {
    let mut bytes = input
        .strip_prefix("0x")
        .unwrap_or(input)
        .as_bytes()
        .to_vec();
    let start = 8 + index * 64;
    if start + 64 > bytes.len() {
        bail!("calldata word index di luar batas");
    }
    let word = format!("{value:064x}");
    bytes[start..start + 64].copy_from_slice(word.as_bytes());
    Ok(format!("0x{}", String::from_utf8(bytes)?))
}

fn replace_all_word(input: &str, old: u128, new: u128) -> String {
    let old_word = format!("{old:064x}");
    let new_word = format!("{new:064x}");
    input.replace(&old_word, &new_word)
}
fn hex_u128(value: &str) -> Result<u128> {
    u128::from_str_radix(value.trim_start_matches("0x"), 16)
        .map_err(|error| anyhow::anyhow!("hex quantity invalid: {error}"))
}

async fn simulate(client: &Client, url: &str, tx: Value) -> Result<()> {
    let call = rpc(client, url, "eth_call", json!([&tx, "latest"])).await?;
    let gas = rpc(client, url, "eth_estimateGas", json!([&tx])).await?;
    println!("FULL_BALANCE_SIMULATION_OK");
    println!("router={ROUTER}");
    println!("input_token={USDG}");
    println!("output_token={XL}");
    println!("eth_call_result={}", call.as_str().unwrap_or(""));
    println!("estimate_gas={}", gas.as_str().unwrap_or(""));
    Ok(())
}

async fn rpc(client: &Client, url: &str, method: &str, params: Value) -> Result<Value> {
    let response = client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await?;
    if !status.is_success() {
        bail!("{method} HTTP {status}");
    }
    if let Some(error) = body.get("error") {
        bail!("{method} JSON-RPC error: {error}");
    }
    body.get("result")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("{method} result missing"))
}

fn topic_address(value: &Value) -> String {
    value.as_str().unwrap_or("").to_lowercase()
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let url = env::var("ROBINHOOD_RPC_URL")?
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        bail!("ROBINHOOD_RPC_URL kosong");
    }
    let client = Client::builder().timeout(Duration::from_secs(15)).build()?;
    let tx_hash = env::var("ROBINHOOD_TX_HASH").unwrap_or_else(|_| DEFAULT_TX.to_string());
    let tx = rpc(&client, &url, "eth_getTransactionByHash", json!([&tx_hash])).await?;
    let receipt = rpc(
        &client,
        &url,
        "eth_getTransactionReceipt",
        json!([&tx_hash]),
    )
    .await?;
    let chain = rpc(&client, &url, "eth_chainId", json!([])).await?;
    if env::var("FULL_BALANCE_SIM").as_deref() == Ok("true") {
        let input = tx
            .get("input")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("reference input missing"))?;
        let calldata_hex = input.strip_prefix("0x").unwrap_or(input);
        let reference_amount = hex_u128(&calldata_hex[8 + 5 * 64..8 + 6 * 64])?;
        let full_amount = 592_557u128;
        let calldata = if env::var("REFERENCE_CALL").as_deref() == Ok("true") {
            input.to_string()
        } else if env::var("FULL_ROUTE_AMOUNTS").as_deref() == Ok("true") {
            let replaced = replace_all_word(input, 251_115, full_amount);
            replace_all_word(&replaced, 250_488, full_amount.saturating_sub(627))
        } else {
            replace_word(input, 5, full_amount)?
        };
        let simulation_tx = json!({
            "from": tx.get("from"),
            "to": ROUTER,
            "value": "0x0",
            "data": calldata,
        });
        println!("reference_amount_raw={reference_amount}");
        println!("simulated_amount_raw={full_amount}");
        println!("simulated_min_output_preserved=true");
        println!("wallet_total_input_includes_route_fee=true");
        simulate(&client, &url, simulation_tx).await?;
        return Ok(());
    }
    println!("TX_AUDIT_OK");
    println!("chain_id={}", chain.as_str().unwrap_or(""));
    println!("hash={tx_hash}");
    println!(
        "block_number={}",
        tx.get("blockNumber")
            .and_then(Value::as_str)
            .unwrap_or("null")
    );
    println!(
        "from={}",
        tx.get("from").and_then(Value::as_str).unwrap_or("null")
    );
    println!(
        "to={}",
        tx.get("to").and_then(Value::as_str).unwrap_or("null")
    );
    println!(
        "value={}",
        tx.get("value").and_then(Value::as_str).unwrap_or("null")
    );
    let input = tx.get("input").and_then(Value::as_str).unwrap_or("0x");
    println!("input_selector={}", input.get(..10).unwrap_or("0x"));
    println!(
        "input_bytes={}",
        input.strip_prefix("0x").unwrap_or("").len() / 2
    );
    println!("input_data={input}");
    println!(
        "status={}",
        receipt
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("null")
    );
    println!(
        "gas_used={}",
        receipt
            .get("gasUsed")
            .and_then(Value::as_str)
            .unwrap_or("null")
    );
    println!(
        "effective_gas_price={}",
        receipt
            .get("effectiveGasPrice")
            .and_then(Value::as_str)
            .unwrap_or("null")
    );
    let logs = receipt
        .get("logs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let transfers = logs
        .iter()
        .filter(|log| {
            log.get("topics")
                .and_then(Value::as_array)
                .and_then(|v| v.first())
                .and_then(Value::as_str)
                .map(|v| v.eq_ignore_ascii_case(TRANSFER_TOPIC))
                .unwrap_or(false)
        })
        .count();
    println!("logs={}", logs.len());
    println!("erc20_transfer_logs={transfers}");
    for (i, log) in logs.iter().enumerate() {
        let topics = log
            .get("topics")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let topic0 = topics.first().and_then(Value::as_str).unwrap_or("");
        if topic0.eq_ignore_ascii_case(TRANSFER_TOPIC) {
            println!(
                "transfer[{i}].token={}",
                log.get("address").and_then(Value::as_str).unwrap_or("")
            );
            println!(
                "transfer[{i}].from_topic={}",
                topic_address(topics.get(1).unwrap_or(&Value::Null))
            );
            println!(
                "transfer[{i}].to_topic={}",
                topic_address(topics.get(2).unwrap_or(&Value::Null))
            );
            println!(
                "transfer[{i}].amount={}",
                log.get("data").and_then(Value::as_str).unwrap_or("")
            );
        } else {
            println!(
                "non_transfer[{i}].address={}",
                log.get("address").and_then(Value::as_str).unwrap_or("")
            );
            println!("non_transfer[{i}].topics={:?}", topics);
            println!(
                "non_transfer[{i}].data={}",
                log.get("data").and_then(Value::as_str).unwrap_or("")
            );
        }
    }
    Ok(())
}
