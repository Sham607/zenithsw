use crate::models::Chain;
use crate::radar_activity::{ActivityKind, NormalizedActivity};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    ParentChild,
    ChildToVault,
    ClusterMember,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedRelation {
    pub chain: Chain,
    pub from_address: String,
    pub to_address: String,
    pub relation: RelationKind,
    pub tx_id: String,
    pub event_index: u32,
    pub block_or_slot: String,
    pub cluster_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionRole {
    DirectExecutor,
    Child,
    MasterVault,
    Satellite,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressAttribution {
    pub chain: Chain,
    pub address: String,
    pub root_address: String,
    pub role: AttributionRole,
    pub cluster_id: Option<String>,
}

fn valid(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_whitespace)
}

fn key(chain: Chain, address: &str) -> (Chain, String) {
    let address = match chain {
        Chain::Solana => address.trim().to_string(),
        Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
            address.trim().to_ascii_lowercase()
        }
    };
    (chain, address)
}

pub fn build_attributions(relations: &[VerifiedRelation]) -> Result<Vec<AddressAttribution>> {
    let mut parent_of: HashMap<(Chain, String), (Chain, String)> = HashMap::new();
    let mut role: HashMap<(Chain, String), AttributionRole> = HashMap::new();
    let mut clusters: HashMap<(Chain, String), String> = HashMap::new();
    let mut seen_edges = HashSet::new();

    for relation in relations {
        if !valid(&relation.from_address)
            || !valid(&relation.to_address)
            || !valid(&relation.tx_id)
            || !valid(&relation.block_or_slot)
        {
            bail!("verified relation provenance is incomplete");
        }
        let from = key(relation.chain, &relation.from_address);
        let to = key(relation.chain, &relation.to_address);
        if from == to {
            bail!("verified relation cannot point to itself");
        }
        if !seen_edges.insert((relation.chain, relation.tx_id.clone(), relation.event_index)) {
            continue;
        }
        match relation.relation {
            RelationKind::ParentChild | RelationKind::ChildToVault => {
                if parent_of.insert(from.clone(), to.clone()).is_some() {
                    bail!("address has conflicting verified attribution parent");
                }
                role.insert(from.clone(), AttributionRole::Child);
                role.entry(to.clone())
                    .or_insert(AttributionRole::MasterVault);
            }
            RelationKind::ClusterMember => {
                role.entry(from.clone())
                    .or_insert(AttributionRole::Satellite);
                role.entry(to.clone()).or_insert(AttributionRole::Satellite);
            }
        }
        if let Some(cluster_id) = relation.cluster_id.as_deref().filter(|id| valid(id)) {
            clusters.insert(from, cluster_id.to_string());
            clusters.insert(to, cluster_id.to_string());
        }
    }

    let addresses: HashSet<(Chain, String)> = role.keys().cloned().collect();
    let mut output = Vec::new();
    for address in addresses {
        let mut current = address.clone();
        let mut path = HashSet::new();
        while let Some(parent) = parent_of.get(&current) {
            if !path.insert(current.clone()) {
                bail!("verified attribution graph contains a cycle");
            }
            current = parent.clone();
        }
        output.push(AddressAttribution {
            chain: address.0,
            address: address.1.clone(),
            root_address: current.1,
            role: *role.get(&address).unwrap_or(&AttributionRole::Unknown),
            cluster_id: clusters.get(&address).cloned(),
        });
    }
    output.sort_by(|left, right| {
        left.chain
            .to_string()
            .cmp(&right.chain.to_string())
            .then_with(|| left.address.cmp(&right.address))
    });
    Ok(output)
}

