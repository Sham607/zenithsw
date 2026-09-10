use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chain {
    Solana,
    Robinhood,
    Arbitrum,
    Base,
    Ethereum,
}

impl fmt::Display for Chain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Chain::Solana => write!(f, "SOLANA"),
            Chain::Robinhood => write!(f, "ROBINHOOD"),
            Chain::Arbitrum => write!(f, "ARBITRUM"),
            Chain::Base => write!(f, "BASE"),
            Chain::Ethereum => write!(f, "ETHEREUM"),
        }
    }
}

impl FromStr for Chain {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_uppercase().as_str() {
            "SOLANA" | "SOL" => Ok(Chain::Solana),
            "ROBINHOOD" | "RH" => Ok(Chain::Robinhood),
            "ARBITRUM" | "ARB" => Ok(Chain::Arbitrum),
            "BASE" => Ok(Chain::Base),
            "ETHEREUM" | "ETH" => Ok(Chain::Ethereum),
            _ => Ok(Chain::Solana),
        }
    }
}

impl<'de> Deserialize<'de> for Chain {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Chain::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Chain {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletTier {
    TierS, // Top Whale / Legend (PnL > $100k, instant copy eligibility)
    TierA, // Alpha Hunter (PnL $30k - $100k, requires 2x confluence)
    TierB, // Fast Mover (PnL < $30k, monitoring only)
}

impl fmt::Display for WalletTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WalletTier::TierS => write!(f, "TIER-S"),
            WalletTier::TierA => write!(f, "TIER-A"),
            WalletTier::TierB => write!(f, "TIER-B"),
        }
    }
}

impl FromStr for WalletTier {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_uppercase().as_str() {
            "TIER-S" | "TIERS" | "S" => Ok(WalletTier::TierS),
            "TIER-A" | "TIERA" | "A" => Ok(WalletTier::TierA),
            "TIER-B" | "TIERB" | "B" => Ok(WalletTier::TierB),
            _ => Ok(WalletTier::TierB),
        }
    }
}

impl<'de> Deserialize<'de> for WalletTier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        WalletTier::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for WalletTier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartWallet {
    #[serde(default)]
    pub id: Option<i64>,
    pub rank: u32,
    pub name: String,
    pub handle: String,
    pub chain: Chain,
    pub address: String,
    pub tier: WalletTier,
    pub pnl_24h_usd: f64,
    pub win_rate_pct: f64,
    pub total_trades: u32,
    #[serde(default = "default_source")]
    pub source: String,             // "FOMO" | "ONCHAIN_INDEXER"
    #[serde(default = "default_reputation")]
    pub reputation_score: f64,      // 0.0 - 100.0
    #[serde(default = "default_active")]
    pub is_active: bool,
    #[serde(default)]
    pub last_seen_at: Option<DateTime<Utc>>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

fn default_active() -> bool {
    true
}

fn default_source() -> String {
    "FOMO".to_string()
}

fn default_reputation() -> f64 {
    85.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityType {
    Buy,
    Sell,
    Transfer,
}

impl fmt::Display for ActivityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ActivityType::Buy => write!(f, "BUY"),
            ActivityType::Sell => write!(f, "SELL"),
            ActivityType::Transfer => write!(f, "TRANSFER"),
        }
    }
}

impl FromStr for ActivityType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_uppercase().as_str() {
            "BUY" => Ok(ActivityType::Buy),
            "SELL" => Ok(ActivityType::Sell),
            _ => Ok(ActivityType::Transfer),
        }
    }
}

