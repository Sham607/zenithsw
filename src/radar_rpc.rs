use crate::db::{SmartWalletDb, WalletScanAddress};
use crate::dex_adapters::{
    materialize_and_persist_normalized_batch, plan_solana_poll_batch, DexVenue,
    NormalizedActivityMaterializationContext, ScanCursor,
};
use crate::execution::solana_executor::SolanaExecutor;
use crate::ipc::IpcServer;
use crate::models::Chain;
use crate::radar::{OnChainTokenEvidence, RadarEvidenceKind};
use crate::radar_activity::NormalizedActivity;
use crate::radar_activity::VerifiedVenue;
use crate::radar_solana_activity::{
    build_signatures_for_address_request, build_transaction_request, parse_signature_references,
    parse_transaction_token_deltas, parse_transaction_venue_instructions, reconcile_verified_swap,
    SolanaTransactionReference,
};
use anyhow::{bail, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

/// Transport RPC khusus observasi. Tidak memiliki fallback endpoint dan
/// menolak method yang dapat mengubah state chain.
#[derive(Clone)]
pub struct ReadOnlyRpcClient {
    client: Client,
    endpoints: Vec<String>,
}

impl ReadOnlyRpcClient {
    pub fn new(endpoints: &str) -> Result<Self> {
        let endpoints: Vec<String> = endpoints
            .split(',')
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if endpoints.is_empty() {
            bail!("read-only RPC membutuhkan minimal satu endpoint");
        }
        if endpoints
            .iter()
            .any(|endpoint| !(endpoint.starts_with("http://") || endpoint.starts_with("https://")))
        {
            bail!("read-only RPC endpoint memakai skema yang tidak didukung");
        }
        Ok(Self {
            client: Client::builder().timeout(Duration::from_secs(10)).build()?,
            endpoints,
        })
    }

    pub async fn call(&self, request: &Value) -> Result<Value> {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("RPC method missing"))?;
        if matches!(method, "sendTransaction" | "sendRawTransaction") {
            bail!("read-only RPC menolak method mutating: {method}");
        }
        let mut last_error = anyhow::anyhow!("read-only RPC endpoint missing");
        for endpoint in &self.endpoints {
            let response = match self.client.post(endpoint).json(request).send().await {
                Ok(response) => response,
                Err(error) => {
                    last_error = error.into();
                    continue;
                }
            };
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            let body = match response.text().await {
                Ok(body) => body,
                Err(error) => {
                    last_error = error.into();
                    continue;
                }
            };
            if !status.is_success() {
                last_error = anyhow::anyhow!(
                    "RPC HTTP status {status} content_type={content_type} detail={}",
                    serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("error")
                                .and_then(|error| error.get("message"))
                                .and_then(Value::as_str)
                                .map(ToOwned::to_owned)
                        })
                        .unwrap_or_else(|| "provider_http_error".to_string())
                );
                continue;
            }
            let value: Value = match serde_json::from_str(&body) {
                Ok(value) => value,
                Err(_) => {
                    last_error = anyhow::anyhow!("RPC body bukan JSON content_type={content_type}");
                    continue;
                }
            };
            if let Some(error) = value.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                last_error = anyhow::anyhow!("RPC JSON-RPC error code={code} message={message}");
                continue;
            }
            if value.get("result").is_none() || value.get("result") == Some(&Value::Null) {
                last_error = anyhow::anyhow!("RPC JSON-RPC result missing_or_null");
                continue;
            }
            return Ok(value);
        }
        Err(last_error)
    }
}

/// Membuat client observasi tanpa pernah menambahkan fallback endpoint.
pub fn build_read_only_rpc_client(endpoints: &str) -> Result<ReadOnlyRpcClient> {
    ReadOnlyRpcClient::new(endpoints)
}

const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

pub fn build_solana_token_accounts_request(owner: &str, request_id: u64) -> Result<Value> {
    build_token_accounts_request(owner, request_id, SPL_TOKEN_PROGRAM)
}

pub fn build_solana_token_2022_accounts_request(owner: &str, request_id: u64) -> Result<Value> {
    build_token_accounts_request(owner, request_id, TOKEN_2022_PROGRAM)
}

fn build_token_accounts_request(owner: &str, request_id: u64, program_id: &str) -> Result<Value> {
    validate_observation_identity(owner, "owner")?;
    Ok(json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "getTokenAccountsByOwner",
        "params": [owner, {"programId": program_id}, {"encoding": "jsonParsed", "commitment": "confirmed"}]
    }))
}

