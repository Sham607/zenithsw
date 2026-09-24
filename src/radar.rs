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

#[derive(Debug, Clone, PartialEq)]
pub struct OnChainTokenEvidence {
    pub chain: Chain,
    pub token_address: String,
    pub wallet_address: String,
    pub kind: RadarEvidenceKind,
    pub tx_id: Option<String>,
    pub observed_at: DateTime<Utc>,
    pub cluster_id: Option<String>,
    pub qualified_wallet: bool,
    pub ui_amount: Option<f64>,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OhlcvBar {
    pub timestamp: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
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
    pub whales: Option<String>,
    pub status: RadarStatus,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadarPolicy {
    pub min_market_cap_usd: f64,
    pub max_market_cap_usd: f64,
    pub min_liquidity_usd: f64,
    pub min_holding_value_usd: f64,
}

impl RadarPolicy {
    pub fn validate(&self) -> Result<()> {
        if !self.min_market_cap_usd.is_finite()
            || !self.max_market_cap_usd.is_finite()
            || !self.min_liquidity_usd.is_finite()
            || !self.min_holding_value_usd.is_finite()
            || self.min_market_cap_usd <= 0.0
            || self.max_market_cap_usd < self.min_market_cap_usd
            || self.min_liquidity_usd <= 0.0
            || self.min_holding_value_usd < 0.0
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

/// Parse data OHLCV dari response GeckoTerminal API
pub fn parse_gecko_ohlcv(response: &Value) -> Result<Vec<OhlcvBar>> {
    let list = response
        .get("data")
        .and_then(|d| d.get("attributes"))
        .and_then(|a| a.get("ohlcv_list"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("gecko response missing ohlcv_list"))?;

    let mut bars = Vec::with_capacity(list.len());
    for item in list {
        let arr = item
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("ohlcv item is not an array"))?;
        if arr.len() < 6 {
            continue;
        }
        let timestamp = arr[0]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("invalid timestamp"))?;
        let open = arr[1]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid open"))?;
        let high = arr[2]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid high"))?;
        let low = arr[3]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid low"))?;
        let close = arr[4]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid close"))?;
        let volume = arr[5]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("invalid volume"))?;

        if !open.is_finite()
            || !high.is_finite()
            || !low.is_finite()
            || !close.is_finite()
            || !volume.is_finite()
        {
            continue;
        }
        bars.push(OhlcvBar {
            timestamp,
            open,
            high,
            low,
            close,
            volume,
        });
    }

    if bars.is_empty() {
        bail!("no valid ohlcv bars in response");
    }

    // Urutkan kronologis dari yang terlama ke terbaru
    bars.sort_by_key(|b| b.timestamp);
    Ok(bars)
}

/// Menghitung Market Structure & Price Action dari deret bar lilin secara deterministik
pub fn compute_market_structure_and_action(bars: &[OhlcvBar]) -> (String, String) {
    if bars.len() < 5 {
        return (
            "UNAVAILABLE_OHLCV".to_string(),
            "UNAVAILABLE_OHLCV".to_string(),
        );
    }

    // 1. Market Structure analysis over available bars
    let half = bars.len() / 2;
    let first_half = &bars[..half];
    let second_half = &bars[half..];

    let early_high = first_half.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    let early_low = first_half.iter().map(|b| b.low).fold(f64::MAX, f64::min);
    let late_high = second_half.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    let late_low = second_half.iter().map(|b| b.low).fold(f64::MAX, f64::min);

    let structure = if early_low > 0.0
        && ((early_high - early_low) / early_low) < 0.03
        && late_low > 0.0
        && ((late_high - late_low) / late_low) < 0.03
    {
        "RANGING".to_string()
    } else if late_high > early_high && late_low > early_low {
        "BULLISH_HH_HL".to_string()
    } else if late_high < early_high && late_low < early_low {
        "BEARISH_LH_LL".to_string()
    } else if late_high > early_high && late_low < early_low {
        "EXPANSION_RANGE".to_string()
    } else {
        "RANGING".to_string()
    };

    // 2. Price Action analysis on the latest bar
    let latest = &bars[bars.len() - 1];
    let range = latest.high - latest.low;
    let avg_prior_vol = if bars.len() > 1 {
        bars[..bars.len() - 1].iter().map(|b| b.volume).sum::<f64>() / (bars.len() - 1) as f64
    } else {
        latest.volume
    };
    let avg_prior_range = if bars.len() > 1 {
        bars[..bars.len() - 1]
            .iter()
            .map(|b| b.high - b.low)
            .sum::<f64>()
            / (bars.len() - 1) as f64
    } else {
        range
    };

    let action = if range <= 0.0 || (avg_prior_range > 0.0 && range < avg_prior_range * 0.4) {
        "CONSOLIDATION".to_string()
    } else {
        let lower_wick = latest.open.min(latest.close) - latest.low;
        let upper_wick = latest.high - latest.open.max(latest.close);

        if lower_wick / range >= 0.40 && latest.volume >= avg_prior_vol * 0.8 {
            "ABSORPTION".to_string()
        } else if upper_wick / range >= 0.50 {
            "EXHAUSTION".to_string()
        } else if latest.close > early_high.max(late_high) * 0.98 && latest.volume >= avg_prior_vol
        {
            "BREAKOUT".to_string()
        } else if latest.close >= latest.open {
            "BULLISH_FLOW".to_string()
        } else {
            "BEARISH_FLOW".to_string()
        }
    };

    (structure, action)
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

    let mut wallet_holdings: HashMap<String, f64> = HashMap::new();
    let mut wallet_has_direct_swap: HashSet<String> = HashSet::new();
    let mut wallet_clusters: HashMap<String, String> = HashMap::new();
    let mut wallet_is_qualified: HashMap<String, bool> = HashMap::new();

    for item in evidence {
        let wallet = item.wallet_address.trim().to_ascii_lowercase();
        if wallet.is_empty() {
            bail!("radar evidence wallet identity missing");
        }
        if item.qualified_wallet {
            wallet_is_qualified.insert(wallet.clone(), true);
        } else {
            wallet_is_qualified.entry(wallet.clone()).or_insert(false);
        }
        if let Some(cluster) = &item.cluster_id {
            if !cluster.trim().is_empty() {
                wallet_clusters.insert(wallet.clone(), cluster.trim().to_string());
            }
        }
        if item.kind == RadarEvidenceKind::DirectSwap {
            wallet_has_direct_swap.insert(wallet.clone());
        }
        if item.kind == RadarEvidenceKind::TokenHolding {
            if let Some(amount) = item.ui_amount {
                *wallet_holdings.entry(wallet.clone()).or_insert(0.0) += amount;
            }
        }
    }

    for (wallet, &is_qualified) in &wallet_is_qualified {
        let has_swap = wallet_has_direct_swap.contains(wallet);
        let total_holding_tokens = wallet_holdings.get(wallet).copied().unwrap_or(0.0);
        let passes_holding_value = match market.price_usd {
            Some(price) => (total_holding_tokens * price) >= policy.min_holding_value_usd,
            None => true,
        };

        if has_swap || passes_holding_value {
            if has_swap {
                direct_accumulators.insert(wallet.clone());
            }
            if is_qualified {
                qualified_holders.insert(wallet.clone());
            } else {
                candidate_holders.insert(wallet.clone());
            }
            if let Some(cluster) = wallet_clusters.get(wallet) {
                clusters.insert(cluster.clone());
            }
        }
    }

    if qualified_holders.is_empty() && candidate_holders.is_empty() {
        return Ok(None);
    }

    let status = if market.price_usd.is_some() && !direct_accumulators.is_empty() {
        RadarStatus::HighPriorityRadar
    } else if market.price_usd.is_some() {
        RadarStatus::StructureConfirmed
    } else {
        RadarStatus::PortfolioOnly
    };
    let mut whale_names: Vec<String> = clusters.iter().cloned().collect();
    whale_names.sort();
    let whales = if whale_names.is_empty() {
        None
    } else {
        Some(whale_names.join(", "))
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
        whales,
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
            ui_amount: Some(10_000.0),
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
            min_holding_value_usd: 100.0,
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
    fn direct_swap_with_price_is_high_priority_but_not_entry() {
        let result = reduce_portfolio_to_radar(
            &[evidence(RadarEvidenceKind::DirectSwap, "wallet-a", true)],
            &market(Some(0.25)),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.status, RadarStatus::HighPriorityRadar);
        assert_eq!(result.qualified_holder_count, 1);
        assert_eq!(result.direct_accumulator_count, 1);
    }

    #[test]
    fn holding_below_min_holding_value_usd_is_excluded() {
        let mut item = evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true);
        item.ui_amount = Some(50.0); // 50 * 0.1 = $5.0 < $100.0
        let result = reduce_portfolio_to_radar(&[item], &market(Some(0.1)), policy()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn holding_below_min_holding_value_with_direct_swap_is_preserved_as_high_priority() {
        let mut holding_item = evidence(RadarEvidenceKind::TokenHolding, "wallet-a", true);
        holding_item.ui_amount = Some(50.0); // $5.0 < $100.0
        let swap_item = evidence(RadarEvidenceKind::DirectSwap, "wallet-a", true);
        let result = reduce_portfolio_to_radar(
            &[holding_item, swap_item],
            &market(Some(0.1)),
            policy(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.status, RadarStatus::HighPriorityRadar);
        assert_eq!(result.qualified_holder_count, 1);
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

    #[test]
    fn test_parse_gecko_ohlcv_and_compute_structure() {
        let json = serde_json::json!({
            "data": {
                "attributes": {
                    "ohlcv_list": [
                        [1700000000, 10.0, 12.0, 9.5, 11.0, 1000.0],
                        [1700003600, 11.0, 13.0, 10.5, 12.5, 1200.0],
                        [1700007200, 12.5, 14.0, 12.0, 13.5, 1500.0],
                        [1700010800, 13.5, 15.0, 13.0, 14.5, 1800.0],
                        [1700014400, 14.5, 16.0, 14.0, 15.8, 2000.0],
                        [1700018000, 15.8, 17.5, 15.5, 17.2, 2500.0]
                    ]
                }
            }
        });
        let bars = parse_gecko_ohlcv(&json).unwrap();
        assert_eq!(bars.len(), 6);
        assert_eq!(bars[0].timestamp, 1700000000);
        assert_eq!(bars[5].timestamp, 1700018000);

        let (structure, action) = compute_market_structure_and_action(&bars);
        assert_eq!(structure, "BULLISH_HH_HL");
        assert_eq!(action, "BREAKOUT");
    }

    #[test]
    fn test_compute_market_structure_absorption() {
        let bars = vec![
            OhlcvBar {
                timestamp: 1,
                open: 10.0,
                high: 11.0,
                low: 9.5,
                close: 10.5,
                volume: 100.0,
            },
            OhlcvBar {
                timestamp: 2,
                open: 10.5,
                high: 11.5,
                low: 10.0,
                close: 11.0,
                volume: 100.0,
            },
            OhlcvBar {
                timestamp: 3,
                open: 11.0,
                high: 12.0,
                low: 10.5,
                close: 11.5,
                volume: 100.0,
            },
            OhlcvBar {
                timestamp: 4,
                open: 11.5,
                high: 12.5,
                low: 11.0,
                close: 12.0,
                volume: 100.0,
            },
            // Latest bar: dips sharply to 8.0, but closes near 12.2 (huge lower wick, absorption)
            OhlcvBar {
                timestamp: 5,
                open: 12.0,
                high: 12.5,
                low: 8.0,
                close: 12.2,
                volume: 500.0,
            },
        ];
        let (_, action) = compute_market_structure_and_action(&bars);
        assert_eq!(action, "ABSORPTION");
    }
}