impl<'de> Deserialize<'de> for ActivityType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ActivityType::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for ActivityType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletActivity {
    #[serde(default)]
    pub id: Option<i64>,
    pub wallet_handle: String,
    pub wallet_address: String,
    pub chain: Chain,
    pub activity_type: ActivityType,
    pub token_symbol: String,
    pub token_mint: String,
    pub amount_usd: f64,
    pub token_amount: f64,
    pub execution_price_usd: f64,
    #[serde(default)]
    pub whale_balance_before: f64,
    #[serde(default)]
    pub whale_balance_after: f64,
    #[serde(default)]
    pub sell_ratio_pct: f64,        // 0.0 for Buy, 0.0-100.0 for Sell (e.g. 50.0% partial, 100.0% full)
    pub tx_signature: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowSignal {
    pub chain: Chain,
    pub token_symbol: String,
    pub token_mint: String,
    pub whale_handle: String,
    pub whale_address: String,
    pub whale_tier: WalletTier,
    pub whale_entry_price: f64,
    pub current_pool_price: f64,
    pub price_delta_pct: f64,
    pub pool_liquidity_usd: f64,
    pub is_passed: bool,
    pub rejection_reason: Option<String>,
    pub confluence_count: u32,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowPosition {
    pub id: String,
    pub chain: Chain,
    pub token_symbol: String,
    pub token_mint: String,
    pub target_whale_handle: String,
    pub target_whale_address: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub peak_price: f64,
    pub amount_native: f64,
    pub amount_usd: f64,
    #[serde(default = "default_portion")]
    pub remaining_portion_pct: f64, // 100.0% down to 0.0% on partial sells
    pub unrealized_pnl_pct: f64,
    #[serde(default)]
    pub realized_pnl_usd: f64,
    pub entry_time: DateTime<Utc>,
    pub last_update: DateTime<Utc>,
    pub status: String,             // "OPEN" | "PARTIAL_EXIT" | "CLOSED"
}

pub fn default_portion() -> f64 {
    100.0
}

/// Checks if a token is a base currency, gas token, or stablecoin (USDC, USDT, WETH, etc.)
pub fn is_blacklisted_asset(mint: &str, symbol: &str) -> bool {
    let sym_upper = symbol.trim().to_uppercase();
    let mint_lower = mint.trim().to_lowercase();

    // Blacklisted base/stablecoin symbols
    let blacklisted_symbols = [
        "USDC", "USDT", "USD₮0", "USDT0", "DAI", "USDE", "USDS", "PYUSD",
        "BUSD", "TUSD", "EURC", "USDG", "UZDT", "WETH", "ETH", "WSOL",
        "SOL", "WBTC", "BTC", "ARB", "OTM", "WERC"
    ];

    if blacklisted_symbols.iter().any(|&s| sym_upper == s || sym_upper == format!("W{}", s)) {
        return true;
    }

    // Blacklisted contract addresses on EVM & Solana
    let blacklisted_mints = [
        // EVM Arbitrum / Base / Mainnet
        "0xaf88d065e77c8cc2239327c5edb3a432268e5831", // USDC Arb
        "0xff970a61a04b1ca14834a43f5de4533ebddb5cc8", // USDC.e Arb
        "0xfd086bc7cd5c481dcc9c85ebe478a1c0b69fcbb9", // USDT Arb
        "0x82af49447d8a07e3bd95bd0d56f35241523fbab1", // WETH Arb
        "0x912ce59144191c1204e64559fe8253a0e49e6548", // ARB
        "0x2f2a2543b76a4166549f7aab2e75bef0aefc5b0f", // WBTC Arb
        "0xda10009cbd5d07dd0cecc66161fc93d7c9000da1", // DAI Arb
        "0x833589fcd6edb6e08f4c7c32d4f71b54bda02913", // USDC Base
        "0x4200000000000000000000000000000000000006", // WETH Base
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48", // USDC Eth
        "0xdac17f958d2ee523a2206206994597c13d831ec7", // USDT Eth
        // Solana
        "epjfwdd5aufqssqem2qn1xzybapc8g4weggkzwytdt1v", // USDC Sol
        "es9vmfrzacerrmjfrf4h2fyd4kconkcy11mcce8benwny", // USDT Sol
        "so11111111111111111111111111111111111111112", // WSOL Sol
    ];

    if blacklisted_mints.iter().any(|&m| mint_lower == m) {
        return true;
    }

    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub timestamp: DateTime<Utc>,
    pub chain: Chain,
    pub action: String,
    pub symbol: String,
    pub mint: String,
    pub target_whale: String,
    pub exit_reason: String,
    #[serde(default = "default_portion")]
    pub portion_sold_pct: f64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub pnl_pct: f64,
    pub pnl_usd: f64,
    pub duration_secs: u64,
}
