use crate::models::Chain;
use crate::radar_activity::NormalizedActivity;
use anyhow::{bail, Result};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaTransactionReference {
    pub chain: Chain,
    pub wallet_address: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaTokenBalanceDelta {
    pub chain: Chain,
    pub wallet_address: String,
    pub signature: String,
    pub slot: u64,
    pub token_address: String,
    pub balance_before: u128,
    pub balance_after: u128,
    pub delta: i128,
    pub observed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaVenueInstructionEvidence {
    pub venue: crate::radar_activity::VerifiedVenue,
    pub instruction_index: u32,
    pub program_id: String,
    /// Pool hanya diisi oleh decoder venue-specific yang memiliki layout tervalidasi.
    pub pool_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaInnerVenueInstructionEvidence {
    pub venue: crate::radar_activity::VerifiedVenue,
    pub parent_instruction_index: u32,
    pub inner_instruction_index: u32,
    pub program_id: String,
}

const JUPITER_V6_PROGRAM_ID: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
const RAYDIUM_AMM_V4_PROGRAM_ID: &str = "675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
const RAYDIUM_CPMM_PROGRAM_ID: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
const RAYDIUM_CLMM_PROGRAM_ID: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
const PUMPFUN_PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPSWAP_PROGRAM_ID: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";

fn venue_for_program_id(program_id: &str) -> Option<crate::radar_activity::VerifiedVenue> {
    match program_id {
        JUPITER_V6_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaJupiter),
        RAYDIUM_AMM_V4_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaRaydiumAmm),
        RAYDIUM_CPMM_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaRaydiumCpmm),
        RAYDIUM_CLMM_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaRaydiumClmm),
        PUMPFUN_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaPumpFun),
        PUMPSWAP_PROGRAM_ID => Some(crate::radar_activity::VerifiedVenue::SolanaPumpSwap),
        _ => None,
    }
}

/// Mengekstrak pool AMM v4 hanya dari layout account Raydium yang dipin.
/// Tag 9 adalah legacy SwapBaseIn (17/18 account), tag 16 adalah
/// SwapBaseInV2 (8 account). Layout lain tidak dianggap sebagai swap.
fn raydium_amm_v4_pool_from_instruction(instruction: &Value) -> Result<Option<String>> {
    let program_id = instruction
        .get("programId")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("instruction programId missing"))?;
    if program_id != RAYDIUM_AMM_V4_PROGRAM_ID {
        return Ok(None);
    }
    let data = instruction
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Raydium instruction data missing"))?;
    let decoded = decode_base58(data)?;
    let tag = *decoded
        .first()
        .ok_or_else(|| anyhow::anyhow!("Raydium instruction data empty"))?;
    let accounts = instruction
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Raydium instruction accounts missing"))?;
    let pool_index = match (tag, accounts.len()) {
        (9, 17 | 18) | (11, 17 | 18) => 1,
        (16 | 17, 8) => 1,
        _ => return Ok(None),
    };
    let pool = accounts
        .get(pool_index)
        .and_then(|account| {
            account
                .as_str()
                .or_else(|| account.get("pubkey").and_then(Value::as_str))
        })
        .ok_or_else(|| anyhow::anyhow!("Raydium pool account is not a pubkey"))?;
    validate_identity(pool, "Raydium pool")?;
    Ok(Some(pool.to_string()))
}