pub async fn collect_solana_holdings(
    executor: &SolanaExecutor,
    addresses: &[WalletScanAddress],
) -> Result<Vec<OnChainTokenEvidence>> {
    let mut evidence = Vec::new();
    for (request_id, address) in addresses.iter().enumerate() {
        if address.chain != Chain::Solana {
            continue;
        }
        let request = build_solana_token_accounts_request(&address.address, request_id as u64 + 1)?;
        let response = executor.post_rpc(&request).await?;
        evidence.extend(parse_solana_token_accounts(
            &response,
            &address.address,
            !address.is_candidate,
        )?);
    }
    Ok(evidence)
}

/// Mengambil holdings menggunakan transport observasi tanpa fallback endpoint.
pub async fn collect_solana_holdings_read_only(
    client: &ReadOnlyRpcClient,
    addresses: &[WalletScanAddress],
) -> Result<Vec<OnChainTokenEvidence>> {
    let mut evidence = Vec::new();
    for (request_id, address) in addresses.iter().enumerate() {
        if address.chain != Chain::Solana {
            continue;
        }
        for request in [
            build_solana_token_accounts_request(&address.address, request_id as u64 * 2 + 1)?,
            build_solana_token_2022_accounts_request(&address.address, request_id as u64 * 2 + 2)?,
        ] {
            let response = match client.call(&request).await {
                Ok(resp) => resp,
                Err(err) => {
                    eprintln!(
                        "[RADAR RPC SKIP] wallet={} query gagal: {err}",
                        address.address
                    );
                    continue;
                }
            };
            match parse_solana_token_accounts(&response, &address.address, !address.is_candidate) {
                Ok(items) => evidence.extend(items),
                Err(err) => {
                    eprintln!(
                        "[RADAR RPC SKIP] wallet={} parse gagal: {err}",
                        address.address
                    );
                }
            }
        }
        // Jeda 2000ms antar wallet agar RPC publik resmi Solana tetap lancar tanpa rate-limit
        tokio::time::sleep(Duration::from_millis(2000)).await;
    }
    Ok(evidence)
}

