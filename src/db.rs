use crate::models::{
    ActivityType, Chain, ShadowPosition, ShadowSignal, SmartWallet, TrackedToken, TradeRecord,
    WalletActivity, WalletTier,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;

pub struct SmartWalletDb {
    conn: Mutex<Connection>,
}

impl SmartWalletDb {
    pub fn new(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             
             CREATE TABLE IF NOT EXISTS wallets (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 rank INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 handle TEXT NOT NULL UNIQUE,
                 chain TEXT NOT NULL,
                 address TEXT NOT NULL,
                 tier TEXT NOT NULL,
                 pnl_24h_usd REAL NOT NULL,
                 win_rate_pct REAL NOT NULL,
                 total_trades INTEGER NOT NULL,
                 source TEXT NOT NULL DEFAULT 'FOMO',
                 reputation_score REAL NOT NULL DEFAULT 80.0,
                 is_active INTEGER NOT NULL DEFAULT 1,
                 last_seen_at TEXT,
                 updated_at TEXT NOT NULL
             );
             
             CREATE TABLE IF NOT EXISTS wallet_activities (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 wallet_handle TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 activity_type TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 amount_usd REAL NOT NULL,
                 token_amount REAL NOT NULL,
                 execution_price_usd REAL NOT NULL,
                 whale_balance_before REAL NOT NULL DEFAULT 0.0,
                 whale_balance_after REAL NOT NULL DEFAULT 0.0,
                 sell_ratio_pct REAL NOT NULL DEFAULT 0.0,
                 tx_signature TEXT NOT NULL,
                 timestamp TEXT NOT NULL
             );
             
             CREATE TABLE IF NOT EXISTS shadow_signals (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 chain TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 whale_handle TEXT NOT NULL,
                 whale_address TEXT NOT NULL,
                 whale_tier TEXT NOT NULL,
                 whale_entry_price REAL NOT NULL,
                 current_pool_price REAL NOT NULL,
                 price_delta_pct REAL NOT NULL,
                 pool_liquidity_usd REAL NOT NULL,
                 is_passed INTEGER NOT NULL,
                 rejection_reason TEXT,
                 confluence_count INTEGER NOT NULL,
                 timestamp TEXT NOT NULL
             );
             
             CREATE TABLE IF NOT EXISTS shadow_positions (
                 id TEXT PRIMARY KEY,
                 chain TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 target_whale_handle TEXT NOT NULL,
                 target_whale_address TEXT NOT NULL,
                 entry_price REAL NOT NULL,
                 current_price REAL NOT NULL,
                 peak_price REAL NOT NULL,
                 amount_native REAL NOT NULL,
                 amount_usd REAL NOT NULL,
                 remaining_portion_pct REAL NOT NULL DEFAULT 100.0,
                 unrealized_pnl_pct REAL NOT NULL,
                 realized_pnl_usd REAL NOT NULL DEFAULT 0.0,
                 entry_time TEXT NOT NULL,
                 last_update TEXT NOT NULL,
                 status TEXT NOT NULL
             );
             
             CREATE TABLE IF NOT EXISTS trade_history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 action TEXT NOT NULL,
                 symbol TEXT NOT NULL,
                 mint TEXT NOT NULL,
                 target_whale TEXT NOT NULL,
                 exit_reason TEXT NOT NULL,
                 portion_sold_pct REAL NOT NULL DEFAULT 100.0,
                 entry_price REAL NOT NULL,
                 exit_price REAL NOT NULL,
                 pnl_pct REAL NOT NULL,
                 pnl_usd REAL NOT NULL,
                 duration_secs INTEGER NOT NULL
             );

             CREATE TABLE IF NOT EXISTS tracked_tokens (
                 token_mint TEXT PRIMARY KEY,
                 token_symbol TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 market_cap_usd REAL NOT NULL,
                 liquidity_usd REAL NOT NULL,
                 price_usd REAL NOT NULL,
                 change_24h_pct REAL NOT NULL,
                 change_6h_pct REAL NOT NULL,
                 change_1h_pct REAL NOT NULL,
                 whale_count INTEGER NOT NULL,
                 total_whale_usd REAL NOT NULL,
                 top_whale_handles TEXT NOT NULL,
                 status TEXT NOT NULL,
                 upside_tier TEXT NOT NULL,
                 last_scanned_at TEXT NOT NULL
             );
             
             CREATE INDEX IF NOT EXISTS idx_wallets_handle ON wallets(handle);
             CREATE INDEX IF NOT EXISTS idx_wallets_address ON wallets(address);
             CREATE INDEX IF NOT EXISTS idx_activities_mint ON wallet_activities(token_mint);
             CREATE INDEX IF NOT EXISTS idx_signals_mint ON shadow_signals(token_mint);
             CREATE INDEX IF NOT EXISTS idx_signals_timestamp ON shadow_signals(timestamp);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wallets (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 rank INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 handle TEXT NOT NULL UNIQUE,
                 chain TEXT NOT NULL,
                 address TEXT NOT NULL,
                 tier TEXT NOT NULL,
                 pnl_24h_usd REAL NOT NULL,
                 win_rate_pct REAL NOT NULL,
                 total_trades INTEGER NOT NULL,
                 source TEXT NOT NULL DEFAULT 'FOMO',
                 reputation_score REAL NOT NULL DEFAULT 80.0,
                 is_active INTEGER NOT NULL DEFAULT 1,
                 last_seen_at TEXT,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS wallet_activities (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 wallet_handle TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 activity_type TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 amount_usd REAL NOT NULL,
                 token_amount REAL NOT NULL,
                 execution_price_usd REAL NOT NULL,
                 whale_balance_before REAL NOT NULL DEFAULT 0.0,
                 whale_balance_after REAL NOT NULL DEFAULT 0.0,
                 sell_ratio_pct REAL NOT NULL DEFAULT 0.0,
                 tx_signature TEXT NOT NULL,
                 timestamp TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS shadow_signals (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 chain TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 whale_handle TEXT NOT NULL,
                 whale_address TEXT NOT NULL,
                 whale_tier TEXT NOT NULL,
                 whale_entry_price REAL NOT NULL,
                 current_pool_price REAL NOT NULL,
                 price_delta_pct REAL NOT NULL,
                 pool_liquidity_usd REAL NOT NULL,
                 is_passed INTEGER NOT NULL,
                 rejection_reason TEXT,
                 confluence_count INTEGER NOT NULL,
                 timestamp TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS shadow_positions (
                 id TEXT PRIMARY KEY,
                 chain TEXT NOT NULL,
                 token_symbol TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 target_whale_handle TEXT NOT NULL,
                 target_whale_address TEXT NOT NULL,
                 entry_price REAL NOT NULL,
                 current_price REAL NOT NULL,
                 peak_price REAL NOT NULL,
                 amount_native REAL NOT NULL,
                 amount_usd REAL NOT NULL,
                 remaining_portion_pct REAL NOT NULL DEFAULT 100.0,
                 unrealized_pnl_pct REAL NOT NULL,
                 realized_pnl_usd REAL NOT NULL DEFAULT 0.0,
                 entry_time TEXT NOT NULL,
                 last_update TEXT NOT NULL,
                 status TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS trade_history (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 timestamp TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 action TEXT NOT NULL,
                 symbol TEXT NOT NULL,
                 mint TEXT NOT NULL,
                 target_whale TEXT NOT NULL,
                 exit_reason TEXT NOT NULL,
                 portion_sold_pct REAL NOT NULL DEFAULT 100.0,
                 entry_price REAL NOT NULL,
                 exit_price REAL NOT NULL,
                 pnl_pct REAL NOT NULL,
                 pnl_usd REAL NOT NULL,
                 duration_secs INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS tracked_tokens (
                 token_mint TEXT PRIMARY KEY,
                 token_symbol TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 market_cap_usd REAL NOT NULL,
                 liquidity_usd REAL NOT NULL,
                 price_usd REAL NOT NULL,
                 change_24h_pct REAL NOT NULL,
                 change_6h_pct REAL NOT NULL,
                 change_1h_pct REAL NOT NULL,
                 whale_count INTEGER NOT NULL,
                 total_whale_usd REAL NOT NULL,
                 top_whale_handles TEXT NOT NULL,
                 status TEXT NOT NULL,
                 upside_tier TEXT NOT NULL,
                 last_scanned_at TEXT NOT NULL
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_wallet(&self, wallet: &SmartWallet) -> Result<()> {
        let updated_at_str = wallet.updated_at.to_rfc3339();
        let last_seen_str = wallet.last_seen_at.map(|t| t.to_rfc3339());
        let chain_str = wallet.chain.to_string();
        let tier_str = wallet.tier.to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO wallets (
                rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(handle) DO UPDATE SET
                rank = excluded.rank,
                name = excluded.name,
                chain = excluded.chain,
                address = excluded.address,
                tier = excluded.tier,
                pnl_24h_usd = excluded.pnl_24h_usd,
                win_rate_pct = excluded.win_rate_pct,
                total_trades = excluded.total_trades,
                source = excluded.source,
                reputation_score = excluded.reputation_score,
                is_active = excluded.is_active,
                last_seen_at = coalesce(excluded.last_seen_at, wallets.last_seen_at),
                updated_at = excluded.updated_at",
            params![
                wallet.rank,
                wallet.name,
                wallet.handle,
                chain_str,
                wallet.address,
                tier_str,
                wallet.pnl_24h_usd,
                wallet.win_rate_pct,
                wallet.total_trades,
                wallet.source,
                wallet.reputation_score,
                if wallet.is_active { 1 } else { 0 },
                last_seen_str,
                updated_at_str,
            ],
        )?;
        Ok(())
    }

    pub fn get_active_wallets(&self) -> Result<Vec<SmartWallet>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at
             FROM wallets WHERE is_active = 1 ORDER BY pnl_24h_usd DESC, rank ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: Option<i64> = row.get(0)?;
            let rank: u32 = row.get(1)?;
            let name: String = row.get(2)?;
            let handle: String = row.get(3)?;
            let chain_str: String = row.get(4)?;
            let address: String = row.get(5)?;
            let tier_str: String = row.get(6)?;
            let pnl_24h_usd: f64 = row.get(7)?;
            let win_rate_pct: f64 = row.get(8)?;
            let total_trades: u32 = row.get(9)?;
            let source: String = row.get(10).unwrap_or_else(|_| "FOMO".to_string());
            let reputation_score: f64 = row.get(11).unwrap_or(80.0);
            let is_active_int: i32 = row.get(12)?;
            let last_seen_str: Option<String> = row.get(13)?;
            let updated_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let tier = WalletTier::from_str(&tier_str).unwrap_or(WalletTier::TierB);
            let last_seen_at = last_seen_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier,
                pnl_24h_usd,
                win_rate_pct,
                total_trades,
                source,
                reputation_score,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })
        })?;

        let mut wallets = Vec::new();
        for r in rows {
            wallets.push(r?);
        }
        Ok(wallets)
    }

    pub fn get_wallets_by_tier(&self, tier: WalletTier) -> Result<Vec<SmartWallet>> {
        let conn = self.conn.lock().unwrap();
        let tier_str1 = tier.to_string();
        let tier_str2 = format!("Tier{}", tier_str1.trim_start_matches("TIER-"));
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at
             FROM wallets WHERE is_active = 1 AND (tier = ?1 OR tier = ?2 OR tier LIKE ?3) ORDER BY pnl_24h_usd DESC, reputation_score DESC",
        )?;
        let like_str = format!("%{}%", tier);
        let rows = stmt.query_map(params![tier_str1, tier_str2, like_str], |row| {
            let id: Option<i64> = row.get(0)?;
            let rank: u32 = row.get(1)?;
            let name: String = row.get(2)?;
            let handle: String = row.get(3)?;
            let chain_str: String = row.get(4)?;
            let address: String = row.get(5)?;
            let tier_s: String = row.get(6)?;
            let pnl_24h_usd: f64 = row.get(7)?;
            let win_rate_pct: f64 = row.get(8)?;
            let total_trades: u32 = row.get(9)?;
            let source: String = row.get(10).unwrap_or_else(|_| "FOMO".to_string());
            let reputation_score: f64 = row.get(11).unwrap_or(80.0);
            let is_active_int: i32 = row.get(12)?;
            let last_seen_str: Option<String> = row.get(13)?;
            let updated_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let t = WalletTier::from_str(&tier_s).unwrap_or(tier);
            let last_seen_at = last_seen_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier: t,
                pnl_24h_usd,
                win_rate_pct,
                total_trades,
                source,
                reputation_score,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })
        })?;

        let mut wallets = Vec::new();
        for r in rows {
            wallets.push(r?);
        }
        Ok(wallets)
    }

    pub fn get_qualified_wallets(&self) -> Result<Vec<SmartWallet>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at
             FROM wallets WHERE is_active = 1 AND tier IN ('TIER-S', 'TIER-A') ORDER BY pnl_24h_usd DESC, reputation_score DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: Option<i64> = row.get(0)?;
            let rank: u32 = row.get(1)?;
            let name: String = row.get(2)?;
            let handle: String = row.get(3)?;
            let chain_str: String = row.get(4)?;
            let address: String = row.get(5)?;
            let tier_str: String = row.get(6)?;
            let pnl_24h_usd: f64 = row.get(7)?;
            let win_rate_pct: f64 = row.get(8)?;
            let total_trades: u32 = row.get(9)?;
            let source: String = row.get(10).unwrap_or_else(|_| "FOMO".to_string());
            let reputation_score: f64 = row.get(11).unwrap_or(80.0);
            let is_active_int: i32 = row.get(12)?;
            let last_seen_str: Option<String> = row.get(13)?;
            let updated_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let tier = WalletTier::from_str(&tier_str).unwrap_or(WalletTier::TierB);
            let last_seen_at = last_seen_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier,
                pnl_24h_usd,
                win_rate_pct,
                total_trades,
                source,
                reputation_score,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })
        })?;

        let mut wallets = Vec::new();
        for r in rows {
            wallets.push(r?);
        }
        Ok(wallets)
    }

    pub fn get_wallet_by_address(&self, address: &str) -> Result<Option<SmartWallet>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at
             FROM wallets WHERE address = ?1 LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![address], |row| {
            let id: Option<i64> = row.get(0)?;
            let rank: u32 = row.get(1)?;
            let name: String = row.get(2)?;
            let handle: String = row.get(3)?;
            let chain_str: String = row.get(4)?;
            let address: String = row.get(5)?;
            let tier_str: String = row.get(6)?;
            let pnl_24h_usd: f64 = row.get(7)?;
            let win_rate_pct: f64 = row.get(8)?;
            let total_trades: u32 = row.get(9)?;
            let source: String = row.get(10).unwrap_or_else(|_| "FOMO".to_string());
            let reputation_score: f64 = row.get(11).unwrap_or(80.0);
            let is_active_int: i32 = row.get(12)?;
            let last_seen_str: Option<String> = row.get(13)?;
            let updated_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let tier = WalletTier::from_str(&tier_str).unwrap_or(WalletTier::TierB);
            let last_seen_at = last_seen_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&Utc))
            });
            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier,
                pnl_24h_usd,
                win_rate_pct,
                total_trades,
                source,
                reputation_score,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })
        })?;

        if let Some(res) = rows.next() {
            Ok(Some(res?))
        } else {
            Ok(None)
        }
    }

    pub fn insert_activity(&self, act: &WalletActivity) -> Result<()> {
        let ts_str = act.timestamp.to_rfc3339();
        let chain_str = act.chain.to_string();
        let act_str = act.activity_type.to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO wallet_activities (
                wallet_handle, wallet_address, chain, activity_type, token_symbol, token_mint, amount_usd, token_amount, execution_price_usd, whale_balance_before, whale_balance_after, sell_ratio_pct, tx_signature, timestamp
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                act.wallet_handle,
                act.wallet_address,
                chain_str,
                act_str,
                act.token_symbol,
                act.token_mint,
                act.amount_usd,
                act.token_amount,
                act.execution_price_usd,
                act.whale_balance_before,
                act.whale_balance_after,
                act.sell_ratio_pct,
                act.tx_signature,
                ts_str,
            ],
        )?;

        // Update last seen in wallet
        conn.execute(
            "UPDATE wallets SET last_seen_at = ?1 WHERE handle = ?2 OR address = ?3",
            params![ts_str, act.wallet_handle, act.wallet_address],
        )?;

        Ok(())
    }

    pub fn get_recent_activities(&self, limit: usize) -> Result<Vec<WalletActivity>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, wallet_handle, wallet_address, chain, activity_type, token_symbol, token_mint, amount_usd, token_amount, execution_price_usd, whale_balance_before, whale_balance_after, sell_ratio_pct, tx_signature, timestamp
             FROM wallet_activities ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let id: Option<i64> = row.get(0)?;
            let wallet_handle: String = row.get(1)?;
            let wallet_address: String = row.get(2)?;
            let chain_str: String = row.get(3)?;
            let act_str: String = row.get(4)?;
            let token_symbol: String = row.get(5)?;
            let token_mint: String = row.get(6)?;
            let amount_usd: f64 = row.get(7)?;
            let token_amount: f64 = row.get(8)?;
            let execution_price_usd: f64 = row.get(9)?;
            let whale_balance_before: f64 = row.get(10).unwrap_or(0.0);
            let whale_balance_after: f64 = row.get(11).unwrap_or(0.0);
            let sell_ratio_pct: f64 = row.get(12).unwrap_or(0.0);
            let tx_signature: String = row.get(13)?;
            let ts_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let activity_type = ActivityType::from_str(&act_str).unwrap_or(ActivityType::Buy);
            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(WalletActivity {
                id,
                wallet_handle,
                wallet_address,
                chain,
                activity_type,
                token_symbol,
                token_mint,
                amount_usd,
                token_amount,
                execution_price_usd,
                whale_balance_before,
                whale_balance_after,
                sell_ratio_pct,
                tx_signature,
                timestamp,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn insert_signal(&self, sig: &ShadowSignal) -> Result<()> {
        let ts_str = sig.timestamp.to_rfc3339();
        let chain_str = sig.chain.to_string();
        let tier_str = sig.whale_tier.to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO shadow_signals (
                chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                chain_str,
                sig.token_symbol,
                sig.token_mint,
                sig.whale_handle,
                sig.whale_address,
                tier_str,
                sig.whale_entry_price,
                sig.current_pool_price,
                sig.price_delta_pct,
                sig.pool_liquidity_usd,
                if sig.is_passed { 1 } else { 0 },
                sig.rejection_reason,
                sig.confluence_count,
                ts_str,
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_signals(&self, limit: usize) -> Result<Vec<ShadowSignal>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp
             FROM shadow_signals ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let chain_str: String = row.get(0)?;
            let token_symbol: String = row.get(1)?;
            let token_mint: String = row.get(2)?;
            let whale_handle: String = row.get(3)?;
            let whale_address: String = row.get(4)?;
            let tier_str: String = row.get(5)?;
            let whale_entry_price: f64 = row.get(6)?;
            let current_pool_price: f64 = row.get(7)?;
            let price_delta_pct: f64 = row.get(8)?;
            let pool_liquidity_usd: f64 = row.get(9)?;
            let is_passed_int: i32 = row.get(10)?;
            let rejection_reason: Option<String> = row.get(11)?;
            let confluence_count: u32 = row.get(12)?;
            let ts_str: String = row.get(13)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let whale_tier = WalletTier::from_str(&tier_str).unwrap_or(WalletTier::TierB);
            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ShadowSignal {
                chain,
                token_symbol,
                token_mint,
                whale_handle,
                whale_address,
                whale_tier,
                whale_entry_price,
                current_pool_price,
                price_delta_pct,
                pool_liquidity_usd,
                is_passed: is_passed_int == 1,
                rejection_reason,
                confluence_count,
                timestamp,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn upsert_position(&self, pos: &ShadowPosition) -> Result<()> {
        let entry_str = pos.entry_time.to_rfc3339();
        let update_str = pos.last_update.to_rfc3339();
        let chain_str = pos.chain.to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO shadow_positions (
                id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(id) DO UPDATE SET
                entry_price = excluded.entry_price,
                amount_usd = excluded.amount_usd,
                amount_native = excluded.amount_native,
                target_whale_handle = excluded.target_whale_handle,
                target_whale_address = excluded.target_whale_address,
                current_price = excluded.current_price,
                peak_price = excluded.peak_price,
                remaining_portion_pct = excluded.remaining_portion_pct,
                unrealized_pnl_pct = excluded.unrealized_pnl_pct,
                realized_pnl_usd = excluded.realized_pnl_usd,
                last_update = excluded.last_update,
                status = excluded.status",
            params![
                pos.id,
                chain_str,
                pos.token_symbol,
                pos.token_mint,
                pos.target_whale_handle,
                pos.target_whale_address,
                pos.entry_price,
                pos.current_price,
                pos.peak_price,
                pos.amount_native,
                pos.amount_usd,
                pos.remaining_portion_pct,
                pos.unrealized_pnl_pct,
                pos.realized_pnl_usd,
                entry_str,
                update_str,
                pos.status,
            ],
        )?;
        Ok(())
    }

    pub fn get_active_positions(&self) -> Result<Vec<ShadowPosition>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status
             FROM shadow_positions WHERE status != 'CLOSED' ORDER BY entry_time DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let chain_str: String = row.get(1)?;
            let token_symbol: String = row.get(2)?;
            let token_mint: String = row.get(3)?;
            let target_whale_handle: String = row.get(4)?;
            let target_whale_address: String = row.get(5)?;
            let entry_price: f64 = row.get(6)?;
            let current_price: f64 = row.get(7)?;
            let peak_price: f64 = row.get(8)?;
            let amount_native: f64 = row.get(9)?;
            let amount_usd: f64 = row.get(10)?;
            let remaining_portion_pct: f64 = row.get(11).unwrap_or(100.0);
            let unrealized_pnl_pct: f64 = row.get(12)?;
            let realized_pnl_usd: f64 = row.get(13).unwrap_or(0.0);
            let entry_str: String = row.get(14)?;
            let update_str: String = row.get(15)?;
            let status: String = row.get(16)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let entry_time = DateTime::parse_from_rfc3339(&entry_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let last_update = DateTime::parse_from_rfc3339(&update_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ShadowPosition {
                id,
                chain,
                token_symbol,
                token_mint,
                target_whale_handle,
                target_whale_address,
                entry_price,
                current_price,
                peak_price,
                amount_native,
                amount_usd,
                remaining_portion_pct,
                unrealized_pnl_pct,
                realized_pnl_usd,
                entry_time,
                last_update,
                status,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    /// Closes a shadow position partially (e.g. 50% partial exit) or completely (100% full exit)
    pub fn close_position_portion(
        &self,
        id: &str,
        portion_pct: f64,
        exit_reason: &str,
        exit_price: f64,
    ) -> Result<()> {
        let portion_clamped = portion_pct.clamp(1.0, 100.0);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT entry_price, amount_usd, remaining_portion_pct, realized_pnl_usd, entry_time, chain, token_symbol, token_mint, target_whale_handle FROM shadow_positions WHERE id = ?1")?;
        let pos_opt = stmt
            .query_row(params![id], |row| {
                let entry_price: f64 = row.get(0)?;
                let amount_usd: f64 = row.get(1)?;
                let remaining_portion_pct: f64 = row.get(2).unwrap_or(100.0);
                let realized_pnl_usd: f64 = row.get(3).unwrap_or(0.0);
                let entry_str: String = row.get(4)?;
                let chain_str: String = row.get(5)?;
                let symbol: String = row.get(6)?;
                let mint: String = row.get(7)?;
                let target_whale: String = row.get(8)?;
                Ok((
                    entry_price,
                    amount_usd,
                    remaining_portion_pct,
                    realized_pnl_usd,
                    entry_str,
                    chain_str,
                    symbol,
                    mint,
                    target_whale,
                ))
            })
            .optional()?;

        if let Some((
            entry_price,
            amount_usd,
            remaining_portion_pct,
            current_realized_pnl,
            entry_str,
            chain_str,
            symbol,
            mint,
            target_whale,
        )) = pos_opt
        {
            let now = Utc::now();
            let entry_time = DateTime::parse_from_rfc3339(&entry_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);
            let duration_secs = (now - entry_time).num_seconds().max(0) as u64;

            // Calculate portion sold
            let actual_sold_pct = portion_clamped.min(remaining_portion_pct);
            let sold_usd_basis = amount_usd * (actual_sold_pct / 100.0);
            let pnl_pct = if entry_price > 0.0 {
                ((exit_price - entry_price) / entry_price) * 100.0
            } else {
                0.0
            };
            let pnl_usd_this_trade = sold_usd_basis * (pnl_pct / 100.0);
            let new_remaining_portion = (remaining_portion_pct - actual_sold_pct).max(0.0);
            let new_realized_pnl = current_realized_pnl + pnl_usd_this_trade;

            let is_full_close = new_remaining_portion <= 1.0 || actual_sold_pct >= 95.0;
            let new_status = if is_full_close {
                "CLOSED"
            } else {
                "PARTIAL_EXIT"
            };
            let action_type = if is_full_close {
                "SELL"
            } else {
                "PARTIAL_SELL"
            };
            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);

            // Record into trade_history
            conn.execute(
                "INSERT INTO trade_history (
                    timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    now.to_rfc3339(),
                    chain.to_string(),
                    action_type,
                    symbol,
                    mint,
                    target_whale,
                    exit_reason,
                    actual_sold_pct,
                    entry_price,
                    exit_price,
                    pnl_pct,
                    pnl_usd_this_trade,
                    duration_secs,
                ],
            )?;

            // Update shadow_positions
            conn.execute(
                "UPDATE shadow_positions SET status = ?1, remaining_portion_pct = ?2, realized_pnl_usd = ?3, last_update = ?4 WHERE id = ?5",
                params![new_status, new_remaining_portion, new_realized_pnl, now.to_rfc3339(), id],
            )?;
        }

        Ok(())
    }

    pub fn close_position(&self, id: &str, exit_reason: &str, exit_price: f64) -> Result<()> {
        self.close_position_portion(id, 100.0, exit_reason, exit_price)
    }

    pub fn get_recent_trades(&self, limit: usize) -> Result<Vec<TradeRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs
             FROM trade_history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            let ts_str: String = row.get(0)?;
            let chain_str: String = row.get(1)?;
            let action: String = row.get(2)?;
            let symbol: String = row.get(3)?;
            let mint: String = row.get(4)?;
            let target_whale: String = row.get(5)?;
            let exit_reason: String = row.get(6)?;
            let portion_sold_pct: f64 = row.get(7).unwrap_or(100.0);
            let entry_price: f64 = row.get(8)?;
            let exit_price: f64 = row.get(9)?;
            let pnl_pct: f64 = row.get(10)?;
            let pnl_usd: f64 = row.get(11)?;
            let duration_secs: u64 = row.get(12)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(TradeRecord {
                timestamp,
                chain,
                action,
                symbol,
                mint,
                target_whale,
                exit_reason,
                portion_sold_pct,
                entry_price,
                exit_price,
                pnl_pct,
                pnl_usd,
                duration_secs,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn count_recent_buyers_for_mint(&self, mint: &str, window_secs: i64) -> Result<u32> {
        let since = (Utc::now() - chrono::Duration::seconds(window_secs)).to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT wallet_handle) FROM wallet_activities
             WHERE token_mint = ?1 AND activity_type = 'BUY' AND timestamp >= ?2",
            params![mint, since],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    pub fn upsert_tracked_token(&self, token: &TrackedToken) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracked_tokens (
                token_mint, token_symbol, chain, market_cap_usd, liquidity_usd, price_usd,
                change_24h_pct, change_6h_pct, change_1h_pct, whale_count, total_whale_usd,
                top_whale_handles, status, upside_tier, last_scanned_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(token_mint) DO UPDATE SET
                token_symbol = excluded.token_symbol,
                chain = excluded.chain,
                market_cap_usd = excluded.market_cap_usd,
                liquidity_usd = excluded.liquidity_usd,
                price_usd = excluded.price_usd,
                change_24h_pct = excluded.change_24h_pct,
                change_6h_pct = excluded.change_6h_pct,
                change_1h_pct = excluded.change_1h_pct,
                whale_count = excluded.whale_count,
                total_whale_usd = excluded.total_whale_usd,
                top_whale_handles = excluded.top_whale_handles,
                status = excluded.status,
                upside_tier = excluded.upside_tier,
                last_scanned_at = excluded.last_scanned_at",
            params![
                token.token_mint,
                token.token_symbol,
                token.chain.to_string(),
                token.market_cap_usd,
                token.liquidity_usd,
                token.price_usd,
                token.change_24h_pct,
                token.change_6h_pct,
                token.change_1h_pct,
                token.whale_count,
                token.total_whale_usd,
                token.top_whale_handles,
                token.status,
                token.upside_tier,
                token.last_scanned_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_whale_activity_tokens(
        &self,
        limit: usize,
    ) -> Result<Vec<(String, String, String, u32, f64, Vec<String>)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token_mint, 
                    MAX(token_symbol) as token_symbol, 
                    MAX(chain) as chain, 
                    COUNT(DISTINCT wallet_handle) as whale_count, 
                    SUM(amount_usd) as total_usd, 
                    GROUP_CONCAT(DISTINCT wallet_handle) as handles
             FROM wallet_activities
             WHERE amount_usd > 0.0 AND token_mint != '' AND token_mint != 'NATIVE'
             GROUP BY token_mint
             ORDER BY total_usd DESC, whale_count DESC
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            let mint: String = row.get(0)?;
            let symbol: String = row.get(1)?;
            let chain: String = row.get(2)?;
            let count: u32 = row.get(3)?;
            let total_usd: f64 = row.get(4)?;
            let handles_raw: Option<String> = row.get(5)?;
            let handles = handles_raw
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<String>>();
            Ok((mint, symbol, chain, count, total_usd, handles))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn get_tracked_tokens(&self, limit: usize) -> Result<Vec<TrackedToken>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token_mint, token_symbol, chain, market_cap_usd, liquidity_usd, price_usd,
                    change_24h_pct, change_6h_pct, change_1h_pct, whale_count, total_whale_usd,
                    top_whale_handles, status, upside_tier, last_scanned_at
             FROM tracked_tokens
             WHERE market_cap_usd >= 50000.0 AND market_cap_usd <= 20000000.0
             ORDER BY 
                CASE 
                    WHEN market_cap_usd > 0.0 AND market_cap_usd < 1000000.0 THEN 1
                    WHEN market_cap_usd >= 1000000.0 AND market_cap_usd < 3000000.0 THEN 2
                    WHEN market_cap_usd >= 3000000.0 AND market_cap_usd < 15000000.0 THEN 3
                    WHEN market_cap_usd >= 15000000.0 AND market_cap_usd < 20000000.0 THEN 4
                    ELSE 5 
                END ASC,
                whale_count DESC, 
                total_whale_usd DESC 
             LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            let token_mint: String = row.get(0)?;
            let token_symbol: String = row.get(1)?;
            let chain_str: String = row.get(2)?;
            let market_cap_usd: f64 = row.get(3)?;
            let liquidity_usd: f64 = row.get(4)?;
            let price_usd: f64 = row.get(5)?;
            let change_24h_pct: f64 = row.get(6)?;
            let change_6h_pct: f64 = row.get(7)?;
            let change_1h_pct: f64 = row.get(8)?;
            let whale_count: u32 = row.get(9)?;
            let total_whale_usd: f64 = row.get(10)?;
            let top_whale_handles: String = row.get(11)?;
            let status: String = row.get(12)?;
            let upside_tier: String = row.get(13)?;
            let last_scanned_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let last_scanned_at = DateTime::parse_from_rfc3339(&last_scanned_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(TrackedToken {
                token_mint,
                token_symbol,
                chain,
                market_cap_usd,
                liquidity_usd,
                price_usd,
                change_24h_pct,
                change_6h_pct,
                change_1h_pct,
                whale_count,
                total_whale_usd,
                top_whale_handles,
                status,
                upside_tier,
                last_scanned_at,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn get_tracked_token_by_mint(&self, mint: &str) -> Result<Option<TrackedToken>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token_mint, token_symbol, chain, market_cap_usd, liquidity_usd, price_usd,
                    change_24h_pct, change_6h_pct, change_1h_pct, whale_count, total_whale_usd,
                    top_whale_handles, status, upside_tier, last_scanned_at
             FROM tracked_tokens
             WHERE token_mint = ?1 COLLATE NOCASE LIMIT 1",
        )?;

        let mut rows = stmt.query(params![mint])?;
        if let Some(row) = rows.next()? {
            let token_mint: String = row.get(0)?;
            let token_symbol: String = row.get(1)?;
            let chain_str: String = row.get(2)?;
            let market_cap_usd: f64 = row.get(3)?;
            let liquidity_usd: f64 = row.get(4)?;
            let price_usd: f64 = row.get(5)?;
            let change_24h_pct: f64 = row.get(6)?;
            let change_6h_pct: f64 = row.get(7)?;
            let change_1h_pct: f64 = row.get(8)?;
            let whale_count: u32 = row.get(9)?;
            let total_whale_usd: f64 = row.get(10)?;
            let top_whale_handles: String = row.get(11)?;
            let status: String = row.get(12)?;
            let upside_tier: String = row.get(13)?;
            let last_scanned_str: String = row.get(14)?;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let last_scanned_at = DateTime::parse_from_rfc3339(&last_scanned_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(TrackedToken {
                token_mint,
                token_symbol,
                chain,
                market_cap_usd,
                liquidity_usd,
                price_usd,
                change_24h_pct,
                change_6h_pct,
                change_1h_pct,
                whale_count,
                total_whale_usd,
                top_whale_handles,
                status,
                upside_tier,
                last_scanned_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn clear_tracked_tokens(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM tracked_tokens", [])?;
        Ok(())
    }
}
