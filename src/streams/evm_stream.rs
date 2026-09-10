use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use crate::streams::solana_stream::SolanaStreamListener;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

    /// Pulls latest block logs or token balance changes for target EVM wallet
    pub async fn check_wallet_activity(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
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
                    if let Some(txs) = json_res.get("result").and_then(|r| r.get("transfers")).and_then(|t| t.as_array()) {
                        all_transfers.extend(txs.clone());
                    }
                }
            }
        }

        if let Ok(res) = res_out {
            if res.status().is_success() {
                if let Ok(json_res) = res.json::<Value>().await {
                    if let Some(txs) = json_res.get("result").and_then(|r| r.get("transfers")).and_then(|t| t.as_array()) {
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
            let ts_a = a.get("metadata").and_then(|m| m.get("blockTimestamp")).and_then(|t| t.as_str()).unwrap_or("");
            let ts_b = b.get("metadata").and_then(|m| m.get("blockTimestamp")).and_then(|t| t.as_str()).unwrap_or("");
            ts_b.cmp(ts_a)
        });

        // Pick the most recent valid transfer
        for tx in all_transfers {
            let hash = tx.get("hash").and_then(|h| h.as_str()).unwrap_or("");
            let raw_contract = tx.get("rawContract")
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
            let tx_age_secs = if let Some(ts_str) = tx.get("metadata").and_then(|m| m.get("blockTimestamp")).and_then(|t| t.as_str()) {
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

            let (activity_type, sell_ratio_pct) = if is_buy {
                (ActivityType::Buy, 0.0)
            } else {
                (ActivityType::Sell, 100.0)
            };

            // Fetch live pool price & aggregated liquidity from DexScreener
            let mut token_symbol = asset.to_string();
            let mut execution_price_usd = 0.0;

            if let Ok((sym, price, _)) = SolanaStreamListener::fetch_token_price(raw_contract).await {
                if sym != "UNKNOWN" && !sym.is_empty() {
                    token_symbol = sym;
                }
                execution_price_usd = price;
            }

            let amount_usd = if value_num > 0.0 && execution_price_usd > 0.0 {
                (value_num * execution_price_usd).max(10.0)
            } else {
                50.0
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
    }
}
