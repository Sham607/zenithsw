use crate::config::Config;
use crate::db::{ExecutionIntentMetadata, SmartWalletDb};
use crate::models::{is_blacklisted_asset, Chain, ExecutionTrack, ShadowPosition, ShadowSignal};
use crate::streams::solana_stream::SolanaStreamListener;
use anyhow::{anyhow, Result};
use chrono::Utc;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Interval rebroadcast transaksi bertanda tangan yang sama.
const SEND_LOOP_INTERVAL_MS: u64 = 2_000;
/// Batas keras loop kirim (blockhash Solana valid ~150 blok ≈ 60–90 detik).
const SEND_LOOP_MAX_SECS: u64 = 120;
/// Percobaan `getTransaction` untuk membaca fill aktual setelah konfirmasi.
const FILL_FETCH_ATTEMPTS: u32 = 5;
/// Jeda minimum antar pencarian bukti jual eksternal per posisi (hemat RPC).
const EXTERNAL_EXIT_RECHECK_SECS: u64 = 30;
/// Jumlah signature wallet yang dipindai untuk mencari jual eksternal sejak entry.
const EXTERNAL_EXIT_SIG_LIMIT: usize = 200;
/// Batas `getTransaction` per pencarian; lebih dari ini = riwayat terlalu ramai, ditahan.
const EXTERNAL_EXIT_MAX_TX_FETCH: usize = 40;
/// Umur minimum posisi sebelum saldo 0 tanpa bukti jual boleh dicatat -100% (anti RPC tertinggal).
const EXTERNAL_EXIT_UNVERIFIED_MIN_AGE_SECS: i64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignatureState {
    NotFound,
    Processing,
    Confirmed,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LandingStatus {
    Landed,
    FailedOnChain(String),
    Expired,
    Unresolved,
}

/// Fill aktual dari meta transaksi untuk wallet bot (fee payer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaSwapFill {
    pub fee_lamports: u64,
    /// post - pre lamports fee payer (sudah termasuk fee, rent ATA, wrap/unwrap SOL).
    pub owner_lamports_delta: i128,
    /// post - pre raw token milik wallet untuk mint target.
    pub token_raw_delta: i128,
    pub token_decimals: Option<u32>,
}

#[derive(Debug)]
enum SwapOutcome {
    Landed {
        signature: String,
        fill: Option<SolanaSwapFill>,
        quote_out_amount: u64,
    },
    Unresolved {
        signature: String,
    },
    Rejected(String),
}

/// Blockhash expired hanya bila keduanya diketahui dan block height sudah melewati batas.
pub fn blockhash_expired(
    last_valid_block_height: Option<u64>,
    current_height: Option<u64>,
) -> bool {
    matches!(
        (last_valid_block_height, current_height),
        (Some(last_valid), Some(current)) if current > last_valid
    )
}

#[derive(Debug, Clone)]
pub struct CachedRealizableQuote {
    pub token_mint: String,
    pub token_amount_raw: u64,
    pub estimated_out_lamports: u64,
    pub estimated_out_sol: f64,
    pub price_impact_pct: f64,
    pub fetched_at: Instant,
}

