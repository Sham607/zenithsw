use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn is_evm_address(value: &str) -> bool {
    value.len() == 42 && value.starts_with("0x") && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_hex_quantity(value: &str) -> Result<u128> {
    Ok(u128::from_str_radix(value.strip_prefix("0x").ok_or_else(|| anyhow::anyhow!("invalid hex quantity"))?, 16)?)
}

pub struct EvmStreamListener {
    rpc_url: String,
    chain: Chain,
    last_hashes: Arc<Mutex<HashMap<String, String>>>,
    processed_hashes: Arc<Mutex<HashSet<String>>>,
    client: reqwest::Client,
}

impl EvmStreamListener {
    pub fn new(rpc_url: &str, chain: Chain) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self {
            rpc_url: rpc_url.to_string(),
            chain,
            last_hashes: Arc::new(Mutex::new(HashMap::new())),
            processed_hashes: Arc::new(Mutex::new(HashSet::new())),
            client,
        }
    }

    pub async fn validate_robinhood_chain(&self) -> Result<()> {
        if self.chain != Chain::Robinhood {
            return Err(anyhow::anyhow!("EVM listener chain is not Robinhood"));
        }
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_chainId",
            "params": []
        });
        let response = self.client.post(&self.rpc_url).json(&payload).send().await?;
        let value: Value = response.error_for_status()?.json().await?;
        let chain_id = value.get("result").and_then(Value::as_str).unwrap_or("");
        if chain_id != "0x1237" {
            return Err(anyhow::anyhow!(
                "wrong Robinhood chain id: expected 0x1237 (4663), got {}",
                chain_id
            ));
        }
        Ok(())
    }

    /// Pulls recent wallet Transfer logs, then classifies the complete receipt.
    /// Unknown/transfer-only receipts are ignored; no token-inflow heuristic is used.
    pub async fn check_wallet_activity(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        use crate::chains::robinhood::{classify_robinhood_receipt, RobinhoodVenue, ERC20_TRANSFER_TOPIC};
        if self.chain != Chain::Robinhood || !is_evm_address(address) { return Ok(None); }
        let latest = self.rpc_call("eth_blockNumber", json!([])).await?;
        let latest_n = parse_hex_quantity(latest.as_str().unwrap_or(""))?;
        let from = latest_n.saturating_sub(2_000);
        let wallet_topic = format!("0x{}{}", "0".repeat(24), address[2..].to_lowercase());
        let mut txs = Vec::new();
        for topics in [
            json!([ERC20_TRANSFER_TOPIC, wallet_topic, Value::Null]),
            json!([ERC20_TRANSFER_TOPIC, Value::Null, wallet_topic]),
        ] {
            let filter = json!({"fromBlock": format!("0x{from:x}"), "toBlock": format!("0x{latest_n:x}"), "topics": topics});
            if let Ok(value) = self.rpc_call("eth_getLogs", json!([filter])).await {
                if let Some(logs) = value.as_array() {
                    for log in logs {
                        if let Some(tx) = log.get("transactionHash").and_then(Value::as_str) {
                            if !txs.iter().any(|v: &String| v.eq_ignore_ascii_case(tx)) { txs.push(tx.to_string()); }
                        }
                    }
                }
            }
        }
        txs.sort();
        for tx_hash in txs.into_iter().rev() {
            if self.processed_hashes.lock().unwrap().contains(&tx_hash) { continue; }
            let receipt = self.rpc_call("eth_getTransactionReceipt", json!([tx_hash])).await?;
            let classified = classify_robinhood_receipt(&receipt, address)?;
            if !classified.success || !classified.has_transfer || matches!(classified.venue, RobinhoodVenue::Unknown | RobinhoodVenue::TransferOnly) { continue; }
            let logs = receipt.get("logs").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut selected: Option<(String, bool, u128)> = None;
            for log in logs {
                let token = log.get("address").and_then(Value::as_str).unwrap_or("");
                let topics = log.get("topics").and_then(Value::as_array).cloned().unwrap_or_default();
                if topics.len() < 3 || !topics[0].as_str().unwrap_or("").eq_ignore_ascii_case(ERC20_TRANSFER_TOPIC) || !is_evm_address(token) { continue; }
                let from_addr = format!("0x{}", topics[1].as_str().unwrap_or("").trim_start_matches("0x")[24..].to_string());
                let to_addr = format!("0x{}", topics[2].as_str().unwrap_or("").trim_start_matches("0x")[24..].to_string());
                let incoming = to_addr.eq_ignore_ascii_case(address);
                let outgoing = from_addr.eq_ignore_ascii_case(address);
                if !incoming && !outgoing { continue; }
                let data = log.get("data").and_then(Value::as_str).unwrap_or("0x");
                let amount = parse_hex_quantity(data).unwrap_or(0);
                if amount == 0 { continue; }
                // Prefer an incoming token for BUY and an outgoing token for SELL;
                // the first qualifying delta is still receipt-backed, not balance-only.
                if selected.is_none() || incoming { selected = Some((token.to_string(), incoming, amount)); }
            }
            let Some((token, incoming, raw_amount)) = selected else { continue; };
            let decimals = self.erc20_uint(&token, "0x313ce567").await.unwrap_or(18);
            let symbol = self.erc20_string(&token, "0x95d89b41").await.unwrap_or_else(|_| "TOKEN".to_string());
            if crate::models::is_blacklisted_asset(&token, &symbol) { continue; }
            let token_amount = raw_amount as f64 / 10f64.powi(decimals as i32);
            let now = Utc::now();
            let activity = WalletActivity {
                id: None, wallet_handle: handle.to_string(), wallet_address: address.to_string(), chain: self.chain,
                activity_type: if incoming { ActivityType::Buy } else { ActivityType::Sell },
                token_symbol: symbol, token_mint: token, amount_usd: 0.0, token_amount,
                execution_price_usd: 0.0, whale_balance_before: 0.0, whale_balance_after: 0.0,
                sell_ratio_pct: if incoming { 0.0 } else { 100.0 }, tx_signature: tx_hash.clone(), timestamp: now,
            };
            self.processed_hashes.lock().unwrap().insert(tx_hash);
            let _ = db.insert_activity(&activity);
            return Ok(Some(activity));
        }
        Ok(None)

        /*
        if self.rpc_url.is_empty() {
            return Ok(None);
        }

        // 1. Fetch both incoming (BUY) and outgoing (SELL) recent ERC20 transfers
        let payload_in = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "alchemy_getAssetTransfers",
            "params": [{
                "fromBlock": "0x0",
                "toAddress": address,
                "category": ["erc20"],
                "order": "desc",
                "maxCount": "0x3"
            }]
        });

        let payload_out = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "alchemy_getAssetTransfers",
            "params": [{
                "fromBlock": "0x0",
                "fromAddress": address,
                "category": ["erc20"],
                "order": "desc",
                "maxCount": "0x3"
            }]
        });

        let (res_in, res_out) = tokio::join!(
            self.client.post(&self.rpc_url).json(&payload_in).send(),
            self.client.post(&self.rpc_url).json(&payload_out).send()
        );

        let mut all_transfers: Vec<Value> = Vec::new();

        if let Ok(res) = res_in {
            if res.status().is_success() {
                if let Ok(json_res) = res.json::<Value>().await {
                    if let Some(txs) = json_res
                        .get("result")
                        .and_then(|r| r.get("transfers"))
                        .and_then(|t| t.as_array())
                    {
                        all_transfers.extend(txs.clone());
                    }
                }
            }
        }

        if let Ok(res) = res_out {
            if res.status().is_success() {
                if let Ok(json_res) = res.json::<Value>().await {
                    if let Some(txs) = json_res
                        .get("result")
                        .and_then(|r| r.get("transfers"))
                        .and_then(|t| t.as_array())
                    {
                        all_transfers.extend(txs.clone());
                    }
                }
            }
        }

        if all_transfers.is_empty() {
            return Ok(None);
        }

        // Sort transfers by blockTimestamp descending
        all_transfers.sort_by(|a, b| {
            let ts_a = a
                .get("metadata")
                .and_then(|m| m.get("blockTimestamp"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let ts_b = b
                .get("metadata")
                .and_then(|m| m.get("blockTimestamp"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            ts_b.cmp(ts_a)
        });

        // Pick the most recent valid transfer
        for tx in all_transfers {
            let hash = tx.get("hash").and_then(|h| h.as_str()).unwrap_or("");
            let raw_contract = tx
                .get("rawContract")
                .and_then(|r| r.get("address"))
                .and_then(|a| a.as_str())
                .unwrap_or("");
            let asset = tx.get("asset").and_then(|a| a.as_str()).unwrap_or("TOKEN");
            let value_num = tx.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);

            if hash.is_empty() || raw_contract.is_empty() {
                continue;
            }

            // Deduplication & Signature tracking
            {
                let processed = self.processed_hashes.lock().unwrap();
                if processed.contains(hash) {
                    return Ok(None);
                }
            }

            let now = Utc::now();
            let tx_age_secs = if let Some(ts_str) = tx
                .get("metadata")
                .and_then(|m| m.get("blockTimestamp"))
                .and_then(|t| t.as_str())
            {
                if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                    (now.timestamp() - dt.timestamp()).max(0)
                } else {
                    0
                }
            } else {
                0
            };

            // Baseline registration on startup
            {
                let mut last_map = self.last_hashes.lock().unwrap();
                if let Some(last) = last_map.get(address) {
                    if last == hash {
                        return Ok(None);
                    }
                } else {
                    last_map.insert(address.to_string(), hash.to_string());
                    let mut processed = self.processed_hashes.lock().unwrap();
                    processed.insert(hash.to_string());

                    // If older than 120s, it's historical data from before startup
                    if tx_age_secs > 120 {
                        return Ok(None);
                    }
                }
                last_map.insert(address.to_string(), hash.to_string());
            }

            {
                let mut processed = self.processed_hashes.lock().unwrap();
                processed.insert(hash.to_string());
                if processed.len() > 1000 {
                    let to_remove: Vec<String> = processed.iter().take(200).cloned().collect();
                    for r in to_remove {
                        processed.remove(&r);
                    }
                }
            }

            // Determine if incoming (BUY) or outgoing (SELL)
            let to_addr = tx.get("to").and_then(|t| t.as_str()).unwrap_or("");
            let is_buy = to_addr.eq_ignore_ascii_case(address);

            // Fetch live pool price & aggregated liquidity from DexScreener
            let mut token_symbol = asset.to_string();
            let mut execution_price_usd = 0.0;

            if let Ok((sym, price, _, _)) = SolanaStreamListener::fetch_token_price(raw_contract).await
            {
                if sym != "UNKNOWN" && !sym.is_empty() {
                    token_symbol = sym;
                }
                execution_price_usd = price;
            }

            let is_base_stable = crate::models::is_blacklisted_asset(raw_contract, &token_symbol)
                || crate::models::is_blacklisted_asset(raw_contract, asset);

            let (activity_type, sell_ratio_pct) = if is_buy {
                if is_base_stable {
                    (ActivityType::Transfer, 0.0) // Transfer / Deposit of base stablecoin
                } else {
                    (ActivityType::Buy, 0.0)
                }
            } else {
                (ActivityType::Sell, 100.0)
            };

            let amount_usd = if value_num > 0.0 && execution_price_usd > 0.0 {
                value_num * execution_price_usd
            } else {
                0.0
            };

            let act = WalletActivity {
                id: None,
                wallet_handle: handle.to_string(),
                wallet_address: address.to_string(),
                chain: self.chain,
                activity_type,
                token_symbol,
                token_mint: raw_contract.to_string(),
                amount_usd,
                token_amount: value_num,
                execution_price_usd,
                whale_balance_before: 0.0,
                whale_balance_after: 0.0,
                sell_ratio_pct,
                tx_signature: hash.to_string(),
                timestamp: now,
            };

            let _ = db.insert_activity(&act);
            return Ok(Some(act));
        }

        Ok(None)
        */
    }

    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let payload = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let response: Value = self.client.post(&self.rpc_url).json(&payload).send().await?.error_for_status()?.json().await?;
        if let Some(error) = response.get("error") { return Err(anyhow::anyhow!("RPC {method} failed: {error}")); }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn erc20_uint(&self, token: &str, selector: &str) -> Result<u8> {
        let value = self.rpc_call("eth_call", json!([{"to":token,"data":selector},"latest"])).await?;
        Ok(parse_hex_quantity(value.as_str().unwrap_or("0x0"))?.min(255) as u8)
    }

    async fn erc20_string(&self, token: &str, selector: &str) -> Result<String> {
        let value = self.rpc_call("eth_call", json!([{"to":token,"data":selector},"latest"])).await?;
        let raw = value.as_str().unwrap_or("").strip_prefix("0x").unwrap_or("");
        if raw.len() < 128 { return Err(anyhow::anyhow!("ERC20 string output too short")); }
        let offset = usize::from_str_radix(&raw[..64],16)? * 2;
        if raw.len() < offset + 64 { return Err(anyhow::anyhow!("ERC20 string offset invalid")); }
        let len = usize::from_str_radix(&raw[offset..offset+64],16)? * 2;
        if raw.len() < offset + 64 + len { return Err(anyhow::anyhow!("ERC20 string length invalid")); }
        Ok(String::from_utf8_lossy(&hex::decode(&raw[offset+64..offset+64+len])?).to_string())
    }
}
