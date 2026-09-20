use crate::radar::{
    reduce_portfolios_to_radar, OnChainTokenEvidence, RadarMarketSnapshot, RadarPolicy,
    RadarProjection,
};
use crate::radar_activity::{classify_activity, deduplicate_activities, NormalizedActivity};
use crate::radar_attribution::{attribute_activity, AddressAttribution};
use crate::radar_portfolio::{
    reduce_portfolio_observations, PortfolioObservation, PortfolioSnapshot,
};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct RadarPipelineOutput {
    pub projections: Vec<RadarProjection>,
    pub portfolio_snapshots: Vec<PortfolioSnapshot>,
}

/// Orchestrates only in-memory Radar stages. It has no database or executor side effects.
pub fn run_read_only_pipeline(
    evidence: &[OnChainTokenEvidence],
    activities: &[NormalizedActivity],
    attributions: &[AddressAttribution],
    markets: &HashMap<(crate::models::Chain, String), RadarMarketSnapshot>,
    policy: RadarPolicy,
) -> Result<RadarPipelineOutput> {
    let unique_activities = deduplicate_activities(activities.iter().cloned())?;
    let mut observations = Vec::new();
    for activity in &unique_activities {
        let classified = classify_activity(activity)?;
        let attributed = attribute_activity(activity, classified.kind, attributions)?;
        observations.push(PortfolioObservation {
            activity: attributed,
            balance_before: activity.balance_before,
            balance_after: activity.balance_after,
            observed_at: activity.observed_at,
        });
    }
    let portfolio_snapshots = reduce_portfolio_observations(&observations)?;
    let projections = reduce_portfolios_to_radar(evidence, markets, policy)?;
    Ok(RadarPipelineOutput {
        projections,
        portfolio_snapshots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Chain;
    use crate::radar_activity::VerifiedVenue;
    use crate::radar_attribution::{build_attributions, RelationKind, VerifiedRelation};
    use chrono::Utc;

    fn evidence(qualified: bool, wallet: &str) -> OnChainTokenEvidence {
        OnChainTokenEvidence {
            chain: Chain::Solana,
            token_address: "Mint111".to_string(),
            wallet_address: wallet.to_string(),
            kind: crate::radar::RadarEvidenceKind::TokenHolding,
            tx_id: None,
            observed_at: Utc::now(),
            cluster_id: Some("cluster-1".to_string()),
            qualified_wallet: qualified,
        }
    }

    #[test]
    fn pipeline_keeps_radar_and_portfolio_outputs_separate() {
        let activity = NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "child".to_string(),
            token_address: "Mint111".to_string(),
            tx_id: "sig-1".to_string(),
            event_index: 1,
            venue: Some(VerifiedVenue::SolanaJupiter),
            pool_address: Some("Pool111".to_string()),
            quote_asset: Some("USDC".to_string()),
            token_in_amount: 100,
            token_out_amount: 200,
            balance_before: 10,
            balance_after: 20,
            recipient: Some("child".to_string()),
            observed_at: Utc::now(),
        };
        let relation = VerifiedRelation {
            chain: Chain::Solana,
            from_address: "child".to_string(),
            to_address: "vault".to_string(),
            relation: RelationKind::ChildToVault,
            tx_id: "relation-1".to_string(),
            event_index: 1,
            block_or_slot: "slot-1".to_string(),
            cluster_id: Some("cluster-1".to_string()),
        };
        let attributions = build_attributions(&[relation]).unwrap();
        let mut markets = HashMap::new();
        markets.insert(
            (Chain::Solana, "Mint111".to_string()),
            RadarMarketSnapshot {
                symbol: Some("TEST".to_string()),
                pool_address: Some("Pool111".to_string()),
                dex_id: Some("test-dex".to_string()),
                market_cap_usd: 1_000_000.0,
                liquidity_usd: 50_000.0,
                price_usd: None,
                change_24h_pct: None,
                change_6h_pct: None,
                change_1h_pct: None,
                market_structure: "UNAVAILABLE_OHLCV".to_string(),
                price_action: "UNAVAILABLE_OHLCV".to_string(),
                observed_at: Utc::now(),
            },
        );
        let output = run_read_only_pipeline(
            &[evidence(true, "child")],
            &[activity],
            &attributions,
            &markets,
            RadarPolicy {
                min_market_cap_usd: 50_000.0,
                max_market_cap_usd: 20_000_000.0,
                min_liquidity_usd: 30_000.0,
            },
        )
        .unwrap();
        assert_eq!(output.projections.len(), 1);
        assert_eq!(
            output.projections[0].status,
            crate::radar::RadarStatus::PortfolioOnly
        );
        assert_eq!(output.portfolio_snapshots.len(), 1);
        assert_eq!(output.portfolio_snapshots[0].swap_count, 1);
    }

    #[test]
    fn pipeline_deduplicates_same_transaction_event() {
        let activity = NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "wallet".to_string(),
            token_address: "Mint111".to_string(),
            tx_id: "sig-duplicate".to_string(),
            event_index: 1,
            venue: Some(VerifiedVenue::SolanaJupiter),
            pool_address: Some("Pool111".to_string()),
            quote_asset: Some("USDC".to_string()),
            token_in_amount: 10,
            token_out_amount: 20,
            balance_before: 5,
            balance_after: 25,
            recipient: Some("wallet".to_string()),
            observed_at: chrono::Utc::now(),
        };
        let output = run_read_only_pipeline(
            &[],
            &[activity.clone(), activity],
            &[],
            &HashMap::new(),
            RadarPolicy {
                min_market_cap_usd: 1.0,
                max_market_cap_usd: 2.0,
                min_liquidity_usd: 1.0,
            },
        )
        .unwrap();
        assert_eq!(output.portfolio_snapshots.len(), 1);
        assert_eq!(output.portfolio_snapshots[0].swap_count, 1);
        assert_eq!(output.portfolio_snapshots[0].net_flow, 20);
    }

    #[test]
    fn pipeline_omits_token_without_market_snapshot() {
        let output = run_read_only_pipeline(
            &[evidence(true, "wallet")],
            &[],
            &[],
            &HashMap::new(),
            RadarPolicy {
                min_market_cap_usd: 50_000.0,
                max_market_cap_usd: 20_000_000.0,
                min_liquidity_usd: 30_000.0,
            },
        )
        .unwrap();
        assert!(output.projections.is_empty());
    }

    #[test]
    fn pipeline_rejects_malformed_activity() {
        let mut activity = NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "wallet".to_string(),
            token_address: "Mint111".to_string(),
            tx_id: String::new(),
            event_index: 0,
            venue: None,
            pool_address: None,
            quote_asset: None,
            token_in_amount: 0,
            token_out_amount: 1,
            balance_before: 0,
            balance_after: 1,
            recipient: Some("wallet".to_string()),
            observed_at: Utc::now(),
        };
        activity.tx_id.clear();
        assert!(run_read_only_pipeline(
            &[],
            &[activity],
            &[],
            &HashMap::new(),
            RadarPolicy {
                min_market_cap_usd: 1.0,
                max_market_cap_usd: 2.0,
                min_liquidity_usd: 1.0,
            }
        )
        .is_err());
    }
}