impl CachedRealizableQuote {
    /// Menghitung harga realizable per UI token dalam USD berdasarkan sol_usd dan token decimals.
    pub fn calculate_realizable_price_usd(&self, sol_usd: f64, decimals: u8) -> f64 {
        if self.token_amount_raw == 0 || sol_usd <= 0.0 {
            return 0.0;
        }
        let ui_tokens = self.token_amount_raw as f64 / 10f64.powi(decimals as i32);
        if ui_tokens <= 0.0 {
            return 0.0;
        }
        let realizable_value_usd = self.estimated_out_sol * sol_usd;
        realizable_value_usd / ui_tokens
    }
}

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
    quote_cache: Mutex<HashMap<(String, u64), CachedRealizableQuote>>,
    decimals_cache: Mutex<HashMap<String, u8>>,
    external_exit_checks: Mutex<HashMap<String, Instant>>,
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
            quote_cache: Mutex::new(HashMap::new()),
            decimals_cache: Mutex::new(HashMap::new()),
            external_exit_checks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_latest_blockhash(&self) -> Result<(String, u64)> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getLatestBlockhash",
            "params": [{"commitment": "confirmed"}]
        });
        let response = self.post_rpc(&payload).await?;
        let value = response
            .get("result")
            .and_then(|result| result.get("value"))
            .ok_or_else(|| anyhow!("getLatestBlockhash response missing value"))?;
        let blockhash = value
            .get("blockhash")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("getLatestBlockhash response missing blockhash"))?;
        let last_valid_block_height = value
            .get("lastValidBlockHeight")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("getLatestBlockhash response missing lastValidBlockHeight"))?;
        Ok((blockhash.to_string(), last_valid_block_height))
    }

    fn extract_recent_blockhash(transaction: &[u8]) -> Result<String> {
        fn read_shortvec(bytes: &[u8], offset: &mut usize) -> Result<usize> {
            let mut value = 0usize;
            let mut shift = 0;
            loop {
                let byte = *bytes
                    .get(*offset)
                    .ok_or_else(|| anyhow!("truncated Solana shortvec"))?;
                *offset += 1;
                value |= usize::from(byte & 0x7f) << shift;
                if byte & 0x80 == 0 {
                    return Ok(value);
                }
                shift += 7;
                if shift >= usize::BITS {
                    return Err(anyhow!("Solana shortvec overflow"));
                }
            }
        }

        let mut offset = 0;
        let signatures = read_shortvec(transaction, &mut offset)?;
        let signature_bytes = signatures
            .checked_mul(64)
            .ok_or_else(|| anyhow!("Solana signature length overflow"))?;
        offset = offset
            .checked_add(signature_bytes)
            .ok_or_else(|| anyhow!("Solana transaction offset overflow"))?;
        let first_message_byte = *transaction
            .get(offset)
            .ok_or_else(|| anyhow!("missing Solana message header"))?;
        if first_message_byte & 0x80 != 0 {
            offset += 1;
        }
        offset = offset
            .checked_add(3)
            .ok_or_else(|| anyhow!("Solana header offset overflow"))?;
        let accounts = read_shortvec(transaction, &mut offset)?;
        let account_bytes = accounts
            .checked_mul(32)
            .ok_or_else(|| anyhow!("Solana account length overflow"))?;
        offset = offset
            .checked_add(account_bytes)
            .ok_or_else(|| anyhow!("Solana account offset overflow"))?;
        let end = offset
            .checked_add(32)
            .ok_or_else(|| anyhow!("Solana blockhash offset overflow"))?;
        let blockhash = transaction
            .get(offset..end)
            .ok_or_else(|| anyhow!("missing Solana recent blockhash"))?;
        Ok(bs58::encode(blockhash).into_string())
    }

    fn sign_transaction_base64(unsigned_base64: &str, signing_key: &SigningKey) -> Result<String> {
        let tx_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, unsigned_base64)?;
        if tx_bytes.len() <= 65 || tx_bytes[0] != 1 {
            return Err(anyhow!(
                "Solana transaction must contain exactly one signer"
            ));
        }
        let mut signed = tx_bytes.clone();
        let signature = signing_key.sign(&tx_bytes[65..]);
        signed[1..65].copy_from_slice(&signature.to_bytes());
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            signed,
        ))
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

    /// Derives wallet Base58 public key if private key is configured
    pub fn get_wallet_pubkey(&self) -> Option<String> {
        self.get_keypair().ok().map(|(_, pubkey)| pubkey)
    }

    /// Mengirim satu request read/broadcast ke pool RPC Solana.
    /// Broadcast tetap hanya sekali; caller wajib merekonsiliasi signature.
    pub async fn post_rpc(&self, payload: &Value) -> Result<Value> {
        let mut rpc_urls: Vec<String> = self
            .config
            .solana_rpc_url
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if rpc_urls.is_empty() {
            rpc_urls.push("https://api.mainnet-beta.solana.com".to_string());
        }
        let count = rpc_urls.len();
        if count == 0 {
            return Err(anyhow!("No Solana RPC endpoints configured"));
        }
        let is_broadcast = payload.get("method").and_then(Value::as_str) == Some("sendTransaction");
        let is_blockhash =
            payload.get("method").and_then(Value::as_str) == Some("getLatestBlockhash");
        let is_confirm =
            payload.get("method").and_then(Value::as_str) == Some("getSignatureStatuses");

        // Prioritas kanal presisi (Helius Execution) untuk broadcast transaksi riil, fetch blockhash kritis, dan konfirmasi transaksi
        if is_broadcast || is_blockhash || is_confirm {
            if let Some(ref exec_rpc) = self.config.solana_execution_rpc_url {
                self.rpc_requests.fetch_add(1, Ordering::Relaxed);
                if let Ok(res) = self.client.post(exec_rpc).json(payload).send().await {
                    if res.status().is_success() {
                        if let Ok(json) = res.json::<Value>().await {
                            if json.get("error").is_none() && json.get("result").is_some() {
                                self.rpc_successes.fetch_add(1, Ordering::Relaxed);
                                return Ok(json);
                            }
                        }
                    }
                }
            }
        }

        // Prioritas kanal khusus (Helius Portfolio) untuk pembacaan saldo & kepemilikan akun token
        let is_portfolio_read = matches!(
            payload.get("method").and_then(Value::as_str),
            Some(
                "getBalance"
                    | "getTokenAccountsByOwner"
                    | "getTokenAccountBalance"
                    | "getAccountInfo"
            )
        );
        if is_portfolio_read {
            if let Some(ref portfolio_rpc) = self.config.solana_portfolio_rpc_url {
                self.rpc_requests.fetch_add(1, Ordering::Relaxed);
                if let Ok(res) = self.client.post(portfolio_rpc).json(payload).send().await {
                    if res.status().as_u16() == 429 {
                        self.rpc_rate_limited.fetch_add(1, Ordering::Relaxed);
                    } else if res.status().is_success() {
                        if let Ok(json) = res.json::<Value>().await {
                            if json.get("error").is_none() && json.get("result").is_some() {
                                self.rpc_successes.fetch_add(1, Ordering::Relaxed);
                                return Ok(json);
                            }
                        }
                    }
                }
            }
        }

        let start = self.rpc_index.fetch_add(1, Ordering::Relaxed) % count;
        let mut last_err = anyhow!("All Solana RPC endpoints failed");
        for offset in 0..count {
            let index = (start + offset) % count;
            if self.rpc_cooldowns.lock().unwrap()[index]
                .map(|until| until > Instant::now())
                .unwrap_or(false)
            {
                continue;
            }
            self.rpc_requests.fetch_add(1, Ordering::Relaxed);
            match self
                .client
                .post(&rpc_urls[index])
                .json(payload)
                .send()
                .await
            {
                Ok(res) if res.status().as_u16() == 429 => {
                    self.rpc_rate_limited.fetch_add(1, Ordering::Relaxed);
                    self.rpc_cooldowns.lock().unwrap()[index] =
                        Some(Instant::now() + Duration::from_secs(30));
                    last_err = anyhow!("Solana RPC endpoint {index} rate-limited");
                }
                Ok(res) => match res.json::<Value>().await {
                    Ok(json) if json.get("error").is_none() && json.get("result").is_some() => {
                        self.rpc_successes.fetch_add(1, Ordering::Relaxed);
                        return Ok(json);
                    }
                    Ok(json) => {
                        self.rpc_failures.fetch_add(1, Ordering::Relaxed);
                        last_err = anyhow!(
                            "Solana RPC endpoint {index} error: {}",
                            json.get("error").unwrap_or(&json)
                        );
                        if is_broadcast {
                            return Err(last_err);
                        }
                    }
                    Err(error) => {
                        self.rpc_failures.fetch_add(1, Ordering::Relaxed);
                        last_err = anyhow!("Solana RPC endpoint {index} invalid JSON: {error}");
                        if is_broadcast {
                            return Err(last_err);
                        }
                    }
                },
                Err(error) => {
                    self.rpc_failures.fetch_add(1, Ordering::Relaxed);
                    self.rpc_cooldowns.lock().unwrap()[index] =
                        Some(Instant::now() + Duration::from_secs(15));
                    last_err = anyhow!("Solana RPC endpoint {index} transport error: {error}");
                    if is_broadcast {
                        return Err(last_err);
                    }
                }
            }
        }
        Err(last_err)
    }

    pub fn rpc_health_snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.rpc_requests.load(Ordering::Relaxed),
            self.rpc_successes.load(Ordering::Relaxed),
            self.rpc_rate_limited.load(Ordering::Relaxed),
            self.rpc_failures.load(Ordering::Relaxed),
        )
    }

    /// Saldo native SOL (lamports) via RPC. Fail-closed: respons tanpa `result.value`
    /// numerik adalah error, bukan saldo nol.
    pub async fn get_native_balance_lamports(&self, pubkey: &str) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBalance",
            "params": [pubkey, {"commitment": "confirmed"}]
        });

        let res_json = self.post_rpc(&payload).await?;
        res_json["result"]["value"]
            .as_u64()
            .ok_or_else(|| anyhow!("getBalance response malformed: {:?}", res_json))
    }

    /// Saldo native SOL dalam SOL. Fail-closed (lihat `get_native_balance_lamports`).
    pub async fn get_native_balance(&self, pubkey: &str) -> Result<f64> {
        Ok(self.get_native_balance_lamports(pubkey).await? as f64 / 1_000_000_000.0)
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

    /// Menjumlahkan saldo semua token account untuk satu mint dari respons
    /// `getTokenAccountsByOwner` (jsonParsed). Daftar kosong = saldo nol terbukti;
    /// struktur malformed = error (tidak pernah dianggap nol).
    pub fn parse_token_accounts_balance(json: &Value) -> Result<(u64, f64)> {
        let accounts = json["result"]["value"]
            .as_array()
            .ok_or_else(|| anyhow!("getTokenAccountsByOwner response malformed: {:?}", json))?;
        let mut raw_total: u64 = 0;
        let mut decimals: Option<u32> = None;
        for account in accounts {
            let token_amount = &account["account"]["data"]["parsed"]["info"]["tokenAmount"];
            let raw = token_amount["amount"]
                .as_str()
                .ok_or_else(|| anyhow!("token account tanpa tokenAmount.amount"))?
                .parse::<u64>()
                .map_err(|e| anyhow!("tokenAmount.amount invalid: {e}"))?;
            let account_decimals = token_amount["decimals"]
                .as_u64()
                .and_then(|d| u32::try_from(d).ok())
                .ok_or_else(|| anyhow!("token account tanpa tokenAmount.decimals"))?;
            if decimals.is_some_and(|d| d != account_decimals) {
                return Err(anyhow!("decimals token account tidak konsisten"));
            }
            decimals = Some(account_decimals);
            raw_total = raw_total
                .checked_add(raw)
                .ok_or_else(|| anyhow!("token balance overflow"))?;
        }
        let ui = match decimals {
            Some(d) => raw_total as f64 / 10f64.powi(d as i32),
            None => 0.0,
        };
        Ok((raw_total, ui))
    }

    pub async fn get_token_balance(&self, user_pubkey: &str, mint: &str) -> Result<(u64, f64)> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenAccountsByOwner",
            "params": [
                user_pubkey,
                {"mint": mint},
                {"encoding": "jsonParsed", "commitment": "confirmed"}
            ]
        });
        let json = self.post_rpc(&payload).await?;
        Self::parse_token_accounts_balance(&json)
    }

    /// Saldo UI token untuk satu mint (semua token account dijumlahkan). Fail-closed.
    pub async fn get_ui_token_balance(&self, user_pubkey: &str, mint: &str) -> Result<f64> {
        Ok(self.get_token_balance(user_pubkey, mint).await?.1)
    }

    /// Saldo raw token untuk satu mint (semua token account dijumlahkan). Fail-closed.
    pub async fn get_raw_token_balance(&self, user_pubkey: &str, mint: &str) -> Result<u64> {
        Ok(self.get_token_balance(user_pubkey, mint).await?.0)
    }

    /// Saldo token (raw dan UI) untuk dompet bot sendiri yang terkonfigurasi. Fail-closed.
    pub async fn get_own_token_balance(&self, mint: &str) -> Result<(u64, f64)> {
        let pubkey = self
            .get_wallet_pubkey()
            .ok_or_else(|| anyhow!("Wallet not configured"))?;
        self.get_token_balance(&pubkey, mint).await
    }

    /// Fetches all SPL and Token-2022 tokens held by user_pubkey with non-zero balance.
    /// Best-effort (dipakai heartbeat/equity); jangan dipakai untuk keputusan menutup posisi.
    pub async fn get_all_wallet_token_balances(&self, user_pubkey: &str) -> Vec<(String, f64)> {
        self.get_all_wallet_token_balances_strict(user_pubkey)
            .await
            .unwrap_or_default()
    }

    /// Versi fail-closed: error RPC / respons malformed pada salah satu program token
    /// mengembalikan error, bukan daftar kosong.
    pub async fn get_all_wallet_token_balances_strict(
        &self,
        user_pubkey: &str,
    ) -> Result<Vec<(String, f64)>> {
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
                    {"encoding": "jsonParsed", "commitment": "confirmed"}
                ]
            });

            let res = self.post_rpc(&payload).await?;
            let accs = res["result"]["value"]
                .as_array()
                .ok_or_else(|| anyhow!("getTokenAccountsByOwner({pid}) response malformed"))?;
            for acc in accs {
                let info = &acc["account"]["data"]["parsed"]["info"];
                let mint = info["mint"]
                    .as_str()
                    .ok_or_else(|| anyhow!("token account tanpa mint"))?;
                let ui_amt = info["tokenAmount"]["uiAmount"].as_f64().unwrap_or(0.0);
                if ui_amt > 0.000001 {
                    tokens.push((mint.to_string(), ui_amt));
                }
            }
        }
        Ok(tokens)
    }

    /// Startup Reconciliation & Orphan Token Recovery:
    /// Reconciles on-chain physical SPL token balances with DB shadow positions.
    /// Closes ghost DB positions where on-chain balance is 0.
    /// Adopts on-chain orphan token holdings as trackable shadow positions.
    pub async fn reconcile_startup_positions(&self) -> Result<(usize, usize)> {
        if self.config.dry_run {
            println!("[STARTUP RECONCILIATION SKIP 🧪] DRY_RUN=true. Simulation mode does not adopt real on-chain wallet balances.");
            return Ok((0, 0));
        }

        let (_, wallet_pubkey) = match self.get_keypair() {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("[STARTUP RECONCILIATION SKIP] Keypair not configured or invalid: {e}");
                return Ok((0, 0));
            }
        };

        println!(
            "[STARTUP RECONCILIATION 🔍] Scanning physical on-chain SPL holdings for {}...",
            wallet_pubkey
        );
        let onchain_tokens = self
            .get_all_wallet_token_balances_strict(&wallet_pubkey)
            .await
            .map_err(|error| {
                anyhow!("saldo token fisik tidak terbaca; rekonsiliasi dibatalkan: {error}")
            })?;
        let mut onchain_map: HashMap<String, f64> = HashMap::new();
        for (mint, amt) in onchain_tokens {
            if !is_blacklisted_asset(&mint, "") {
                onchain_map.insert(mint, amt);
            }
        }

        let mut db_positions = self.db.get_active_positions()?;
        db_positions.retain(|p| p.chain == Chain::Solana);

        let mut ghost_closed = 0usize;
        let mut orphans_adopted = 0usize;

        // 1. Check existing DB positions against physical on-chain balance
        for pos in &db_positions {
            // Isolasi kepemilikan posisi: jika posisi tercatat milik wallet lain, abaikan (jangan ditutup / jangan disentuh)
            if let Some(ref owner) = pos.owner_pubkey {
                if owner != &wallet_pubkey {
                    println!(
                        "[STARTUP RECONCILIATION 🛡️] Posisi {} ({}) dimiliki wallet lain ({}), dilewati dari audit wallet aktif {}.",
                        pos.id, pos.token_symbol, owner, wallet_pubkey
                    );
                    continue;
                }
            }

            if let Some(&physical_amt) = onchain_map.get(&pos.token_mint) {
                if (pos.amount_native - physical_amt).abs() > 0.000001 {
                    let mut updated = pos.clone();
                    updated.amount_native = physical_amt;
                    updated.last_update = Utc::now();
                    if let Err(e) = self.db.upsert_position(&updated) {
                        eprintln!("[STARTUP RECONCILIATION ERROR] Failed updating amount_native for {}: {e}", pos.token_mint);
                    } else {
                        println!(
                            "[STARTUP RECONCILIATION ⚖️] Reconciled {} balance: DB {:.4} -> Physical {:.4}",
                            pos.token_symbol, pos.amount_native, physical_amt
                        );
                    }
                }
                // Token accounted for
                onchain_map.remove(&pos.token_mint);
            } else {
                // Saldo fisik 0 -> dijual di luar bot (Titan, Raydium, dll.).
                // PnL wajib dari meta transaksi jual, bukan harga model; tanpa bukti posisi ditahan.
                match self.reconcile_external_exit(pos).await {
                    Ok(true) => ghost_closed += 1,
                    Ok(false) => {}
                    Err(e) => eprintln!(
                        "[STARTUP RECONCILIATION HOLD ⚠️] Posisi {} ({}) saldo on-chain 0 tetapi bukti jual belum lengkap: {e}",
                        pos.id, pos.token_symbol
                    ),
                }
            }
        }

        // 2. Any remaining items in onchain_map are orphan holdings (on-chain balance > 0, but no active DB position)
        for (orphan_mint, orphan_amt) in onchain_map {
            // Attempt to resolve symbol & current price via Solana resolver
            let (symbol, price, _, _) =
                match SolanaStreamListener::fetch_token_price(&orphan_mint).await {
                    Ok(res) => res,
                    Err(_) => ("UNKNOWN".to_string(), 0.0, 0.0, 0.0),
                };

            let approx_usd = if price > 0.0 { orphan_amt * price } else { 0.0 };
            if price <= 0.0 || approx_usd < 1.0 {
                continue;
            }
            let orphan_pos = ShadowPosition {
                id: format!("orphan_{}", &orphan_mint[..orphan_mint.len().min(12)]),
                chain: Chain::Solana,
                token_symbol: if symbol.is_empty() {
                    "ORPHAN".to_string()
                } else {
                    symbol
                },
                token_mint: orphan_mint.clone(),
                target_whale_handle: "ORPHAN_ADOPTED".to_string(),
                target_whale_address: wallet_pubkey.clone(),
                track: ExecutionTrack::FastLane,
                entry_price: price,
                current_price: price,
                peak_price: price,
                amount_native: orphan_amt,
                amount_usd: approx_usd,
                remaining_portion_pct: 100.0,
                unrealized_pnl_pct: 0.0,
                realized_pnl_usd: 0.0,
                entry_time: Utc::now(),
                last_update: Utc::now(),
                status: "OPEN".to_string(),
                is_dry_run: false, // On-chain adopted orphan is a real physical holding
                owner_pubkey: Some(wallet_pubkey.clone()),
                price_source: "DEXSCREENER".to_string(),
                price_updated_at: Utc::now(),
            };

            if let Err(e) = self.db.upsert_position(&orphan_pos) {
                eprintln!(
                    "[STARTUP RECONCILIATION ERROR] Failed adopting orphan token {}: {e}",
                    orphan_mint
                );
            } else {
                orphans_adopted += 1;
                println!(
                    "[STARTUP RECONCILIATION 🐣] Adopted orphan token {} ({}) balance: {:.4} (${:.2})",
                    orphan_pos.token_symbol, orphan_mint, orphan_amt, approx_usd
                );
            }
        }

        Ok((ghost_closed, orphans_adopted))
    }

    /// Fetches a Jupiter Swap quote (v1)
    /// Rekonsiliasi posisi live yang saldo token on-chain-nya terbukti 0 (dijual di luar bot).
    /// PnL dari meta transaksi jual eksternal (SOL bersih diterima wallet), bukan harga model.
    /// `Ok(true)` = posisi ditutup, `Ok(false)` = dilewati (jeda cek), `Err` = bukti belum
    /// lengkap sehingga posisi tetap aktif dan dicoba lagi.
    pub async fn reconcile_external_exit(&self, pos: &ShadowPosition) -> Result<bool> {
        {
            let mut guard = self
                .external_exit_checks
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if guard
                .get(&pos.id)
                .is_some_and(|last| last.elapsed() < Duration::from_secs(EXTERNAL_EXIT_RECHECK_SECS))
            {
                return Ok(false);
            }
            guard.insert(pos.id.clone(), Instant::now());
        }

        let wallet = self
            .get_wallet_pubkey()
            .ok_or_else(|| anyhow!("wallet bot tidak terkonfigurasi"))?;
        if pos.owner_pubkey.as_deref().is_some_and(|owner| owner != wallet) {
            return Err(anyhow!("posisi milik wallet lain; tidak direkonsiliasi"));
        }
        if self
            .db
            .has_active_execution_intent(Chain::Solana, &pos.token_mint)?
        {
            return Err(anyhow!("intent bot untuk mint ini masih aktif; menunggu resolver"));
        }

        let entry_ts = pos.entry_time.timestamp();
        let sigs_json = self
            .post_rpc(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "getSignaturesForAddress",
                "params": [wallet, {"limit": EXTERNAL_EXIT_SIG_LIMIT, "commitment": "confirmed"}]
            }))
            .await?;
        let sigs = sigs_json["result"]
            .as_array()
            .ok_or_else(|| anyhow!("getSignaturesForAddress malformed"))?;
        let mut covered = sigs.len() < EXTERNAL_EXIT_SIG_LIMIT;
        let mut candidates = Vec::new();
        for item in sigs {
            if item["blockTime"].as_i64().is_some_and(|t| t < entry_ts) {
                covered = true;
                break;
            }
            if !item["err"].is_null() {
                continue;
            }
            let sig = item["signature"]
                .as_str()
                .ok_or_else(|| anyhow!("signature tidak valid di riwayat"))?;
            // Transaksi milik bot sendiri sudah dibukukan oleh jalur eksekusi.
            if !self.db.is_execution_signature(sig)? {
                candidates.push(sig.to_string());
            }
        }
        if !covered {
            return Err(anyhow!(
                "riwayat wallet > {EXTERNAL_EXIT_SIG_LIMIT} transaksi sejak entry; bukti tidak lengkap"
            ));
        }
        if candidates.len() > EXTERNAL_EXIT_MAX_TX_FETCH {
            return Err(anyhow!(
                "{} transaksi kandidat > batas {EXTERNAL_EXIT_MAX_TX_FETCH}; ditahan",
                candidates.len()
            ));
        }

        let mut tokens_sold_raw: i128 = 0;
        let mut proceeds_lamports: i128 = 0;
        let mut decimals: Option<u32> = None;
        let mut evidence = Vec::new();
        for sig in &candidates {
            let tx = self
                .post_rpc(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getTransaction",
                    "params": [sig, {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 1}]
                }))
                .await?;
            if tx["result"].is_null() {
                return Err(anyhow!("getTransaction {sig} belum tersedia"));
            }
            // Bukan swap wallet ini untuk mint ini (fee payer lain / mint lain) -> bukan bukti.
            let Ok(fill) = Self::parse_swap_fill(&tx["result"], &wallet, &pos.token_mint) else {
                continue;
            };
            if fill.token_raw_delta < 0 {
                tokens_sold_raw += -fill.token_raw_delta;
                proceeds_lamports += fill.owner_lamports_delta;
                decimals = decimals.or(fill.token_decimals);
                evidence.push(sig.clone());
            }
        }

        if evidence.is_empty() {
            if pos.owner_pubkey.is_none() {
                return Err(anyhow!(
                    "tidak ada transaksi jual di wallet aktif dan pemilik posisi tidak tercatat; tidak ditutup otomatis"
                ));
            }
            // Posisi baru + saldo 0 bisa karena node RPC tertinggal; jangan tutup -100% terlalu dini.
            let age_secs = (Utc::now() - pos.entry_time).num_seconds();
            if age_secs < EXTERNAL_EXIT_UNVERIFIED_MIN_AGE_SECS {
                return Err(anyhow!(
                    "saldo 0 tanpa bukti jual pada posisi berumur {age_secs}s; ditahan (kemungkinan RPC tertinggal)"
                ));
            }
            self.db
                .close_position_without_fill(&pos.id, "EXTERNAL_EXIT_UNVERIFIED")?;
            println!(
                "[EXTERNAL EXIT UNVERIFIED ⚠️] {} ({}) saldo 0 tanpa transaksi jual terbukti; dicatat -100% konservatif.",
                pos.token_symbol, pos.id
            );
            return Ok(true);
        }

        let decimals = decimals.ok_or_else(|| anyhow!("decimals tidak ada di meta transaksi"))?;
        // ponytail: harga SOL/USD saat deteksi, bukan saat jual; selisih kecil untuk deteksi runtime,
        // bisa membesar bila terdeteksi saat startup berjam-jam kemudian. Upgrade: harga SOL historis.
        let sol_usd = self.get_sol_price().await?;
        match external_exit_price_usd(tokens_sold_raw, decimals, proceeds_lamports, sol_usd) {
            Some(exit_price) => {
                self.db
                    .close_position(&pos.id, "MANUAL_EXIT_EXTERNAL", exit_price)?;
                println!(
                    "[EXTERNAL EXIT RECORDED ✅] {} ({}) exit=${:.10} proceeds={} lamports bukti={}",
                    pos.token_symbol,
                    pos.id,
                    exit_price,
                    proceeds_lamports,
                    evidence.join(",")
                );
            }
            None => {
                self.db
                    .close_position_without_fill(&pos.id, "MANUAL_EXIT_EXTERNAL_ZERO_PROCEEDS")?;
                println!(
                    "[EXTERNAL EXIT RECORDED ⚠️] {} ({}) hasil jual <= 0 SOL; dicatat -100%. bukti={}",
                    pos.token_symbol,
                    pos.id,
                    evidence.join(",")
                );
            }
        }
        Ok(true)
    }

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

    /// Decimals mint dari `getTokenSupply`. Gagal baca = error (tidak ada nilai default).
    pub async fn get_token_decimals(&self, mint: &str) -> Result<u8> {
        {
            let guard = self
                .decimals_cache
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(&dec) = guard.get(mint) {
                return Ok(dec);
            }
        }
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTokenSupply",
            "params": [mint]
        });
        let json = self.post_rpc(&payload).await?;
        let dec = json
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.get("decimals"))
            .and_then(|d| d.as_u64())
            .and_then(|d| u8::try_from(d).ok())
            .ok_or_else(|| anyhow!("getTokenSupply {mint}: decimals tidak valid di respons"))?;
        let mut guard = self
            .decimals_cache
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        guard.insert(mint.to_string(), dec);
        Ok(dec)
    }

    /// Fetches a realizable exit quote from Jupiter (token -> WSOL).
    /// Uses in-memory cache with TTL exit_quote_ttl_ms to avoid hammering Jupiter rate-limits.
    /// Soft quote: allows fallback to cache (up to stale_price_max_secs) if fresh fetch fails.
    pub async fn get_realizable_exit_quote(
        &self,
        token_mint: &str,
        token_amount_raw: u64,
    ) -> Result<CachedRealizableQuote> {
        self.get_exit_quote_internal(token_mint, token_amount_raw, false, true)
            .await
    }

    /// Fetches a fresh realizable exit quote from Jupiter bypassing cache.
    /// Hard exit/SL quote: fails immediately if fresh fetch fails, never falls back to stale cache.
    pub async fn get_fresh_exit_quote(
        &self,
        token_mint: &str,
        token_amount_raw: u64,
    ) -> Result<CachedRealizableQuote> {
        self.get_exit_quote_internal(token_mint, token_amount_raw, true, false)
            .await
    }

    async fn get_exit_quote_internal(
        &self,
        token_mint: &str,
        token_amount_raw: u64,
        bypass_cache: bool,
        allow_stale_fallback: bool,
    ) -> Result<CachedRealizableQuote> {
        let wsol_mint = "So11111111111111111111111111111111111111112";
        let now = Instant::now();
        let cache_ttl = Duration::from_millis(self.config.exit_quote_ttl_ms);

        // Kuotasi wajib untuk jumlah token riil posisi; tanpa jumlah tidak ada harga (sampel dilarang).
        if token_amount_raw == 0 {
            return Err(anyhow!(
                "kuotasi exit {token_mint} ditolak: jumlah token 0 / tidak terbukti"
            ));
        }
        let amount = token_amount_raw;

        let cache_key = (token_mint.to_string(), amount);

        // 1. Check cache freshness (< exit_quote_ttl_ms) unless bypass_cache is requested
        if !bypass_cache {
            let guard = self
                .quote_cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = guard.get(&cache_key) {
                if now.duration_since(cached.fetched_at) < cache_ttl {
                    return Ok(cached.clone());
                }
            }
        }

        // 2. Fetch fresh quote from Jupiter
        match self
            .fetch_jupiter_quote(token_mint, wsol_mint, amount, 300)
            .await
        {
            Ok(val) => {
                let out_amount_str = val
                    .get("outAmount")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("Jupiter quote missing outAmount"))?;
                let out_lamports = out_amount_str
                    .parse::<u64>()
                    .map_err(|e| anyhow!("Failed to parse outAmount {}: {}", out_amount_str, e))?;
                let estimated_out_sol = out_lamports as f64 / 1_000_000_000.0;
                let price_impact_pct = val
                    .get("priceImpactPct")
                    .and_then(|v| {
                        v.as_str()
                            .and_then(|s| s.parse::<f64>().ok())
                            .or_else(|| v.as_f64())
                    })
                    .filter(|p| p.is_finite())
                    .ok_or_else(|| anyhow!("Jupiter quote missing or invalid priceImpactPct"))?;

                let quote = CachedRealizableQuote {
                    token_mint: token_mint.to_string(),
                    token_amount_raw: amount,
                    estimated_out_lamports: out_lamports,
                    estimated_out_sol,
                    price_impact_pct,
                    fetched_at: now,
                };

                let mut guard = self
                    .quote_cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.insert(cache_key, quote.clone());
                Ok(quote)
            }
            Err(err) => {
                // Fallback to cache only if explicitly permitted (soft display quote only)
                if allow_stale_fallback {
                    let guard = self
                        .quote_cache
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if let Some(cached) = guard.get(&cache_key) {
                        let max_stale =
                            Duration::from_secs(self.config.stale_price_max_secs.max(2));
                        if now.duration_since(cached.fetched_at) < max_stale {
                            return Ok(cached.clone());
                        }
                    }
                }
                Err(err)
            }
        }
    }

    /// Builds an unsigned transaction and returns it with aggregator expiry.
    pub async fn build_jupiter_unsigned_tx(
        &self,
        quote_json: Value,
        user_pubkey: &str,
    ) -> Result<(String, Option<u64>)> {
        let swap_url = "https://api.jup.ag/swap/v1/swap";
        let payload = serde_json::json!({
            "quoteResponse": quote_json,
            "userPublicKey": user_pubkey,
            "wrapAndUnwrapSol": true,
            "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": {
                "priorityLevelWithMaxLamports": {
                    "maxLamports": self.config.solana_priority_fee_max_lamports,
                    "priorityLevel": "veryHigh"
                }
            }
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

        if tx_bytes.len() <= 65 {
            return Err(anyhow!("Invalid Jupiter transaction payload received"));
        }
        Self::extract_recent_blockhash(&tx_bytes)?;
        Ok((
            swap_resp.swap_transaction,
            swap_resp.last_valid_block_height,
        ))
    }

    /// Satu kali `sendTransaction`. Simulasi sudah dilakukan terpisah sebelum signing,
    /// jadi preflight dilewati; `max_retries` 0 berarti rebroadcast dikendalikan engine.
    async fn send_signed_once(&self, signed_base64: &str, max_retries: u8) -> Result<String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendTransaction",
            "params": [
                signed_base64,
                {
                    "skipPreflight": true,
                    "maxRetries": max_retries,
                    "encoding": "base64"
                }
            ]
        });

        let res_json = self.post_rpc(&payload).await?;
        res_json["result"]
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("Invalid RPC sendTransaction response: {:?}", res_json))
    }

    /// Signature transaksi Solana = 64 byte pertama setelah shortvec jumlah signature.
    pub fn signature_from_signed(signed_base64: &str) -> Result<String> {
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, signed_base64)?;
        if bytes.len() <= 65 || bytes[0] != 1 {
            return Err(anyhow!(
                "signed transaction harus memiliki tepat satu signature"
            ));
        }
        let signature = &bytes[1..65];
        if signature.iter().all(|b| *b == 0) {
            return Err(anyhow!("signed transaction memiliki signature kosong"));
        }
        Ok(bs58::encode(signature).into_string())
    }

    /// Mengurai respons `getSignatureStatuses` untuk satu signature. Malformed = error.
    pub fn parse_signature_status(json: &Value) -> Result<SignatureState> {
        let statuses = json["result"]["value"]
            .as_array()
            .ok_or_else(|| anyhow!("getSignatureStatuses response malformed: {:?}", json))?;
        let Some(status) = statuses.first() else {
            return Err(anyhow!("getSignatureStatuses tanpa entri"));
        };
        if status.is_null() {
            return Ok(SignatureState::NotFound);
        }
        if let Some(err) = status.get("err") {
            if !err.is_null() {
                return Ok(SignatureState::Failed(err.to_string()));
            }
        }
        match status.get("confirmationStatus").and_then(Value::as_str) {
            Some("confirmed" | "finalized") => Ok(SignatureState::Confirmed),
            _ => Ok(SignatureState::Processing),
        }
    }

    pub async fn fetch_signature_state(&self, signature: &str) -> Result<SignatureState> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getSignatureStatuses",
            "params": [[signature], {"searchTransactionHistory": true}]
        });
        let json = self.post_rpc(&payload).await?;
        Self::parse_signature_status(&json)
    }

    pub async fn get_block_height(&self) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getBlockHeight",
            "params": [{"commitment": "confirmed"}]
        });
        let json = self.post_rpc(&payload).await?;
        json["result"]
            .as_u64()
            .ok_or_else(|| anyhow!("getBlockHeight response malformed: {:?}", json))
    }

    /// Simulasi nyata transaksi unsigned terhadap state chain terkini (tanpa verifikasi
    /// signature, blockhash asli dipertahankan). Error RPC maupun error program = tolak.
    async fn simulate_unsigned(&self, unsigned_base64: &str) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateTransaction",
            "params": [
                unsigned_base64,
                {
                    "encoding": "base64",
                    "sigVerify": false,
                    "replaceRecentBlockhash": false,
                    "commitment": "processed"
                }
            ]
        });
        let json = self.post_rpc(&payload).await?;
        Self::parse_simulation(&json)
    }

    pub fn parse_simulation(json: &Value) -> Result<u64> {
        let value = json["result"]
            .get("value")
            .filter(|v| v.is_object())
            .ok_or_else(|| anyhow!("simulateTransaction response malformed: {:?}", json))?;
        if let Some(err) = value.get("err").filter(|e| !e.is_null()) {
            let logs: Vec<&str> = value["logs"]
                .as_array()
                .map(|l| l.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let tail = logs
                .iter()
                .rev()
                .take(3)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(anyhow!("simulasi gagal: {err} logs: {tail}"));
        }
        Ok(value["unitsConsumed"].as_u64().unwrap_or(0))
    }

    /// Mengurai fill aktual dari `getTransaction` (encoding json). Fee payer wajib owner.
    pub fn parse_swap_fill(tx: &Value, owner: &str, token_mint: &str) -> Result<SolanaSwapFill> {
        let meta = tx
            .get("meta")
            .filter(|m| m.is_object())
            .ok_or_else(|| anyhow!("transaksi tanpa meta"))?;
        if !meta["err"].is_null() {
            return Err(anyhow!("transaksi gagal on-chain: {}", meta["err"]));
        }
        let fee_payer = tx["transaction"]["message"]["accountKeys"]
            .as_array()
            .and_then(|keys| keys.first())
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("accountKeys tidak tersedia"))?;
        if fee_payer != owner {
            return Err(anyhow!("fee payer {fee_payer} bukan wallet bot {owner}"));
        }
        let fee_lamports = meta["fee"]
            .as_u64()
            .ok_or_else(|| anyhow!("meta.fee tidak tersedia"))?;
        let pre = meta["preBalances"]
            .as_array()
            .and_then(|b| b.first())
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("meta.preBalances tidak tersedia"))?;
        let post = meta["postBalances"]
            .as_array()
            .and_then(|b| b.first())
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow!("meta.postBalances tidak tersedia"))?;

        let mut decimals: Option<u32> = None;
        let mut sum_balances = |field: &str| -> Result<i128> {
            let entries = meta[field]
                .as_array()
                .ok_or_else(|| anyhow!("meta.{field} tidak tersedia"))?;
            let mut total: i128 = 0;
            for entry in entries {
                if entry["mint"].as_str() != Some(token_mint)
                    || entry["owner"].as_str() != Some(owner)
                {
                    continue;
                }
                let amount = entry["uiTokenAmount"]["amount"]
                    .as_str()
                    .ok_or_else(|| anyhow!("uiTokenAmount.amount tidak tersedia"))?
                    .parse::<u64>()
                    .map_err(|e| anyhow!("uiTokenAmount.amount invalid: {e}"))?;
                if let Some(d) = entry["uiTokenAmount"]["decimals"]
                    .as_u64()
                    .and_then(|d| u32::try_from(d).ok())
                {
                    decimals = Some(d);
                }
                total += i128::from(amount);
            }
            Ok(total)
        };
        let token_pre = sum_balances("preTokenBalances")?;
        let token_post = sum_balances("postTokenBalances")?;

        Ok(SolanaSwapFill {
            fee_lamports,
            owner_lamports_delta: i128::from(post) - i128::from(pre),
            token_raw_delta: token_post - token_pre,
            token_decimals: decimals,
        })
    }

    async fn fetch_swap_fill(
        &self,
        signature: &str,
        owner: &str,
        token_mint: &str,
    ) -> Option<SolanaSwapFill> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getTransaction",
            "params": [
                signature,
                {"encoding": "json", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}
            ]
        });
        for attempt in 1..=FILL_FETCH_ATTEMPTS {
            match self.post_rpc(&payload).await {
                Ok(json) if !json["result"].is_null() => {
                    match Self::parse_swap_fill(&json["result"], owner, token_mint) {
                        Ok(fill) => return Some(fill),
                        Err(error) => {
                            eprintln!("[FILL PARSE ERROR] tx={signature} error={error}");
                            return None;
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("[FILL FETCH RETRY] tx={signature} attempt={attempt} error={error}")
                }
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        None
    }

    /// Mengirim ulang transaksi bertanda tangan yang sama sampai landing, gagal on-chain,
    /// atau blockhash terbukti expired (block height > lastValidBlockHeight).
    async fn send_until_landed_or_expired(
        &self,
        intent_id: &str,
        signed_tx: &str,
        signature: &str,
        last_valid_block_height: u64,
    ) -> Result<(LandingStatus, bool)> {
        let deadline = Instant::now() + Duration::from_secs(SEND_LOOP_MAX_SECS);
        let mut broadcasted = false;
        loop {
            match self.send_signed_once(signed_tx, 0).await {
                Ok(returned) => {
                    if returned != signature {
                        eprintln!(
                            "[SOLANA SEND WARN] RPC mengembalikan signature {returned} != {signature}"
                        );
                    }
                    if !broadcasted {
                        self.db.update_execution_intent(
                            intent_id,
                            "BROADCASTED",
                            None,
                            Some(signature),
                        )?;
                        broadcasted = true;
                        println!("[ONCHAIN BROADCASTED 📡] intent={intent_id} sig={signature}");
                    }
                }
                Err(error) => eprintln!("[SOLANA SEND RETRY] sig={signature} error={error}"),
            }
            tokio::time::sleep(Duration::from_millis(SEND_LOOP_INTERVAL_MS)).await;

            match self.fetch_signature_state(signature).await {
                Ok(SignatureState::Confirmed) => return Ok((LandingStatus::Landed, broadcasted)),
                Ok(SignatureState::Failed(err)) => {
                    return Ok((LandingStatus::FailedOnChain(err), broadcasted))
                }
                _ => {}
            }

            if let Ok(height) = self.get_block_height().await {
                if blockhash_expired(Some(last_valid_block_height), Some(height)) {
                    // Cek terakhir setelah expiry: transaksi bisa landing di blok terakhir.
                    return Ok(match self.fetch_signature_state(signature).await {
                        Ok(SignatureState::Confirmed) => (LandingStatus::Landed, broadcasted),
                        Ok(SignatureState::Failed(err)) => {
                            (LandingStatus::FailedOnChain(err), broadcasted)
                        }
                        Ok(SignatureState::NotFound) => (LandingStatus::Expired, broadcasted),
                        _ => (LandingStatus::Unresolved, broadcasted),
                    });
                }
            }
            if Instant::now() >= deadline {
                return Ok((LandingStatus::Unresolved, broadcasted));
            }
        }
    }

    fn reject_intent(&self, intent_id: &str, reason: String) -> Result<SwapOutcome> {
        self.db
            .update_execution_intent(intent_id, "FAILED", None, None)?;
        Ok(SwapOutcome::Rejected(reason))
    }

    /// Pipeline swap kanonik Jupiter: quote → build (fee eksplisit, expiry wajib) →
    /// simulasi → sign + persist signature → rebroadcast sampai landed/expired →
    /// fill dari meta transaksi. Intent wajib sudah dibuat pemanggil (state INTENT).
    #[allow(clippy::too_many_arguments)]
    async fn execute_jupiter_swap(
        &self,
        intent_id: &str,
        input_mint: &str,
        output_mint: &str,
        token_mint: &str,
        amount_raw: u64,
        slippage_bps: u16,
        user_pubkey: &str,
        signing_key: &SigningKey,
    ) -> Result<SwapOutcome> {
        let quote = match self
            .fetch_jupiter_quote(input_mint, output_mint, amount_raw, slippage_bps)
            .await
        {
            Ok(quote) => quote,
            Err(error) => return self.reject_intent(intent_id, format!("quote gagal: {error}")),
        };
        let Some(quote_out_amount) = quote["outAmount"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|v| *v > 0)
        else {
            return self.reject_intent(intent_id, "quote outAmount invalid".to_string());
        };
        self.db
            .update_execution_intent(intent_id, "QUOTED", None, None)?;

        let (unsigned_tx, last_valid_block_height) =
            match self.build_jupiter_unsigned_tx(quote, user_pubkey).await {
                Ok((tx, Some(height))) => (tx, height),
                Ok((_, None)) => {
                    return self.reject_intent(
                        intent_id,
                        "Jupiter tidak memberi lastValidBlockHeight".to_string(),
                    )
                }
                Err(error) => {
                    return self.reject_intent(intent_id, format!("build swap gagal: {error}"))
                }
            };
        let tx_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &unsigned_tx)?;
        let transaction_blockhash = Self::extract_recent_blockhash(&tx_bytes)?;
        self.db.update_execution_intent_metadata(
            intent_id,
            &ExecutionIntentMetadata {
                nonce: None,
                recent_blockhash: Some(transaction_blockhash.clone()),
                last_valid_block_height: Some(
                    i64::try_from(last_valid_block_height)
                        .map_err(|_| anyhow!("lastValidBlockHeight exceeds i64"))?,
                ),
                calldata_hash: None,
                quote_block: None,
                expiry: None,
            },
        )?;

        match self.simulate_unsigned(&unsigned_tx).await {
            Ok(units) => println!("[SOLANA SIMULATION ✅] intent={intent_id} units={units}"),
            Err(error) => return self.reject_intent(intent_id, format!("{error}")),
        }
        self.db
            .update_execution_intent(intent_id, "SIMULATED", None, None)?;

        let signed_tx = match Self::sign_transaction_base64(&unsigned_tx, signing_key) {
            Ok(tx) => tx,
            Err(error) => return self.reject_intent(intent_id, format!("signing gagal: {error}")),
        };
        let signed_blockhash = Self::extract_recent_blockhash(&base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &signed_tx,
        )?)?;
        if signed_blockhash != transaction_blockhash {
            return self.reject_intent(intent_id, "blockhash berubah setelah signing".to_string());
        }
        let signature = Self::signature_from_signed(&signed_tx)?;
        // Signature dipersist SEBELUM broadcast agar crash window tetap dapat direkonsiliasi.
        self.db
            .update_execution_intent(intent_id, "SIGNED", None, Some(&signature))?;

        let (status, broadcasted) = self
            .send_until_landed_or_expired(
                intent_id,
                &signed_tx,
                &signature,
                last_valid_block_height,
            )
            .await?;

        match status {
            LandingStatus::Landed => {
                if !broadcasted {
                    self.db.update_execution_intent(
                        intent_id,
                        "BROADCASTED",
                        None,
                        Some(&signature),
                    )?;
                }
                self.db
                    .update_execution_intent(intent_id, "CONFIRMED", None, Some(&signature))?;
                println!("[ONCHAIN CONFIRMED 🟢] intent={intent_id} sig={signature}");
                let fill = self
                    .fetch_swap_fill(&signature, user_pubkey, token_mint)
                    .await;
                if fill.is_some() {
                    self.db.update_execution_intent(
                        intent_id,
                        "RECONCILED",
                        None,
                        Some(&signature),
                    )?;
                }
                Ok(SwapOutcome::Landed {
                    signature,
                    fill,
                    quote_out_amount,
                })
            }
            LandingStatus::FailedOnChain(err) => {
                self.db
                    .update_execution_intent(intent_id, "FAILED", None, Some(&signature))?;
                Ok(SwapOutcome::Rejected(format!(
                    "transaksi {signature} gagal on-chain: {err}"
                )))
            }
            LandingStatus::Expired => {
                self.db
                    .update_execution_intent(intent_id, "FAILED", None, Some(&signature))?;
                Ok(SwapOutcome::Rejected(format!(
                    "transaksi {signature} tidak landing sampai blockhash expired"
                )))
            }
            LandingStatus::Unresolved => {
                self.db.update_execution_intent(
                    intent_id,
                    "PENDING_RECONCILIATION",
                    None,
                    Some(&signature),
                )?;
                Ok(SwapOutcome::Unresolved { signature })
            }
        }
    }

    /// Canary live memakai pipeline swap kanonik yang sama dengan engine
    /// (intent eksklusif → simulasi → sign → rebroadcast → fill dari meta tx).
    /// Sukses hanya bila transaksi landing DAN fill terbaca dari meta transaksi.
    pub async fn execute_canary_swap(
        &self,
        action: &str,
        input_mint: &str,
        output_mint: &str,
        token_mint: &str,
        amount_raw: u64,
        slippage_bps: u16,
    ) -> Result<(String, SolanaSwapFill)> {
        let (signing_key, user_pubkey) = self.get_keypair()?;
        let intent_id = format!("SOL-CANARY-{}-{}", action, Utc::now().timestamp_millis());
        if !self.db.create_execution_intent_exclusive(
            &intent_id,
            Chain::Solana,
            action,
            token_mint,
            &user_pubkey,
            amount_raw as f64,
        )? {
            return Err(anyhow!(
                "canary ditolak: intent aktif lain masih ada untuk mint {token_mint}"
            ));
        }
        println!("• Intent: {intent_id}");
        match self
            .execute_jupiter_swap(
                &intent_id,
                input_mint,
                output_mint,
                token_mint,
                amount_raw,
                slippage_bps,
                &user_pubkey,
                &signing_key,
            )
            .await?
        {
            SwapOutcome::Landed {
                signature,
                fill: Some(fill),
                ..
            } => Ok((signature, fill)),
            SwapOutcome::Landed {
                signature,
                fill: None,
                ..
            } => Err(anyhow!(
                "transaksi {signature} landing tetapi fill meta transaksi tidak terbaca"
            )),
            SwapOutcome::Unresolved { signature } => Err(anyhow!(
                "transaksi {signature} belum terbukti landing/expired (PENDING_RECONCILIATION)"
            )),
            SwapOutcome::Rejected(reason) => Err(anyhow!("canary ditolak: {reason}")),
        }
    }

    /// Resolver intent Solana berbasis bukti on-chain. `FAILED` hanya bila transaksi gagal
    /// on-chain atau blockhash terbukti expired; tanpa bukti intent tetap tertahan.
    pub async fn resolve_pending_intents(&self, older_than_secs: i64) -> Result<usize> {
        let intents = self
            .db
            .get_stale_pending_intents(Chain::Solana, older_than_secs)?;
        if intents.is_empty() {
            return Ok(0);
        }
        let current_height = self.get_block_height().await.ok();
        let mut resolved = 0usize;
        for intent in intents {
            let Some(signature) = intent.signature.clone() else {
                let next = match intent.state.as_str() {
                    "INTENT" | "QUOTED" | "SIMULATED" => "FAILED",
                    _ => "UNKNOWN",
                };
                self.db
                    .update_execution_intent(&intent.intent_id, next, None, None)?;
                println!(
                    "[INTENT RESOLVER] intent={} action={} {} -> {} reason=no_signature",
                    intent.intent_id, intent.action, intent.state, next
                );
                resolved += 1;
                continue;
            };
            let state = match self.fetch_signature_state(&signature).await {
                Ok(state) => state,
                Err(error) => {
                    eprintln!(
                        "[INTENT RESOLVER] intent={} status RPC gagal: {error}; ditahan",
                        intent.intent_id
                    );
                    continue;
                }
            };
            let next = match state {
                SignatureState::Confirmed => {
                    if intent.state == "SIGNED" {
                        self.db.update_execution_intent(
                            &intent.intent_id,
                            "BROADCASTED",
                            None,
                            Some(&signature),
                        )?;
                    }
                    eprintln!(
                        "[INTENT RESOLVER ⚠️ LATE LANDED] intent={} action={} mint={} sig={} landing tanpa posisi DB; butuh rekonsiliasi saldo (startup reconciliation).",
                        intent.intent_id, intent.action, intent.token_mint, signature
                    );
                    "CONFIRMED"
                }
                SignatureState::Failed(_) => "FAILED",
                SignatureState::NotFound
                    if blockhash_expired(
                        intent
                            .last_valid_block_height
                            .and_then(|h| u64::try_from(h).ok()),
                        current_height,
                    ) =>
                {
                    "FAILED"
                }
                _ if intent.state != "PENDING_RECONCILIATION" => "PENDING_RECONCILIATION",
                _ => continue,
            };
            self.db
                .update_execution_intent(&intent.intent_id, next, None, Some(&signature))?;
            println!(
                "[INTENT RESOLVER] intent={} action={} {} -> {}",
                intent.intent_id, intent.action, intent.state, next
            );
            resolved += 1;
        }
        Ok(resolved)
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

        let mut entry_price =
            if signal.current_pool_price.is_finite() && signal.current_pool_price > 0.0 {
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

        let sol_price_live = if self.config.dry_run {
            None
        } else {
            Some(self.get_sol_price().await?)
        };
        let amount_native_sol = sol_price_live.map_or(0.0, |price| amount_usd / price);
        let amount_lamports = (amount_native_sol * 1_000_000_000.0) as u64;

        let wsol_mint = "So11111111111111111111111111111111111111112";

        // REAL ON-CHAIN EXECUTION (when dry_run is false and keypair exists)
        let mut cost_usd = amount_usd;
        if !self.config.dry_run {
            let Ok((signing_key, user_pubkey)) = self.get_keypair() else {
                eprintln!(
                    "[LIVE EXECUTION ERROR ❌] No valid Solana private key found. Refusing trade."
                );
                return Ok(None);
            };
            let sol_price = sol_price_live.ok_or_else(|| anyhow!("SOL/USD price missing"))?;

            // Gas Buffer Guard (fail-closed): saldo wajib terbaca dan cukup.
            let balance_lamports = match self.get_native_balance_lamports(&user_pubkey).await {
                Ok(lamports) => lamports,
                Err(error) => {
                    eprintln!(
                        "[GAS BUFFER GUARD 🛡️] Saldo SOL tidak dapat diverifikasi ({error}). Live buy ditolak."
                    );
                    return Ok(None);
                }
            };
            let balance_sol = balance_lamports as f64 / 1_000_000_000.0;
            let required_sol = amount_native_sol + 0.04;
            if balance_sol < required_sol {
                eprintln!(
                    "[GAS BUFFER GUARD 🛡️] Insufficient SOL balance ({:.4} SOL < required {:.4} SOL = 0.04 buffer + {:.4} trade). Skipping live buy!",
                    balance_sol, required_sol, amount_native_sol
                );
                return Ok(None);
            }

            // Saldo token sebelum swap (fail-closed) untuk fallback rekonsiliasi delta.
            let pre_token_ui = match self
                .get_ui_token_balance(&user_pubkey, &signal.token_mint)
                .await
            {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("[LIVE BUY BLOCK 🛡️] Saldo token awal tidak terbaca ({error}).");
                    return Ok(None);
                }
            };

            let buy_intent_id = format!(
                "SOL-BUY-{}-{}",
                signal.token_mint,
                Utc::now().timestamp_millis()
            );
            if !self.db.create_execution_intent_exclusive(
                &buy_intent_id,
                Chain::Solana,
                "BUY",
                &signal.token_mint,
                &user_pubkey,
                amount_native_sol,
            )? {
                eprintln!(
                    "[LIVE BUY BLOCK 🛡️] Intent aktif lain masih ada untuk mint {}; BUY ditolak (anti double-buy).",
                    signal.token_mint
                );
                return Ok(None);
            }

            println!(
                "[JUPITER ROUTE 🪐] Executing swap for {} (${:.2}) with pubkey {}",
                signal.token_symbol, amount_usd, user_pubkey
            );
            let outcome = self
                .execute_jupiter_swap(
                    &buy_intent_id,
                    wsol_mint,
                    &signal.token_mint,
                    &signal.token_mint,
                    amount_lamports,
                    self.config.solana_buy_slippage_bps,
                    &user_pubkey,
                    &signing_key,
                )
                .await?;

            match outcome {
                SwapOutcome::Rejected(reason) => {
                    eprintln!(
                        "[ONCHAIN BUY REJECTED ❌] intent={} {}; no position will be persisted.",
                        buy_intent_id, reason
                    );
                    return Ok(None);
                }
                SwapOutcome::Unresolved { signature } => {
                    eprintln!(
                        "[ONCHAIN BUY UNRESOLVED ⚠️] intent={} sig={} belum terbukti landing/expired; intent ditahan PENDING_RECONCILIATION.",
                        buy_intent_id, signature
                    );
                    return Ok(None);
                }
                SwapOutcome::Landed {
                    signature,
                    fill,
                    quote_out_amount,
                } => {
                    let (tokens_received, cost_lamports, source) = match fill {
                        Some(fill) => {
                            let decimals = fill.token_decimals.unwrap_or(0);
                            if fill.token_raw_delta <= 0 || fill.token_decimals.is_none() {
                                eprintln!(
                                    "[ONCHAIN BUY ANOMALY ❌] sig={} token delta {} tidak positif; posisi tidak dibuat.",
                                    signature, fill.token_raw_delta
                                );
                                return Ok(None);
                            }
                            let spent = -fill.owner_lamports_delta;
                            let swap_cost =
                                i128::from(amount_lamports) + i128::from(fill.fee_lamports);
                            println!(
                                "[ONCHAIN FILL 📊] sig={} token_raw_delta={} (quote {}) fee={} lamports spent_total={} lamports (non-swap/rent={})",
                                signature,
                                fill.token_raw_delta,
                                quote_out_amount,
                                fill.fee_lamports,
                                spent,
                                spent - swap_cost
                            );
                            (
                                fill.token_raw_delta as f64 / 10f64.powi(decimals as i32),
                                swap_cost as f64,
                                "tx_meta",
                            )
                        }
                        None => {
                            let post_token_ui = self
                                .get_ui_token_balance(&user_pubkey, &signal.token_mint)
                                .await
                                .map_err(|error| {
                                    anyhow!("buy balance reconciliation failed: {error}")
                                })?;
                            (
                                post_token_ui - pre_token_ui,
                                amount_lamports as f64,
                                "balance_delta",
                            )
                        }
                    };
                    if !tokens_received.is_finite() || tokens_received <= 0.0 {
                        eprintln!(
                            "[ONCHAIN BUY REJECTED ❌] Confirmed transaction {} produced no positive token delta; no position will be persisted.",
                            signature
                        );
                        return Ok(None);
                    }
                    cost_usd = cost_lamports / 1_000_000_000.0 * sol_price;
                    new_tokens = tokens_received;
                    entry_price = cost_usd / tokens_received;
                    println!(
                        "[ONCHAIN RECONCILED 📊] source={} tokens={:.4} cost=${:.4} entry=${:.10}",
                        source, tokens_received, cost_usd, entry_price
                    );
                }
            }
        } else {
            // Realistic AMM Fill Simulation for Solana (Jupiter / Raydium Quote)
            if let Ok(sol_price) = self.get_sol_price().await {
                if sol_price > 0.0 {
                    let sim_sol = amount_usd / sol_price;
                    let sim_lamports = (sim_sol * 1_000_000_000.0) as u64;
                    if let Ok(quote) = self
                        .fetch_jupiter_quote(wsol_mint, &signal.token_mint, sim_lamports, 150)
                        .await
                    {
                        let impact_pct = quote
                            .get("priceImpactPct")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse::<f64>().ok())
                            .unwrap_or(0.0);
                        if impact_pct > 15.0 {
                            eprintln!(
                                "[SIMULATION REJECT ❌] Excessive price impact on AMM ({:.2}% > 15.0%) for {}. Simulation trade canceled.",
                                impact_pct, signal.token_symbol
                            );
                            return Ok(None);
                        }
                        println!(
                            "[SIMULATION AMM QUOTE 🪐] Jupiter quote confirmed for {}: Price Impact {:.2}%",
                            signal.token_symbol, impact_pct
                        );
                    }
                }
            }
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
                // MULTI-LOT AKUMULASI: whale yang sama BOLEH menambah lot (mengikuti
                // pembelian berkala whale). Tiap lot = POSITION_SIZE_USD; avg gabungan
                // dihitung ulang di bawah (weighted_avg_entry). Cap per-mint tetap
                // dijaga gating MAX_TOKEN_EXPOSURE ($50).
                let whale_addr_lower = signal.whale_address.trim().to_lowercase();
                let whale_already_in = existing
                    .target_whale_address
                    .split(',')
                    .map(|a| a.trim().to_lowercase())
                    .any(|a| a == whale_addr_lower);
                if whale_already_in {
                    eprintln!(
                        "[WHALE ACCUMULATION ➕] @{} menambah lot ke posisi {} ({}). Avg gabungan akan dihitung ulang.",
                        signal.whale_handle, existing.id, existing.token_symbol
                    );
                }
                // Basis yang benar-benar masih dipegang: partial exit mengurangi
                // remaining_portion_pct (bukan amount_usd), jadi skala dulu
                // sebelum digabung dengan lot baru — kalau tidak, avg & total
                // posisi overstate setelah partial-exit-then-add.
                let rem_frac = (existing.remaining_portion_pct / 100.0).clamp(0.0, 1.0);
                let prev_usd = existing.amount_usd * rem_frac;
                let prev_tokens = existing.amount_native * rem_frac;
                let total_usd = prev_usd + cost_usd;
                // Lot baru memakai token delta aktual (live: meta transaksi), bukan saldo
                // total wallet, sehingga token di luar posisi tidak ikut terhitung.
                let total_tokens = prev_tokens + new_tokens;
                let weighted_avg_entry = if total_tokens > 0.0 {
                    total_usd / total_tokens
                } else {
                    entry_price
                };

                println!(
                    "[POSITION AVERAGING ➕] Adding ${:.2} to existing {} position (ID: {}). Prev Entry: ${:.6} -> New Avg Entry: ${:.6} (Total: ${:.2})",
                    cost_usd, existing.token_symbol, existing.id, existing.entry_price, weighted_avg_entry, total_usd
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
            amount_usd: cost_usd,
            remaining_portion_pct: 100.0,
            unrealized_pnl_pct: 0.0,
            realized_pnl_usd: 0.0,
            entry_time: now,
            last_update: now,
            status: "OPEN".to_string(),
            is_dry_run: self.config.dry_run,
            owner_pubkey: self.get_wallet_pubkey(),
            price_source: "DEXSCREENER".to_string(),
            price_updated_at: now,
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
        if self.config.dry_run {
            self.db.close_position_portion(
                position_id,
                sell_ratio_pct,
                exit_reason,
                current_price,
            )?;
            return Ok(());
        }

        let wsol_mint = "So11111111111111111111111111111111111111112";
        let (signing_key, user_pubkey) = self
            .get_keypair()
            .map_err(|error| anyhow!("live SELL ditolak: keypair tidak valid ({error})"))?;
        let pos = self
            .db
            .get_active_positions()?
            .into_iter()
            .find(|p| p.id == position_id)
            .ok_or_else(|| {
                anyhow!("live SELL ditolak: posisi aktif {position_id} tidak ditemukan")
            })?;
        println!(
            "[ONCHAIN SOLANA SELL 🚀] Executing {:.1}% partial swap for {} ({}) to SOL (Reason: {})",
            sell_ratio_pct, pos.token_symbol, pos.token_mint, exit_reason
        );

        // Saldo token fisik wajib terbaca; error RPC tidak pernah dianggap nol.
        let (token_amount_raw, token_amount_ui) = self
            .get_token_balance(&user_pubkey, &pos.token_mint)
            .await
            .map_err(|error| anyhow!("live SELL ditolak: saldo token tidak terbaca ({error})"))?;
        if token_amount_raw == 0 {
            println!(
                "[ONCHAIN BALANCE ZERO ⚪] Wallet terbukti memegang 0 token {}. Posisi ditutup tanpa PnL (fill tidak dapat dibuktikan).",
                pos.token_symbol
            );
            self.db
                .close_position_without_fill(position_id, "ONCHAIN_ZERO_BALANCE")?;
            return Ok(());
        }

        let effective_wallet_fraction = if pos.remaining_portion_pct > 0.0 {
            (sell_ratio_pct / pos.remaining_portion_pct).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let sell_units = if effective_wallet_fraction >= 1.0 {
            token_amount_raw
        } else {
            ((token_amount_raw as f64) * effective_wallet_fraction) as u64
        };
        if sell_units == 0 {
            return Err(anyhow!("live SELL ditolak: sell units = 0"));
        }

        let slippage_bps: u16 =
            if exit_reason.contains("EMERGENCY") || exit_reason.contains("HARD_SL") {
                self.config.solana_sl_slippage_bps
            } else if exit_reason.contains("PEAK") || exit_reason.contains("DUMP") {
                self.config.solana_trailing_slippage_bps
            } else {
                self.config.solana_tp_slippage_bps
            };

        let sol_price = self.get_sol_price().await?;
        let pre_lamports = self
            .get_native_balance_lamports(&user_pubkey)
            .await
            .map_err(|error| anyhow!("live SELL ditolak: saldo SOL tidak terbaca ({error})"))?;

        let intent_id = format!("SOL-SELL-{}-{}", position_id, Utc::now().timestamp_millis());
        if !self.db.create_execution_intent_exclusive(
            &intent_id,
            Chain::Solana,
            "SELL",
            &pos.token_mint,
            &user_pubkey,
            sell_units as f64,
        )? {
            return Err(anyhow!(
                "live SELL ditolak: intent aktif lain masih ada untuk mint {} (anti double-sell)",
                pos.token_mint
            ));
        }

        let outcome = self
            .execute_jupiter_swap(
                &intent_id,
                &pos.token_mint,
                wsol_mint,
                &pos.token_mint,
                sell_units,
                slippage_bps,
                &user_pubkey,
                &signing_key,
            )
            .await?;

        let (signature, fill) = match outcome {
            SwapOutcome::Rejected(reason) => {
                eprintln!(
                    "[SELL REJECTED ❌] intent={} {}. Position {} remains OPEN.",
                    intent_id, reason, position_id
                );
                return Err(anyhow!("SELL ditolak: {reason}"));
            }
            SwapOutcome::Unresolved { signature } => {
                eprintln!(
                    "[SELL UNRESOLVED ⚠️] intent={} sig={} belum terbukti landing/expired. Position {} remains OPEN; SELL berikutnya diblokir sampai resolver memutuskan.",
                    intent_id, signature, position_id
                );
                return Err(anyhow!("SELL unresolved: {signature}"));
            }
            SwapOutcome::Landed {
                signature, fill, ..
            } => (signature, fill),
        };

        // Harga exit aktual: SOL bersih diterima (setelah fee) ÷ token terjual.
        let (tokens_sold, proceeds_lamports, source) = match fill {
            Some(fill) if fill.token_raw_delta < 0 && fill.token_decimals.is_some() => {
                let decimals = fill.token_decimals.unwrap_or(0);
                (
                    (-fill.token_raw_delta) as f64 / 10f64.powi(decimals as i32),
                    fill.owner_lamports_delta,
                    "tx_meta",
                )
            }
            _ => {
                let post_lamports = self.get_native_balance_lamports(&user_pubkey).await?;
                let post_raw = self
                    .get_raw_token_balance(&user_pubkey, &pos.token_mint)
                    .await?;
                let sold_raw = token_amount_raw.saturating_sub(post_raw);
                // Rasio raw→UI eksak dari saldo pra-swap (= 10^-decimals).
                let ui_per_raw = token_amount_ui / token_amount_raw as f64;
                (
                    sold_raw as f64 * ui_per_raw,
                    i128::from(post_lamports) - i128::from(pre_lamports),
                    "balance_delta",
                )
            }
        };
        let proceeds_usd = proceeds_lamports as f64 / 1_000_000_000.0 * sol_price;
        let exit_price = if tokens_sold > 0.0 {
            proceeds_usd / tokens_sold
        } else {
            0.0
        };
        println!(
            "[ONCHAIN SELL FILL 📊] sig={} source={} tokens_sold={:.4} proceeds={} lamports (${:.4}) exit=${:.10}",
            signature, source, tokens_sold, proceeds_lamports, proceeds_usd, exit_price
        );

        if exit_price.is_finite() && exit_price > 0.0 {
            self.db
                .close_position_portion(position_id, sell_ratio_pct, exit_reason, exit_price)?;
        } else {
            eprintln!(
                "[SELL FILL ANOMALY ⚠️] sig={} proceeds/token tidak valid; porsi ditutup tanpa PnL.",
                signature
            );
            if sell_ratio_pct >= pos.remaining_portion_pct - 1.0 {
                self.db
                    .close_position_without_fill(position_id, "SELL_FILL_UNVERIFIED")?;
            } else {
                return Err(anyhow!(
                    "SELL landed ({signature}) tetapi fill tidak terverifikasi; posisi tidak diubah"
                ));
            }
        }

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

/// Harga exit per token (USD) dari hasil jual on-chain. `None` bila hasil/jumlah tidak positif.
pub fn external_exit_price_usd(
    tokens_sold_raw: i128,
    decimals: u32,
    proceeds_lamports: i128,
    sol_usd: f64,
) -> Option<f64> {
    let tokens_sold = tokens_sold_raw as f64 / 10f64.powi(decimals as i32);
    let proceeds_usd = proceeds_lamports as f64 / 1_000_000_000.0 * sol_usd;
    let price = proceeds_usd / tokens_sold;
    (tokens_sold > 0.0 && proceeds_usd > 0.0 && price.is_finite()).then_some(price)
}

#[cfg(test)]
mod transaction_tests {
    use super::{external_exit_price_usd, SolanaExecutor};

    #[test]
    fn external_exit_price_uses_onchain_proceeds_not_model_price() {
        // Data nyata SLOPCORE: jual 273390.000115 token (6 decimals), wallet terima
        // 8_462_311 lamports bersih (tx 4RAMGN...). Entry $0.0000366; harga model DB $0.00004895.
        let sol_usd = 200.0;
        let exit = external_exit_price_usd(273_390_000_115, 6, 8_462_311, sol_usd).unwrap();
        let expected = 8_462_311.0 / 1e9 * sol_usd / 273_390.000_115;
        assert!((exit - expected).abs() < 1e-15);
        assert!(exit < 0.0000366 * 0.5, "rugi nyata wajib tercatat, bukan profit model");
        assert_eq!(external_exit_price_usd(273_390_000_115, 6, 0, sol_usd), None);
        assert_eq!(external_exit_price_usd(273_390_000_115, 6, -5_000, sol_usd), None);
        assert_eq!(external_exit_price_usd(0, 6, 8_462_311, sol_usd), None);
    }

    fn fixture(versioned: bool, blockhash: [u8; 32]) -> Vec<u8> {
        let mut tx = vec![1u8];
        tx.extend([0u8; 64]);
        if versioned {
            tx.push(0x80);
        }
        tx.extend([0u8, 0u8, 0u8]);
        tx.push(1u8);
        tx.extend([7u8; 32]);
        tx.extend(blockhash);
        tx
    }

    #[test]
    fn extracts_blockhash_from_versioned_and_legacy_messages() {
        let expected = [9u8; 32];
        let encoded = bs58::encode(expected).into_string();
        assert_eq!(
            SolanaExecutor::extract_recent_blockhash(&fixture(true, expected)).unwrap(),
            encoded
        );
        assert_eq!(
            SolanaExecutor::extract_recent_blockhash(&fixture(false, expected)).unwrap(),
            encoded
        );
    }

    #[test]
    fn blockhash_expiry_requires_both_heights() {
        use super::blockhash_expired;
        assert!(!blockhash_expired(None, Some(10)));
        assert!(!blockhash_expired(Some(10), None));
        assert!(!blockhash_expired(Some(10), Some(10)));
        assert!(blockhash_expired(Some(10), Some(11)));
    }

    #[test]
    fn signature_status_parser_is_fail_closed() {
        use super::SignatureState;
        use serde_json::json;
        let parse = |v| SolanaExecutor::parse_signature_status(&v);
        assert_eq!(
            parse(json!({"result": {"value": [null]}})).unwrap(),
            SignatureState::NotFound
        );
        assert_eq!(
            parse(json!({"result": {"value": [{"err": null, "confirmationStatus": "processed"}]}}))
                .unwrap(),
            SignatureState::Processing
        );
        assert_eq!(
            parse(json!({"result": {"value": [{"err": null, "confirmationStatus": "finalized"}]}}))
                .unwrap(),
            SignatureState::Confirmed
        );
        assert!(matches!(
            parse(json!({"result": {"value": [{"err": {"InstructionError": [2, "Custom"]}, "confirmationStatus": "confirmed"}]}}))
                .unwrap(),
            SignatureState::Failed(_)
        ));
        assert!(parse(json!({"error": {"code": 429}})).is_err());
        assert!(parse(json!({"result": {"value": []}})).is_err());
    }

    #[test]
    fn simulation_parser_rejects_program_error_and_malformed() {
        use serde_json::json;
        assert_eq!(
            SolanaExecutor::parse_simulation(
                &json!({"result": {"value": {"err": null, "unitsConsumed": 67099, "logs": []}}})
            )
            .unwrap(),
            67099
        );
        assert!(
            SolanaExecutor::parse_simulation(&json!({"result": {"value": {
                "err": {"InstructionError": [3, {"Custom": 6001}]},
                "logs": ["Program log: slippage exceeded"]
            }}}))
            .is_err()
        );
        assert!(SolanaExecutor::parse_simulation(&json!({"error": {"code": -32002}})).is_err());
    }

    #[test]
    fn token_balance_parser_sums_accounts_and_rejects_malformed() {
        use serde_json::json;
        let account = |amount: &str| {
            json!({"account": {"data": {"parsed": {"info": {"tokenAmount": {
                "amount": amount, "decimals": 6
            }}}}}})
        };
        let (raw, ui) = SolanaExecutor::parse_token_accounts_balance(&json!({
            "result": {"value": [account("1500000"), account("500000")]}
        }))
        .unwrap();
        assert_eq!(raw, 2_000_000);
        assert!((ui - 2.0).abs() < 1e-9);
        assert_eq!(
            SolanaExecutor::parse_token_accounts_balance(&json!({"result": {"value": []}}))
                .unwrap(),
            (0, 0.0)
        );
        assert!(
            SolanaExecutor::parse_token_accounts_balance(&json!({"error": {"code": 429}})).is_err()
        );
        assert!(SolanaExecutor::parse_token_accounts_balance(&json!({
            "result": {"value": [{"account": {"data": {"parsed": {"info": {}}}}}]}
        }))
        .is_err());
    }

    #[test]
    fn swap_fill_parser_uses_owner_deltas_only() {
        use serde_json::json;
        let owner = "Owner1111111111111111111111111111111111111";
        let mint = "Mint11111111111111111111111111111111111111";
        let tx = json!({
            "transaction": {"message": {"accountKeys": [owner, "Other"]}},
            "meta": {
                "err": null,
                "fee": 15000,
                "preBalances": [1_000_000_000u64, 5],
                "postBalances": [900_000_000u64, 5],
                "preTokenBalances": [
                    {"mint": mint, "owner": owner, "uiTokenAmount": {"amount": "100", "decimals": 6}},
                    {"mint": mint, "owner": "Pool", "uiTokenAmount": {"amount": "999999", "decimals": 6}}
                ],
                "postTokenBalances": [
                    {"mint": mint, "owner": owner, "uiTokenAmount": {"amount": "5000100", "decimals": 6}},
                    {"mint": mint, "owner": "Pool", "uiTokenAmount": {"amount": "1", "decimals": 6}}
                ]
            }
        });
        let fill = SolanaExecutor::parse_swap_fill(&tx, owner, mint).unwrap();
        assert_eq!(fill.token_raw_delta, 5_000_000);
        assert_eq!(fill.owner_lamports_delta, -100_000_000);
        assert_eq!(fill.fee_lamports, 15000);
        assert_eq!(fill.token_decimals, Some(6));

        let mut failed = tx.clone();
        failed["meta"]["err"] = json!({"InstructionError": [1, "Custom"]});
        assert!(SolanaExecutor::parse_swap_fill(&failed, owner, mint).is_err());
        assert!(SolanaExecutor::parse_swap_fill(&tx, "SomeoneElse", mint).is_err());
    }

    #[test]
    fn signature_is_extracted_from_signed_transaction() {
        let mut tx = vec![1u8];
        tx.extend([7u8; 64]);
        tx.extend([0u8; 40]);
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &tx);
        assert_eq!(
            SolanaExecutor::signature_from_signed(&encoded).unwrap(),
            bs58::encode([7u8; 64]).into_string()
        );
        let mut unsigned = vec![1u8];
        unsigned.extend([0u8; 104]);
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &unsigned);
        assert!(SolanaExecutor::signature_from_signed(&encoded).is_err());
    }

    #[test]
    fn rejects_truncated_and_overflowed_shortvec() {
        assert!(SolanaExecutor::extract_recent_blockhash(&[]).is_err());
        assert!(SolanaExecutor::extract_recent_blockhash(&[0x80; 10]).is_err());
    }

    #[test]
    fn startup_reconciliation_logic_filters_blacklisted_and_zero_balances() {
        use crate::models::is_blacklisted_asset;
        let mut onchain_tokens = vec![
            (
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
                100.0,
            ), // USDC -> Blacklisted
            (
                "So11111111111111111111111111111111111111112".to_string(),
                2.5,
            ), // WSOL -> Blacklisted
            (
                "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string(),
                1500.0,
            ), // BONK -> Valid
        ];

        onchain_tokens.retain(|(mint, _)| !is_blacklisted_asset(mint, ""));
        assert_eq!(onchain_tokens.len(), 1);
        assert_eq!(
            onchain_tokens[0].0,
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263"
        );
        assert_eq!(onchain_tokens[0].1, 1500.0);
    }
}