/// Mereduksi response transaction yang sudah diambil oleh transport read-only.
///
/// Hanya response untuk signature yang diminta yang diproses. Transaction yang
/// gagal, malformed, atau tidak memiliki pool authoritative dilewati; hasil
/// tersebut bukan bukti bahwa chain tidak memiliki activity.
pub fn collect_solana_activity_from_responses(
    address: &WalletScanAddress,
    token_address: &str,
    quote_asset: &str,
    references: &[SolanaTransactionReference],
    transaction_responses: &[(String, Value)],
) -> Result<Vec<NormalizedActivity>> {
    if address.chain != Chain::Solana {
        bail!("activity reducer hanya menerima address Solana");
    }
    validate_observation_identity(&address.address, token_address)?;
    validate_observation_identity(&address.address, quote_asset)?;
    let mut activities = Vec::new();
    for reference in references {
        if reference.chain != Chain::Solana
            || reference.wallet_address != address.address
            || reference.signature.trim().is_empty()
            || reference.slot == 0
        {
            continue;
        }
        let Some((_, response)) = transaction_responses
            .iter()
            .find(|(signature, _)| signature == &reference.signature)
        else {
            continue;
        };
        let venues = match parse_transaction_venue_instructions(response, &reference.signature) {
            Ok(venues) => venues,
            Err(_) => continue,
        };
        let deltas = match parse_transaction_token_deltas(
            response,
            &address.address,
            &reference.signature,
        ) {
            Ok(deltas) => deltas,
            Err(_) => continue,
        };
        if let Ok(activity) = reconcile_verified_swap(&deltas, &venues, token_address, quote_asset)
        {
            if activity.pool_address.is_some() {
                activities.push(activity);
            }
        }
    }
    Ok(activities)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaResponsePollResult {
    pub activities: Vec<NormalizedActivity>,
    pub next_cursor: Option<ScanCursor>,
    pub selected_references: usize,
    pub missing_responses: usize,
}

/// Menggabungkan planner bounded dan reducer response-only tanpa side effect.
///
/// Cursor hanya tersedia bila setiap reference terpilih memiliki response.
/// Response yang tersedia tetapi failed/malformed/missing-pool ditolak sebagai
/// activity, namun tidak dianggap sebagai provider failure.
pub fn plan_and_reduce_solana_responses(
    address: &WalletScanAddress,
    token_address: &str,
    quote_asset: &str,
    references: &[SolanaTransactionReference],
    cursor: Option<&ScanCursor>,
    max_batch: usize,
    transaction_responses: &[(String, Value)],
) -> Result<SolanaResponsePollResult> {
    let batch = plan_solana_poll_batch(references, cursor, max_batch)?;
    let missing_responses = batch
        .references
        .iter()
        .filter(|reference| {
            !transaction_responses
                .iter()
                .any(|(signature, _)| signature == &reference.signature)
        })
        .count();
    let activities = collect_solana_activity_from_responses(
        address,
        token_address,
        quote_asset,
        &batch.references,
        transaction_responses,
    )?;
    Ok(SolanaResponsePollResult {
        activities,
        next_cursor: (missing_responses == 0)
            .then_some(batch.next_cursor)
            .flatten(),
        selected_references: batch.references.len(),
        missing_responses,
    })
}

/// Mengumpulkan activity Solana read-only dari signature history dan transaction
/// detail. Activity yang tidak dapat direkonsiliasi sebagai swap terverifikasi
/// ditolak dan tidak menjadi evidence trading.
pub async fn poll_solana_activity_read_only(
    client: &ReadOnlyRpcClient,
    address: &WalletScanAddress,
    token_address: &str,
    quote_asset: &str,
    cursor: Option<&ScanCursor>,
    history_limit: u64,
    max_batch: usize,
) -> Result<SolanaResponsePollResult> {
    if address.chain != Chain::Solana {
        bail!("activity collector hanya menerima address Solana");
    }
    let signatures_response = client
        .call(&build_signatures_for_address_request(
            &address.address,
            history_limit,
            1,
        )?)
        .await?;
    let references = parse_signature_references(&signatures_response, &address.address)?;
    let batch = plan_solana_poll_batch(&references, cursor, max_batch)?;
    let mut transaction_responses = Vec::with_capacity(batch.references.len());
    for (offset, reference) in batch.references.iter().enumerate() {
        let response = client
            .call(&build_transaction_request(
                &reference.signature,
                offset as u64 + 2,
            )?)
            .await?;
        transaction_responses.push((reference.signature.clone(), response));
    }
    plan_and_reduce_solana_responses(
        address,
        token_address,
        quote_asset,
        &batch.references,
        None,
        max_batch,
        &transaction_responses,
    )
}

pub struct SolanaActivityPersistenceContext<'a> {
    pub client: &'a ReadOnlyRpcClient,
    pub db: &'a SmartWalletDb,
    pub ipc: &'a IpcServer,
    pub address: &'a WalletScanAddress,
    pub token_address: &'a str,
    pub quote_asset: &'a str,
    pub cursor: Option<&'a ScanCursor>,
    pub history_limit: u64,
    pub max_batch: usize,
    pub materialization: NormalizedActivityMaterializationContext<'a>,
    pub scope_key: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaActivityPersistenceReport {
    pub selected_references: usize,
    pub missing_responses: usize,
    pub verified_activities: usize,
    pub persisted_activities: usize,
    pub cursor_advanced: bool,
}

/// Menjalankan satu siklus DEX read-only sampai boundary DB/IPC pada state terisolasi.
/// Metadata wallet/token harus authoritative; tidak ada fallback sintetis.
pub async fn poll_and_persist_solana_activity_read_only_for_venue(
    context: SolanaActivityPersistenceContext<'_>,
    expected_venue: DexVenue,
) -> Result<SolanaActivityPersistenceReport> {
    if expected_venue.chain() != Chain::Solana {
        bail!("venue worker bukan venue Solana");
    }
    let loaded_cursor = if context.cursor.is_none() {
        context
            .db
            .load_dex_cursor(Chain::Solana, expected_venue, context.scope_key)?
    } else {
        None
    };
    let cursor = context.cursor.or(loaded_cursor.as_ref());
    let result = poll_solana_activity_read_only(
        context.client,
        context.address,
        context.token_address,
        context.quote_asset,
        cursor,
        context.history_limit,
        context.max_batch,
    )
    .await?;
    for activity in &result.activities {
        let venue = match activity.venue {
            Some(VerifiedVenue::SolanaRaydiumAmm) => DexVenue::SolanaRaydiumAmm,
            Some(VerifiedVenue::SolanaRaydiumCpmm) => DexVenue::SolanaRaydiumCpmm,
            Some(VerifiedVenue::SolanaRaydiumClmm) => DexVenue::SolanaRaydiumClmm,
            Some(VerifiedVenue::SolanaPumpFun) => DexVenue::SolanaPumpFun,
            Some(VerifiedVenue::SolanaPumpSwap) => DexVenue::SolanaPumpSwap,
            Some(VerifiedVenue::SolanaJupiter) => DexVenue::SolanaJupiterRouter,
            _ => bail!("activity memiliki venue Solana yang tidak terpetakan"),
        };
        if venue != expected_venue {
            bail!("activity venue berbeda dari worker scope");
        }
    }
    let context = SolanaActivityPersistenceContext { cursor, ..context };
    persist_solana_activity_result(context, result).await
}

async fn persist_solana_activity_result(
    context: SolanaActivityPersistenceContext<'_>,
    result: SolanaResponsePollResult,
) -> Result<SolanaActivityPersistenceReport> {
    if result.missing_responses > 0 || result.activities.is_empty() {
        return Ok(SolanaActivityPersistenceReport {
            selected_references: result.selected_references,
            missing_responses: result.missing_responses,
            verified_activities: result.activities.len(),
            persisted_activities: 0,
            cursor_advanced: false,
        });
    }
    let next_cursor = result
        .next_cursor
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("activity verified tanpa cursor berikutnya"))?;
    let persisted = materialize_and_persist_normalized_batch(
        context.db,
        context.ipc,
        &result.activities,
        context.materialization,
        context.scope_key,
        next_cursor,
    )?;
    Ok(SolanaActivityPersistenceReport {
        selected_references: result.selected_references,
        missing_responses: result.missing_responses,
        verified_activities: result.activities.len(),
        persisted_activities: persisted,
        cursor_advanced: true,
    })
}

