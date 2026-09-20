use crate::models::Chain;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadarEvidenceKind {
    DirectSwap,
    TokenHolding,
    TokenReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnChainTokenEvidence {
    pub chain: Chain,
    pub token_address: String,
    pub wallet_address: String,
    pub kind: RadarEvidenceKind,
    pub tx_id: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub cluster_id: Option<String>,
    pub qualified_wallet: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadarMarketSnapshot {
    pub symbol: Option<String>,
    pub pool_address: Option<String>,
    pub dex_id: Option<String>,
    pub market_cap_usd: f64,
    pub liquidity_usd: f64,
    pub price_usd: Option<f64>,
    pub change_24h_pct: Option<f64>,
    pub change_6h_pct: Option<f64>,
    pub change_1h_pct: Option<f64>,
    pub market_structure: String,
    pub price_action: String,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadarStatus {
    PortfolioOnly,
    StructureConfirmed,
    HighPriorityRadar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadarProjection {
    pub chain: Chain,
    pub token_address: String,
    pub symbol: Option<String>,
    pub pool_address: Option<String>,
    pub dex_id: Option<String>,
    pub market_cap_usd: f64,
    pub liquidity_usd: f64,
    pub price_usd: Option<f64>,
    pub change_24h_pct: Option<f64>,
    pub change_6h_pct: Option<f64>,
    pub change_1h_pct: Option<f64>,
    pub market_structure: String,
    pub price_action: String,
    pub qualified_holder_count: usize,
    pub candidate_holder_count: usize,
    pub independent_cluster_count: usize,
    pub direct_accumulator_count: usize,
    pub status: RadarStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarPolicy {
    pub min_market_cap_usd: f64,
    pub max_market_cap_usd: f64,
    pub min_liquidity_usd: f64,
}

impl RadarPolicy {
    pub fn validate(&self) -> Result<()> {
        if !self.min_market_cap_usd.is_finite()
            || !self.max_market_cap_usd.is_finite()
            || !self.min_liquidity_usd.is_finite()
            || self.min_market_cap_usd <= 0.0
            || self.max_market_cap_usd < self.min_market_cap_usd
            || self.min_liquidity_usd <= 0.0
        {
            bail!("radar market policy invalid");
        }
        Ok(())
    }
}

fn market_chain_id(chain: Chain) -> &'static str {
    match chain {
        Chain::Solana => "solana",
        Chain::Robinhood => "robinhood",
        Chain::Arbitrum => "arbitrum",
        Chain::Base => "base",
        Chain::Ethereum => "ethereum",
    }
}

fn finite_number(value: Option<&Value>, field: &str) -> Result<f64> {
    let number = match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("{field} is not a finite number"))?,
        Some(Value::String(text)) => text
            .parse::<f64>()
            .map_err(|_| anyhow::anyhow!("{field} is not numeric"))?,
        _ => bail!("{field} missing"),
    };
    if !number.is_finite() || number < 0.0 {
        bail!("{field} is invalid");
    }
    Ok(number)
}

/// Parse market data only for an already verified chain/token identity.
/// No FDV or synthetic liquidity fallback is accepted.
pub fn parse_market_snapshot(
    chain: Chain,
    token_address: &str,
    response: &Value,
    observed_at: DateTime<Utc>,
) -> Result<Option<RadarMarketSnapshot>> {
    let token = token_address.trim();
    if token.is_empty() {
        bail!("market token identity missing");
    }
    let pairs = response
        .get("pairs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("market response missing pairs"))?;
    let mut best: Option<RadarMarketSnapshot> = None;
    for pair in pairs {
        if pair.get("chainId").and_then(Value::as_str) != Some(market_chain_id(chain)) {
            continue;
        }
        let base = pair
            .get("baseToken")
            .and_then(|value| value.get("address"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let token_matches = match chain {
            Chain::Solana => base == token,
            Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
                base.eq_ignore_ascii_case(token)
            }
        };
        if !token_matches {
            continue;
        }
        let market_cap_usd = finite_number(pair.get("marketCap"), "marketCap")?;
        let pool_address = pair
            .get("pairAddress")
            .and_then(Value::as_str)
            .map(str::to_string);
        let dex_id = pair
            .get("dexId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let symbol = pair
            .get("baseToken")
            .and_then(|value| value.get("symbol"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let price_change = |period: &str| {
            pair.get("priceChange")
                .and_then(|value| value.get(period))
                .and_then(|value| finite_number(Some(value), "priceChange").ok())
        };
        let change_24h_pct = price_change("h24");
        let change_6h_pct = price_change("h6");
        let change_1h_pct = price_change("h1");
        let liquidity_usd = finite_number(
            pair.get("liquidity").and_then(|value| value.get("usd")),
            "liquidity.usd",
        )?;
        let price_usd = match pair.get("priceUsd") {
            None | Some(Value::Null) => None,
            Some(value) => {
                Some(finite_number(Some(value), "priceUsd")?).filter(|price| *price > 0.0)
            }
        };
        if best
            .as_ref()
            .map(|current| liquidity_usd > current.liquidity_usd)
            .unwrap_or(true)
        {
            best = Some(RadarMarketSnapshot {
                symbol,
                pool_address,
                dex_id,
                market_cap_usd,
                liquidity_usd,
                price_usd,
                change_24h_pct,
                change_6h_pct,
                change_1h_pct,
                market_structure: "UNAVAILABLE_OHLCV".to_string(),
                price_action: "UNAVAILABLE_OHLCV".to_string(),
                observed_at,
            });
        }
    }
    Ok(best)
}

pub fn reduce_portfolios_to_radar(
    evidence: &[OnChainTokenEvidence],
    markets: &HashMap<(Chain, String), RadarMarketSnapshot>,
    policy: RadarPolicy,
) -> Result<Vec<RadarProjection>> {
    let mut grouped: HashMap<(Chain, String), Vec<OnChainTokenEvidence>> = HashMap::new();
    for item in evidence {
        let token = match item.chain {
            Chain::Solana => item.token_address.trim().to_string(),
            Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
                item.token_address.trim().to_ascii_lowercase()
            }
        };
        if token.is_empty() {
            bail!("radar evidence token identity missing");
        }
        grouped
            .entry((item.chain, token))
            .or_default()
            .push(item.clone());
    }
    let mut projections = Vec::new();
    for ((chain, token), items) in grouped {
        if let Some(market) = markets.get(&(chain, token)) {
            if let Some(projection) = reduce_portfolio_to_radar(&items, market, policy)? {
                projections.push(projection);
            }
        }
    }
    projections.sort_by(|left, right| {
        right
            .qualified_holder_count
            .cmp(&left.qualified_holder_count)
            .then_with(|| right.liquidity_usd.total_cmp(&left.liquidity_usd))
    });
    Ok(projections)
}

pub fn reduce_portfolio_to_radar(
    evidence: &[OnChainTokenEvidence],
    market: &RadarMarketSnapshot,
    policy: RadarPolicy,
) -> Result<Option<RadarProjection>> {
    policy.validate()?;
    if !market.market_cap_usd.is_finite()
        || !market.liquidity_usd.is_finite()
        || market.market_cap_usd < policy.min_market_cap_usd
        || market.market_cap_usd > policy.max_market_cap_usd
        || market.liquidity_usd < policy.min_liquidity_usd
    {
        return Ok(None);
    }
    if let Some(price) = market.price_usd {
        if !price.is_finite() || price <= 0.0 {
            bail!("radar price invalid");
        }
    }
    let first = match evidence.first() {
        Some(item) => item,
        None => return Ok(None),
    };
    let canonical_token = match first.chain {
        Chain::Solana => first.token_address.trim().to_string(),
        Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
            first.token_address.trim().to_ascii_lowercase()
        }
    };
    if canonical_token.is_empty() {
        bail!("radar evidence token identity missing");
    }
    if evidence.iter().any(|item| {
        let token = match item.chain {
            Chain::Solana => item.token_address.trim().to_string(),
            Chain::Robinhood | Chain::Arbitrum | Chain::Base | Chain::Ethereum => {
                item.token_address.trim().to_ascii_lowercase()
            }
        };
        item.chain != first.chain || token.is_empty() || token != canonical_token
    }) {
        bail!("radar evidence token identity mismatch");
    }

    let mut qualified_holders = HashSet::new();
    let mut candidate_holders = HashSet::new();
    let mut clusters = HashSet::new();
    let mut direct_accumulators = HashSet::new();
    for item in evidence {
        let wallet = item.wallet_address.trim().to_ascii_lowercase();
        if wallet.is_empty() {
            bail!("radar evidence wallet identity missing");
        }
        if item.qualified_wallet {
            qualified_holders.insert(wallet.clone());
        }
        if let Some(cluster) = &item.cluster_id {
            if !cluster.trim().is_empty() {
                clusters.insert(cluster.trim().to_string());
            }
        }
        if item.kind == RadarEvidenceKind::DirectSwap {
            direct_accumulators.insert(wallet);
        }
    }
    for item in evidence.iter().filter(|item| !item.qualified_wallet) {
        let wallet = item.wallet_address.trim().to_ascii_lowercase();
        if !qualified_holders.contains(&wallet) {
            candidate_holders.insert(wallet);
        }
    }

    let status = if market.price_usd.is_some() && !direct_accumulators.is_empty() {
        RadarStatus::HighPriorityRadar
    } else if market.price_usd.is_some() {
        RadarStatus::StructureConfirmed
    } else {
        RadarStatus::PortfolioOnly
    };
    Ok(Some(RadarProjection {
        chain: first.chain,
        token_address: canonical_token,
        symbol: market.symbol.clone(),
        pool_address: market.pool_address.clone(),
        dex_id: market.dex_id.clone(),
        market_cap_usd: market.market_cap_usd,
        liquidity_usd: market.liquidity_usd,
        price_usd: market.price_usd,
        change_24h_pct: market.change_24h_pct,
        change_6h_pct: market.change_6h_pct,
        change_1h_pct: market.change_1h_pct,
        market_structure: market.market_structure.clone(),
        price_action: market.price_action.clone(),
        qualified_holder_count: qualified_holders.len(),
        candidate_holder_count: candidate_holders.len(),
        independent_cluster_count: clusters.len(),
        direct_accumulator_count: direct_accumulators.len(),
        status,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(kind: RadarEvidenceKind, wallet: &str, qualified: bool) -> OnChainTokenEvidence {
        OnChainTokenEvidence {
            chain: Chain::Solana,
            token_address: "Mint111".to_string(),
            wallet_address: wallet.to_string(),
            kind,
            tx_id: Some("sig-1".to_string()),
            observed_at: Utc::now(),
            cluster_id: Some(format!("cluster-{wallet}")),
            qualified_wallet: qualified,
        }
    }

    #[test]
    fn market_parser_requires_exact_chain_and_base_token() {
        let response = serde_json::json!({
            "pairs": [
                {"chainId": "base", "baseToken": {"address": "Mint111"}, "marketCap": 900000, "liquidity": {"usd": 90000}},
                {"chainId": "solana", "baseToken": {"address": "Other111"}, "marketCap": 1000000, "liquidity": {"usd": 100000}}
            ]
        });
        assert!(
            parse_market_snapshot(Chain::Solana, "Mint111", &response, Utc::now())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn market_parser_selects_most_liquid_valid_pool_without_price() {
        let response = serde_json::json!({
            "pairs": [
                {"chainId": "solana", "baseToken": {"address": "Mint111"}, "marketCap": 1000000, "liquidity": {"usd": 40000}, "priceUsd": "0.10"},
                {"chainId": "solana", "baseToken": {"address": "Mint111"}, "marketCap": "1200000", "liquidity": {"usd": "70000"}}
            ]
        });
        let snapshot = parse_market_snapshot(Chain::Solana, "Mint111", &response, Utc::now())
            .unwrap()
            .unwrap();
        assert_eq!(snapshot.liquidity_usd, 70000.0);
        assert_eq!(snapshot.price_usd, None);
    }

    #[test]
    fn market_parser_does_not_use_fdv_or_synthetic_liquidity() {
        let response = serde_json::json!({
            "pairs": [{"chainId": "solana", "baseToken": {"address": "Mint111"}, "fdv": 1000000, "liquidity": {"usd": 0}}]
        });
        assert!(parse_market_snapshot(Chain::Solana, "Mint111", &response, Utc::now()).is_err());
    }

    fn market(price_usd: Option<f64>) -> RadarMarketSnapshot {
        RadarMarketSnapshot {
            symbol: Some("TEST".to_string()),
            pool_address: Some("Pool111".to_string()),
            dex_id: Some("test-dex".to_string()),
            market_cap_usd: 1_000_000.0,
            liquidity_usd: 50_000.0,
            price_usd,
            change_24h_pct: None,
            change_6h_pct: None,
            change_1h_pct: None,
            market_structure: "UNAVAILABLE_OHLCV".to_string(),
            price_action: "UNAVAILABLE_OHLCV".to_string(),
            observed_at: Utc::now(),
        }
    }

    fn policy() -> RadarPolicy {
        RadarPolicy {
            min_market_cap_usd: 50_000.0,
            max_market_cap_usd: 20_000_000.0,
            min_liquidity_usd: 30_000.0,
        }
    }

    #[test]
    fn batch_reducer_groups_by_chain_token_and_omits_missing_market() {
        let mut markets = HashMap::new();
        markets.insert((Chain::Solana, "Mint111".to_string()), market(Some(0.1)));
        let evidence_items = vec![
            evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true),
            evidence(RadarEvidenceKind::TokenHolding, "wallet-b", true),
            evidence(RadarEvidenceKind::TokenHolding, "wallet-c", false),
            {
                let mut item = evidence(RadarEvidenceKind::TokenHolding, "wallet-d", true);
                item.token_address = "MissingMarket".to_string();
                item
            },
        ];
        let result = reduce_portfolios_to_radar(&evidence_items, &markets, policy()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].token_address, "Mint111");
        assert_eq!(result[0].qualified_holder_count, 2);
        assert_eq!(result[0].candidate_holder_count, 1);
    }

    #[test]
    fn price_missing_still_allows_portfolio_only() {
        let result = reduce_portfolio_to_radar(
            &[evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true)],
            &market(None),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.status, RadarStatus::PortfolioOnly);
        assert_eq!(result.qualified_holder_count, 1);
        assert_eq!(result.price_usd, None);
    }

    #[test]
    fn direct_swap_with_price_is_high_priority_but_not_entry() {
        let result = reduce_portfolio_to_radar(
            &[evidence(RadarEvidenceKind::DirectSwap, "wallet-a", true)],
            &market(Some(0.25)),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.status, RadarStatus::HighPriorityRadar);
        assert_eq!(result.direct_accumulator_count, 1);
    }

    #[test]
    fn cap_or_liquidity_failure_excludes_token() {
        let mut snapshot = market(None);
        snapshot.liquidity_usd = 29_999.0;
        assert!(reduce_portfolio_to_radar(
            &[evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true)],
            &snapshot,
            policy(),
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn solana_token_identity_rejects_case_mismatch() {
        let mut second = evidence(RadarEvidenceKind::TokenHolding, "wallet-b", true);
        second.token_address = "mInT111".to_string();
        let result = reduce_portfolio_to_radar(
            &[
                evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true),
                second,
            ],
            &market(None),
            policy(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn evm_token_identity_is_case_insensitive() {
        let mut first = evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true);
        first.chain = Chain::Robinhood;
        first.token_address = "0xAbC".to_string();
        let mut second = first.clone();
        second.wallet_address = "wallet-b".to_string();
        second.token_address = "0xabc".to_string();
        let result = reduce_portfolio_to_radar(&[first, second], &market(None), policy())
            .unwrap()
            .unwrap();
        assert_eq!(result.token_address, "0xabc");
    }

    #[test]
    fn qualified_wallet_precedence_prevents_double_counting() {
        let result = reduce_portfolio_to_radar(
            &[
                evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true),
                evidence(RadarEvidenceKind::TokenHolding, "wallet-a", false),
                evidence(RadarEvidenceKind::TokenHolding, "wallet-b", false),
            ],
            &market(None),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.qualified_holder_count, 1);
        assert_eq!(result.candidate_holder_count, 1);
    }

    #[test]
    fn candidate_and_qualified_holders_are_counted_separately() {
        let result = reduce_portfolio_to_radar(
            &[
                evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true),
                evidence(RadarEvidenceKind::TokenHolding, "wallet-b", false),
            ],
            &market(None),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.qualified_holder_count, 1);
        assert_eq!(result.candidate_holder_count, 1);
        assert_eq!(result.independent_cluster_count, 2);
    }
}
