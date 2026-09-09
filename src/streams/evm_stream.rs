use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct EvmStreamListener {
    rpc_url: String,
    chain: Chain,
    client: reqwest::Client,
}

impl EvmStreamListener {
    pub fn new(rpc_url: &str, chain: Chain) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        Self {
            rpc_url: rpc_url.to_string(),
            chain,
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
        // Query recent Alchemy asset transfers or ERC20 Transfer logs
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "alchemy_getAssetTransfers",
            "params": [{
                "fromBlock": "latest",
                "toAddress": address,
                "category": ["erc20"],
                "maxCount": "0x3"
            }]
        });

        let res = self.client.post(&self.rpc_url)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            return Ok(None);
        }

        let json_res: Value = res.json().await.unwrap_or_default();
        let transfers = json_res.get("result")
            .and_then(|r| r.get("transfers"))
            .and_then(|t| t.as_array());

        let now = Utc::now();

        if let Some(txs) = transfers {
            if let Some(latest_tx) = txs.last() {
                let raw_contract = latest_tx.get("rawContract")
                    .and_then(|r| r.get("address"))
                    .and_then(|a| a.as_str())
                    .unwrap_or("");
                let asset = latest_tx.get("asset")
                    .and_then(|a| a.as_str())
                    .unwrap_or("TOKEN");
                let hash = latest_tx.get("hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                let value_num = latest_tx.get("value")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);

                if !raw_contract.is_empty() {
                    let act = WalletActivity {
                        id: None,
                        wallet_handle: handle.to_string(),
                        wallet_address: address.to_string(),
                        chain: self.chain,
                        activity_type: ActivityType::Buy,
                        token_symbol: asset.to_string(),
                        token_mint: raw_contract.to_string(),
                        amount_usd: 500.0, // Default estimated
                        token_amount: value_num,
                        execution_price_usd: 0.0,
                        whale_balance_before: 0.0,
                        whale_balance_after: 0.0,
                        sell_ratio_pct: 0.0,
                        tx_signature: hash.to_string(),
                        timestamp: now,
                    };

                    let _ = db.insert_activity(&act);
                    return Ok(Some(act));
                }
            }
        }

        Ok(None)
    }
}
