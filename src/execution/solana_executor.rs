use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{is_blacklisted_asset, Chain, ShadowPosition, ShadowSignal};
use anyhow::{anyhow, Result};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
pub struct JupiterQuoteResponse {
    #[serde(rename = "inputMint")]
    pub input_mint: String,
    #[serde(rename = "inAmount")]
    pub in_amount: String,
    #[serde(rename = "outputMint")]
    pub output_mint: String,
    #[serde(rename = "outAmount")]
    pub out_amount: String,
    #[serde(rename = "priceImpactPct")]
    pub price_impact_pct: Option<String>,
    pub route_plan: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JupiterSwapResponse {
    #[serde(rename = "swapTransaction")]
    pub swap_transaction: String,
    #[serde(rename = "lastValidBlockHeight")]
    pub last_valid_block_height: Option<u64>,
}

pub struct SolanaExecutor {
    config: Config,
    db: Arc<SmartWalletDb>,
    client: Client,
}

impl SolanaExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self {
        Self {
            config,
            db,
            client: Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Derives Solana SigningKey and Base58 Public Key from private key in config
    fn get_keypair(&self) -> Result<(SigningKey, String)> {
        let pk_str = self
            .config
            .solana_private_key
            .as_ref()
            .ok_or_else(|| anyhow!("Solana private key not configured"))?;

        let bytes = bs58::decode(pk_str)
            .into_vec()
            .map_err(|e| anyhow!("Failed to decode base58 solana private key: {}", e))?;

        if bytes.len() < 32 {
            return Err(anyhow!("Invalid private key length: {}", bytes.len()));
        }

        let key_bytes: [u8; 32] = bytes[0..32].try_into()?;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        let pubkey_b58 = bs58::encode(verifying_key.as_bytes()).into_string();

        Ok((signing_key, pubkey_b58))
    }

    /// Fetches native SOL balance via Solana RPC
    pub async fn get_native_balance(&self, pubkey: &str) -> Result<f64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [pubkey]
        });

        let res = self
            .client
            .post(&self.config.solana_rpc_url)
            .json(&payload)
            .send()
            .await?;

