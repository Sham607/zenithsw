use crate::db::SmartWalletDb;
use crate::models::{ActivityType, Chain, WalletActivity};
use anyhow::Result;
use chrono::Utc;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Envelope JSON yang dikeluarkan Nitro feed relay pada endpoint `/feed`.
/// `l2Msg` adalah bytes ter-encode base64; bukan JSON raw transaction.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencerFeedPayload {
    pub version: u32,
    #[serde(default)]
    pub messages: Vec<SequencerFeedMessage>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencerFeedMessage {
    pub sequence_number: u64,
    pub message: SequencerMessageWithMetadata,
    #[serde(default)]
    pub signature: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencerMessageWithMetadata {
    pub message: NitroIncomingMessage,
    pub delayed_messages_read: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NitroIncomingMessage {
    pub header: Option<NitroIncomingMessageHeader>,
    #[serde(rename = "l2Msg")]
    pub l2_msg: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NitroIncomingMessageHeader {
    pub kind: u64,
    pub sender: String,
    pub block_number: u64,
    pub timestamp: i64,
    #[serde(default)]
    pub request_id: Option<String>,
    #[serde(default)]
    pub base_fee_l1: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NitroL2MessageKind {
    UnsignedUserTx,
    ContractTx,
    NonmutatingCall,
    Batch,
    SignedTx,
    Heartbeat,
    SignedCompressedTx,
    Unknown(u8),
}

impl NitroL2MessageKind {
    fn from_byte(value: u8) -> Self {
        match value {
            0 => Self::UnsignedUserTx,
            1 => Self::ContractTx,
            2 => Self::NonmutatingCall,
            3 => Self::Batch,
            4 => Self::SignedTx,
            6 => Self::Heartbeat,
            7 => Self::SignedCompressedTx,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedSignedNitroTransaction {
    pub tx_type: u8,
    pub chain_id: u64,
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_priority_fee_per_gas: Option<u128>,
    pub max_fee_per_gas: Option<u128>,
    pub to: Option<String>,
    pub value: u128,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedUnsignedNitroTransaction {
    pub nonce: u64,
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub to: Option<String>,
    pub value: u128,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedContractNitroTransaction {
    pub gas_limit: u64,
    pub max_fee_per_gas: u128,
    pub to: Option<String>,
    pub value: u128,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedNitroBatch {
    pub messages: Vec<Vec<u8>>,
}

impl NitroIncomingMessage {
    pub fn decode_l2_msg(&self) -> Result<Vec<u8>> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.l2_msg)
            .map_err(|error| anyhow::anyhow!("invalid Nitro l2Msg base64: {error}"))?;
        if bytes.len() > 256 * 1024 {
            anyhow::bail!("Nitro l2Msg exceeds 256 KiB limit");
        }
        if bytes.is_empty() {
            anyhow::bail!("Nitro l2Msg is empty");
        }
        Ok(bytes)
    }

    pub fn l2_message_kind(&self) -> Result<NitroL2MessageKind> {
        Ok(NitroL2MessageKind::from_byte(
            *self.decode_l2_msg()?.first().unwrap(),
        ))
    }

    /// Decodes Nitro kind 0 (unsigned EOA transaction).
    /// Fields are fixed-width 32-byte unsigned integers/addresses; the remaining bytes are calldata.
    pub fn decode_unsigned_transaction(&self) -> Result<DecodedUnsignedNitroTransaction> {
        let bytes = self.decode_l2_msg()?;
        if bytes.first().copied() != Some(0) {
            anyhow::bail!("l2Msg is not Nitro UnsignedUserTx kind 0");
        }
        let mut cursor = 1usize;
        let gas_limit = read_nitro_u64(&bytes, &mut cursor, "gas limit")?;
        let max_fee_per_gas = read_nitro_u128(&bytes, &mut cursor, "max fee per gas")?;
        let nonce = read_nitro_u64(&bytes, &mut cursor, "nonce")?;
        let to_bytes = read_nitro_word(&bytes, &mut cursor, "to")?;
        let to = if to_bytes[..12].iter().any(|byte| *byte != 0) {
            anyhow::bail!("Nitro unsigned transaction to field has non-zero high bytes");
        } else if to_bytes[12..].iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(format!("0x{}", hex::encode(&to_bytes[12..])))
        };
        let value = read_nitro_u128(&bytes, &mut cursor, "value")?;
        Ok(DecodedUnsignedNitroTransaction {
            nonce,
            gas_limit,
            max_fee_per_gas,
            to,
            value,
            data: bytes[cursor..].to_vec(),
        })
    }

    /// Decodes Nitro kind 1 (unsigned contract transaction).
    /// The request-id linkage is envelope metadata and is not part of `l2Msg`.
    pub fn decode_contract_transaction(&self) -> Result<DecodedContractNitroTransaction> {
        let bytes = self.decode_l2_msg()?;
        if bytes.first().copied() != Some(1) {
            anyhow::bail!("l2Msg is not Nitro ContractTx kind 1");
        }
        let mut cursor = 1usize;
        let gas_limit = read_nitro_u64(&bytes, &mut cursor, "gas limit")?;
        let max_fee_per_gas = read_nitro_u128(&bytes, &mut cursor, "max fee per gas")?;
        let to_bytes = read_nitro_word(&bytes, &mut cursor, "to")?;
        let to = if to_bytes[..12].iter().any(|byte| *byte != 0) {
            anyhow::bail!("Nitro contract transaction to field has non-zero high bytes");
        } else if to_bytes[12..].iter().all(|byte| *byte == 0) {
            None
        } else {
            Some(format!("0x{}", hex::encode(&to_bytes[12..])))
        };
        let value = read_nitro_u128(&bytes, &mut cursor, "value")?;
        Ok(DecodedContractNitroTransaction {
            gas_limit,
            max_fee_per_gas,
            to,
            value,
            data: bytes[cursor..].to_vec(),
        })
    }

    /// Decodes Nitro kind 3 into its leaf L2 messages.
    /// Each batch item is framed as a big-endian uint64 length followed by bytes.
    pub fn decode_batch(&self) -> Result<DecodedNitroBatch> {
        let bytes = self.decode_l2_msg()?;
        if bytes.first().copied() != Some(3) {
            anyhow::bail!("l2Msg is not Nitro Batch kind 3");
        }
        let mut messages = Vec::new();
        decode_batch_items(&bytes[1..], 0, &mut messages)?;
        Ok(DecodedNitroBatch { messages })
    }

    /// Decodes only Nitro kind 4 (signed legacy/EIP-1559 transaction).
    /// Unsigned, batch, and compressed messages require Nitro's full parser.
    pub fn decode_signed_transaction(&self) -> Result<DecodedSignedNitroTransaction> {
        if self.l2_message_kind()? != NitroL2MessageKind::SignedTx {
            anyhow::bail!("l2Msg is not Nitro SignedTx kind 4");
        }
        let bytes = self.decode_l2_msg()?;
        let raw = &bytes[1..];
        if raw.is_empty() {
            anyhow::bail!("signed transaction payload is empty");
        }
        let (tx_type, rlp) = if raw[0] == 0x01 || raw[0] == 0x02 {
            (raw[0], &raw[1..])
        } else {
            (0, raw)
        };
        let fields = decode_rlp_list(rlp)?;
        let required_fields = match tx_type {
            2 => 9,
            1 => 8,
            0 => 9,
            _ => 0,
        };
        if fields.len() < required_fields {
            anyhow::bail!("signed transaction has insufficient RLP fields");
        }
        let (
            chain_id_idx,
            nonce_idx,
            gas_limit_idx,
            max_priority,
            max_fee,
            to_idx,
            value_idx,
            data_idx,
        ) = match tx_type {
            2 => (Some(0), 1, 4, Some(2), Some(3), 5, 6, 7),
            1 => (Some(0), 1, 3, None, None, 4, 5, 6),
            0 => (None, 0, 2, None, None, 3, 4, 5),
            _ => unreachable!(),
        };
        let chain_id = chain_id_idx
            .map(|index| rlp_u128(&fields[index]).map(|value| value as u64))
            .transpose()?
            .unwrap_or(0);
        let nonce = rlp_u128(&fields[nonce_idx])? as u64;
        let gas_limit = rlp_u128(&fields[gas_limit_idx])? as u64;
        let to = if fields[to_idx].is_empty() {
            None
        } else if fields[to_idx].len() == 20 {
            Some(format!("0x{}", hex::encode(&fields[to_idx])))
        } else {
            anyhow::bail!("transaction to field is not 20 bytes");
        };
        let value = rlp_u128(&fields[value_idx])?;
        let data = fields[data_idx].clone();
        Ok(DecodedSignedNitroTransaction {
            tx_type,
            chain_id,
            nonce,
            gas_limit,
            max_priority_fee_per_gas: max_priority.map(|i| rlp_u128(&fields[i])).transpose()?,
            max_fee_per_gas: max_fee.map(|i| rlp_u128(&fields[i])).transpose()?,
            to,
            value,
            data,
        })
    }
}

const MAX_NITRO_BATCH_DEPTH: usize = 16;
const MAX_NITRO_BATCH_MESSAGES: usize = 4096;

fn decode_batch_items(input: &[u8], depth: usize, output: &mut Vec<Vec<u8>>) -> Result<()> {
    if depth >= MAX_NITRO_BATCH_DEPTH {
        anyhow::bail!("Nitro batch nesting exceeds depth 16");
    }
    let mut cursor = 0usize;
    while cursor < input.len() {
        let length_bytes = input
            .get(cursor..cursor + 8)
            .ok_or_else(|| anyhow::anyhow!("Nitro batch item length is truncated"))?;
        let length = u64::from_be_bytes(length_bytes.try_into().unwrap());
        let length = usize::try_from(length)
            .map_err(|_| anyhow::anyhow!("Nitro batch item length exceeds platform size"))?;
        cursor += 8;
        if length == 0 {
            anyhow::bail!("Nitro batch item is empty");
        }
        let end = cursor
            .checked_add(length)
            .ok_or_else(|| anyhow::anyhow!("Nitro batch item length overflows"))?;
        let item = input
            .get(cursor..end)
            .ok_or_else(|| anyhow::anyhow!("Nitro batch item is truncated"))?;
        cursor = end;
        if output.len() >= MAX_NITRO_BATCH_MESSAGES {
            anyhow::bail!("Nitro batch has too many messages");
        }
        match NitroL2MessageKind::from_byte(item[0]) {
            NitroL2MessageKind::Batch => decode_batch_items(&item[1..], depth + 1, output)?,
            _ => output.push(item.to_vec()),
        }
    }
    Ok(())
}

fn read_nitro_word(input: &[u8], cursor: &mut usize, field: &str) -> Result<[u8; 32]> {
    let end = cursor
        .checked_add(32)
        .ok_or_else(|| anyhow::anyhow!("Nitro {field} offset overflow"))?;
    let bytes = input
        .get(*cursor..end)
        .ok_or_else(|| anyhow::anyhow!("Nitro {field} field is truncated"))?;
    let word: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Nitro {field} field has invalid length"))?;
    *cursor = end;
    Ok(word)
}

fn read_nitro_u64(input: &[u8], cursor: &mut usize, field: &str) -> Result<u64> {
    let word = read_nitro_word(input, cursor, field)?;
    if word[..24].iter().any(|byte| *byte != 0) {
        anyhow::bail!("Nitro {field} exceeds u64");
    }
    Ok(u64::from_be_bytes(word[24..].try_into().unwrap()))
}

fn read_nitro_u128(input: &[u8], cursor: &mut usize, field: &str) -> Result<u128> {
    let word = read_nitro_word(input, cursor, field)?;
    if word[..16].iter().any(|byte| *byte != 0) {
        anyhow::bail!("Nitro {field} exceeds u128");
    }
    Ok(u128::from_be_bytes(word[16..].try_into().unwrap()))
}

fn decode_rlp_list(input: &[u8]) -> Result<Vec<Vec<u8>>> {
    let (payload, consumed) = rlp_payload(input)?;
    if consumed != input.len() {
        anyhow::bail!("RLP list has trailing bytes");
    }
    let mut values = Vec::new();
    let mut offset = 0;
    while offset < payload.len() {
        let (value, used) = rlp_payload(&payload[offset..])?;
        values.push(value);
        offset += used;
    }
    Ok(values)
}

fn rlp_payload(input: &[u8]) -> Result<(Vec<u8>, usize)> {
    let first = *input
        .first()
        .ok_or_else(|| anyhow::anyhow!("RLP input empty"))?;
    if first <= 0x7f {
        return Ok((vec![first], 1));
    }
    let (is_list, payload_start, payload_len) = if first <= 0xb7 {
        (false, 1usize, (first - 0x80) as usize)
    } else if first <= 0xbf {
        let length_bytes = (first - 0xb7) as usize;
        (false, 1 + length_bytes, read_len(input, 1, length_bytes)?)
    } else if first <= 0xf7 {
        (true, 1usize, (first - 0xc0) as usize)
    } else {
        let length_bytes = (first - 0xf7) as usize;
        (true, 1 + length_bytes, read_len(input, 1, length_bytes)?)
    };
    let end = payload_start
        .checked_add(payload_len)
        .ok_or_else(|| anyhow::anyhow!("RLP length overflow"))?;
    if end > input.len() {
        anyhow::bail!(if is_list {
            "RLP list truncated"
        } else {
            "RLP bytes truncated"
        });
    }
    let _ = is_list;
    Ok((input[payload_start..end].to_vec(), end))
}

fn read_len(input: &[u8], start: usize, count: usize) -> Result<usize> {
    if count == 0 || count > 8 || start + count > input.len() {
        anyhow::bail!("invalid RLP length");
    }
    let mut value = 0usize;
    for byte in &input[start..start + count] {
        value = value
            .checked_mul(256)
            .and_then(|v| v.checked_add(*byte as usize))
            .ok_or_else(|| anyhow::anyhow!("RLP length overflow"))?;
    }
    Ok(value)
}

fn rlp_u128(value: &[u8]) -> Result<u128> {
    if value.len() > 16 {
        anyhow::bail!("RLP integer exceeds u128");
    }
    let mut out = 0u128;
    for byte in value {
        out = out
            .checked_mul(256)
            .and_then(|v| v.checked_add(*byte as u128))
            .ok_or_else(|| anyhow::anyhow!("RLP integer overflow"))?;
    }
    Ok(out)
}

fn is_evm_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_hex_quantity(value: &str) -> Result<u128> {
    Ok(u128::from_str_radix(
        value
            .strip_prefix("0x")
            .ok_or_else(|| anyhow::anyhow!("invalid hex quantity"))?,
        16,
    )?)
}

pub struct EvmStreamListener {
    rpc_urls: Vec<String>,
    rpc_index: AtomicUsize,
    rpc_cooldowns: Arc<Mutex<Vec<Option<Instant>>>>,
    chain: Chain,
    processed_hashes: Arc<Mutex<HashSet<String>>>,
    client: reqwest::Client,
    latest_block_cache: Arc<Mutex<Option<(u128, Instant)>>>,
    requests: AtomicU64,
    successes: AtomicU64,
    rate_limited: AtomicU64,
    failures: AtomicU64,
}

impl EvmStreamListener {
    pub fn new(rpc_url: &str, chain: Chain) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default();
        let rpc_urls: Vec<String> = rpc_url
            .split(',')
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        Self {
            rpc_cooldowns: Arc::new(Mutex::new(vec![None; rpc_urls.len()])),
            rpc_urls,
            rpc_index: AtomicUsize::new(0),
            chain,
            processed_hashes: Arc::new(Mutex::new(HashSet::new())),
            client,
            latest_block_cache: Arc::new(Mutex::new(None)),
            requests: AtomicU64::new(0),
            successes: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        }
    }

    pub fn parse_sequencer_feed_payload(raw: &str) -> Result<SequencerFeedPayload> {
        let payload: SequencerFeedPayload = serde_json::from_str(raw)
            .map_err(|error| anyhow::anyhow!("invalid sequencer feed JSON: {error}"))?;
        if payload.version != 1 {
            anyhow::bail!("unsupported sequencer feed version: {}", payload.version);
        }
        for item in &payload.messages {
            if item.message.message.decode_l2_msg()?.is_empty() {
                anyhow::bail!(
                    "sequencer feed message {} has empty l2Msg",
                    item.sequence_number
                );
            }
        }
        Ok(payload)
    }

    pub fn health_snapshot(&self) -> (u64, u64, u64, u64) {
        (
            self.requests.load(Ordering::Relaxed),
            self.successes.load(Ordering::Relaxed),
            self.rate_limited.load(Ordering::Relaxed),
            self.failures.load(Ordering::Relaxed),
        )
    }

    /// Reads a bounded number of uncompressed Nitro feed envelopes.
    /// This is read-only; it never signs, broadcasts, or writes trading state.
    pub async fn read_sequencer_feed(
        &self,
        feed_url: &str,
        max_envelopes: usize,
    ) -> Result<Vec<SequencerFeedPayload>> {
        if !feed_url.starts_with("ws://") && !feed_url.starts_with("wss://") {
            anyhow::bail!("Nitro feed URL must use ws:// or wss://");
        }
        if max_envelopes == 0 {
            return Ok(Vec::new());
        }
        let mut envelopes = Vec::new();
        let mut seen_sequences = HashSet::new();
        let mut consecutive_disconnects = 0usize;
        while envelopes.len() < max_envelopes {
            if consecutive_disconnects >= 3 {
                anyhow::bail!("Nitro feed disconnected after 3 reconnect attempts");
            }
            let (mut socket, _) = match connect_async(feed_url).await {
                Ok(connection) => connection,
                Err(error) => {
                    consecutive_disconnects += 1;
                    if consecutive_disconnects >= 3 {
                        return Err(anyhow::anyhow!(
                            "Nitro feed connection failed after retries: {error}"
                        ));
                    }
                    tokio::time::sleep(Duration::from_millis(250 * consecutive_disconnects as u64))
                        .await;
                    continue;
                }
            };
            while let Some(frame) = socket.next().await {
                let frame =
                    frame.map_err(|error| anyhow::anyhow!("Nitro feed read failed: {error}"))?;
                let raw = match frame {
                    Message::Text(text) => text.to_string(),
                    Message::Binary(bytes) => {
                        String::from_utf8(bytes.to_vec()).map_err(|error| {
                            anyhow::anyhow!("Nitro feed binary frame is not UTF-8: {error}")
                        })?
                    }
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => break,
                    _ => continue,
                };
                let payload = Self::parse_sequencer_feed_payload(&raw)?;
                let has_new_sequence = payload
                    .messages
                    .iter()
                    .any(|message| seen_sequences.insert(message.sequence_number));
                if has_new_sequence {
                    consecutive_disconnects = 0;
                    envelopes.push(payload);
                }
                if envelopes.len() >= max_envelopes {
                    return Ok(envelopes);
                }
            }
            consecutive_disconnects += 1;
            tokio::time::sleep(Duration::from_millis(250 * consecutive_disconnects as u64)).await;
        }
        Ok(envelopes)
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
        let endpoint = self
            .rpc_urls
            .first()
            .ok_or_else(|| anyhow::anyhow!("no Robinhood RPC endpoints configured"))?;
        let response = self.client.post(endpoint).json(&payload).send().await?;
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

    /// Scans one bounded block tail for all monitored wallets in two RPC calls.
    /// Indexed wallet topics are supplied as arrays, avoiding one eth_getLogs call per wallet.
    pub async fn scan_global_wallet_activity(
        &self,
        wallets: &[(String, String)],
        db: Arc<SmartWalletDb>,
    ) -> Result<Vec<WalletActivity>> {
        use crate::chains::robinhood::{
            classify_robinhood_receipt, RobinhoodVenue, ERC20_TRANSFER_TOPIC,
        };
        if self.chain != Chain::Robinhood || wallets.is_empty() {
            return Ok(Vec::new());
        }
        let latest = self.rpc_call("eth_blockNumber", json!([])).await?;
        let latest_n = parse_hex_quantity(latest.as_str().unwrap_or(""))?;
        let from = latest_n.saturating_sub(10);
        let mut address_to_handle = HashMap::new();
        let mut wallet_topics = Vec::new();
        for (handle, address) in wallets {
            if !is_evm_address(address) {
                continue;
            }
            let topic = format!("0x{}{}", "0".repeat(24), address[2..].to_lowercase());
            wallet_topics.push(topic);
            address_to_handle.insert(address.to_lowercase(), handle.clone());
        }
        if wallet_topics.is_empty() {
            return Ok(Vec::new());
        }
        let mut logs = Vec::new();
        for topics in [
            json!([ERC20_TRANSFER_TOPIC, wallet_topics, Value::Null]),
            json!([ERC20_TRANSFER_TOPIC, Value::Null, wallet_topics]),
        ] {
            let result = self
                .rpc_call(
                    "eth_getLogs",
                    json!([{
                        "fromBlock": format!("0x{from:x}"),
                        "toBlock": format!("0x{latest_n:x}"),
                        "topics": topics,
                    }]),
                )
                .await?;
            if let Some(values) = result.as_array() {
                logs.extend(values.iter().cloned());
            }
        }
        let mut activities = Vec::new();
        let mut seen = HashSet::new();
        for log in logs {
            let tx_hash = match log.get("transactionHash").and_then(Value::as_str) {
                Some(value) if seen.insert(value.to_lowercase()) => value.to_string(),
                _ => continue,
            };
            let topics = match log.get("topics").and_then(Value::as_array) {
                Some(values) if values.len() >= 3 => values,
                _ => continue,
            };
            let from_topic = topics[1].as_str().unwrap_or("");
            let to_topic = topics[2].as_str().unwrap_or("");
            if from_topic.len() != 66 || to_topic.len() != 66 {
                continue;
            }
            let from_addr = format!("0x{}", &from_topic[26..]).to_lowercase();
            let to_addr = format!("0x{}", &to_topic[26..]).to_lowercase();
            let (wallet_address, incoming) = if address_to_handle.contains_key(&to_addr) {
                (to_addr, true)
            } else if address_to_handle.contains_key(&from_addr) {
                (from_addr, false)
            } else {
                continue;
            };
            let handle = address_to_handle
                .get(&wallet_address)
                .cloned()
                .unwrap_or_default();
            let receipt = self
                .rpc_call("eth_getTransactionReceipt", json!([tx_hash]))
                .await?;
            let classified = classify_robinhood_receipt(&receipt, &wallet_address)?;
            if !classified.success
                || !classified.has_transfer
                || matches!(
                    classified.venue,
                    RobinhoodVenue::Unknown | RobinhoodVenue::TransferOnly
                )
            {
                continue;
            }
            let token = log
                .get("address")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let raw_amount =
                parse_hex_quantity(log.get("data").and_then(Value::as_str).unwrap_or("0x0"))?;
            if !is_evm_address(&token) || raw_amount == 0 {
                continue;
            }
            let decimals = self.erc20_uint(&token, "0x313ce567").await.unwrap_or(18);
            let symbol = self
                .erc20_string(&token, "0x95d89b41")
                .await
                .unwrap_or_else(|_| "TOKEN".to_string());
            if crate::models::is_blacklisted_asset(&token, &symbol) {
                continue;
            }
            let activity = WalletActivity {
                id: None,
                wallet_handle: handle,
                wallet_address,
                chain: self.chain,
                activity_type: if incoming {
                    ActivityType::Buy
                } else {
                    ActivityType::Sell
                },
                token_symbol: symbol,
                token_mint: token,
                amount_usd: 0.0,
                token_amount: raw_amount as f64 / 10f64.powi(decimals as i32),
                execution_price_usd: 0.0,
                whale_balance_before: 0.0,
                whale_balance_after: 0.0,
                sell_ratio_pct: if incoming { 0.0 } else { 100.0 },
                tx_signature: tx_hash,
                timestamp: Utc::now(),
            };
            db.insert_activity(&activity)?;
            activities.push(activity);
        }
        Ok(activities)
    }

    /// Pulls recent wallet Transfer logs, then classifies the complete receipt.
    /// Unknown/transfer-only receipts are ignored; no token-inflow heuristic is used.
    pub async fn check_wallet_activity(
        &self,
        handle: &str,
        address: &str,
        db: Arc<SmartWalletDb>,
    ) -> Result<Option<WalletActivity>> {
        use crate::chains::robinhood::{
            classify_robinhood_receipt, RobinhoodVenue, ERC20_TRANSFER_TOPIC,
        };
        if self.chain != Chain::Robinhood || !is_evm_address(address) {
            return Ok(None);
        }
        let cached_block = { *self.latest_block_cache.lock().unwrap() };
        let latest_n = if let Some((block, observed)) = cached_block {
            if observed.elapsed() < Duration::from_secs(5) {
                block
            } else {
                let latest = self.rpc_call("eth_blockNumber", json!([])).await?;
                let block = parse_hex_quantity(latest.as_str().unwrap_or(""))?;
                *self.latest_block_cache.lock().unwrap() = Some((block, Instant::now()));
                block
            }
        } else {
            let latest = self.rpc_call("eth_blockNumber", json!([])).await?;
            let block = parse_hex_quantity(latest.as_str().unwrap_or(""))?;
            *self.latest_block_cache.lock().unwrap() = Some((block, Instant::now()));
            block
        };
        // Keep the bounded read small; a global cursor/receipt indexer should be used for history.
        // Re-scanning 2,000 blocks per wallet multiplies provider load by the wallet count.
        // Public RPC endpoints reject dense historical windows; poll a short live tail.
        let from = latest_n.saturating_sub(10);
        // Indexed address topics are 32-byte ABI words: 24 zero hex chars + 40 address chars.
        let wallet_topic = format!("0x{}{}", "0".repeat(24), address[2..].to_lowercase());
        let mut txs = Vec::new();
        for topics in [
            json!([ERC20_TRANSFER_TOPIC, wallet_topic, Value::Null]),
            json!([ERC20_TRANSFER_TOPIC, Value::Null, wallet_topic]),
        ] {
            let filter = json!({"fromBlock": format!("0x{from:x}"), "toBlock": format!("0x{latest_n:x}"), "topics": topics});
            match self.rpc_call("eth_getLogs", json!([filter])).await {
                Ok(value) => {
                    if let Some(logs) = value.as_array() {
                        for log in logs {
                            if let Some(tx) = log.get("transactionHash").and_then(Value::as_str) {
                                if !txs.iter().any(|v: &String| v.eq_ignore_ascii_case(tx)) {
                                    txs.push(tx.to_string());
                                }
                            }
                        }
                    }
                }
                Err(error) => {
                    eprintln!(
                        "[STREAM RPC ERROR] chain=ROBINHOOD wallet={} method=eth_getLogs error={}",
                        address, error
                    );
                }
            }
        }
        txs.sort();
        for tx_hash in txs.into_iter().rev() {
            if self.processed_hashes.lock().unwrap().contains(&tx_hash) {
                continue;
            }
            let receipt = self
                .rpc_call("eth_getTransactionReceipt", json!([tx_hash]))
                .await?;
            let classified = classify_robinhood_receipt(&receipt, address)?;
            if !classified.success
                || !classified.has_transfer
                || matches!(
                    classified.venue,
                    RobinhoodVenue::Unknown | RobinhoodVenue::TransferOnly
                )
            {
                continue;
            }
            let logs = receipt
                .get("logs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let mut selected: Option<(String, bool, u128)> = None;
            for log in logs {
                let token = log.get("address").and_then(Value::as_str).unwrap_or("");
                let topics = log
                    .get("topics")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if topics.len() < 3
                    || !topics[0]
                        .as_str()
                        .unwrap_or("")
                        .eq_ignore_ascii_case(ERC20_TRANSFER_TOPIC)
                    || !is_evm_address(token)
                {
                    continue;
                }
                let from_addr = format!(
                    "0x{}",
                    &topics[1].as_str().unwrap_or("").trim_start_matches("0x")[24..]
                );
                let to_addr = format!(
                    "0x{}",
                    &topics[2].as_str().unwrap_or("").trim_start_matches("0x")[24..]
                );
                let incoming = to_addr.eq_ignore_ascii_case(address);
                let outgoing = from_addr.eq_ignore_ascii_case(address);
                if !incoming && !outgoing {
                    continue;
                }
                let data = log.get("data").and_then(Value::as_str).unwrap_or("0x");
                let amount = parse_hex_quantity(data).unwrap_or(0);
                if amount == 0 {
                    continue;
                }
                // Prefer an incoming token for BUY and an outgoing token for SELL;
                // the first qualifying delta is still receipt-backed, not balance-only.
                if selected.is_none() || incoming {
                    selected = Some((token.to_string(), incoming, amount));
                }
            }
            let Some((token, incoming, raw_amount)) = selected else {
                continue;
            };
            let decimals = self.erc20_uint(&token, "0x313ce567").await.unwrap_or(18);
            let symbol = self
                .erc20_string(&token, "0x95d89b41")
                .await
                .unwrap_or_else(|_| "TOKEN".to_string());
            if crate::models::is_blacklisted_asset(&token, &symbol) {
                continue;
            }
            let token_amount = raw_amount as f64 / 10f64.powi(decimals as i32);
            let now = Utc::now();
            let activity = WalletActivity {
                id: None,
                wallet_handle: handle.to_string(),
                wallet_address: address.to_string(),
                chain: self.chain,
                activity_type: if incoming {
                    ActivityType::Buy
                } else {
                    ActivityType::Sell
                },
                token_symbol: symbol,
                token_mint: token,
                amount_usd: 0.0,
                token_amount,
                execution_price_usd: 0.0,
                whale_balance_before: 0.0,
                whale_balance_after: 0.0,
                sell_ratio_pct: if incoming { 0.0 } else { 100.0 },
                tx_signature: tx_hash.clone(),
                timestamp: now,
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

            // Skip transactions already processed for this address; treat
            // pre-startup (older than 120s) transactions as historical data.
            if self.processed_hashes.lock().unwrap().contains(hash) {
                return Ok(None);
            }
            if tx_age_secs > 120 {
                let mut processed = self.processed_hashes.lock().unwrap();
                processed.insert(hash.to_string());
                return Ok(None);
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
        if self.rpc_urls.is_empty() {
            anyhow::bail!("no Robinhood RPC endpoints configured");
        }
        let payload = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        let endpoint_count = self.rpc_urls.len();
        let start = self.rpc_index.fetch_add(1, Ordering::Relaxed) % endpoint_count;
        let mut last_error = String::from("all Robinhood RPC endpoints unavailable");
        for offset in 0..endpoint_count {
            let index = (start + offset) % endpoint_count;
            let now = Instant::now();
            let available = self.rpc_cooldowns.lock().unwrap()[index]
                .map(|until| until <= now)
                .unwrap_or(true);
            if !available {
                continue;
            }
            self.requests.fetch_add(1, Ordering::Relaxed);
            let response = match self
                .client
                .post(&self.rpc_urls[index])
                .json(&payload)
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    self.failures.fetch_add(1, Ordering::Relaxed);
                    last_error = format!("endpoint {index} transport failed: {error}");
                    self.rpc_cooldowns.lock().unwrap()[index] =
                        Some(Instant::now() + Duration::from_secs(15));
                    continue;
                }
            };
            if response.status().as_u16() == 429 {
                self.rate_limited.fetch_add(1, Ordering::Relaxed);
                last_error = format!("endpoint {index} rate-limited (HTTP 429)");
                self.rpc_cooldowns.lock().unwrap()[index] =
                    Some(Instant::now() + Duration::from_secs(30));
                continue;
            }
            if !response.status().is_success() {
                self.failures.fetch_add(1, Ordering::Relaxed);
                last_error = format!("endpoint {index} returned HTTP {}", response.status());
                self.rpc_cooldowns.lock().unwrap()[index] =
                    Some(Instant::now() + Duration::from_secs(15));
                continue;
            }
            let response: Value = match response.json().await {
                Ok(value) => value,
                Err(error) => {
                    self.failures.fetch_add(1, Ordering::Relaxed);
                    last_error = format!("endpoint {index} returned invalid JSON: {error}");
                    continue;
                }
            };
            if let Some(error) = response.get("error") {
                self.failures.fetch_add(1, Ordering::Relaxed);
                last_error = format!("endpoint {index} RPC {method} failed: {error}");
                continue;
            }
            self.successes.fetch_add(1, Ordering::Relaxed);
            return Ok(response.get("result").cloned().unwrap_or(Value::Null));
        }
        anyhow::bail!("RPC {method} failed after endpoint rotation: {last_error}")
    }

    async fn erc20_uint(&self, token: &str, selector: &str) -> Result<u8> {
        let value = self
            .rpc_call("eth_call", json!([{"to":token,"data":selector},"latest"]))
            .await?;
        Ok(parse_hex_quantity(value.as_str().unwrap_or("0x0"))?.min(255) as u8)
    }

    async fn erc20_string(&self, token: &str, selector: &str) -> Result<String> {
        let value = self
            .rpc_call("eth_call", json!([{"to":token,"data":selector},"latest"]))
            .await?;
        let raw = value
            .as_str()
            .unwrap_or("")
            .strip_prefix("0x")
            .unwrap_or("");
        if raw.len() < 128 {
            return Err(anyhow::anyhow!("ERC20 string output too short"));
        }
        let offset = usize::from_str_radix(&raw[..64], 16)? * 2;
        if raw.len() < offset + 64 {
            return Err(anyhow::anyhow!("ERC20 string offset invalid"));
        }
        let len = usize::from_str_radix(&raw[offset..offset + 64], 16)? * 2;
        if raw.len() < offset + 64 + len {
            return Err(anyhow::anyhow!("ERC20 string length invalid"));
        }
        Ok(
            String::from_utf8_lossy(&hex::decode(&raw[offset + 64..offset + 64 + len])?)
                .to_string(),
        )
    }
}

#[cfg(test)]
mod feed_tests {
    use super::*;

    #[test]
    fn parses_official_relay_envelope() {
        let raw = r#"{"version":1,"messages":[{"sequenceNumber":7,"message":{"message":{"header":{"kind":3,"sender":"0xa4b000000000000000000073657175656e636572","blockNumber":12,"timestamp":1700000000,"requestId":null,"baseFeeL1":null},"l2Msg":"aGVsbG8="},"delayedMessagesRead":4},"signature":null}]}"#;
        let payload = EvmStreamListener::parse_sequencer_feed_payload(raw).unwrap();
        assert_eq!(payload.messages[0].sequence_number, 7);
        assert_eq!(
            payload.messages[0].message.message.decode_l2_msg().unwrap(),
            b"hello"
        );
    }

    #[test]
    fn rejects_unsupported_relay_version() {
        assert!(
            EvmStreamListener::parse_sequencer_feed_payload(r#"{"version":2,"messages":[]}"#)
                .is_err()
        );
    }

    #[tokio::test]
    async fn feed_reader_rejects_non_websocket_url_without_connecting() {
        let listener = EvmStreamListener::new("http://127.0.0.1:1", Chain::Robinhood);
        let error = listener
            .read_sequencer_feed("https://example.invalid/feed", 1)
            .await;
        assert!(error.is_err());
    }

    #[tokio::test]
    async fn feed_reader_zero_bound_is_read_only_noop() {
        let listener = EvmStreamListener::new("", Chain::Robinhood);
        let result = listener
            .read_sequencer_feed("wss://example.invalid/feed", 0)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn classifies_nitro_l2_message_kind() {
        use base64::Engine;
        let encoded = |kind: u8| base64::engine::general_purpose::STANDARD.encode([kind, 1]);
        let message = |kind| NitroIncomingMessage {
            header: None,
            l2_msg: encoded(kind),
        };
        assert_eq!(
            message(4).l2_message_kind().unwrap(),
            NitroL2MessageKind::SignedTx
        );
        assert_eq!(
            message(3).l2_message_kind().unwrap(),
            NitroL2MessageKind::Batch
        );
        assert_eq!(
            message(99).l2_message_kind().unwrap(),
            NitroL2MessageKind::Unknown(99)
        );
    }

    #[test]
    fn rejects_empty_nitro_l2_message() {
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: String::new(),
        };
        assert!(message.decode_l2_msg().is_err());
    }

    #[test]
    fn decodes_signed_eip1559_nitro_transaction_fixture() {
        use base64::Engine;
        fn item(value: &[u8]) -> Vec<u8> {
            let mut out = vec![0x80 + value.len() as u8];
            out.extend_from_slice(value);
            out
        }
        let mut fields = Vec::new();
        fields.extend(item(&[0x12, 0x37]));
        fields.extend(item(&[1]));
        fields.extend(item(&[2]));
        fields.extend(item(&[3]));
        fields.extend(item(&[0x52, 0x08]));
        fields.extend(item(&[0x11; 20]));
        fields.extend(item(&[4]));
        fields.extend(item(&[0xab]));
        fields.push(0xc0);
        let mut rlp = vec![0xc0 + fields.len() as u8];
        rlp.extend(fields);
        let mut raw = vec![4u8, 2u8];
        raw.extend(rlp);
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        let tx = message.decode_signed_transaction().unwrap();
        assert_eq!(tx.tx_type, 2);
        assert_eq!(tx.chain_id, 0x1237);
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.gas_limit, 0x5208);
        assert_eq!(tx.max_priority_fee_per_gas, Some(2));
        assert_eq!(tx.max_fee_per_gas, Some(3));
        assert_eq!(tx.value, 4);
        assert_eq!(tx.data, vec![0xab]);
        assert_eq!(
            tx.to.as_deref(),
            Some("0x1111111111111111111111111111111111111111")
        );
    }

    #[test]
    fn decodes_unsigned_kind_zero_fixture() {
        use base64::Engine;
        fn word(value: u128) -> Vec<u8> {
            let mut out = vec![0u8; 32];
            out[16..].copy_from_slice(&value.to_be_bytes());
            out
        }
        let mut raw = vec![0u8];
        raw.extend(word(21_000));
        raw.extend(word(7));
        raw.extend(word(9));
        let mut to = vec![0u8; 32];
        to[12..].fill(0x22);
        raw.extend(to);
        raw.extend(word(123));
        raw.extend([0xde, 0xad, 0xbe, 0xef]);
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        let tx = message.decode_unsigned_transaction().unwrap();
        assert_eq!(tx.gas_limit, 21_000);
        assert_eq!(tx.max_fee_per_gas, 7);
        assert_eq!(tx.nonce, 9);
        assert_eq!(
            tx.to.as_deref(),
            Some("0x2222222222222222222222222222222222222222")
        );
        assert_eq!(tx.value, 123);
        assert_eq!(tx.data, vec![0xde, 0xad, 0xbe, 0xef]);
    }

    #[test]
    fn decodes_nested_batch_kind_three_fixture() {
        use base64::Engine;
        fn frame(message: &[u8]) -> Vec<u8> {
            let mut out = (message.len() as u64).to_be_bytes().to_vec();
            out.extend_from_slice(message);
            out
        }
        let nested = [3u8]
            .into_iter()
            .chain(frame(&[0u8, 0xaa]))
            .collect::<Vec<_>>();
        let raw = [vec![3u8], frame(&[4u8, 0xbb]), frame(&nested)].concat();
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        let batch = message.decode_batch().unwrap();
        assert_eq!(batch.messages, vec![vec![4, 0xbb], vec![0, 0xaa]]);
    }

    #[test]
    fn rejects_truncated_batch_frame() {
        use base64::Engine;
        let raw = [3u8, 0, 0, 0, 0, 0, 0, 0];
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        assert!(message.decode_batch().is_err());
    }

    #[test]
    fn rejects_batch_nesting_over_depth_limit() {
        use base64::Engine;
        let mut nested = vec![4u8, 0];
        for _ in 0..17 {
            nested = [vec![3u8], {
                let mut framed = (nested.len() as u64).to_be_bytes().to_vec();
                framed.extend_from_slice(&nested);
                framed
            }]
            .concat();
        }
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(nested),
        };
        assert!(message.decode_batch().is_err());
    }

    #[test]
    fn decodes_contract_kind_one_fixture() {
        use base64::Engine;
        fn word(value: u128) -> Vec<u8> {
            let mut out = vec![0u8; 32];
            out[16..].copy_from_slice(&value.to_be_bytes());
            out
        }
        let mut raw = vec![1u8];
        raw.extend(word(50_000));
        raw.extend(word(11));
        let mut to = vec![0u8; 32];
        to[12..].fill(0x33);
        raw.extend(to);
        raw.extend(word(456));
        raw.extend([0xca, 0xfe]);
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        let tx = message.decode_contract_transaction().unwrap();
        assert_eq!(tx.gas_limit, 50_000);
        assert_eq!(tx.max_fee_per_gas, 11);
        assert_eq!(
            tx.to.as_deref(),
            Some("0x3333333333333333333333333333333333333333")
        );
        assert_eq!(tx.value, 456);
        assert_eq!(tx.data, vec![0xca, 0xfe]);
    }

    #[test]
    fn rejects_truncated_contract_kind_one_field() {
        use base64::Engine;
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode([1u8; 64]),
        };
        assert!(message.decode_contract_transaction().is_err());
    }

    #[test]
    fn rejects_wrong_kind_for_contract_decoder() {
        use base64::Engine;
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode([0u8; 129]),
        };
        assert!(message.decode_contract_transaction().is_err());
    }

    #[test]
    fn rejects_truncated_unsigned_kind_zero_field() {
        use base64::Engine;
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode([0u8; 96]),
        };
        assert!(message.decode_unsigned_transaction().is_err());
    }

    #[test]
    fn rejects_unsigned_kind_zero_integer_overflow() {
        use base64::Engine;
        let mut raw = vec![0u8];
        raw.extend([0u8; 32]);
        let mut oversized = vec![0u8; 32];
        oversized[0] = 1;
        raw.extend(oversized);
        let message = NitroIncomingMessage {
            header: None,
            l2_msg: base64::engine::general_purpose::STANDARD.encode(raw),
        };
        assert!(message.decode_unsigned_transaction().is_err());
    }
}
