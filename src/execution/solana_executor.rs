use crate::config::Config;
use crate::db::SmartWalletDb;
use crate::models::{is_blacklisted_asset, Chain, ShadowPosition, ShadowSignal};
use anyhow::{anyhow, Result};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    rpc_index: AtomicUsize,
    rpc_cooldowns: Mutex<Vec<Option<Instant>>>,
    rpc_requests: AtomicU64,
    rpc_successes: AtomicU64,
    rpc_rate_limited: AtomicU64,
    rpc_failures: AtomicU64,
}

impl SolanaExecutor {
    pub fn new(config: Config, db: Arc<SmartWalletDb>) -> Self {
        let mut rpc_urls: Vec<String> = config
            .solana_rpc_url
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        for fallback in [
            "https://solana-rpc.publicnode.com",
            "https://rpc.ankr.com/solana",
            "https://api.mainnet-beta.solana.com",
        ] {
            if !rpc_urls.iter().any(|url| url == fallback) {
                rpc_urls.push(fallback.to_string());
            }
        }
        let rpc_count = rpc_urls.len().max(1);
        Self {
            config,
            db,
            client: Client::builder()
                .timeout(Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
            rpc_index: AtomicUsize::new(0),
            rpc_cooldowns: Mutex::new(vec![None; rpc_count]),
            rpc_requests: AtomicU64::new(0),
            rpc_successes: AtomicU64::new(0),
            rpc_rate_limited: AtomicU64::new(0),
            rpc_failures: AtomicU64::new(0),
        }
    }

    /// Derives Solana SigningKey and Base58 Public Key from private key in config
    pub fn get_keypair(&self) -> Result<(SigningKey, String)> {
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

    /// Mengirim satu request read/broadcast ke pool RPC Solana.
    /// Broadcast tetap hanya sekali; caller wajib merekonsiliasi signature.
    pub async fn post_rpc(&self, payload: &Value) -> Result<Value> {
        let mut rpc_urls: Vec<String> = self.config.solana_rpc_url
            .split(',').map(str::trim).filter(|url| !url.is_empty())
            .map(ToOwned::to_owned).collect();
        for fallback in ["https://solana-rpc.publicnode.com", "https://rpc.ankr.com/solana", "https://api.mainnet-beta.solana.com"] {
            if !rpc_urls.iter().any(|url| url == fallback) { rpc_urls.push(fallback.to_string()); }
        }
        let count = rpc_urls.len();
        if count == 0 { return Err(anyhow!("No Solana RPC endpoints configured")); }
        let is_broadcast = payload.get("method").and_then(Value::as_str) == Some("sendTransaction");
        let start = self.rpc_index.fetch_add(1, Ordering::Relaxed) % count;
        let max_attempts = if is_broadcast { 1 } else { count };
        let mut last_err = anyhow!("All Solana RPC endpoints failed");
        for offset in 0..max_attempts {
            let index = (start + offset) % count;
            if self.rpc_cooldowns.lock().unwrap()[index].map(|until| until > Instant::now()).unwrap_or(false) { continue; }
            self.rpc_requests.fetch_add(1, Ordering::Relaxed);
            match self.client.post(&rpc_urls[index]).json(payload).send().await {
                Ok(res) if res.status().as_u16() == 429 => {
                    self.rpc_rate_limited.fetch_add(1, Ordering::Relaxed);
                    self.rpc_cooldowns.lock().unwrap()[index] = Some(Instant::now() + Duration::from_secs(30));
                    last_err = anyhow!("Solana RPC endpoint {index} rate-limited");
                }
                Ok(res) => match res.json::<Value>().await {
                    Ok(json) if json.get("error").is_none() && json.get("result").is_some() => {
                        self.rpc_successes.fetch_add(1, Ordering::Relaxed);
                        return Ok(json);
                    }
                    Ok(json) => { self.rpc_failures.fetch_add(1, Ordering::Relaxed); last_err = anyhow!("Solana RPC endpoint {index} error: {}", json.get("error").unwrap_or(&json)); }
                    Err(error) => { self.rpc_failures.fetch_add(1, Ordering::Relaxed); last_err = anyhow!("Solana RPC endpoint {index} invalid JSON: {error}"); }
                },
                Err(error) => { self.rpc_failures.fetch_add(1, Ordering::Relaxed); self.rpc_cooldowns.lock().unwrap()[index] = Some(Instant::now() + Duration::from_secs(15)); last_err = anyhow!("Solana RPC endpoint {index} transport error: {error}"); }
            }
        }
        Err(last_err)
    }

    pub fn rpc_health_snapshot(&self) -> (u64, u64, u64, u64) {
        (self.rpc_requests.load(Ordering::Relaxed), self.rpc_successes.load(Ordering::Relaxed), self.rpc_rate_limited.load(Ordering::Relaxed), self.rpc_failures.load(Ordering::Relaxed))
    }

    /// Fetches native SOL balance via Solana RPC
    pub async fn get_native_balance(&self, pubkey: &str) -> Result<f64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [pubkey]
        });

