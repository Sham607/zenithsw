use crate::models::Chain;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedVenue {
    SolanaJupiter,
    SolanaRaydiumAmm,
    SolanaRaydiumCpmm,
    SolanaRaydiumClmm,
    SolanaPumpFun,
    SolanaPumpSwap,
    RobinhoodPonsV2,
    RobinhoodUniswapV3,
    RobinhoodUniswapV4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    DirectSwap,
    TokenReceipt,
    FundingTransfer,
    LiquidityAction,
    InternalTransfer,
    UnknownActivity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedActivity {
    pub chain: Chain,
    pub wallet_address: String,
    pub token_address: String,
    pub tx_id: String,
    pub event_index: u32,
    pub venue: Option<VerifiedVenue>,
    pub pool_address: Option<String>,
    pub quote_asset: Option<String>,
    pub token_in_amount: u128,
    pub token_out_amount: u128,
    pub balance_before: u128,
    pub balance_after: u128,
    pub recipient: Option<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedActivity {
    pub kind: ActivityKind,
    pub chain: Chain,
    pub wallet_address: String,
    pub token_address: String,
    pub tx_id: String,
    pub event_index: u32,
    pub reason: &'static str,
    pub observed_at: DateTime<Utc>,
}

fn valid_identity(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
}

pub fn deduplicate_activities(
    activities: impl IntoIterator<Item = NormalizedActivity>,
) -> Result<Vec<NormalizedActivity>> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for activity in activities {
        classify_activity(&activity)?;
        let tx_id = match activity.chain {
            Chain::Solana => activity.tx_id.clone(),
            Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
                activity.tx_id.to_ascii_lowercase()
            }
        };
        if seen.insert((activity.chain, tx_id, activity.event_index)) {
            unique.push(activity);
        }
    }
    Ok(unique)
}

pub fn classify_activity(activity: &NormalizedActivity) -> Result<ClassifiedActivity> {
    if !valid_identity(&activity.wallet_address)
        || !valid_identity(&activity.token_address)
        || !valid_identity(&activity.tx_id)
    {
        bail!("activity identity is incomplete");
    }
    let venue = activity.venue;
    let quote_asset = activity
        .quote_asset
        .as_deref()
        .filter(|value| valid_identity(value));

    let (kind, reason) = if venue.is_some()
        && activity.pool_address.as_deref().is_some_and(valid_identity)
        && quote_asset.is_some()
        && activity.token_in_amount > 0
        && activity.token_out_amount > 0
    {
        (
            ActivityKind::DirectSwap,
            "verified venue and two-sided asset movement",
        )
    } else if activity.token_out_amount > 0 && activity.token_in_amount == 0 {
        if activity.recipient.as_deref() == Some(activity.wallet_address.as_str()) {
            (
                ActivityKind::TokenReceipt,
                "token received without verified swap input",
            )
        } else {
            (
                ActivityKind::InternalTransfer,
                "token movement has non-wallet recipient",
            )
        }
    } else if activity.token_in_amount > 0 && activity.token_out_amount == 0 {
        (
            ActivityKind::FundingTransfer,
            "token movement has no verified output",
        )
    } else {
        (
            ActivityKind::UnknownActivity,
            "swap or transfer direction is ambiguous",
        )
    };

    Ok(ClassifiedActivity {
        kind,
        chain: activity.chain,
        wallet_address: activity.wallet_address.clone(),
        token_address: activity.token_address.clone(),
        tx_id: activity.tx_id.clone(),
        event_index: activity.event_index,
        reason,
        observed_at: activity.observed_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity() -> NormalizedActivity {
        NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "Wallet111".to_string(),
            token_address: "Mint111".to_string(),
            tx_id: "Signature111".to_string(),
            event_index: 3,
            venue: Some(VerifiedVenue::SolanaJupiter),
            pool_address: Some("Pool111".to_string()),
            quote_asset: Some("USDC".to_string()),
            token_in_amount: 100,
            token_out_amount: 200,
            balance_before: 10,
            balance_after: 20,
            recipient: Some("Wallet111".to_string()),
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn deduplication_uses_chain_transaction_and_event_index() {
        let first = activity();
        let duplicate = first.clone();
        let mut second_event = first.clone();
        second_event.event_index = 4;
        let result = deduplicate_activities([first, duplicate, second_event]).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].event_index, 3);
        assert_eq!(result[1].event_index, 4);
    }

    #[test]
    fn deduplication_rejects_malformed_activity() {
        let mut input = activity();
        input.tx_id.clear();
        assert!(deduplicate_activities([input]).is_err());
    }

    #[test]
    fn two_sided_verified_venue_is_direct_swap() {
        let result = classify_activity(&activity()).unwrap();
        assert_eq!(result.kind, ActivityKind::DirectSwap);
        assert_eq!(result.event_index, 3);
    }

    #[test]
    fn transfer_without_venue_is_not_swap() {
        let mut input = activity();
        input.venue = None;
        input.quote_asset = None;
        input.token_in_amount = 0;
        let result = classify_activity(&input).unwrap();
        assert_eq!(result.kind, ActivityKind::TokenReceipt);
    }

    #[test]
    fn ambiguous_activity_is_unknown() {
        let mut input = activity();
        input.venue = None;
        input.quote_asset = None;
        input.token_in_amount = 100;
        input.token_out_amount = 100;
        let result = classify_activity(&input).unwrap();
        assert_eq!(result.kind, ActivityKind::UnknownActivity);
    }

    #[test]
    fn incomplete_provenance_is_rejected() {
        let mut input = activity();
        input.tx_id = "".to_string();
        assert!(classify_activity(&input).is_err());
    }
}