fn decode_base58(value: &str) -> Result<Vec<u8>> {
    if value.is_empty() {
        bail!("base58 data kosong");
    }
    let alphabet = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut bytes = vec![0u8];
    for character in value.bytes() {
        let digit = alphabet
            .iter()
            .position(|item| *item == character)
            .ok_or_else(|| anyhow::anyhow!("base58 data invalid"))? as u32;
        let mut carry = digit;
        for byte in bytes.iter_mut().rev() {
            let value = u32::from(*byte) * 58 + carry;
            *byte = (value & 0xff) as u8;
            carry = value >> 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let leading_zeroes = value.bytes().take_while(|byte| *byte == b'1').count();
    let mut output = vec![0u8; leading_zeroes];
    output.extend(bytes.into_iter().skip_while(|byte| *byte == 0));
    Ok(output)
}

pub fn parse_transaction_venue_instructions(
    response: &Value,
    signature: &str,
) -> Result<Vec<SolanaVenueInstructionEvidence>> {
    validate_identity(signature, "signature")?;
    let result = response
        .get("result")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("transaction result missing"))?;
    let transaction = result
        .get("transaction")
        .ok_or_else(|| anyhow::anyhow!("transaction payload missing"))?;
    let signatures = transaction
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("transaction signatures missing"))?;
    if !signatures
        .iter()
        .any(|item| item.as_str() == Some(signature))
    {
        bail!("transaction signature does not match requested signature");
    }
    let instructions = transaction
        .get("message")
        .and_then(|message| message.get("instructions"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("transaction instructions missing"))?;
    let mut evidence = Vec::new();
    for (index, instruction) in instructions.iter().enumerate() {
        let program_id = instruction
            .get("programId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("instruction programId missing"))?;
        if let Some(venue) = venue_for_program_id(program_id) {
            let pool_address = if venue == crate::radar_activity::VerifiedVenue::SolanaRaydiumAmm
                && instruction.get("data").is_some()
                && instruction.get("accounts").is_some()
            {
                raydium_amm_v4_pool_from_instruction(instruction)?
            } else {
                None
            };
            evidence.push(SolanaVenueInstructionEvidence {
                venue,
                instruction_index: u32::try_from(index)
                    .map_err(|_| anyhow::anyhow!("instruction index overflow"))?,
                program_id: program_id.to_string(),
                pool_address,
            });
        }
    }
    Ok(evidence)
}

pub fn reconcile_verified_swap(
    deltas: &[SolanaTokenBalanceDelta],
    venues: &[SolanaVenueInstructionEvidence],
    token_address: &str,
    quote_asset: &str,
) -> Result<NormalizedActivity> {
    validate_identity(token_address, "token")?;
    validate_identity(quote_asset, "quote asset")?;
    if deltas.is_empty() || venues.is_empty() {
        bail!("swap evidence is incomplete");
    }
    let first = deltas
        .first()
        .ok_or_else(|| anyhow::anyhow!("delta missing"))?;
    if deltas.iter().any(|item| {
        item.chain != Chain::Solana || item.signature != first.signature || item.slot != first.slot
    }) {
        bail!("swap delta provenance does not match");
    }
    let token = deltas
        .iter()
        .filter(|item| item.token_address == token_address && item.delta > 0)
        .collect::<Vec<_>>();
    let quote = deltas
        .iter()
        .filter(|item| item.token_address == quote_asset && item.delta < 0)
        .collect::<Vec<_>>();
    if token.len() != 1 || quote.len() != 1 || deltas.len() != 2 {
        bail!("swap requires exactly one positive token and one negative quote delta");
    }
    let venue = venues
        .first()
        .ok_or_else(|| anyhow::anyhow!("venue evidence missing"))?;
    Ok(NormalizedActivity {
        chain: Chain::Solana,
        wallet_address: first.wallet_address.clone(),
        token_address: token_address.to_string(),
        tx_id: first.signature.clone(),
        event_index: venue.instruction_index,
        venue: Some(venue.venue),
        pool_address: venue.pool_address.clone(),
        quote_asset: Some(quote_asset.to_string()),
        token_in_amount: quote[0].delta.unsigned_abs(),
        token_out_amount: token[0].delta as u128,
        balance_before: token[0].balance_before,
        balance_after: token[0].balance_after,
        recipient: Some(first.wallet_address.clone()),
        observed_at: first.observed_at.unwrap_or_else(Utc::now),
    })
}

pub fn parse_inner_venue_instructions(
    response: &Value,
    signature: &str,
) -> Result<Vec<SolanaInnerVenueInstructionEvidence>> {
    validate_identity(signature, "signature")?;
    let result = response
        .get("result")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("transaction result missing"))?;
    let transaction = result
        .get("transaction")
        .ok_or_else(|| anyhow::anyhow!("transaction payload missing"))?;
    let signatures = transaction
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("transaction signatures missing"))?;
    if !signatures
        .iter()
        .any(|item| item.as_str() == Some(signature))
    {
        bail!("transaction signature does not match requested signature");
    }
    let meta = result
        .get("meta")
        .ok_or_else(|| anyhow::anyhow!("transaction meta missing"))?;
    if !meta.get("err").is_some_and(Value::is_null) {
        bail!("failed transaction has no valid inner activity evidence");
    }
    let groups = meta
        .get("innerInstructions")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("inner instructions missing"))?;
    let mut evidence = Vec::new();
    for group in groups {
        let parent = group
            .get("index")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("inner instruction parent index missing"))?;
        let instructions = group
            .get("instructions")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("inner instruction list missing"))?;
        for (index, instruction) in instructions.iter().enumerate() {
            let program_id = instruction
                .get("programId")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("inner instruction programId missing"))?;
            if let Some(venue) = venue_for_program_id(program_id) {
                evidence.push(SolanaInnerVenueInstructionEvidence {
                    venue,
                    parent_instruction_index: u32::try_from(parent)
                        .map_err(|_| anyhow::anyhow!("parent instruction index overflow"))?,
                    inner_instruction_index: u32::try_from(index)
                        .map_err(|_| anyhow::anyhow!("inner instruction index overflow"))?,
                    program_id: program_id.to_string(),
                });
            }
        }
    }
    Ok(evidence)
}