        let res_json: Value = res.json().await?;
        if let Some(lamports) = res_json["result"]["value"].as_f64() {
            Ok(lamports / 1_000_000_000.0)
        } else {
            Ok(0.0)
        }
    }

    /// Fetches live SOL/USD price via DexScreener or Jupiter
    pub async fn get_sol_price(&self) -> f64 {
        let url = "https://api.dexscreener.com/latest/dex/tokens/So11111111111111111111111111111111111111112";
        if let Ok(res) = self.client.get(url).send().await {
            if let Ok(data) = res.json::<Value>().await {
                if let Some(price_str) = data["pairs"][0]["priceUsd"].as_str() {
                    if let Ok(p) = price_str.parse::<f64>() {
                        if p > 10.0 {
                            return p;
                        }
                    }
                }
            }
        }
        100.0 // safe fallback
    }

    /// Fetches on-chain raw SPL token balance for an exact mint
    pub async fn get_raw_token_balance(&self, user_pubkey: &str, mint: &str) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                user_pubkey,
                {"mint": mint},
                {"encoding": "jsonParsed"}
            ]
        });

        let res = self
            .client
            .post(&self.config.solana_rpc_url)
            .json(&payload)
            .send()
            .await?;
        let json: Value = res.json().await?;
        if let Some(accs) = json["result"]["value"].as_array() {
            if let Some(first) = accs.first() {
                if let Some(amt_str) =
                    first["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"].as_str()
                {
                    if let Ok(raw_u64) = amt_str.parse::<u64>() {
                        return Ok(raw_u64);
                    }
                }
            }
        }
        Ok(0)
    }

    /// Fetches a Jupiter Swap quote (v1)
    pub async fn fetch_jupiter_quote(
        &self,
        input_mint: &str,
        output_mint: &str,
        amount_lamports: u64,
        slippage_bps: u16,
    ) -> Result<Value> {
        let url = format!(
            "https://api.jup.ag/swap/v1/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            input_mint, output_mint, amount_lamports, slippage_bps
        );

        let res = self.client.get(&url).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("Jupiter Quote API failed ({}): {}", status, body));
        }

        let json = res.json::<Value>().await?;
        Ok(json)
    }

    /// Builds and signs a real on-chain transaction via Jupiter Swap API v1
    pub async fn build_and_sign_jupiter_tx(
        &self,
        quote_json: Value,
        user_pubkey: &str,
        signing_key: &SigningKey,
    ) -> Result<String> {
        let swap_url = "https://api.jup.ag/swap/v1/swap";
        let payload = serde_json::json!({
            "quoteResponse": quote_json,
            "userPublicKey": user_pubkey,
            "wrapAndUnwrapSol": true,
            "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": "auto"
        });

        let res = self.client.post(swap_url).json(&payload).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(anyhow!("Jupiter Swap API failed ({}): {}", status, body));
        }

        let swap_resp: JupiterSwapResponse = res.json().await?;
        let tx_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &swap_resp.swap_transaction,
        )?;

        // Sign the VersionedTransaction payload
        // In Solana VersionedTransaction, the first byte(s) encode signature count, followed by 64-byte signature slots, followed by the message
        let signed_tx_base64 = if tx_bytes.len() > 65 {
            let mut mutable_tx = tx_bytes.clone();
            // Message offset starts after signature count (1 byte for single-sig) + 64 bytes signature
            let message_bytes = &tx_bytes[65..];
            let signature = signing_key.sign(message_bytes);
            mutable_tx[1..65].copy_from_slice(&signature.to_bytes());
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &mutable_tx)
        } else {
            swap_resp.swap_transaction
        };

        Ok(signed_tx_base64)
    }

    /// Broadcasts signed transaction via Helius RPC & Jito Anti-MEV Bundles
    pub async fn broadcast_tx(&self, signed_base64: &str) -> Result<String> {
        // 1. Direct Helius RPC broadcast
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                signed_base64,
                {
                    "skipPreflight": true,
                    "maxRetries": 3,
                    "preflightCommitment": "processed",
                    "encoding": "base64"
                }
            ]
        });

        let res = self
            .client
            .post(&self.config.solana_rpc_url)
            .json(&payload)
            .send()
            .await?;

        let res_json: Value = res.json().await?;
        if let Some(err) = res_json.get("error") {
            return Err(anyhow!("RPC Broadcast error: {}", err));
        }

        let tx_signature = res_json["result"]
            .as_str()
            .unwrap_or("UNKNOWN_SIG")
            .to_string();

        // 2. Dual Broadcast to Jito Anti-MEV Block Engine
        let jito_url = "https://mainnet.block-engine.jito.wtf/api/v1/bundles";
        let jito_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [[signed_base64]]
        });
        let _ = self.client.post(jito_url).json(&jito_payload).send().await;

        Ok(tx_signature)
    }

    /// Executes End-to-End Shadow Buy on Solana
    pub async fn execute_shadow_buy(
        &self,
        signal: &ShadowSignal,
    ) -> Result<Option<ShadowPosition>> {
        if is_blacklisted_asset(&signal.token_mint, &signal.token_symbol) {
            eprintln!(
                "[SOLANA EXECUTION BLOCK 🛡️] Refusing blacklisted/impersonation asset {} ({})",
                signal.token_symbol, signal.token_mint
            );
            return Ok(None);
        }
        if !signal.is_passed {
            return Ok(None);
        }

        let entry_price = if signal.current_pool_price > 0.0 {
            signal.current_pool_price
        } else {
            signal.whale_entry_price
        };

        if entry_price <= 0.0 {
            eprintln!("[SOLANA EXEC ERROR] Cannot open shadow position with 0.0 price!");
            return Ok(None);
        }

        let now = Utc::now();
        let amount_usd = self.config.position_size_usd;
        let new_tokens = if entry_price > 0.0 {
            amount_usd / entry_price
        } else {
            0.0
        };
        let position_id = format!("SOL-{}", signal.token_mint);

        let sol_price = self.get_sol_price().await;
        let amount_native_sol = amount_usd / sol_price;
        let amount_lamports = (amount_native_sol * 1_000_000_000.0) as u64;

        let wsol_mint = "So11111111111111111111111111111111111111112";

        // REAL ON-CHAIN EXECUTION (when dry_run is false and keypair exists)
        let mut _onchain_tx_sig = None;
        if !self.config.dry_run {
            if let Ok((signing_key, user_pubkey)) = self.get_keypair() {
                // Gas Buffer Guard: Require at least 0.04 SOL reserve for gas fees + trade amount
                if let Ok(balance_sol) = self.get_native_balance(&user_pubkey).await {
                    let required_sol = amount_native_sol + 0.04;
                    if balance_sol < required_sol {
                        eprintln!(
                            "[GAS BUFFER GUARD 🛡️] Insufficient SOL balance ({:.4} SOL < required {:.4} SOL = 0.04 buffer + {:.4} trade). Skipping live buy!",
                            balance_sol, required_sol, amount_native_sol
                        );
                        return Ok(None);
                    }
                }

                println!(
                    "[ONCHAIN SOLANA BUY 🚀] Executing real Jupiter swap for {} (${:.2}) with pubkey {}",
                    signal.token_symbol, amount_usd, user_pubkey
                );

                match self
                    .fetch_jupiter_quote(wsol_mint, &signal.token_mint, amount_lamports, 100)
                    .await
                {
                    Ok(quote) => {
                        match self
                            .build_and_sign_jupiter_tx(quote, &user_pubkey, &signing_key)
                            .await
                        {
                            Ok(signed_tx) => match self.broadcast_tx(&signed_tx).await {
                                Ok(sig) => {
                                    println!("[ONCHAIN BUY CONFIRMED 🟢] TX Sig: {}", sig);
                                    _onchain_tx_sig = Some(sig);
                                }
                                Err(e) => {
                                    eprintln!("[ONCHAIN BROADCAST FAILED ❌] {}", e);
                                }
                            },
                            Err(e) => eprintln!("[JUPITER TX BUILD FAILED ❌] {}", e),
                        }
                    }
                    Err(e) => eprintln!("[JUPITER QUOTE FAILED ❌] {}", e),
                }
            } else {
                println!("[SIMULATION MODE 🧪] No Solana private key found. Recording paper shadow position.");
            }
        } else {
            println!(
                "[SIMULATION MODE 🧪] DRY_RUN=true. Shadow buy for {} (${:.2}) recorded in DB.",
                signal.token_symbol, amount_usd
            );
        }

        // Check if position already exists for this token mint on Solana -> Average in!
        if let Ok(active_pos) = self.db.get_active_positions() {
            if let Some(mut existing) = active_pos.into_iter().find(|p| {
                p.token_mint.eq_ignore_ascii_case(&signal.token_mint) && p.chain == Chain::Solana
            }) {
                let prev_usd = existing.amount_usd;
                let prev_tokens = existing.amount_native;
                let total_usd = prev_usd + amount_usd;
                let total_tokens = prev_tokens + new_tokens;
                let weighted_avg_entry = if total_tokens > 0.0 {
                    total_usd / total_tokens
                } else {
                    entry_price
                };

                println!(
                    "[POSITION AVERAGING ➕] Adding ${:.2} to existing {} position (ID: {}). Prev Entry: ${:.6} -> New Avg Entry: ${:.6} (Total: ${:.2})",
                    amount_usd, existing.token_symbol, existing.id, existing.entry_price, weighted_avg_entry, total_usd
                );

                existing.entry_price = weighted_avg_entry;
                existing.amount_usd = total_usd;
                existing.amount_native = total_tokens;
                existing.current_price = entry_price;
                existing.peak_price = existing.peak_price.max(entry_price);
                existing.remaining_portion_pct = 100.0;
                existing.last_update = now;
                existing.status = "OPEN".to_string();

                if !existing.target_whale_handle.contains(&signal.whale_handle) {
                    existing.target_whale_handle =
                        format!("{}, @{}", existing.target_whale_handle, signal.whale_handle);
                }
                if !existing
                    .target_whale_address
                    .contains(&signal.whale_address)
                {
                    existing.target_whale_address = format!(
                        "{}, {}",
                        existing.target_whale_address, signal.whale_address
                    );
                }

                self.db.upsert_position(&existing)?;
                return Ok(Some(existing));
            }
        }

        let position = ShadowPosition {
            id: position_id,
            chain: Chain::Solana,
            token_symbol: signal.token_symbol.clone(),
            token_mint: signal.token_mint.clone(),
            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            entry_price,
            current_price: entry_price,
            peak_price: entry_price,
            amount_native: new_tokens,
            amount_usd,
            remaining_portion_pct: 100.0,
            unrealized_pnl_pct: 0.0,
            realized_pnl_usd: 0.0,
            entry_time: now,
            last_update: now,
            status: "OPEN".to_string(),
        };

        self.db.upsert_position(&position)?;
        Ok(Some(position))
    }

    /// Executes Proportional Shadow Sell (Partial Exit) on Solana
    pub async fn execute_partial_shadow_sell(
        &self,
        position_id: &str,
        sell_ratio_pct: f64,
        exit_reason: &str,
        current_price: f64,
    ) -> Result<()> {
        let wsol_mint = "So11111111111111111111111111111111111111112";

        if !self.config.dry_run {
            if let Ok((signing_key, user_pubkey)) = self.get_keypair() {
                // Fetch position mint from db
                if let Ok(active_pos) = self.db.get_active_positions() {
                    if let Some(pos) = active_pos.iter().find(|p| p.id == position_id) {
                        println!(
                            "[ONCHAIN SOLANA SELL 🚀] Executing {:.1}% partial swap for {} to SOL",
                            sell_ratio_pct, pos.token_symbol
                        );
                        // Query real on-chain token balance
                        let token_amount_raw = self
                            .get_raw_token_balance(&user_pubkey, &pos.token_mint)
                            .await
                            .unwrap_or(0);
                        let sell_units =
                            ((token_amount_raw as f64) * (sell_ratio_pct / 100.0)) as u64;
                        if sell_units > 0 {
                            if let Ok(quote) = self
                                .fetch_jupiter_quote(
                                    &pos.token_mint,
                                    wsol_mint,
                                    sell_units,
                                    150,
                                )
                                .await
                            {
                                if let Ok(signed_tx) = self
                                    .build_and_sign_jupiter_tx(quote, &user_pubkey, &signing_key)
                                    .await
                                {
                                    let _ = self.broadcast_tx(&signed_tx).await;
                                }
                            }
                        }
                    }
                }
            }
        }

        self.db
            .close_position_portion(position_id, sell_ratio_pct, exit_reason, current_price)?;
        Ok(())
    }

    /// Executes Complete Shadow Sell (100% Full Dump Exit) on Solana
    pub async fn execute_shadow_sell(
        &self,
        position_id: &str,
        exit_reason: &str,
        current_price: f64,
    ) -> Result<()> {
        self.execute_partial_shadow_sell(position_id, 100.0, exit_reason, current_price)
            .await
    }
}
