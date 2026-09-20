use crate::models::Chain;
use crate::radar_attribution::{AttributedActivity, ExposureKind};
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortfolioState {
    Holding,
    Accumulating,
    Distributing,
    ConsolidatingToVault,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioObservation {
    pub activity: AttributedActivity,
    pub balance_before: u128,
    pub balance_after: u128,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortfolioSnapshot {
    pub chain: Chain,
    pub root_address: String,
    pub token_address: String,
    pub balance_before: u128,
    pub balance_after: u128,
    pub net_flow: i128,
    pub swap_count: u32,
    pub transfer_count: u32,
    pub consolidated_transfer_count: u32,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub state: PortfolioState,
}

fn checked_delta(before: u128, after: u128) -> Result<i128> {
    if after >= before {
        i128::try_from(after - before)
            .map_err(|_| anyhow::anyhow!("positive balance delta overflow"))
    } else {
        i128::try_from(before - after)
            .map(|delta| -delta)
            .map_err(|_| anyhow::anyhow!("negative balance delta overflow"))
    }
}

pub fn reduce_portfolio_observations(
    observations: &[PortfolioObservation],
) -> Result<Vec<PortfolioSnapshot>> {
    let mut grouped: HashMap<(Chain, String, String), Vec<&PortfolioObservation>> = HashMap::new();
    for observation in observations {
        if observation.activity.root_address.trim().is_empty()
            || observation.activity.token_address.trim().is_empty()
            || observation.activity.tx_id.trim().is_empty()
        {
            bail!("portfolio observation identity is incomplete");
        }
        grouped
            .entry((
                observation.activity.chain,
                observation.activity.root_address.clone(),
                observation.activity.token_address.clone(),
            ))
            .or_default()
            .push(observation);
    }

    let mut snapshots = Vec::new();
    for ((chain, root_address, token_address), mut items) in grouped {
        items.sort_by_key(|item| item.observed_at);
        let first = items.first().expect("group cannot be empty");
        let last = items.last().expect("group cannot be empty");
        let mut net_flow = 0i128;
        let mut swap_count = 0u32;
        let mut transfer_count = 0u32;
        let mut consolidated_transfer_count = 0u32;
        for item in &items {
            net_flow = net_flow
                .checked_add(checked_delta(item.balance_before, item.balance_after)?)
                .ok_or_else(|| anyhow::anyhow!("portfolio net flow overflow"))?;
            match item.activity.kind {
                ExposureKind::DirectActivity => swap_count = swap_count.saturating_add(1),
                ExposureKind::ConsolidatedExposure => {
                    transfer_count = transfer_count.saturating_add(1);
                    consolidated_transfer_count = consolidated_transfer_count.saturating_add(1);
                }
                ExposureKind::ObservationOnly => transfer_count = transfer_count.saturating_add(1),
            }
        }
        let state = if consolidated_transfer_count > 0 {
            PortfolioState::ConsolidatingToVault
        } else if net_flow > 0 && swap_count > 0 {
            PortfolioState::Accumulating
        } else if net_flow < 0 && swap_count > 0 {
            PortfolioState::Distributing
        } else if first.balance_after > 0 || last.balance_after > 0 {
            PortfolioState::Holding
        } else {
            PortfolioState::Unknown
        };
        snapshots.push(PortfolioSnapshot {
            chain,
            root_address,
            token_address,
            balance_before: first.balance_before,
            balance_after: last.balance_after,
            net_flow,
            swap_count,
            transfer_count,
            consolidated_transfer_count,
            first_seen: first.observed_at,
            last_seen: last.observed_at,
            state,
        });
    }
    snapshots.sort_by(|left, right| {
        left.chain
            .to_string()
            .cmp(&right.chain.to_string())
            .then_with(|| left.root_address.cmp(&right.root_address))
            .then_with(|| left.token_address.cmp(&right.token_address))
    });
    Ok(snapshots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(kind: ExposureKind, before: u128, after: u128, at: i64) -> PortfolioObservation {
        PortfolioObservation {
            activity: AttributedActivity {
                kind,
                chain: Chain::Solana,
                wallet_address: "child".to_string(),
                root_address: "vault".to_string(),
                token_address: "Mint111".to_string(),
                tx_id: format!("sig-{at}"),
                event_index: 1,
            },
            balance_before: before,
            balance_after: after,
            observed_at: DateTime::from_timestamp(at, 0).unwrap(),
        }
    }

    #[test]
    fn direct_swap_with_positive_delta_is_accumulating() {
        let result =
            reduce_portfolio_observations(&[observation(ExposureKind::DirectActivity, 10, 20, 1)])
                .unwrap();
        assert_eq!(result[0].state, PortfolioState::Accumulating);
        assert_eq!(result[0].net_flow, 10);
        assert_eq!(result[0].swap_count, 1);
    }

    #[test]
    fn child_to_vault_is_consolidation_not_new_swap() {
        let result = reduce_portfolio_observations(&[observation(
            ExposureKind::ConsolidatedExposure,
            20,
            20,
            1,
        )])
        .unwrap();
        assert_eq!(result[0].state, PortfolioState::ConsolidatingToVault);
        assert_eq!(result[0].swap_count, 0);
        assert_eq!(result[0].consolidated_transfer_count, 1);
    }

    #[test]
    fn observation_only_holding_is_not_accumulation() {
        let result =
            reduce_portfolio_observations(&[observation(ExposureKind::ObservationOnly, 0, 50, 1)])
                .unwrap();
        assert_eq!(result[0].state, PortfolioState::Holding);
        assert_eq!(result[0].swap_count, 0);
    }

    #[test]
    fn incomplete_observation_is_rejected() {
        let mut input = observation(ExposureKind::DirectActivity, 0, 1, 1);
        input.activity.tx_id.clear();
        assert!(reduce_portfolio_observations(&[input]).is_err());
    }
}