pub fn build_transaction_request(signature: &str, request_id: u64) -> Result<Value> {
    validate_identity(signature, "signature")?;
    Ok(json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "getTransaction",
        "params": [signature, {"encoding": "jsonParsed", "commitment": "confirmed", "maxSupportedTransactionVersion": 0}]
    }))
}

pub fn parse_transaction_token_deltas(
    response: &Value,
    owner: &str,
    signature: &str,
) -> Result<Vec<SolanaTokenBalanceDelta>> {
    validate_identity(owner, "owner")?;
    validate_identity(signature, "signature")?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("invalid JSON-RPC version");
    }
    if response.get("error").is_some() {
        bail!("transaction RPC returned an error");
    }
    let result = response
        .get("result")
        .filter(|value| !value.is_null())
        .ok_or_else(|| anyhow::anyhow!("transaction result missing"))?;
    let slot = result
        .get("slot")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("transaction slot missing or invalid"))?;
    if slot == 0 {
        bail!("transaction slot invalid");
    }
    let transaction = result
        .get("transaction")
        .ok_or_else(|| anyhow::anyhow!("transaction payload missing"))?;
    let signatures = transaction
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("transaction signatures missing"))?;
    if !signatures
        .iter()
        .any(|item| item.as_str() == Some(signature))
    {
        bail!("transaction signature does not match requested signature");
    }
    let meta = result
        .get("meta")
        .ok_or_else(|| anyhow::anyhow!("transaction meta missing"))?;
    if !meta.get("err").is_some_and(Value::is_null) {
        bail!("failed transaction is not activity evidence");
    }
    let pre = parse_balance_entries(meta.get("preTokenBalances"), owner)?;
    let post = parse_balance_entries(meta.get("postTokenBalances"), owner)?;
    let mut keys = pre.keys().cloned().collect::<HashSet<_>>();
    keys.extend(post.keys().cloned());
    let observed_at = match result.get("blockTime") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let timestamp = value
                .as_i64()
                .ok_or_else(|| anyhow::anyhow!("blockTime invalid"))?;
            Some(
                Utc.timestamp_opt(timestamp, 0)
                    .single()
                    .ok_or_else(|| anyhow::anyhow!("blockTime out of range"))?,
            )
        }
    };
    let mut deltas = Vec::new();
    for (account_index, token_address) in keys {
        let before = pre
            .get(&(account_index, token_address.clone()))
            .copied()
            .unwrap_or(0);
        let after = post
            .get(&(account_index, token_address.clone()))
            .copied()
            .unwrap_or(0);
        let delta = if after >= before {
            i128::try_from(after - before)
                .map_err(|_| anyhow::anyhow!("positive token delta overflow"))?
        } else {
            -i128::try_from(before - after)
                .map_err(|_| anyhow::anyhow!("negative token delta overflow"))?
        };
        if delta == 0 {
            continue;
        }
        deltas.push(SolanaTokenBalanceDelta {
            chain: Chain::Solana,
            wallet_address: owner.to_string(),
            signature: signature.to_string(),
            slot,
            token_address,
            balance_before: before,
            balance_after: after,
            delta,
            observed_at,
        });
    }
    Ok(deltas)
}