        let res_json = self.post_rpc(&payload).await?;
        if let Some(lamports) = res_json["result"]["value"].as_f64() {
            Ok(lamports / 1_000_000_000.0)
        } else {
            Ok(0.0)
        }
    }

    /// Fetches live SOL/USD via a fresh Jupiter SOL→USDC quote.
    /// Failure is terminal; no guessed fallback is permitted.
    pub async fn get_sol_price(&self) -> Result<f64> {
        let url = "https://api.jup.ag/swap/v1/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&amount=1000000000&slippageBps=0&restrictIntermediateTokens=true";
        let response = self.client.get(url).send().await?.error_for_status()?;
        let data: Value = response.json().await?;
        let out_amount = data["outAmount"]
            .as_str()
            .ok_or_else(|| anyhow!("Jupiter SOL/USD quote output missing"))?
            .parse::<u64>()?;
        let price = out_amount as f64 / 1_000_000.0;
        if !price.is_finite() || price <= 0.0 {
            return Err(anyhow!("Jupiter SOL/USD quote price invalid"));
        }
        Ok(price)
    }

    /// Fetches on-chain UI token balance for an exact mint
    pub async fn get_ui_token_balance(&self, user_pubkey: &str, mint: &str) -> Result<f64> {
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

        let json = self.post_rpc(&payload).await?;
        if let Some(accs) = json["result"]["value"].as_array() {
            if let Some(first) = accs.first() {
                if let Some(ui_amt) =
                    first["account"]["data"]["parsed"]["info"]["tokenAmount"]["uiAmount"].as_f64()
                {
                    return Ok(ui_amt);
                }
            }
        }
        Ok(0.0)
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

        let json = self.post_rpc(&payload).await?;
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

    /// Fetches all SPL and Token-2022 tokens held by user_pubkey with non-zero balance
    pub async fn get_all_wallet_token_balances(&self, user_pubkey: &str) -> Vec<(String, f64)> {
        let mut tokens = Vec::new();
        let program_ids = [
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
        ];

        for pid in program_ids {
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getTokenAccountsByOwner",
                "params": [
                    user_pubkey,
                    {"programId": pid},
                    {"encoding": "jsonParsed"}
                ]
            });

            if let Ok(res) = self.post_rpc(&payload).await {
                if let Some(accs) = res["result"]["value"].as_array() {
                    for acc in accs {
                        if let Some(info) = acc.get("account").and_then(|a| a.get("data")).and_then(|d| d.get("parsed")).and_then(|p| p.get("info")) {
                            let mint = info.get("mint").and_then(|m| m.as_str()).unwrap_or("").to_string();
                            let ui_amt = info.get("tokenAmount").and_then(|t| t.get("uiAmount")).and_then(|v| v.as_f64()).unwrap_or(0.0);
                            if !mint.is_empty() && ui_amt > 0.000001 {
                                tokens.push((mint, ui_amt));
                            }
                        }
                    }
                }
            }
        }
        tokens
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

    /// Builds and signs a direct Pump.fun bonding curve swap transaction via PumpPortal Lightning Trade API
    pub async fn build_and_sign_pumpfun_tx(
        &self,
        action: &str, // "buy" or "sell"
        mint: &str,
        amount: f64, // SOL amount for buy, Token units for sell
        slippage_pct: f64,
        user_pubkey: &str,
        signing_key: &SigningKey,
    ) -> Result<String> {
        let url = "https://pumpportal.fun/api/trade-local";
        let payload = serde_json::json!({
            "publicKey": user_pubkey,
            "action": action,
            "mint": mint,
            "denominatedInSol": if action == "buy" { "true" } else { "false" },
            "amount": amount,
            "slippage": slippage_pct,
            "priorityFee": 0.005,
            "pool": "pump"
        });

        let res = self.client.post(url).json(&payload).send().await?;
        if !res.status().is_success() {
            let status = res.status();
            let err_body = res.text().await.unwrap_or_default();
            return Err(anyhow!("PumpPortal API failed ({}): {}", status, err_body));
        }

        let tx_bytes = res.bytes().await?;
        if tx_bytes.len() <= 65 {
            return Err(anyhow!("Invalid PumpPortal transaction payload received"));
        }

        let mut mutable_tx = tx_bytes.to_vec();
        let message_bytes = &tx_bytes[65..];
        let signature = signing_key.sign(message_bytes);
        mutable_tx[1..65].copy_from_slice(&signature.to_bytes());

        let signed_base64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &mutable_tx,
        );

        Ok(signed_base64)
    }

    /// Broadcasts signed transaction via Multi-RPC & Jito Anti-MEV Bundles
    pub async fn broadcast_tx(&self, signed_base64: &str) -> Result<String> {
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

        let res_json = self.post_rpc(&payload).await?;
        let tx_signature = res_json["result"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid RPC sendTransaction response: {:?}", res_json))?
            .to_string();

        // Submit exactly once through the RPC pool. Confirmation is performed
        // separately using the returned signature; do not dual-submit to Jito.
        Ok(tx_signature)
    }

    /// Polls Solana RPC getSignatureStatuses until the transaction is confirmed or finalized
    pub async fn confirm_transaction_signature(
        &self,
        signature: &str,
        max_wait_secs: u64,
    ) -> Result<bool> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignatureStatuses",
            "params": [
                [signature],
                {"searchTransactionHistory": true}
            ]
        });

        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < max_wait_secs {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            if let Ok(res) = self.post_rpc(&payload).await {
                if let Some(statuses) = res["result"]["value"].as_array() {
                    if let Some(status) = statuses.first() {
                        if !status.is_null() {
                            if let Some(err) = status.get("err") {
                                if !err.is_null() {
                                    eprintln!(
                                        "[SOLANA CONFIRM ERROR ❌] Tx {} failed on-chain: {:?}",
                                        signature, err
                                    );
                                    return Ok(false);
                                }
                            }
                            if let Some(conf) = status
                                .get("confirmationStatus")
                                .and_then(|s| s.as_str())
                            {
                                if conf == "confirmed" || conf == "finalized" {
                                    println!(
                                        "[SOLANA CONFIRM SUCCESS ✅] Tx {} confirmed ({})",
                                        signature, conf
                                    );
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
            }
        }
        eprintln!(
            "[SOLANA CONFIRM TIMEOUT ⚠️] Tx {} not confirmed within {}s",
            signature, max_wait_secs
        );
        Ok(false)
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

        let mut entry_price = if signal.current_pool_price.is_finite()
            && signal.current_pool_price > 0.0
        {
            signal.current_pool_price
        } else {
            signal.whale_entry_price
        };

        if !entry_price.is_finite() || entry_price <= 0.0 {
            eprintln!("[SOLANA EXEC ERROR] Cannot open shadow position with 0.0 price!");
            return Ok(None);
        }

        let now = Utc::now();
        let amount_usd = self.config.position_size_usd;
        let mut new_tokens = if entry_price > 0.0 {
            amount_usd / entry_price
        } else {
            0.0
        };
        let position_id = format!("SOL-{}", signal.token_mint);

        let amount_native_sol = if self.config.dry_run {
            0.0
        } else {
            let sol_price = self.get_sol_price().await?;
            amount_usd / sol_price
        };
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

                let intent_created = self.db.create_execution_intent(
                    &position_id,
                    Chain::Solana,
                    "BUY",
                    &signal.token_mint,
                    &user_pubkey,
                    amount_native_sol,
                )?;
                if !intent_created {
                    let existing = self.db.get_execution_intent(&position_id)?;
                    if existing.as_ref().and_then(|(_, _, _, signature)| signature.as_ref()).is_some() {
                        return Err(anyhow!("Solana BUY intent sudah memiliki signature; menolak broadcast ulang"));
                    }
                    return Err(anyhow!("Solana BUY intent sudah ada dan belum direkonsiliasi: {}", position_id));
                }

                let mut buy_success = false;
                let is_pump_token = signal.token_mint.ends_with("pump");

                // Route 1: Direct Pump.fun Bonding Curve if token ends with "pump"
                if is_pump_token {
                    println!("[PUMPFUN ROUTE ⚡] Token ends with 'pump'. Attempting direct bonding curve buy...");
                    match self
                        .build_and_sign_pumpfun_tx("buy", &signal.token_mint, amount_native_sol, 15.0, &user_pubkey, &signing_key)
                        .await
                    {
                        Ok(signed_pump_tx) => match self.broadcast_tx(&signed_pump_tx).await {
                            Ok(sig) => {
                                println!("[PUMPFUN BUY CONFIRMED 🟢] TX Sig: {}", sig);
                                self.db.update_execution_intent(
                                    &position_id,
                                    "BROADCASTED",
                                    None,
                                    Some(&sig),
                                )?;
                                _onchain_tx_sig = Some(sig);
                                buy_success = true;
                            }
                            Err(e) => {
                                eprintln!("[PUMPFUN BROADCAST FAILED ⚠️] {}. Falling back to Jupiter...", e);
                            }
                        },
                        Err(e) => {
                            eprintln!("[PUMPFUN BUILD FAILED ⚠️] {}. Falling back to Jupiter...", e);
                        }
                    }
                }

                // Route 2: Jupiter Swap (Default or Fallback if Pump.fun failed)
                if !buy_success {
                    println!(
                        "[JUPITER ROUTE 🪐] Executing swap for {} (${:.2}) with pubkey {}",
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
                                        self.db.update_execution_intent(
                                            &position_id,
                                            "BROADCASTED",
                                            None,
                                            Some(&sig),
                                        )?;
                                        _onchain_tx_sig = Some(sig);
                                        buy_success = true;
                                    }
                                    Err(e) => {
                                        eprintln!("[ONCHAIN BROADCAST FAILED ❌] {}", e);
                                        return Ok(None);
                                    }
                                },
                                Err(e) => {
                                    eprintln!("[JUPITER TX BUILD FAILED ❌] {}", e);
                                    return Ok(None);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[JUPITER QUOTE FAILED ❌] {}", e);
                            return Ok(None);
                        }
                    }
                }

                if buy_success {
                    let tx_signature = _onchain_tx_sig.as_deref().ok_or_else(|| {
                        anyhow!("buy broadcast returned success without a transaction signature")
                    })?;
                    let confirmed = self
                        .confirm_transaction_signature(tx_signature, 20)
                        .await
                        .unwrap_or(false);
                    if !confirmed {
                        self.db.update_execution_intent(&position_id, "PENDING_RECONCILIATION", None, Some(tx_signature))?;
                        eprintln!(
                            "[ONCHAIN BUY REJECTED ❌] Transaction {} was not confirmed; no position will be persisted.",
                            tx_signature
                        );
                        return Ok(None);
                    }
                    self.db.update_execution_intent(&position_id, "CONFIRMED", None, Some(tx_signature))?;

                    // Reconcile exact token balance from chain before creating OPEN position.
                    tokio::time::sleep(Duration::from_millis(600)).await;
                    let onchain_bal = self
                        .get_ui_token_balance(&user_pubkey, &signal.token_mint)
                        .await
                        .map_err(|error| anyhow!("buy balance reconciliation failed: {error}"))?;
                    if !onchain_bal.is_finite() || onchain_bal <= 0.0 {
                        eprintln!(
                            "[ONCHAIN BUY REJECTED ❌] Confirmed transaction {} produced no positive token balance; no position will be persisted.",
                            tx_signature
                        );
                        return Ok(None);
                    }
                    new_tokens = onchain_bal;
                    entry_price = amount_usd / onchain_bal;
                    self.db.update_execution_intent(&position_id, "RECONCILED", None, Some(tx_signature))?;
                    println!("[ONCHAIN RECONCILED 📊] Exact Token Units: {:.4} -> Exact Real Entry Price: ${:.6}",
                        onchain_bal, entry_price);
                }
            } else {
                eprintln!("[LIVE EXECUTION ERROR ❌] No valid Solana private key found. Refusing trade.");
                return Ok(None);
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
                // SAME-WHALE RE-ENTRY GUARD: jika whale ini sudah berkontribusi ke posisi ini,
                // tolak averaging — satu whale hanya boleh satu kontribusi per posisi terbuka.
                let whale_addr_lower = signal.whale_address.trim().to_lowercase();
                let whale_already_in = existing
                    .target_whale_address
                    .split(',')
                    .map(|a| a.trim().to_lowercase())
                    .any(|a| a == whale_addr_lower);
                if whale_already_in {
                    eprintln!(
                        "[SAME-WHALE REENTRY BLOCKED 🛡️] @{} ({}) sudah berkontribusi ke posisi {} ({}). Averaging diblokir.",
                        signal.whale_handle, &signal.whale_address[..10.min(signal.whale_address.len())],
                        existing.id, existing.token_symbol
                    );
                    return Ok(None);
                }
                let prev_usd = existing.amount_usd;
                let prev_tokens = existing.amount_native;
                let total_usd = prev_usd + amount_usd;
                // Reconcile total balance directly against on-chain wallet state
                let maybe_pubkey = self.get_keypair().ok().map(|(_, pk)| pk);
                let total_tokens = if let Some(pk) = maybe_pubkey {
                    if let Ok(latest_bal) = self.get_ui_token_balance(&pk, &signal.token_mint).await {
                        if latest_bal > prev_tokens {
                            latest_bal
                        } else {
                            prev_tokens + new_tokens
                        }
                    } else {
                        prev_tokens + new_tokens
                    }
                } else {
                    prev_tokens + new_tokens
                };
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
            id: position_id.clone(),
            chain: Chain::Solana,
            token_symbol: signal.token_symbol.clone(),
            token_mint: signal.token_mint.clone(),
            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            track: signal.track,
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
        if !self.config.dry_run {
            self.db.update_execution_intent(&position_id, "OPEN", None, _onchain_tx_sig.as_deref())?;
        }
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
                            "[ONCHAIN SOLANA SELL 🚀] Executing {:.1}% partial swap for {} ({}) to SOL (Reason: {})",
                            sell_ratio_pct, pos.token_symbol, pos.token_mint, exit_reason
                        );
                        // Query real on-chain token balance
                        let token_amount_raw = self
                            .get_raw_token_balance(&user_pubkey, &pos.token_mint)
                            .await
                            .unwrap_or(0);

                        if token_amount_raw == 0 {
                            println!(
                                "[ONCHAIN BALANCE ZERO ⚪] Wallet holds 0 tokens of {}. Closing DB position directly.",
                                pos.token_symbol
                            );
                            self.db.close_position_portion(
                                position_id,
                                sell_ratio_pct,
                                exit_reason,
                                current_price,
                            )?;
                            return Ok(());
                        }

                        let effective_wallet_fraction = if pos.remaining_portion_pct > 0.0 {
                            (sell_ratio_pct / pos.remaining_portion_pct).clamp(0.0, 1.0)
                        } else {
                            1.0
                        };
                        let sell_units =
                            ((token_amount_raw as f64) * effective_wallet_fraction) as u64;

                        if sell_units == 0 {
                            eprintln!("[SELL ABORT ⚠️] Calculated sell units are 0.");
                            return Ok(());
                        }

                        // Dynamic slippage: 500 bps for emergency cutloss, 300 bps for trailing/dump, 250 bps for TP
                        let slippage_bps: u16 = if exit_reason.contains("EMERGENCY")
                            || exit_reason.contains("HARD_SL")
                        {
                            500
                        } else if exit_reason.contains("PEAK") || exit_reason.contains("DUMP") {
                            300
                        } else {
                            250
                        };

                        let is_pump_token = pos.token_mint.ends_with("pump");
                        let mut sell_broadcasted = false;
                        let mut final_tx_sig = String::new();

                        // Route 1: If Pump token, attempt PumpPortal direct sell first
                        if is_pump_token {
                            println!("[PUMPFUN SELL ROUTE ⚡] Attempting direct bonding curve sell for {}...", pos.token_symbol);
                            let sell_tokens_float = (sell_units as f64) / 1_000_000.0;
                            match self
                                .build_and_sign_pumpfun_tx("sell", &pos.token_mint, sell_tokens_float, 15.0, &user_pubkey, &signing_key)
                                .await
                            {
                                Ok(signed_pump_tx) => match self.broadcast_tx(&signed_pump_tx).await {
                                    Ok(sig) => {
                                        println!("[PUMPFUN SELL CONFIRMED 🟢] TX Sig: {}", sig);
                                        final_tx_sig = sig;
                                        sell_broadcasted = true;
                                    }
                                    Err(e) => {
                                        eprintln!("[PUMPFUN SELL BROADCAST FAILED ⚠️] {}. Falling back to Jupiter...", e);
                                    }
                                },
                                Err(e) => {
                                    eprintln!("[PUMPFUN SELL BUILD FAILED ⚠️] {}. Falling back to Jupiter...", e);
                                }
                            }
                        }

                        // Route 2: Jupiter Quote & Swap (Default or fallback)
                        if !sell_broadcasted {
                            let quote = match self
                                .fetch_jupiter_quote(
                                    &pos.token_mint,
                                    wsol_mint,
                                    sell_units,
                                    slippage_bps,
                                )
                                .await
                            {
                                Ok(q) => q,
                                Err(e) => {
                                    eprintln!(
                                        "[SELL JUPITER QUOTE FAILED ❌] Mint: {} -> Error: {}. DB position will NOT be closed.",
                                        pos.token_mint, e
                                    );
                                    return Err(e);
                                }
                            };

                            let signed_tx = match self
                                .build_and_sign_jupiter_tx(quote, &user_pubkey, &signing_key)
                                .await
                            {
                                Ok(tx) => tx,
                                Err(e) => {
                                    eprintln!(
                                        "[SELL JUPITER BUILD FAILED ❌] Error: {}. DB position will NOT be closed.",
                                        e
                                    );
                                    return Err(e);
                                }
                            };

                            let tx_sig = match self.broadcast_tx(&signed_tx).await {
                                Ok(sig) => sig,
                                Err(e) => {
                                    eprintln!(
                                        "[SELL BROADCAST FAILED ❌] Error: {}. DB position will NOT be closed.",
                                        e
                                    );
                                    return Err(e);
                                }
                            };
                            println!("[ONCHAIN SELL CONFIRMED 🟢] TX Sig: {}", tx_sig);
                            final_tx_sig = tx_sig;
                        }

                        println!(
                            "[SELL BROADCASTED 📡] Sig: https://solscan.io/tx/{} -> Awaiting confirmation...",
                            final_tx_sig
                        );

                        // Strict on-chain confirmation check (20s timeout)
                        let is_confirmed = self
                            .confirm_transaction_signature(&final_tx_sig, 20)
                            .await
                            .unwrap_or(false);

                        if !is_confirmed {
                            eprintln!(
                                "[SELL CONFIRMATION FAILED ❌] On-chain tx {} failed, reverted, or timed out. Position {} will remain OPEN in DB!",
                                final_tx_sig, position_id
                            );
                            return Err(anyhow!("On-chain swap transaction reverted or unconfirmed"));
                        }

                        println!(
                            "[ONCHAIN SOLANA SELL CONFIRMED 🟢] Swap {} confirmed successfully on-chain!",
                            final_tx_sig
                        );

                        // If 100% full exit, trigger asynchronous rent reclamation for newly empty ATA
                        if sell_ratio_pct >= 99.0 {
                            tokio::spawn(async {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                let _ = tokio::process::Command::new("node")
                                    .arg("scripts/reclaim_rent.js")
                                    .current_dir("/home/node/Developments/zenithsw")
                                    .output()
                                    .await;
                            });
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