pub async fn poll_and_persist_solana_activity_read_only(
    context: SolanaActivityPersistenceContext<'_>,
) -> Result<SolanaActivityPersistenceReport> {
    let result = poll_solana_activity_read_only(
        context.client,
        context.address,
        context.token_address,
        context.quote_asset,
        context.cursor,
        context.history_limit,
        context.max_batch,
    )
    .await?;
    persist_solana_activity_result(context, result).await
}

pub async fn collect_solana_activity_read_only(
    client: &ReadOnlyRpcClient,
    address: &WalletScanAddress,
    token_address: &str,
    quote_asset: &str,
    limit: u64,
) -> Result<Vec<NormalizedActivity>> {
    if address.chain != Chain::Solana {
        bail!("activity collector hanya menerima address Solana");
    }
    let signatures_response = client
        .call(&build_signatures_for_address_request(
            &address.address,
            limit,
            1,
        )?)
        .await?;
    let references = parse_signature_references(&signatures_response, &address.address)?;
    let mut activities = Vec::new();
    for (offset, reference) in references.iter().enumerate() {
        let transaction_response = client
            .call(&build_transaction_request(
                &reference.signature,
                offset as u64 + 2,
            )?)
            .await?;
        let venues =
            match parse_transaction_venue_instructions(&transaction_response, &reference.signature)
            {
                Ok(venues) => venues,
                Err(_) => continue,
            };
        let deltas = match parse_transaction_token_deltas(
            &transaction_response,
            &address.address,
            &reference.signature,
        ) {
            Ok(deltas) => deltas,
            Err(_) => continue,
        };
        if let Ok(activity) = reconcile_verified_swap(&deltas, &venues, token_address, quote_asset)
        {
            activities.push(activity);
        }
    }
    Ok(activities)
}

/// Menyusun evidence dari response RPC yang sudah diterima oleh transport
/// read-only. Fungsi ini tidak melakukan network, fallback endpoint, atau write.
pub fn collect_solana_holdings_from_responses(
    addresses: &[WalletScanAddress],
    responses: &[(String, Value)],
) -> Result<Vec<OnChainTokenEvidence>> {
    let mut evidence = Vec::new();
    for address in addresses.iter().filter(|item| item.chain == Chain::Solana) {
        let response = responses
            .iter()
            .find(|(owner, _)| owner == &address.address)
            .ok_or_else(|| anyhow::anyhow!("missing RPC response for scan address"))?;
        evidence.extend(parse_solana_token_accounts(
            &response.1,
            &address.address,
            !address.is_candidate,
        )?);
    }
    Ok(evidence)
}

/// Mengubah response read-only `getTokenAccountsByOwner` menjadi evidence holding.
/// Hanya saldo positif dengan mint yang eksplisit yang boleh masuk reducer Radar.
pub fn parse_solana_token_accounts(
    response: &Value,
    owner: &str,
    qualified_wallet: bool,
) -> Result<Vec<OnChainTokenEvidence>> {
    validate_observation_identity(owner, "owner")?;
    let result = require_rpc_result(response, "getTokenAccountsByOwner")?;
    let accounts = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("token account result missing value array"))?;
    let mut evidence = Vec::new();
    for account in accounts {
        let parsed = account
            .get("account")
            .and_then(|value| value.get("data"))
            .and_then(|value| value.get("parsed"))
            .ok_or_else(|| anyhow::anyhow!("token account parsed data missing"))?;
        let info = parsed
            .get("info")
            .ok_or_else(|| anyhow::anyhow!("token account info missing"))?;
        let mint = info
            .get("mint")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("token account mint missing"))?;
        let amount = info
            .get("tokenAmount")
            .and_then(|value| value.get("amount"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("token account amount missing"))?;
        if amount
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("token amount invalid"))?
            == 0
        {
            continue;
        }
        validate_observation_identity(owner, mint)?;
        evidence.push(OnChainTokenEvidence {
            chain: Chain::Solana,
            token_address: mint.to_string(),
            wallet_address: owner.to_string(),
            kind: RadarEvidenceKind::TokenHolding,
            tx_id: None,
            observed_at: Utc::now(),
            cluster_id: None,
            qualified_wallet,
        });
    }
    Ok(evidence)
}