fn parse_balance_entries(
    value: Option<&Value>,
    owner: &str,
) -> Result<HashMap<(u64, String), u128>> {
    let entries = value
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("token balance array missing"))?;
    let mut balances = HashMap::new();
    for entry in entries {
        if entry.get("owner").and_then(Value::as_str) != Some(owner) {
            continue;
        }
        let account_index = entry
            .get("accountIndex")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("token account index missing or invalid"))?;
        let mint = entry
            .get("mint")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("token balance mint missing"))?;
        validate_identity(mint, "mint")?;
        let amount = entry
            .get("uiTokenAmount")
            .and_then(|item| item.get("amount"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("token balance amount missing"))?
            .parse::<u128>()
            .map_err(|_| anyhow::anyhow!("token balance amount invalid"))?;
        if balances
            .insert((account_index, mint.to_string()), amount)
            .is_some()
        {
            bail!("duplicate token balance account entry");
        }
    }
    Ok(balances)
}

pub fn build_signatures_for_address_request(
    owner: &str,
    limit: u64,
    request_id: u64,
) -> Result<Value> {
    validate_identity(owner, "owner")?;
    if limit == 0 || limit > 1_000 {
        bail!("signature history limit must be between 1 and 1000");
    }
    Ok(json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "getSignaturesForAddress",
        "params": [owner, {"limit": limit}]
    }))
}

pub fn parse_signature_references(
    response: &Value,
    owner: &str,
) -> Result<Vec<SolanaTransactionReference>> {
    validate_identity(owner, "owner")?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("invalid JSON-RPC version");
    }
    if response.get("error").is_some() {
        bail!("signature history RPC returned an error");
    }
    let values = response
        .get("result")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("signature history result missing"))?;
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    for value in values {
        let signature = value
            .get("signature")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("signature missing"))?;
        let slot = value
            .get("slot")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("signature slot missing or invalid"))?;
        if signature.trim().is_empty() || slot == 0 {
            bail!("signature reference identity is invalid");
        }
        if !seen.insert(signature.to_string()) {
            continue;
        }
        let block_time = match value.get("blockTime") {
            None | Some(Value::Null) => None,
            Some(item) => {
                let timestamp = item
                    .as_i64()
                    .ok_or_else(|| anyhow::anyhow!("blockTime invalid"))?;
                Some(
                    Utc.timestamp_opt(timestamp, 0)
                        .single()
                        .ok_or_else(|| anyhow::anyhow!("blockTime out of range"))?,
                )
            }
        };
        references.push(SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: owner.to_string(),
            signature: signature.to_string(),
            slot,
            block_time,
        });
    }
    Ok(references)
}