pub fn independent_exposure_count(attributions: &[AddressAttribution]) -> usize {
    attributions
        .iter()
        .map(|item| (item.chain, item.root_address.clone()))
        .collect::<HashSet<_>>()
        .len()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExposureKind {
    DirectActivity,
    ConsolidatedExposure,
    ObservationOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedActivity {
    pub kind: ExposureKind,
    pub chain: Chain,
    pub wallet_address: String,
    pub root_address: String,
    pub token_address: String,
    pub tx_id: String,
    pub event_index: u32,
}

pub fn attribute_activity(
    activity: &NormalizedActivity,
    classified_kind: ActivityKind,
    attributions: &[AddressAttribution],
) -> Result<AttributedActivity> {
    let wallet = key(activity.chain, &activity.wallet_address);
    let attribution = attributions
        .iter()
        .find(|item| key(item.chain, &item.address) == wallet);
    let root_address = attribution
        .map(|item| item.root_address.clone())
        .unwrap_or_else(|| activity.wallet_address.clone());
    let kind = match (classified_kind, attribution.map(|item| item.role)) {
        (ActivityKind::DirectSwap, _) => ExposureKind::DirectActivity,
        (ActivityKind::TokenReceipt, Some(AttributionRole::Child))
        | (ActivityKind::InternalTransfer, Some(AttributionRole::Child)) => {
            ExposureKind::ConsolidatedExposure
        }
        (_, _) => ExposureKind::ObservationOnly,
    };
    if !valid(&activity.tx_id) || !valid(&activity.token_address) {
        bail!("activity provenance is incomplete");
    }
    Ok(AttributedActivity {
        kind,
        chain: activity.chain,
        wallet_address: activity.wallet_address.clone(),
        root_address,
        token_address: activity.token_address.clone(),
        tx_id: activity.tx_id.clone(),
        event_index: activity.event_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::radar_activity::VerifiedVenue;
    use chrono::Utc;

    fn activity(kind: ActivityKind) -> NormalizedActivity {
        NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "child".to_string(),
            token_address: "Mint111".to_string(),
            tx_id: "activity-sig".to_string(),
            event_index: 4,
            venue: (kind == ActivityKind::DirectSwap).then_some(VerifiedVenue::SolanaJupiter),
            pool_address: None,
            quote_asset: (kind == ActivityKind::DirectSwap).then(|| "USDC".to_string()),
            token_in_amount: if kind == ActivityKind::DirectSwap {
                100
            } else {
                0
            },
            token_out_amount: 100,
            balance_before: 10,
            balance_after: 20,
            recipient: Some("vault".to_string()),
            observed_at: Utc::now(),
        }
    }

    fn relation(relation: RelationKind, from: &str, to: &str, index: u32) -> VerifiedRelation {
        VerifiedRelation {
            chain: Chain::Solana,
            from_address: from.to_string(),
            to_address: to.to_string(),
            relation,
            tx_id: format!("sig-{index}"),
            event_index: index,
            block_or_slot: "slot-1".to_string(),
            cluster_id: Some("cluster-1".to_string()),
        }
    }

    #[test]
    fn child_receipt_is_consolidated_exposure() {
        let attributions =
            build_attributions(&[relation(RelationKind::ChildToVault, "child", "vault", 1)])
                .unwrap();
        let result = attribute_activity(
            &activity(ActivityKind::TokenReceipt),
            ActivityKind::TokenReceipt,
            &attributions,
        )
        .unwrap();
        assert_eq!(result.kind, ExposureKind::ConsolidatedExposure);
        assert_eq!(result.root_address, "vault");
    }

    #[test]
    fn direct_swap_remains_direct_activity() {
        let attributions =
            build_attributions(&[relation(RelationKind::ChildToVault, "child", "vault", 1)])
                .unwrap();
        let result = attribute_activity(
            &activity(ActivityKind::DirectSwap),
            ActivityKind::DirectSwap,
            &attributions,
        )
        .unwrap();
        assert_eq!(result.kind, ExposureKind::DirectActivity);
    }

    #[test]
    fn unrelated_activity_is_observation_only() {
        let result = attribute_activity(
            &activity(ActivityKind::TokenReceipt),
            ActivityKind::TokenReceipt,
            &[],
        )
        .unwrap();
        assert_eq!(result.kind, ExposureKind::ObservationOnly);
        assert_eq!(result.root_address, "child");
    }

    #[test]
    fn child_to_vault_has_one_independent_exposure() {
        let attributions =
            build_attributions(&[relation(RelationKind::ChildToVault, "child", "vault", 1)])
                .unwrap();
        assert_eq!(attributions.len(), 2);
        assert_eq!(independent_exposure_count(&attributions), 1);
        assert_eq!(
            attributions
                .iter()
                .find(|item| item.address == "child")
                .unwrap()
                .role,
            AttributionRole::Child
        );
    }

    #[test]
    fn duplicate_provenance_edge_is_ignored() {
        let edge = relation(RelationKind::ChildToVault, "child", "vault", 1);
        assert_eq!(build_attributions(&[edge.clone(), edge]).unwrap().len(), 2);
    }

    #[test]
    fn conflicting_parent_is_rejected() {
        assert!(build_attributions(&[
            relation(RelationKind::ParentChild, "child", "vault-a", 1),
            relation(RelationKind::ParentChild, "child", "vault-b", 2),
        ])
        .is_err());
    }

    #[test]
    fn cycle_is_rejected() {
        assert!(build_attributions(&[
            relation(RelationKind::ParentChild, "a", "b", 1),
            relation(RelationKind::ParentChild, "b", "a", 2),
        ])
        .is_err());
    }
}
