use chrono::Utc;
use std::collections::HashMap;
use zenithsw::db::{deduplicate_scan_addresses, WalletScanAddress};
use zenithsw::models::Chain;
use zenithsw::radar::{
    OnChainTokenEvidence, RadarEvidenceKind, RadarMarketSnapshot, RadarPolicy, RadarStatus,
};
use zenithsw::radar_activity::{NormalizedActivity, VerifiedVenue};
use zenithsw::radar_attribution::{build_attributions, RelationKind, VerifiedRelation};
use zenithsw::radar_pipeline::run_read_only_pipeline;

fn scan_address(address: &str, is_candidate: bool) -> WalletScanAddress {
    WalletScanAddress {
        chain: Chain::Solana,
        address: address.to_string(),
        source_db: if is_candidate {
            "candidate_wallet_registry.db"
        } else {
            "wallet_registry.db"
        },
        is_candidate,
        is_active: !is_candidate,
        tier: if is_candidate {
            "TIER-B".to_string()
        } else {
            "TIER-S".to_string()
        },
        execution_track: if is_candidate {
            "MONITOR_ONLY".to_string()
        } else {
            "QUALIFIED".to_string()
        },
    }
}

fn holding(address: &WalletScanAddress) -> OnChainTokenEvidence {
    OnChainTokenEvidence {
        chain: address.chain,
        token_address: "Mint111".to_string(),
        wallet_address: address.address.clone(),
        kind: RadarEvidenceKind::TokenHolding,
        tx_id: None,
        observed_at: Utc::now(),
        cluster_id: Some("cluster-1".to_string()),
        qualified_wallet: !address.is_candidate,
    }
}

fn market() -> RadarMarketSnapshot {
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
    }
}

#[test]
fn isolated_harness_preserves_registry_roles_and_produces_observatory_output() {
    let qualified = scan_address("Qualified111", false);
    let candidate = scan_address("Candidate111", true);
    let duplicate_qualified = scan_address("qualified111", false);
    let addresses =
        deduplicate_scan_addresses([qualified.clone(), candidate.clone(), duplicate_qualified]);

    assert_eq!(addresses.len(), 2);
    assert!(addresses.iter().any(|item| !item.is_candidate));
    assert!(addresses.iter().any(|item| item.is_candidate));
    assert!(addresses
        .iter()
        .filter(|item| item.is_candidate)
        .all(|item| item.execution_track == "MONITOR_ONLY"));

    let relation = VerifiedRelation {
        chain: Chain::Solana,
        from_address: "Candidate111".to_string(),
        to_address: "Vault111".to_string(),
        relation: RelationKind::ChildToVault,
        tx_id: "relation-signature-1".to_string(),
        event_index: 0,
        block_or_slot: "slot-100".to_string(),
        cluster_id: Some("cluster-1".to_string()),
    };
    let attributions = build_attributions(&[relation]).unwrap();
    let activity = NormalizedActivity {
        chain: Chain::Solana,
        wallet_address: "Candidate111".to_string(),
        token_address: "Mint111".to_string(),
        tx_id: "activity-signature-1".to_string(),
        event_index: 1,
        venue: Some(VerifiedVenue::SolanaJupiter),
        pool_address: Some("Pool111".to_string()),
        quote_asset: Some("USDC".to_string()),
        token_in_amount: 100,
        token_out_amount: 200,
        balance_before: 10,
        balance_after: 20,
        recipient: Some("Candidate111".to_string()),
        observed_at: Utc::now(),
    };
    let mut markets = HashMap::new();
    markets.insert((Chain::Solana, "Mint111".to_string()), market());

    let output = run_read_only_pipeline(
        &[holding(&qualified), holding(&candidate)],
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
    let projection = &output.projections[0];
    assert_eq!(projection.status, RadarStatus::PortfolioOnly);
    assert_eq!(projection.qualified_holder_count, 1);
    assert_eq!(projection.candidate_holder_count, 1);
    assert_eq!(projection.direct_accumulator_count, 0);
    assert_eq!(output.portfolio_snapshots.len(), 1);
    assert_eq!(output.portfolio_snapshots[0].swap_count, 1);
}

#[test]
fn isolated_harness_rejects_missing_market_without_entry_output() {
    let qualified = scan_address("Qualified222", false);
    let output = run_read_only_pipeline(
        &[holding(&qualified)],
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
    assert!(output.portfolio_snapshots.is_empty());
}