fn validate_identity(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
        bail!("{field} identity is invalid");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(
        mint: &str,
        value_before: u128,
        value_after: u128,
        delta: i128,
    ) -> SolanaTokenBalanceDelta {
        SolanaTokenBalanceDelta {
            chain: Chain::Solana,
            wallet_address: "Wallet111".to_string(),
            signature: "Sig111".to_string(),
            slot: 42,
            token_address: mint.to_string(),
            balance_before: value_before,
            balance_after: value_after,
            delta,
            observed_at: None,
        }
    }

    fn venue() -> SolanaVenueInstructionEvidence {
        SolanaVenueInstructionEvidence {
            venue: crate::radar_activity::VerifiedVenue::SolanaJupiter,
            instruction_index: 1,
            program_id: JUPITER_V6_PROGRAM_ID.to_string(),
            pool_address: None,
        }
    }

    #[test]
    fn reconciler_builds_direct_activity_from_exact_token_quote_pair() {
        let activity = reconcile_verified_swap(
            &[delta("Mint111", 10, 25, 15), delta("USDC", 1000, 850, -150)],
            &[venue()],
            "Mint111",
            "USDC",
        )
        .unwrap();
        assert_eq!(activity.token_in_amount, 150);
        assert_eq!(activity.token_out_amount, 15);
        assert_eq!(activity.event_index, 1);
        assert_eq!(
            activity.venue,
            Some(crate::radar_activity::VerifiedVenue::SolanaJupiter)
        );
    }

    #[test]
    fn reconciler_rejects_extra_delta_or_mismatched_provenance() {
        assert!(reconcile_verified_swap(
            &[
                delta("Mint111", 10, 25, 15),
                delta("USDC", 1000, 850, -150),
                delta("Fee", 1, 0, -1)
            ],
            &[venue()],
            "Mint111",
            "USDC",
        )
        .is_err());
        let mut mismatched = delta("USDC", 1000, 850, -150);
        mismatched.slot = 43;
        assert!(reconcile_verified_swap(
            &[delta("Mint111", 10, 25, 15), mismatched],
            &[venue()],
            "Mint111",
            "USDC",
        )
        .is_err());
    }

    #[test]
    fn inner_venue_parser_binds_parent_and_inner_indexes() {
        let response = json!({
            "jsonrpc": "2.0", "result": {
                "transaction": {"signatures": ["Sig111"]},
                "meta": {"err": null, "innerInstructions": [
                    {"index": 2, "instructions": [
                        {"programId": "unknown-program"},
                        {"programId": "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"}
                    ]}
                ]}
            }
        });
        let evidence = parse_inner_venue_instructions(&response, "Sig111").unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            evidence[0].venue,
            crate::radar_activity::VerifiedVenue::SolanaPumpFun
        );
        assert_eq!(evidence[0].parent_instruction_index, 2);
        assert_eq!(evidence[0].inner_instruction_index, 1);
    }

    #[test]
    fn inner_venue_parser_rejects_failed_or_malformed_metadata() {
        let failed = json!({"jsonrpc": "2.0", "result": {"transaction": {"signatures": ["Sig111"]}, "meta": {"err": {"failed": true}, "innerInstructions": []}}});
        assert!(parse_inner_venue_instructions(&failed, "Sig111").is_err());
        let malformed = json!({"jsonrpc": "2.0", "result": {"transaction": {"signatures": ["Sig111"]}, "meta": {"err": null, "innerInstructions": [{"instructions": []}]}}});
        assert!(parse_inner_venue_instructions(&malformed, "Sig111").is_err());
    }

    #[test]
    fn raydium_amm_v4_decoder_extracts_pool_from_pinned_layouts() {
        let v2 = json!({
            "programId": RAYDIUM_AMM_V4_PROGRAM_ID,
            "data": "H",
            "accounts": ["Token111", "PoolV2", "Authority111", "VaultA", "VaultB", "UserIn", "UserOut", "Wallet111"]
        });
        assert_eq!(
            raydium_amm_v4_pool_from_instruction(&v2).unwrap(),
            Some("PoolV2".to_string())
        );

        let legacy = json!({
            "programId": RAYDIUM_AMM_V4_PROGRAM_ID,
            "data": "A",
            "accounts": ["Token111", "PoolLegacy", "Authority111", "OpenOrders", "TargetOrders", "VaultA", "VaultB", "Serum", "Market", "Bids", "Asks", "EventQueue", "CoinVault", "PcVault", "Signer", "UserIn", "UserOut"]
        });
        assert_eq!(
            raydium_amm_v4_pool_from_instruction(&legacy).unwrap(),
            Some("PoolLegacy".to_string())
        );
    }

    #[test]
    fn raydium_amm_v4_decoder_rejects_unproven_layout() {
        let wrong_account_count = json!({
            "programId": RAYDIUM_AMM_V4_PROGRAM_ID,
            "data": "2",
            "accounts": ["Token111", "Pool111"]
        });
        assert_eq!(
            raydium_amm_v4_pool_from_instruction(&wrong_account_count).unwrap(),
            None
        );

        let invalid_data = json!({
            "programId": RAYDIUM_AMM_V4_PROGRAM_ID,
            "data": "1",
            "accounts": ["Token111", "Pool111", "Authority111", "VaultA", "VaultB", "UserIn", "UserOut", "Wallet111"]
        });
        assert_eq!(
            raydium_amm_v4_pool_from_instruction(&invalid_data).unwrap(),
            None
        );

        let malformed_data = json!({
            "programId": RAYDIUM_AMM_V4_PROGRAM_ID,
            "data": "0!",
            "accounts": ["Token111", "Pool111", "Authority111", "VaultA", "VaultB", "UserIn", "UserOut", "Wallet111"]
        });
        assert!(raydium_amm_v4_pool_from_instruction(&malformed_data).is_err());
    }

    #[test]
    fn venue_parser_accepts_allowlisted_programs_and_skips_unknown() {
        let response = json!({
            "jsonrpc": "2.0", "result": {
                "transaction": {
                    "signatures": ["Sig111"],
                    "message": {"instructions": [
                        {"programId": "ComputeBudget111111111111111111111111111111"},
                        {"programId": "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"},
                        {"programId": "unknown-program"},
                        {"programId": "675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"}
                    ]}
                }
            }
        });
        let evidence = parse_transaction_venue_instructions(&response, "Sig111").unwrap();
        assert_eq!(evidence.len(), 2);
        assert_eq!(
            evidence[0].venue,
            crate::radar_activity::VerifiedVenue::SolanaJupiter
        );
        assert_eq!(
            evidence[1].venue,
            crate::radar_activity::VerifiedVenue::SolanaRaydiumAmm
        );
        assert_eq!(evidence[1].instruction_index, 3);
    }

    #[test]
    fn venue_parser_rejects_signature_mismatch_or_missing_program_id() {
        let mismatch = json!({"jsonrpc": "2.0", "result": {"transaction": {"signatures": ["Other"], "message": {"instructions": []}}}});
        assert!(parse_transaction_venue_instructions(&mismatch, "Sig111").is_err());
        let missing_program = json!({"jsonrpc": "2.0", "result": {"transaction": {"signatures": ["Sig111"], "message": {"instructions": [{}]}}}});
        assert!(parse_transaction_venue_instructions(&missing_program, "Sig111").is_err());
    }

    #[test]
    fn transaction_request_is_read_only_and_version_bounded() {
        let request = build_transaction_request("Sig111", 3).unwrap();
        assert_eq!(request["method"], "getTransaction");
        assert_eq!(request["params"][1]["encoding"], "jsonParsed");
        assert_eq!(request["params"][1]["maxSupportedTransactionVersion"], 0);
        assert!(build_transaction_request("", 3).is_err());
    }

    #[test]
    fn transaction_parser_extracts_owner_token_deltas_only() {
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": {
                "slot": 42, "blockTime": 1700000000,
                "transaction": {"signatures": ["Sig111"]},
                "meta": {
                    "err": null,
                    "preTokenBalances": [
                        {"accountIndex": 2, "mint": "Mint111", "owner": "Wallet111", "uiTokenAmount": {"amount": "100"}},
                        {"accountIndex": 3, "mint": "Other111", "owner": "OtherWallet", "uiTokenAmount": {"amount": "999"}}
                    ],
                    "postTokenBalances": [
                        {"accountIndex": 2, "mint": "Mint111", "owner": "Wallet111", "uiTokenAmount": {"amount": "250"}},
                        {"accountIndex": 4, "mint": "New111", "owner": "Wallet111", "uiTokenAmount": {"amount": "7"}}
                    ]
                }
            }
        });
        let deltas = parse_transaction_token_deltas(&response, "Wallet111", "Sig111").unwrap();
        assert_eq!(deltas.len(), 2);
        assert!(deltas
            .iter()
            .any(|item| item.token_address == "Mint111" && item.delta == 150));
        assert!(deltas
            .iter()
            .any(|item| item.token_address == "New111" && item.delta == 7));
        assert!(deltas.iter().all(|item| item.slot == 42));
    }

    #[test]
    fn transaction_parser_rejects_signature_mismatch() {
        let response = json!({
            "jsonrpc": "2.0", "result": {
                "slot": 42,
                "transaction": {"signatures": ["DifferentSig"]},
                "meta": {"err": null, "preTokenBalances": [], "postTokenBalances": []}
            }
        });
        assert!(parse_transaction_token_deltas(&response, "Wallet111", "Sig111").is_err());
    }

    #[test]
    fn transaction_parser_rejects_failed_or_incomplete_transaction() {
        let failed = json!({"jsonrpc": "2.0", "result": {"slot": 42, "transaction": {"signatures": ["Sig111"]}, "meta": {"err": {"custom": 1}, "preTokenBalances": [], "postTokenBalances": []}}});
        assert!(parse_transaction_token_deltas(&failed, "Wallet111", "Sig111").is_err());
        let incomplete = json!({"jsonrpc": "2.0", "result": {"slot": 42, "meta": {"err": null, "preTokenBalances": []}}});
        assert!(parse_transaction_token_deltas(&incomplete, "Wallet111", "Sig111").is_err());
    }

    #[test]
    fn request_is_read_only_and_bounded() {
        let request = build_signatures_for_address_request("Wallet111", 25, 9).unwrap();
        assert_eq!(request["method"], "getSignaturesForAddress");
        assert_eq!(request["params"][1]["limit"], 25);
        assert!(build_signatures_for_address_request("Wallet111", 0, 9).is_err());
        assert!(build_signatures_for_address_request("Wallet111", 1001, 9).is_err());
    }

    #[test]
    fn parser_preserves_slot_and_optional_block_time() {
        let response = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": [
                {"signature": "Sig111", "slot": 42, "blockTime": 1700000000},
                {"signature": "Sig111", "slot": 42, "blockTime": 1700000000},
                {"signature": "Sig222", "slot": 43, "blockTime": null}
            ]
        });
        let references = parse_signature_references(&response, "Wallet111").unwrap();
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].slot, 42);
        assert!(references[0].block_time.is_some());
        assert!(references[1].block_time.is_none());
    }

    #[test]
    fn parser_rejects_incomplete_or_failed_response() {
        let missing = json!({"jsonrpc": "2.0", "result": [{"signature": "Sig111"}]});
        assert!(parse_signature_references(&missing, "Wallet111").is_err());
        let failed = json!({"jsonrpc": "2.0", "error": {"code": -32000}});
        assert!(parse_signature_references(&failed, "Wallet111").is_err());
    }
}