/// Mengambil hasil JSON-RPC tanpa pernah menganggap error/null sebagai evidence.
pub fn require_rpc_result<'a>(response: &'a Value, method: &str) -> Result<&'a Value> {
    if method.trim().is_empty() {
        bail!("RPC method missing");
    }
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("invalid JSON-RPC version");
    }
    if response.get("error").is_some() {
        bail!("RPC method {method} returned an error");
    }
    response
        .get("result")
        .filter(|result| !result.is_null())
        .ok_or_else(|| anyhow::anyhow!("RPC method {method} returned no result"))
}

/// Validasi minimal address sebelum response RPC dipakai sebagai observasi.
pub fn validate_observation_identity(wallet: &str, token: &str) -> Result<()> {
    if wallet.trim().is_empty() || token.trim().is_empty() {
        bail!("on-chain observation identity is incomplete");
    }
    if wallet.chars().any(char::is_whitespace) || token.chars().any(char::is_whitespace) {
        bail!("on-chain observation identity contains whitespace");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn qualified_address() -> WalletScanAddress {
        WalletScanAddress {
            chain: Chain::Solana,
            address: "Qualified111".into(),
            source_db: "wallet_registry.db",
            is_candidate: false,
            is_active: true,
            tier: "TIER-S".into(),
            execution_track: "QUALIFIED".into(),
        }
    }

    #[tokio::test]
    async fn read_only_collector_uses_mock_http_and_preserves_evidence() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for expected_program in [SPL_TOKEN_PROGRAM, TOKEN_2022_PROGRAM] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request_text = String::from_utf8(request).unwrap();
                assert!(request_text.starts_with("POST / HTTP/1.1"));
                assert!(request_text.contains("getTokenAccountsByOwner"));
                assert!(request_text.contains("Qualified111"));
                assert!(request_text.contains(expected_program));
                let body = r#"{"jsonrpc":"2.0","id":1,"result":{"value":[{"account":{"data":{"parsed":{"info":{"mint":"Mint111","tokenAmount":{"amount":"42"}}}}}}]}}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = ReadOnlyRpcClient::new(&endpoint).unwrap();
        let evidence = collect_solana_holdings_read_only(&client, &[qualified_address()])
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(evidence.len(), 2);
        assert!(evidence.iter().all(|item| item.qualified_wallet));
        assert!(evidence.iter().all(|item| item.token_address == "Mint111"));
    }

    #[tokio::test]
    async fn read_only_client_rejects_mutating_methods_before_network() {
        let client = ReadOnlyRpcClient::new("http://127.0.0.1:1").unwrap();
        let request = json!({"jsonrpc":"2.0","id":1,"method":"sendTransaction"});
        assert!(client.call(&request).await.is_err());
    }

    #[test]
    fn read_only_client_rejects_empty_or_invalid_endpoints() {
        assert!(ReadOnlyRpcClient::new("").is_err());
        assert!(ReadOnlyRpcClient::new("ws://node.example").is_err());
        assert!(ReadOnlyRpcClient::new("http://node.example").is_ok());
    }

    #[test]
    fn request_builder_is_read_only_and_uses_confirmed_parsing() {
        let request = build_solana_token_accounts_request("Wallet111", 7).unwrap();
        assert_eq!(request["method"], "getTokenAccountsByOwner");
        assert_eq!(request["id"], 7);
        assert_eq!(request["params"][2]["encoding"], "jsonParsed");
        assert_eq!(request["params"][2]["commitment"], "confirmed");
        assert_ne!(request["method"], "sendTransaction");
    }

    #[test]
    fn solana_token_accounts_parse_positive_holdings_only() {
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"value": [
                {"account": {"data": {"parsed": {"info": {
                    "mint": "Mint111", "tokenAmount": {"amount": "42"}
                }}}}},
                {"account": {"data": {"parsed": {"info": {
                    "mint": "Zero111", "tokenAmount": {"amount": "0"}
                }}}}}
            ]}
        });
        let result = parse_solana_token_accounts(&response, "Wallet111", true).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].token_address, "Mint111");
        assert!(result[0].qualified_wallet);
        assert_eq!(result[0].kind, RadarEvidenceKind::TokenHolding);
    }

    #[test]
    fn response_collector_preserves_roles_and_rejects_missing_owner() {
        let addresses = vec![
            WalletScanAddress {
                chain: Chain::Solana,
                address: "Qualified111".into(),
                source_db: "wallet_registry.db",
                is_candidate: false,
                is_active: true,
                tier: "TIER-S".into(),
                execution_track: "QUALIFIED".into(),
            },
            WalletScanAddress {
                chain: Chain::Solana,
                address: "Candidate111".into(),
                source_db: "candidate_wallet_registry.db",
                is_candidate: true,
                is_active: false,
                tier: "TIER-B".into(),
                execution_track: "MONITOR_ONLY".into(),
            },
        ];
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"value": [{"account": {"data": {"parsed": {"info": {
                "mint": "Mint111", "tokenAmount": {"amount": "42"}
            }}}}}]}
        });
        let evidence = collect_solana_holdings_from_responses(
            &addresses,
            &[
                ("Qualified111".into(), response.clone()),
                ("Candidate111".into(), response),
            ],
        )
        .unwrap();
        assert_eq!(evidence.len(), 2);
        assert!(evidence.iter().any(|item| item.qualified_wallet));
        assert!(evidence.iter().any(|item| !item.qualified_wallet));
        assert!(collect_solana_holdings_from_responses(&addresses, &[]).is_err());
    }

    #[test]
    fn malformed_solana_token_account_is_rejected() {
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {"value": [{"account": {"data": {"parsed": {}}}}]}
        });
        assert!(parse_solana_token_accounts(&response, "Wallet111", true).is_err());
    }

    #[test]
    fn rpc_error_is_not_evidence() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {"code": -32000, "message": "rate limited"}
        });
        assert!(require_rpc_result(&response, "getBalance").is_err());
    }

    #[test]
    fn rpc_null_result_is_not_evidence() {
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": null});
        assert!(require_rpc_result(&response, "getTransaction").is_err());
    }

    #[test]
    fn valid_rpc_result_is_preserved() {
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": {"value": 7}});
        assert_eq!(
            require_rpc_result(&response, "getBalance").unwrap()["value"],
            7
        );
    }

    #[test]
    fn incomplete_identity_is_rejected() {
        assert!(validate_observation_identity("wallet", "").is_err());
        assert!(validate_observation_identity("wallet address", "token").is_err());
    }

    #[test]
    fn response_reducer_propagates_authoritative_raydium_pool() {
        let address = qualified_address();
        let reference = SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: address.address.clone(),
            signature: "SigRaydium".into(),
            slot: 42,
            block_time: None,
        };
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "slot": 42, "blockTime": 1700000000,
                "transaction": {"signatures": ["SigRaydium"], "message": {"instructions": [{
                    "programId": "675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
                    "data": "H",
                    "accounts": ["Token111", "PoolRaydium", "Authority111", "VaultA", "VaultB", "UserIn", "UserOut", "Wallet111"]
                }]}},
                "meta": {
                    "err": null,
                    "preTokenBalances": [
                        {"accountIndex": 2, "mint": "Token111", "owner": "Qualified111", "uiTokenAmount": {"amount": "10"}},
                        {"accountIndex": 3, "mint": "USDC111", "owner": "Qualified111", "uiTokenAmount": {"amount": "1000"}}
                    ],
                    "postTokenBalances": [
                        {"accountIndex": 2, "mint": "Token111", "owner": "Qualified111", "uiTokenAmount": {"amount": "20"}},
                        {"accountIndex": 3, "mint": "USDC111", "owner": "Qualified111", "uiTokenAmount": {"amount": "800"}}
                    ]
                }
            }
        });
        let activities = collect_solana_activity_from_responses(
            &address,
            "Token111",
            "USDC111",
            &[reference],
            &[("SigRaydium".into(), response)],
        )
        .unwrap();
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].pool_address.as_deref(), Some("PoolRaydium"));
        assert_eq!(
            activities[0].venue,
            Some(crate::radar_activity::VerifiedVenue::SolanaRaydiumAmm)
        );
    }

    #[test]
    fn response_poll_holds_cursor_when_transaction_response_is_missing() {
        let address = qualified_address();
        let reference = SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: address.address.clone(),
            signature: "SigMissing".into(),
            slot: 42,
            block_time: None,
        };
        let result = plan_and_reduce_solana_responses(
            &address,
            "Token111",
            "USDC111",
            &[reference],
            None,
            1,
            &[],
        )
        .unwrap();
        assert_eq!(result.selected_references, 1);
        assert_eq!(result.missing_responses, 1);
        assert!(result.activities.is_empty());
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn response_poll_advances_cursor_for_available_but_unverified_response() {
        let address = qualified_address();
        let reference = SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: address.address.clone(),
            signature: "SigMalformed".into(),
            slot: 42,
            block_time: None,
        };
        let response = json!({"jsonrpc": "2.0", "id": 1, "result": null});
        let result = plan_and_reduce_solana_responses(
            &address,
            "Token111",
            "USDC111",
            &[reference],
            None,
            1,
            &[("SigMalformed".into(), response)],
        )
        .unwrap();
        assert_eq!(result.missing_responses, 0);
        assert!(result.activities.is_empty());
        assert_eq!(
            result.next_cursor,
            Some(ScanCursor::Solana {
                signature: "SigMalformed".into(),
                slot: 42,
            })
        );
    }

    #[tokio::test]
    async fn bounded_read_only_poll_requests_signatures_and_transaction() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for expected_method in ["getSignaturesForAddress", "getTransaction"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                let header_end;
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "mock menerima request terpotong");
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                        header_end = end + 4;
                        break;
                    }
                }
                let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
                let content_length = headers.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    (name.trim() == "content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                });
                while {
                    let request_text = String::from_utf8_lossy(&request);
                    let body_complete = content_length
                        .is_some_and(|length| request.len() >= header_end + length)
                        || request_text.ends_with("0\r\n\r\n");
                    !(request_text.contains(expected_method) && body_complete)
                } {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0, "mock body request terpotong");
                    request.extend_from_slice(&buffer[..read]);
                }
                let request_text = String::from_utf8(request).unwrap();
                assert!(request_text.starts_with("POST / HTTP/1.1"));
                assert!(request_text.contains(expected_method));
                let body = if expected_method == "getSignaturesForAddress" {
                    r#"{"jsonrpc":"2.0","id":1,"result":[{"signature":"SigRuntime","slot":42,"blockTime":1700000000}]}"#
                } else {
                    r#"{"jsonrpc":"2.0","id":2,"result":{"slot":42,"blockTime":1700000000,"transaction":{"signatures":["SigRuntime"],"message":{"instructions":[{"programId":"675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","data":"H","accounts":["Token111","PoolRuntime","Authority111","VaultA","VaultB","UserIn","UserOut","Wallet111"]}]}},"meta":{"err":null,"preTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"10"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"1000"}}],"postTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"20"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"800"}}]}}}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let client = ReadOnlyRpcClient::new(&endpoint).unwrap();
        let result = poll_solana_activity_read_only(
            &client,
            &qualified_address(),
            "Token111",
            "USDC111",
            None,
            10,
            1,
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(result.selected_references, 1);
        assert_eq!(result.missing_responses, 0);
        assert_eq!(result.activities.len(), 1);
        assert_eq!(
            result.activities[0].pool_address.as_deref(),
            Some("PoolRuntime")
        );
        assert_eq!(
            result.next_cursor,
            Some(ScanCursor::Solana {
                signature: "SigRuntime".into(),
                slot: 42,
            })
        );
    }

    #[tokio::test]
    async fn poll_and_persist_read_only_writes_activity_outbox_and_cursor() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for expected_method in ["getSignaturesForAddress", "getTransaction"] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let body = if expected_method == "getSignaturesForAddress" {
                    r#"{"jsonrpc":"2.0","id":1,"result":[{"signature":"SigPersist","slot":42,"blockTime":1700000000}]}"#
                } else {
                    r#"{"jsonrpc":"2.0","id":2,"result":{"slot":42,"blockTime":1700000000,"transaction":{"signatures":["SigPersist"],"message":{"instructions":[{"programId":"675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","data":"H","accounts":["Token111","PoolPersist","Authority111","VaultA","VaultB","UserIn","UserOut","Qualified111"]}]}},"meta":{"err":null,"preTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"10"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"1000"}}],"postTokenBalances":[{"accountIndex":2,"mint":"Token111","owner":"Qualified111","uiTokenAmount":{"amount":"20"}},{"accountIndex":3,"mint":"USDC111","owner":"Qualified111","uiTokenAmount":{"amount":"800"}}]}}}"#
                };
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = ReadOnlyRpcClient::new(&endpoint).unwrap();
        let db = SmartWalletDb::new_in_memory().unwrap();
        let ipc = IpcServer::new("/tmp/zenithsw-rpc-persist-test.sock");
        let mut receiver = ipc.get_sender().subscribe();
        let wallet = crate::models::SmartWallet {
            id: None,
            rank: 1,
            name: "Persist Wallet".into(),
            handle: "persist-wallet".into(),
            chain: Chain::Solana,
            address: "Qualified111".into(),
            tier: crate::models::WalletTier::TierS,
            pnl_24h_usd: 1000.0,
            win_rate_pct: 70.0,
            total_trades: 10,
            source: "ONCHAIN".into(),
            reputation_score: 90.0,
            execution_track: crate::models::ExecutionTrack::VaultLane,
            is_active: true,
            last_seen_at: None,
            updated_at: Utc::now(),
        };
        let report = poll_and_persist_solana_activity_read_only(SolanaActivityPersistenceContext {
            client: &client,
            db: &db,
            ipc: &ipc,
            address: &qualified_address(),
            token_address: "Token111",
            quote_asset: "USDC111",
            cursor: None,
            history_limit: 10,
            max_batch: 1,
            materialization: NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 1.0,
            },
            scope_key: "persist-scope",
        })
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(report.selected_references, 1);
        assert_eq!(report.verified_activities, 1);
        assert_eq!(report.persisted_activities, 1);
        assert!(report.cursor_advanced);
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 1);
        assert!(db.queued_activity_ipc(10).unwrap().is_empty());
        assert!(matches!(
            serde_json::from_str::<crate::ipc::IpcMessage>(&receiver.try_recv().unwrap()).unwrap(),
            crate::ipc::IpcMessage::ActivityBroadcast { .. }
        ));
        assert_eq!(
            db.load_dex_cursor(
                Chain::Solana,
                crate::dex_adapters::DexVenue::SolanaRaydiumAmm,
                "persist-scope"
            )
            .unwrap(),
            Some(ScanCursor::Solana {
                signature: "SigPersist".into(),
                slot: 42
            })
        );
    }

    #[tokio::test]
    async fn poll_and_persist_rejects_available_unverified_transaction_without_side_effects() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for body in [
                r#"{"jsonrpc":"2.0","id":1,"result":[{"signature":"SigUnverified","slot":42}]}"#,
                r#"{"jsonrpc":"2.0","id":2,"result":{"slot":42,"transaction":{"signatures":["SigUnverified"],"message":{"instructions":[{"programId":"675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"}]}},"meta":{"err":null,"preTokenBalances":[],"postTokenBalances":[]}}}"#,
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    let read = socket.read(&mut buffer).await.unwrap();
                    assert!(read > 0);
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });
        let client = ReadOnlyRpcClient::new(&endpoint).unwrap();
        let db = SmartWalletDb::new_in_memory().unwrap();
        let ipc = IpcServer::new("/tmp/zenithsw-rpc-negative-test.sock");
        let wallet = crate::models::SmartWallet {
            id: None,
            rank: 1,
            name: "Unverified Wallet".into(),
            handle: "unverified-wallet".into(),
            chain: Chain::Solana,
            address: "Qualified111".into(),
            tier: crate::models::WalletTier::TierS,
            pnl_24h_usd: 1000.0,
            win_rate_pct: 70.0,
            total_trades: 10,
            source: "ONCHAIN".into(),
            reputation_score: 90.0,
            execution_track: crate::models::ExecutionTrack::VaultLane,
            is_active: true,
            last_seen_at: None,
            updated_at: Utc::now(),
        };
        let report = poll_and_persist_solana_activity_read_only(SolanaActivityPersistenceContext {
            client: &client,
            db: &db,
            ipc: &ipc,
            address: &qualified_address(),
            token_address: "Token111",
            quote_asset: "USDC111",
            cursor: None,
            history_limit: 10,
            max_batch: 1,
            materialization: NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 1.0,
            },
            scope_key: "negative-scope",
        })
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(report.selected_references, 1);
        assert_eq!(report.missing_responses, 0);
        assert_eq!(report.verified_activities, 0);
        assert_eq!(report.persisted_activities, 0);
        assert!(!report.cursor_advanced);
        assert!(db.get_recent_activities(10).unwrap().is_empty());
        assert!(db.queued_activity_ipc(10).unwrap().is_empty());
        assert!(db
            .load_dex_cursor(
                Chain::Solana,
                crate::dex_adapters::DexVenue::SolanaRaydiumAmm,
                "negative-scope"
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn response_reducer_rejects_allowlisted_swap_without_pool_layout() {
        let address = qualified_address();
        let reference = SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: address.address.clone(),
            signature: "SigNoPool".into(),
            slot: 42,
            block_time: None,
        };
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "slot": 42,
                "transaction": {"signatures": ["SigNoPool"], "message": {"instructions": [{
                    "programId": "675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"
                }]}},
                "meta": {
                    "err": null,
                    "preTokenBalances": [
                        {"accountIndex": 2, "mint": "Token111", "owner": "Qualified111", "uiTokenAmount": {"amount": "10"}},
                        {"accountIndex": 3, "mint": "USDC111", "owner": "Qualified111", "uiTokenAmount": {"amount": "1000"}}
                    ],
                    "postTokenBalances": [
                        {"accountIndex": 2, "mint": "Token111", "owner": "Qualified111", "uiTokenAmount": {"amount": "20"}},
                        {"accountIndex": 3, "mint": "USDC111", "owner": "Qualified111", "uiTokenAmount": {"amount": "800"}}
                    ]
                }
            }
        });
        let activities = collect_solana_activity_from_responses(
            &address,
            "Token111",
            "USDC111",
            &[reference],
            &[("SigNoPool".into(), response)],
        )
        .unwrap();
        assert!(activities.is_empty());
    }
}
