use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletScoreResult {
    pub address: String,
    pub reputation_score: f64, // 0.0 to 100.0
    pub win_rate_pct: f64,
    pub total_trades: u32,
    pub estimated_pnl_usd: f64,
    pub is_dev_or_creator: bool,
    pub multi_token_wins: u32,
    pub is_eligible_tier_a: bool,
    pub is_eligible_tier_s: bool,
    pub rejection_reason: Option<String>,
}

pub struct ReputationScorer;

impl ReputationScorer {
    /// Evaluates whether an on-chain wallet is genuinely Smart Money or a Dev/Insider Trap
    pub fn evaluate(
        address: &str,
        is_creator: bool,
        multi_token_wins: u32,
        win_rate_pct: f64,
        total_trades: u32,
        estimated_pnl_usd: f64,
        wallet_age_days: u32,
    ) -> WalletScoreResult {
        // 1. Trap Check: Creator or Deployer
        if is_creator {
            return WalletScoreResult {
                address: address.to_string(),
                reputation_score: 0.0,
                win_rate_pct,
                total_trades,
                estimated_pnl_usd,
                is_dev_or_creator: true,
                multi_token_wins,
                is_eligible_tier_a: false,
                is_eligible_tier_s: false,
                rejection_reason: Some("REJECTED: Wallet is Token Creator/Deployer (Insider Trap)".to_string()),
            };
        }

        // 2. Disposable / Fresh Wallet Check (< 3 days)
        if wallet_age_days < 3 {
            return WalletScoreResult {
                address: address.to_string(),
                reputation_score: 20.0,
                win_rate_pct,
                total_trades,
                estimated_pnl_usd,
                is_dev_or_creator: false,
                multi_token_wins,
                is_eligible_tier_a: false,
                is_eligible_tier_s: false,
                rejection_reason: Some("REJECTED: Disposable / Fresh Wallet (<3 days old)".to_string()),
            };
        }

        // 3. Multi-Token Success Check (Must win on >= 2 distinct tokens)
        if multi_token_wins < 2 && total_trades > 3 {
            return WalletScoreResult {
                address: address.to_string(),
                reputation_score: 40.0,
                win_rate_pct,
                total_trades,
                estimated_pnl_usd,
                is_dev_or_creator: false,
                multi_token_wins,
                is_eligible_tier_a: false,
                is_eligible_tier_s: false,
                rejection_reason: Some("REJECTED: One-hit Wonder (No multi-token consistency)".to_string()),
            };
        }

        // 4. Compute composite score
        let mut score = 50.0;
        
        // Win rate weight (+- 20)
        score += ((win_rate_pct - 50.0) / 50.0) * 20.0;

        // PnL weight
        if estimated_pnl_usd > 100_000.0 {
            score += 20.0;
        } else if estimated_pnl_usd > 20_000.0 {
            score += 10.0;
        } else if estimated_pnl_usd < 0.0 {
            score -= 20.0;
        }

        // Multi token weight
        if multi_token_wins >= 5 {
            score += 15.0;
        } else if multi_token_wins >= 2 {
            score += 10.0;
        }

        let reputation_score = score.clamp(0.0, 100.0);
        let is_eligible_tier_s = reputation_score >= 88.0 && estimated_pnl_usd >= 50_000.0 && win_rate_pct >= 70.0;
        let is_eligible_tier_a = reputation_score >= 70.0 && estimated_pnl_usd >= 10_000.0 && win_rate_pct >= 60.0;

        WalletScoreResult {
            address: address.to_string(),
            reputation_score,
            win_rate_pct,
            total_trades,
            estimated_pnl_usd,
            is_dev_or_creator: false,
            multi_token_wins,
            is_eligible_tier_a,
            is_eligible_tier_s,
            rejection_reason: if !is_eligible_tier_a && !is_eligible_tier_s {
                Some("REJECTED: Score below threshold (Min 70.0 Score, >$10k PnL)".to_string())
            } else {
                None
            },
        }
    }
}
