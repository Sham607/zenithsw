use crate::dex_adapters::{DexVenue, ScanCursor};
use crate::models::{
    ActivityType, Chain, ExecutionTrack, ShadowPosition, ShadowSignal, SmartWallet, TrackedToken,
    TradeRecord, WalletActivity, WalletTier,
};
use crate::radar::{OnChainTokenEvidence, RadarProjection};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::str::FromStr;
use std::sync::Mutex;

type RecentWhaleActivityToken = (String, String, String, u32, f64, Vec<String>);
type ExecutionIntentSummary = (String, String, Option<String>, Option<String>);
type PendingExecutionIntent = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionIntentMetadata {
    pub nonce: Option<i64>,
    pub recent_blockhash: Option<String>,
    pub last_valid_block_height: Option<i64>,
    pub calldata_hash: Option<String>,
    pub quote_block: Option<i64>,
    pub expiry: Option<i64>,
}

pub struct SmartWalletDb {
    conn: Mutex<Connection>,
    wallet_conn: Option<Mutex<Connection>>,
    demote_consecutive_losses: std::sync::atomic::AtomicU32,
    demote_catastrophic_loss_bits: std::sync::atomic::AtomicU64,
}

/// Registry staging untuk address hasil discovery/forensic sebelum promosi eksplisit.
pub struct CandidateWalletDb {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletScanAddress {
    pub chain: Chain,
    pub address: String,
    pub source_db: &'static str,
    pub is_candidate: bool,
    pub is_active: bool,
    pub tier: String,
    pub execution_track: String,
}

pub fn deduplicate_scan_addresses(
    addresses: impl IntoIterator<Item = WalletScanAddress>,
) -> Vec<WalletScanAddress> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    addresses
        .into_iter()
        .filter(|item| seen.insert((item.chain, item.address.trim().to_ascii_lowercase())))
        .collect()
}

pub fn extract_whale_alias(handle: &str, name: &str) -> String {
    let h = handle.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    if h.contains("solstice") || n.contains("solstice") {
        "solstice".to_string()
    } else if h.contains("veeman") || n.contains("veeman") {
        "veeman".to_string()
    } else if h.contains("detweiler") || n.contains("detweiler") {
        "detweiler".to_string()
    } else if h.contains("unipcs")
        || n.contains("unipcs")
        || h.contains("sniper")
        || h.contains("feeder")
    {
        "unipcs".to_string()
    } else {
        let clean = h.replace("bunker_", "");
        let first = clean.split('_').next().unwrap_or(&clean);
        if first.is_empty() {
            handle.to_string()
        } else {
            first.to_string()
        }
    }
}

impl CandidateWalletDb {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS candidates (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 rank INTEGER NOT NULL,
                 name TEXT NOT NULL,
                 handle TEXT NOT NULL,
                 chain TEXT NOT NULL,
                 address TEXT NOT NULL,
                 tier TEXT NOT NULL DEFAULT 'TIER-B',
                 pnl_24h_usd REAL NOT NULL DEFAULT 0.0,
                 win_rate_pct REAL NOT NULL DEFAULT 0.0,
                 total_trades INTEGER NOT NULL DEFAULT 0,
                 source TEXT NOT NULL,
                 qualification_status TEXT NOT NULL DEFAULT 'UNVERIFIED',
                 execution_track TEXT NOT NULL DEFAULT 'MONITOR_ONLY',
                 is_active INTEGER NOT NULL DEFAULT 0,
                 discovered_at TEXT NOT NULL,
                 last_checked_at TEXT NOT NULL,
                 UNIQUE(chain, address)
             );
             CREATE TABLE IF NOT EXISTS candidate_relations (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 candidate_id INTEGER NOT NULL,
                 parent_address TEXT,
                 cluster_id TEXT,
                 relation_type TEXT NOT NULL,
                 evidence_tx_hash TEXT,
                 evidence_block TEXT,
                 verified INTEGER NOT NULL DEFAULT 0,
                 created_at TEXT NOT NULL,
                 FOREIGN KEY(candidate_id) REFERENCES candidates(id)
             );
             CREATE INDEX IF NOT EXISTS idx_candidates_status ON candidates(qualification_status);
             CREATE INDEX IF NOT EXISTS idx_candidates_address ON candidates(chain, address);
             CREATE INDEX IF NOT EXISTS idx_candidate_relations_parent ON candidate_relations(parent_address);
             CREATE TABLE IF NOT EXISTS candidate_metrics (
                 candidate_id INTEGER PRIMARY KEY,
                 sample_transactions INTEGER NOT NULL DEFAULT 0,
                 verified_swaps INTEGER NOT NULL DEFAULT 0,
                 closed_trades INTEGER NOT NULL DEFAULT 0,
                 realized_pnl_usd REAL NOT NULL DEFAULT 0.0,
                 win_rate_pct REAL NOT NULL DEFAULT 0.0,
                 median_holding_secs INTEGER NOT NULL DEFAULT 0,
                 sniper_ratio_pct REAL NOT NULL DEFAULT 0.0,
                 mev_flag INTEGER NOT NULL DEFAULT 0,
                 last_audit_block TEXT,
                 evidence_source TEXT,
                 audited_at TEXT,
                 FOREIGN KEY(candidate_id) REFERENCES candidates(id)
             );
             CREATE TABLE IF NOT EXISTS candidate_evidence (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 candidate_id INTEGER NOT NULL,
                 evidence_type TEXT NOT NULL,
                 tx_hash TEXT,
                 block_number TEXT,
                 payload TEXT NOT NULL,
                 observed_at TEXT NOT NULL,
                 FOREIGN KEY(candidate_id) REFERENCES candidates(id)
             );
             CREATE INDEX IF NOT EXISTS idx_candidate_evidence_candidate ON candidate_evidence(candidate_id);",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_candidate(
        &self,
        wallet: &SmartWallet,
        parent_address: Option<&str>,
        cluster_id: Option<&str>,
        relation_type: &str,
        evidence_tx_hash: Option<&str>,
        evidence_block: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO candidates
             (rank,name,handle,chain,address,tier,pnl_24h_usd,win_rate_pct,total_trades,source,
              qualification_status,execution_track,is_active,discovered_at,last_checked_at)
             VALUES (?1,?2,?3,?4,?5,'TIER-B',0.0,0.0,0,?6,'UNVERIFIED','MONITOR_ONLY',0,?7,?7)
             ON CONFLICT(chain,address) DO UPDATE SET
               name=excluded.name, handle=excluded.handle, last_checked_at=excluded.last_checked_at,
               source=excluded.source",
            params![
                wallet.rank,
                wallet.name,
                wallet.handle,
                wallet.chain.to_string(),
                wallet.address,
                wallet.source,
                now
            ],
        )?;
        let candidate_id: i64 = conn.query_row(
            "SELECT id FROM candidates WHERE chain=?1 AND address=?2",
            params![wallet.chain.to_string(), wallet.address],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO candidate_relations
             (candidate_id,parent_address,cluster_id,relation_type,evidence_tx_hash,evidence_block,verified,created_at)
             VALUES (?1,?2,?3,?4,?5,?6,0,?7)",
            params![candidate_id, parent_address, cluster_id, relation_type, evidence_tx_hash, evidence_block, now],
        )?;
        Ok(())
    }

    pub fn count(&self) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.query_row("SELECT COUNT(*) FROM candidates", [], |row| row.get(0))?)
    }

    /// Membaca address kandidat untuk observasi tanpa mengubah status atau metrik.
    pub fn get_monitoring_scan_addresses(&self) -> Result<Vec<WalletScanAddress>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chain, address, tier, execution_track, is_active
             FROM candidates
             WHERE is_active = 0 AND execution_track = 'MONITOR_ONLY'
             ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WalletScanAddress {
                chain: Chain::from_str(&row.get::<_, String>(0)?).unwrap_or(Chain::Solana),
                address: row.get(1)?,
                source_db: "candidate_wallet_registry.db",
                is_candidate: true,
                is_active: row.get::<_, i32>(4)? == 1,
                tier: row.get(2)?,
                execution_track: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let exists = conn
        .prepare("SELECT 1 FROM pragma_table_info(?) WHERE name = ?")?
        .exists(params![table, column])?;
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

impl SmartWalletDb {
    pub fn new(path: &str) -> Result<Self> {
        Self::new_with_wallet_path(path, path)
    }

    pub fn new_with_wallet_path(trading_path: &str, wallet_path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(trading_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(parent) = Path::new(wallet_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(trading_path)?;
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
                 updated_at TEXT NOT NULL,
                 execution_track TEXT NOT NULL DEFAULT 'FAST_LANE'
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
                 timestamp TEXT NOT NULL,
                 source_reference TEXT,
                 parser_version TEXT,
                 finality TEXT
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
                 track TEXT NOT NULL DEFAULT 'FAST_LANE',
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
                 track TEXT NOT NULL DEFAULT 'FAST_LANE',
                 entry_time TEXT NOT NULL,
                 last_update TEXT NOT NULL,
                 status TEXT NOT NULL,
                 is_dry_run INTEGER NOT NULL DEFAULT 1
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
                 track TEXT NOT NULL DEFAULT 'FAST_LANE',
                 portion_sold_pct REAL NOT NULL DEFAULT 100.0,
                 entry_price REAL NOT NULL,
                 exit_price REAL NOT NULL,
                 pnl_pct REAL NOT NULL,
                 pnl_usd REAL NOT NULL,
                 duration_secs INTEGER NOT NULL,
                 is_dry_run INTEGER NOT NULL DEFAULT 1
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
             CREATE INDEX IF NOT EXISTS idx_signals_timestamp ON shadow_signals(timestamp);
             CREATE TABLE IF NOT EXISTS execution_intents (
                 intent_id TEXT PRIMARY KEY,
                 chain TEXT NOT NULL,
                 action TEXT NOT NULL,
                 token_mint TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 amount_native REAL NOT NULL,
                 state TEXT NOT NULL,
                 tx_hash TEXT,
                 signature TEXT,
                 nonce INTEGER,
                 recent_blockhash TEXT,
                 last_valid_block_height INTEGER,
                 calldata_hash TEXT,
                 quote_block INTEGER,
                 expiry INTEGER,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS idx_execution_intents_state ON execution_intents(state);
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_intents_tx_hash ON execution_intents(tx_hash) WHERE tx_hash IS NOT NULL;
                 CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_intents_signature ON execution_intents(signature) WHERE signature IS NOT NULL;",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS radar_evidence (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 chain TEXT NOT NULL,
                 token_address TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 evidence_kind TEXT NOT NULL,
                 tx_id TEXT,
                 observed_at TEXT NOT NULL,
                 cluster_id TEXT,
                 qualified_wallet INTEGER NOT NULL CHECK (qualified_wallet IN (0,1)),
                 UNIQUE(chain, token_address, wallet_address, evidence_kind, tx_id, observed_at)
             );
             CREATE INDEX IF NOT EXISTS idx_radar_evidence_token
                 ON radar_evidence(chain, token_address, observed_at);
             CREATE TABLE IF NOT EXISTS radar_market_snapshots (
                 chain TEXT NOT NULL,
                 token_address TEXT NOT NULL,
                 market_cap_usd REAL NOT NULL,
                 liquidity_usd REAL NOT NULL,
                 price_usd REAL,
                 observed_at TEXT NOT NULL,
                 PRIMARY KEY(chain, token_address, observed_at)
             );
             CREATE TABLE IF NOT EXISTS radar_projections (
                 chain TEXT NOT NULL,
                 token_address TEXT NOT NULL,
                 market_cap_usd REAL NOT NULL,
                 liquidity_usd REAL NOT NULL,
                 price_usd REAL,
                 qualified_holder_count INTEGER NOT NULL,
                 candidate_holder_count INTEGER NOT NULL,
                 independent_cluster_count INTEGER NOT NULL,
                 direct_accumulator_count INTEGER NOT NULL,
                 status TEXT NOT NULL,
                 observed_at TEXT NOT NULL,
                 PRIMARY KEY(chain, token_address, observed_at)
             );
             CREATE TABLE IF NOT EXISTS radar_scan_manifests (
                 scan_id TEXT PRIMARY KEY,
                 observed_at TEXT NOT NULL,
                 source TEXT NOT NULL,
                 qualified_wallet_count INTEGER NOT NULL,
                 candidate_wallet_count INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_radar_projections_observed
                 ON radar_projections(observed_at);
             CREATE TABLE IF NOT EXISTS dex_scan_cursors (
                 chain TEXT NOT NULL,
                 venue TEXT NOT NULL,
                 scope_key TEXT NOT NULL,
                 signature TEXT,
                 slot INTEGER,
                 block INTEGER,
                 transaction_index INTEGER,
                 log_index INTEGER,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY(chain, venue, scope_key)
             );
             CREATE TABLE IF NOT EXISTS activity_ipc_outbox (
                 chain TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 tx_signature TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'QUEUED',
                 created_at TEXT NOT NULL,
                 published_at TEXT,
                 PRIMARY KEY(chain, wallet_address, tx_signature)
             );
             CREATE INDEX IF NOT EXISTS idx_activity_ipc_outbox_status
                 ON activity_ipc_outbox(status, created_at);",
        )?;
        let (column, definition) = ("is_dry_run", "INTEGER NOT NULL DEFAULT 1");
        add_column_if_missing(&conn, "shadow_positions", column, definition)?;
        add_column_if_missing(&conn, "trade_history", column, definition)?;
        for (column, definition) in [
            ("source_reference", "TEXT"),
            ("parser_version", "TEXT"),
            ("finality", "TEXT"),
        ] {
            add_column_if_missing(&conn, "wallet_activities", column, definition)?;
        }
        for (column, definition) in [
            ("symbol", "TEXT"),
            ("pool_address", "TEXT"),
            ("dex_id", "TEXT"),
            ("change_24h_pct", "REAL"),
            ("change_6h_pct", "REAL"),
            ("change_1h_pct", "REAL"),
            (
                "market_structure",
                "TEXT NOT NULL DEFAULT 'UNAVAILABLE_OHLCV'",
            ),
            ("price_action", "TEXT NOT NULL DEFAULT 'UNAVAILABLE_OHLCV'"),
        ] {
            add_column_if_missing(&conn, "radar_market_snapshots", column, definition)?;
            add_column_if_missing(&conn, "radar_projections", column, definition)?;
        }
        add_column_if_missing(&conn, "radar_projections", "whales", "TEXT")?;
        for (column, definition) in [
            ("nonce", "INTEGER"),
            ("recent_blockhash", "TEXT"),
            ("last_valid_block_height", "INTEGER"),
            ("calldata_hash", "TEXT"),
            ("quote_block", "INTEGER"),
            ("expiry", "INTEGER"),
        ] {
            add_column_if_missing(&conn, "execution_intents", column, definition)?;
        }
        conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_intents_signature
             ON execution_intents(signature) WHERE signature IS NOT NULL;",
        )?;

        let wallet_conn = if trading_path == wallet_path {
            None
        } else {
            let wallet_conn = Connection::open(wallet_path)?;
            wallet_conn.execute_batch(
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
                     updated_at TEXT NOT NULL,
                     execution_track TEXT NOT NULL DEFAULT 'FAST_LANE'
                 );
                 CREATE INDEX IF NOT EXISTS idx_wallets_handle ON wallets(handle);
                 CREATE INDEX IF NOT EXISTS idx_wallets_address ON wallets(address);",
            )?;
            Some(Mutex::new(wallet_conn))
        };

        Ok(Self {
            conn: Mutex::new(conn),
            wallet_conn,
            demote_consecutive_losses: std::sync::atomic::AtomicU32::new(2),
            demote_catastrophic_loss_bits: std::sync::atomic::AtomicU64::new(30.0f64.to_bits()),
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
             CREATE TABLE IF NOT EXISTS dex_scan_cursors (
                 chain TEXT NOT NULL,
                 venue TEXT NOT NULL,
                 scope_key TEXT NOT NULL,
                 signature TEXT,
                 slot INTEGER,
                 block INTEGER,
                 transaction_index INTEGER,
                 log_index INTEGER,
                 updated_at TEXT NOT NULL,
                 PRIMARY KEY(chain, venue, scope_key)
             );
             CREATE TABLE IF NOT EXISTS activity_ipc_outbox (
                 chain TEXT NOT NULL,
                 wallet_address TEXT NOT NULL,
                 tx_signature TEXT NOT NULL,
                 payload TEXT NOT NULL,
                 status TEXT NOT NULL DEFAULT 'QUEUED',
                 created_at TEXT NOT NULL,
                 published_at TEXT,
                 PRIMARY KEY(chain, wallet_address, tx_signature)
             );
             CREATE INDEX IF NOT EXISTS idx_activity_ipc_outbox_status
                 ON activity_ipc_outbox(status, created_at);
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
                 timestamp TEXT NOT NULL,
                 source_reference TEXT,
                 parser_version TEXT,
                 finality TEXT
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
                 status TEXT NOT NULL,
                 is_dry_run INTEGER NOT NULL DEFAULT 1
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
                 duration_secs INTEGER NOT NULL,
                 is_dry_run INTEGER NOT NULL DEFAULT 1
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

        // Migrations for Dual-Track Execution. An existing column is not an
        // error; every other schema error must stop startup.
        add_column_if_missing(
            &conn,
            "wallets",
            "execution_track",
            "TEXT NOT NULL DEFAULT 'FAST_LANE'",
        )?;
        add_column_if_missing(
            &conn,
            "shadow_positions",
            "track",
            "TEXT NOT NULL DEFAULT 'FAST_LANE'",
        )?;
        add_column_if_missing(
            &conn,
            "shadow_signals",
            "track",
            "TEXT NOT NULL DEFAULT 'FAST_LANE'",
        )?;
        add_column_if_missing(
            &conn,
            "trade_history",
            "track",
            "TEXT NOT NULL DEFAULT 'FAST_LANE'",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS execution_intents (
                intent_id TEXT PRIMARY KEY,
                chain TEXT NOT NULL,
                action TEXT NOT NULL,
                token_mint TEXT NOT NULL,
                wallet_address TEXT NOT NULL,
                amount_native REAL NOT NULL,
                state TEXT NOT NULL,
                tx_hash TEXT,
                signature TEXT,
                nonce INTEGER,
                recent_blockhash TEXT,
                last_valid_block_height INTEGER,
                calldata_hash TEXT,
                quote_block INTEGER,
                expiry INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_execution_intents_state ON execution_intents(state);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_intents_tx_hash ON execution_intents(tx_hash) WHERE tx_hash IS NOT NULL;
            CREATE UNIQUE INDEX IF NOT EXISTS idx_execution_intents_signature ON execution_intents(signature) WHERE signature IS NOT NULL;",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
            wallet_conn: None,
            demote_consecutive_losses: std::sync::atomic::AtomicU32::new(2),
            demote_catastrophic_loss_bits: std::sync::atomic::AtomicU64::new(30.0f64.to_bits()),
        })
    }

    fn wallet_db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.wallet_conn
            .as_ref()
            .unwrap_or(&self.conn)
            .lock()
            .unwrap()
    }

    pub fn save_dex_cursor(
        &self,
        chain: Chain,
        venue: DexVenue,
        scope_key: &str,
        cursor: &ScanCursor,
    ) -> Result<()> {
        if scope_key.trim().is_empty() || venue.chain() != chain {
            anyhow::bail!("scope cursor DEX tidak valid");
        }
        cursor.validate()?;
        let (signature, slot, block, transaction_index, log_index) = match cursor {
            ScanCursor::Solana { signature, slot } if chain == Chain::Solana => (
                Some(signature.as_str()),
                Some(*slot as i64),
                None,
                None,
                None,
            ),
            ScanCursor::Robinhood {
                block,
                transaction_index,
                log_index,
            } if chain == Chain::Robinhood => (
                None,
                None,
                Some(*block as i64),
                Some(*transaction_index as i64),
                Some(*log_index as i64),
            ),
            _ => anyhow::bail!("jenis cursor tidak cocok dengan chain"),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO dex_scan_cursors
             (chain, venue, scope_key, signature, slot, block, transaction_index, log_index, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(chain, venue, scope_key) DO UPDATE SET
                signature = excluded.signature,
                slot = excluded.slot,
                block = excluded.block,
                transaction_index = excluded.transaction_index,
                log_index = excluded.log_index,
                updated_at = excluded.updated_at",
            params![
                chain.to_string(), venue.name(), scope_key, signature, slot, block,
                transaction_index, log_index, Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn load_dex_cursor(
        &self,
        chain: Chain,
        venue: DexVenue,
        scope_key: &str,
    ) -> Result<Option<ScanCursor>> {
        if scope_key.trim().is_empty() || venue.chain() != chain {
            anyhow::bail!("scope cursor DEX tidak valid");
        }
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT signature, slot, block, transaction_index, log_index
                 FROM dex_scan_cursors WHERE chain = ?1 AND venue = ?2 AND scope_key = ?3",
                params![chain.to_string(), venue.name(), scope_key],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((signature, slot, block, transaction_index, log_index)) = row else {
            return Ok(None);
        };
        let cursor = match (signature, slot, block, transaction_index, log_index) {
            (Some(signature), Some(slot), None, None, None) if slot > 0 => ScanCursor::Solana {
                signature,
                slot: slot as u64,
            },
            (None, None, Some(block), Some(transaction_index), Some(log_index))
                if block > 0 && transaction_index >= 0 && log_index >= 0 =>
            {
                ScanCursor::Robinhood {
                    block: block as u64,
                    transaction_index: transaction_index as u32,
                    log_index: log_index as u32,
                }
            }
            _ => anyhow::bail!("cursor DEX tersimpan malformed"),
        };
        cursor.validate()?;
        Ok(Some(cursor))
    }

    pub fn upsert_wallet(&self, wallet: &SmartWallet) -> Result<()> {
        let updated_at_str = wallet.updated_at.to_rfc3339();
        let last_seen_str = wallet.last_seen_at.map(|t| t.to_rfc3339());
        let chain_str = wallet.chain.to_string();
        let tier_str = wallet.tier.to_string();
        let track_str = wallet.execution_track.to_string();

        let conn = self.wallet_db();
        conn.execute(
            "INSERT INTO wallets (
                rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
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
                updated_at = excluded.updated_at,
                execution_track = excluded.execution_track",
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
                track_str,
            ],
        )?;
        Ok(())
    }

    /// Membaca wallet operasional yang qualified untuk observasi on-chain.
    pub fn get_qualified_scan_addresses(&self) -> Result<Vec<WalletScanAddress>> {
        let conn = self.wallet_db();
        let mut stmt = conn.prepare(
            "SELECT chain, address, tier, execution_track, is_active
             FROM wallets
             WHERE is_active = 1 AND tier IN ('TIER-S', 'TIER-A', 'TierS', 'TierA')
             ORDER BY pnl_24h_usd DESC, rank ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(WalletScanAddress {
                chain: Chain::from_str(&row.get::<_, String>(0)?).unwrap_or(Chain::Solana),
                address: row.get(1)?,
                source_db: "wallet_registry.db",
                is_candidate: false,
                is_active: row.get::<_, i32>(4)? == 1,
                tier: row.get(2)?,
                execution_track: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Membaca pemetaan wallet address ke alias whale ringkas untuk tampilan radar.
    pub fn get_wallet_alias_map(&self) -> Result<std::collections::HashMap<String, String>> {
        let conn = self.wallet_db();
        let mut stmt =
            conn.prepare("SELECT address, handle, name FROM wallets WHERE is_active = 1")?;
        let rows = stmt.query_map([], |row| {
            let address: String = row.get(0)?;
            let handle: String = row.get(1)?;
            let name: String = row.get(2)?;
            let alias = extract_whale_alias(&handle, &name);
            Ok((address.trim().to_ascii_lowercase(), alias))
        })?;
        let mut map = std::collections::HashMap::new();
        for r in rows {
            let (addr, alias) = r?;
            map.insert(addr, alias);
        }
        Ok(map)
    }

    pub fn get_active_wallets(&self) -> Result<Vec<SmartWallet>> {
        let conn = self.wallet_db();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track
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
            let track_str: String = row.get(15).unwrap_or_else(|_| "FAST_LANE".to_string());
            let execution_track =
                ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

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
                execution_track,
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
        let conn = self.wallet_db();
        let tier_str1 = tier.to_string();
        let tier_str2 = format!("Tier{}", tier_str1.trim_start_matches("TIER-"));
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track
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
            let track_str: String = row.get(15).unwrap_or_else(|_| "FAST_LANE".to_string());
            let execution_track =
                ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

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
                execution_track,
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
        let conn = self.wallet_db();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track
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
            let track_str: String = row.get(15).unwrap_or_else(|_| "FAST_LANE".to_string());
            let execution_track =
                ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

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
                execution_track,
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
        let conn = self.wallet_db();
        let mut stmt = conn.prepare(
            "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track
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
            let track_str: String = row.get(15).unwrap_or_else(|_| "FAST_LANE".to_string());
            let execution_track =
                ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

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
                execution_track,
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

    pub fn insert_radar_evidence_batch(
        &self,
        evidence_list: &[OnChainTokenEvidence],
    ) -> Result<()> {
        if evidence_list.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO radar_evidence
                 (chain, token_address, wallet_address, evidence_kind, tx_id, observed_at, cluster_id, qualified_wallet)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for evidence in evidence_list {
                stmt.execute(params![
                    evidence.chain.to_string(),
                    evidence.token_address,
                    evidence.wallet_address,
                    format!("{:?}", evidence.kind),
                    evidence.tx_id,
                    evidence.observed_at.to_rfc3339(),
                    evidence.cluster_id,
                    evidence.qualified_wallet as i32,
                ])?;
            }
        }
        let _ = tx.execute(
            "DELETE FROM radar_evidence WHERE observed_at < datetime('now', '-24 hours')",
            [],
        );
        tx.commit()?;
        Ok(())
    }

    pub fn insert_radar_evidence(&self, evidence: &OnChainTokenEvidence) -> Result<()> {
        self.insert_radar_evidence_batch(std::slice::from_ref(evidence))
    }

    pub fn insert_radar_market_snapshot(
        &self,
        snapshot: &crate::radar::RadarMarketSnapshot,
        token_address: &str,
        chain: Chain,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO radar_market_snapshots
             (chain, token_address, symbol, pool_address, dex_id, market_cap_usd, liquidity_usd, price_usd,
              change_24h_pct, change_6h_pct, change_1h_pct, market_structure, price_action, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                chain.to_string(),
                token_address,
                snapshot.symbol,
                snapshot.pool_address,
                snapshot.dex_id,
                snapshot.market_cap_usd,
                snapshot.liquidity_usd,
                snapshot.price_usd,
                snapshot.change_24h_pct,
                snapshot.change_6h_pct,
                snapshot.change_1h_pct,
                snapshot.market_structure,
                snapshot.price_action,
                snapshot.observed_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_radar_projection(
        &self,
        projection: &RadarProjection,
        observed_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO radar_projections
             (chain, token_address, symbol, pool_address, dex_id, market_cap_usd, liquidity_usd, price_usd,
              change_24h_pct, change_6h_pct, change_1h_pct, market_structure, price_action,
              qualified_holder_count, candidate_holder_count, independent_cluster_count,
              direct_accumulator_count, status, whales, observed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                projection.chain.to_string(),
                projection.token_address,
                projection.symbol,
                projection.pool_address,
                projection.dex_id,
                projection.market_cap_usd,
                projection.liquidity_usd,
                projection.price_usd,
                projection.change_24h_pct,
                projection.change_6h_pct,
                projection.change_1h_pct,
                projection.market_structure,
                projection.price_action,
                projection.qualified_holder_count as i64,
                projection.candidate_holder_count as i64,
                projection.independent_cluster_count as i64,
                projection.direct_accumulator_count as i64,
                format!("{:?}", projection.status),
                projection.whales,
                observed_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn insert_activity(&self, act: &WalletActivity) -> Result<()> {
        self.insert_activity_if_new(act).map(|_| ())
    }

    pub fn insert_activity_if_new(&self, act: &WalletActivity) -> Result<bool> {
        let ts_str = act.timestamp.to_rfc3339();
        let chain_str = act.chain.to_string();
        let act_str = act.activity_type.to_string();

        let conn = self.conn.lock().unwrap();
        let already_exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM wallet_activities
                WHERE chain = ?1 AND wallet_address = ?2 AND tx_signature = ?3
            )",
            params![chain_str, act.wallet_address, act.tx_signature],
            |row| row.get(0),
        )?;
        if already_exists {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO wallet_activities (
                wallet_handle, wallet_address, chain, activity_type, token_symbol, token_mint, amount_usd, token_amount, execution_price_usd, whale_balance_before, whale_balance_after, sell_ratio_pct, tx_signature, timestamp, source_reference, parser_version, finality
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                act.source_reference,
                act.parser_version,
                act.finality,
            ],
        )?;

        // Update last seen in wallet
        conn.execute(
            "UPDATE wallets SET last_seen_at = ?1 WHERE handle = ?2 OR address = ?3",
            params![ts_str, act.wallet_handle, act.wallet_address],
        )?;

        Ok(true)
    }

    pub fn enqueue_activity_ipc(&self, act: &WalletActivity) -> Result<bool> {
        let payload = serde_json::to_string(act)?;
        let conn = self.conn.lock().unwrap();
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO activity_ipc_outbox
             (chain, wallet_address, tx_signature, payload, status, created_at)
             VALUES (?1, ?2, ?3, ?4, 'QUEUED', ?5)",
            params![
                act.chain.to_string(),
                act.wallet_address,
                act.tx_signature,
                payload,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(inserted == 1)
    }

    pub fn queued_activity_ipc(&self, limit: usize) -> Result<Vec<WalletActivity>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT payload FROM activity_ipc_outbox
             WHERE status = 'QUEUED' ORDER BY created_at ASC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        let payloads = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        payloads
            .into_iter()
            .map(|payload| Ok(serde_json::from_str(&payload)?))
            .collect()
    }

    pub fn mark_activity_ipc_published(&self, act: &WalletActivity) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let updated = conn.execute(
            "UPDATE activity_ipc_outbox
             SET status = 'PUBLISHED_TO_IPC', published_at = ?1
             WHERE chain = ?2 AND wallet_address = ?3 AND tx_signature = ?4
               AND status = 'QUEUED'",
            params![
                Utc::now().to_rfc3339(),
                act.chain.to_string(),
                act.wallet_address,
                act.tx_signature,
            ],
        )?;
        Ok(updated == 1)
    }

    pub fn get_recent_activities(&self, limit: usize) -> Result<Vec<WalletActivity>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, wallet_handle, wallet_address, chain, activity_type, token_symbol, token_mint, amount_usd, token_amount, execution_price_usd, whale_balance_before, whale_balance_after, sell_ratio_pct, tx_signature, timestamp, source_reference, parser_version, finality
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
                source_reference: row.get(15)?,
                parser_version: row.get(16)?,
                finality: row.get(17)?,
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
                chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, track, timestamp
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                sig.track.to_string(),
                ts_str,
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_signals(&self, limit: usize) -> Result<Vec<ShadowSignal>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp, track
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
            let track_str: String = row.get(14).unwrap_or_else(|_| "FAST_LANE".to_string());
            let track = ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

            Ok(ShadowSignal {
                chain,
                token_symbol,
                token_mint,
                whale_handle,
                whale_address,
                whale_tier,
                track,
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
        let is_dry_run_val = if pos.is_dry_run { 1i64 } else { 0i64 };
        conn.execute(
            "INSERT INTO shadow_positions (
                id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, track, entry_time, last_update, status, is_dry_run
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
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
                track = excluded.track,
                last_update = excluded.last_update,
                status = excluded.status,
                is_dry_run = excluded.is_dry_run",
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
                pos.track.to_string(),
                entry_str,
                update_str,
                pos.status,
                is_dry_run_val,
            ],
        )?;
        Ok(())
    }

    pub fn get_active_positions(&self) -> Result<Vec<ShadowPosition>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status, track, is_dry_run
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
            let track_str: String = row.get(17).unwrap_or_else(|_| "FAST_LANE".to_string());
            let is_dry_run_int: i64 = row.get(18).unwrap_or(1);
            let is_dry_run = is_dry_run_int != 0;

            let chain = Chain::from_str(&chain_str).unwrap_or(Chain::Solana);
            let entry_time = DateTime::parse_from_rfc3339(&entry_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let last_update = DateTime::parse_from_rfc3339(&update_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let track = ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

            Ok(ShadowPosition {
                id,
                chain,
                token_symbol,
                token_mint,
                target_whale_handle,
                target_whale_address,
                track,
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
                is_dry_run,
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
        if !portion_pct.is_finite()
            || portion_pct <= 0.0
            || portion_pct > 100.0
            || !exit_price.is_finite()
            || exit_price <= 0.0
        {
            return Err(anyhow::anyhow!(
                "invalid exit parameters: portion_pct={}, exit_price={}",
                portion_pct,
                exit_price
            ));
        }
        let portion_clamped = portion_pct;
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut stmt = tx.prepare("SELECT entry_price, amount_usd, remaining_portion_pct, realized_pnl_usd, entry_time, chain, token_symbol, token_mint, target_whale_handle, track, is_dry_run FROM shadow_positions WHERE id = ?1")?;
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
                let track_s: String = row.get(9).unwrap_or_else(|_| "FAST_LANE".to_string());
                let is_dry_run_int: i64 = row.get(10).unwrap_or(1);
                let is_dry_run = is_dry_run_int != 0;
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
                    track_s,
                    is_dry_run,
                ))
            })
            .optional()?;

        let mut whale_to_check: Option<String> = None;

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
            track_s,
            is_dry_run,
        )) = pos_opt
        {
            whale_to_check = Some(target_whale.clone());
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

            let is_full_close = new_remaining_portion <= 1.0;
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
            let is_dry_run_val = if is_dry_run { 1i64 } else { 0i64 };
            tx.execute(
                "INSERT INTO trade_history (
                    timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs, track, is_dry_run
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                    track_s,
                    is_dry_run_val,
                ],
            )?;

            // Update shadow_positions
            tx.execute(
                "UPDATE shadow_positions SET status = ?1, remaining_portion_pct = ?2, realized_pnl_usd = ?3, last_update = ?4 WHERE id = ?5",
                params![new_status, new_remaining_portion, new_realized_pnl, now.to_rfc3339(), id],
            )?;
        }
        drop(stmt);
        tx.commit()?;
        drop(conn);

        if let Some(target_whale) = whale_to_check {
            let _ = self.check_and_demote_underperforming_whale(&target_whale);
        }

        Ok(())
    }

    pub fn set_demote_thresholds(&self, consecutive_losses: u32, catastrophic_loss_pct: f64) {
        self.demote_consecutive_losses
            .store(consecutive_losses, std::sync::atomic::Ordering::Relaxed);
        self.demote_catastrophic_loss_bits.store(
            catastrophic_loss_pct.to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// Checks the last N trades attributed to this whale in trade_history.
    /// If consecutive losses or a single catastrophic loss occurred, demotes the whale
    /// in the wallet registry (tier = TIER-B, is_active = 0).
    pub fn check_and_demote_underperforming_whale(
        &self,
        target_whale: &str,
    ) -> Result<Option<String>> {
        if target_whale.trim().is_empty() {
            return Ok(None);
        }

        let consecutive_target = self
            .demote_consecutive_losses
            .load(std::sync::atomic::Ordering::Relaxed)
            .max(1) as usize;
        let catastrophic_thresh = f64::from_bits(
            self.demote_catastrophic_loss_bits
                .load(std::sync::atomic::Ordering::Relaxed),
        );

        let pnl_history = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT pnl_pct FROM trade_history WHERE target_whale = ?1 ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![target_whale, consecutive_target], |r| {
                r.get::<_, f64>(0)
            })?;
            let res: Vec<f64> = rows.filter_map(|r| r.ok()).collect();
            res
        };

        if pnl_history.is_empty() {
            return Ok(None);
        }

        let mut demotion_reason = None;

        // Condition A: consecutive losses
        if pnl_history.len() >= consecutive_target && pnl_history.iter().all(|&p| p < 0.0) {
            demotion_reason = Some(format!(
                "DEMOTED_CONSECUTIVE_LOSSES (count: {}, last: {:.1}%)",
                consecutive_target, pnl_history[0]
            ));
        } else if pnl_history[0] <= -catastrophic_thresh {
            // Condition B: Single catastrophic loss >= threshold
            demotion_reason = Some(format!(
                "DEMOTED_CATASTROPHIC_LOSS (drawdown: {:.1}% <= -{:.1}%)",
                pnl_history[0], catastrophic_thresh
            ));
        }

        if let Some(ref reason) = demotion_reason {
            let wallet_conn = self.wallet_db();
            wallet_conn.execute(
                "UPDATE wallets SET tier = 'TIER-B', is_active = 0 WHERE handle = ?1 OR address = ?1",
                params![target_whale],
            )?;
            println!(
                "[WALLET DEMOTION 🛑] Whale '{}' demoted to TIER-B (is_active=0). Reason: {}",
                target_whale, reason
            );
        }

        Ok(demotion_reason)
    }

    pub fn close_position(&self, id: &str, exit_reason: &str, exit_price: f64) -> Result<()> {
        self.close_position_portion(id, 100.0, exit_reason, exit_price)
    }

    pub fn get_recent_trades(&self, limit: usize) -> Result<Vec<TradeRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs, track
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
            let track_str: String = row.get(13).unwrap_or_else(|_| "FAST_LANE".to_string());
            let track = ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

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
                track,
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
            "SELECT COUNT(DISTINCT wallet_address) FROM wallet_activities
             WHERE token_mint = ?1 AND activity_type = 'BUY' AND timestamp >= ?2",
            params![mint, since],
            |row| row.get(0),
        )?;
        Ok(count as u32)
    }

    pub fn delete_tracked_token(&self, mint: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM tracked_tokens WHERE token_mint = ?1",
            params![mint],
        )?;
        Ok(())
    }

    pub fn prune_stale_tracked_tokens(
        &self,
        active_mints: &[String],
        max_age_seconds: i64,
    ) -> Result<usize> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (Utc::now() - chrono::Duration::seconds(max_age_seconds)).to_rfc3339();

        let removed = if active_mints.is_empty() {
            conn.execute(
                "DELETE FROM tracked_tokens WHERE last_scanned_at < ?1",
                params![cutoff],
            )?
        } else {
            let placeholders = active_mints
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "DELETE FROM tracked_tokens WHERE last_scanned_at < ?1 OR token_mint NOT IN ({})",
                placeholders
            );
            let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            params_vec.push(Box::new(cutoff));
            for m in active_mints {
                params_vec.push(Box::new(m.clone()));
            }
            let slice: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();
            conn.execute(&sql, slice.as_slice())?
        };
        Ok(removed)
    }

    pub fn record_trade(&self, trade: &TradeRecord) -> Result<String> {
        let now = Utc::now();
        let chain_str = trade.chain.to_string();
        let track_str = trade.track.to_string();
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "INSERT INTO trade_history (
                timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs, track
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )?;
        stmt.execute(params![
            now.to_rfc3339(),
            chain_str,
            trade.action,
            trade.symbol,
            trade.mint,
            trade.target_whale,
            trade.exit_reason,
            trade.portion_sold_pct,
            trade.entry_price,
            trade.exit_price,
            trade.pnl_pct,
            trade.pnl_usd,
            trade.duration_secs as i64,
            track_str,
        ])?;
        Ok(now.to_rfc3339())
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
    ) -> Result<Vec<RecentWhaleActivityToken>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT token_mint,
                    MAX(token_symbol) as token_symbol,
                    MAX(chain) as chain,
                    COUNT(DISTINCT wallet_address) as whale_count,
                    SUM(amount_usd) as total_usd,
                    GROUP_CONCAT(DISTINCT wallet_handle) as handles
             FROM wallet_activities
             WHERE amount_usd > 0.0 AND amount_usd <= 100000.0 AND token_mint != '' AND token_mint != 'NATIVE'
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

    pub fn get_radar_projections(&self, limit: usize) -> Result<Vec<RadarProjection>> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (Utc::now() - chrono::Duration::hours(24)).to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT chain, token_address, symbol, pool_address, dex_id, market_cap_usd, liquidity_usd, price_usd,
                    change_24h_pct, change_6h_pct, change_1h_pct, market_structure, price_action,
                    qualified_holder_count, candidate_holder_count, independent_cluster_count,
                    direct_accumulator_count, status, whales
             FROM radar_projections AS current
             WHERE current.observed_at >= ?1
               AND NOT EXISTS (
                   SELECT 1
                   FROM radar_projections AS newer
                   WHERE newer.chain = current.chain
                     AND newer.token_address = current.token_address
                     AND newer.observed_at >= ?1
                     AND newer.observed_at > current.observed_at
               )
             ORDER BY current.observed_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![cutoff, limit], |row| {
            let chain = Chain::from_str(&row.get::<_, String>(0)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let status = match row.get::<_, String>(17)?.as_str() {
                "PortfolioOnly" => crate::radar::RadarStatus::PortfolioOnly,
                "StructureConfirmed" => crate::radar::RadarStatus::StructureConfirmed,
                "HighPriorityRadar" => crate::radar::RadarStatus::HighPriorityRadar,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            let whales: Option<String> = row.get(18).ok();
            Ok(RadarProjection {
                chain,
                token_address: row.get(1)?,
                symbol: row.get(2)?,
                pool_address: row.get(3)?,
                dex_id: row.get(4)?,
                market_cap_usd: row.get(5)?,
                liquidity_usd: row.get(6)?,
                price_usd: row.get(7)?,
                change_24h_pct: row.get(8)?,
                change_6h_pct: row.get(9)?,
                change_1h_pct: row.get(10)?,
                market_structure: row.get(11)?,
                price_action: row.get(12)?,
                qualified_holder_count: row.get::<_, i64>(13)? as usize,
                candidate_holder_count: row.get::<_, i64>(14)? as usize,
                independent_cluster_count: row.get::<_, i64>(15)? as usize,
                direct_accumulator_count: row.get::<_, i64>(16)? as usize,
                whales,
                status,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_tracked_tokens(&self, limit: usize) -> Result<Vec<TrackedToken>> {
        let conn = self.conn.lock().unwrap();
        let cutoff = (Utc::now() - chrono::Duration::minutes(15)).to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT token_mint, token_symbol, chain, market_cap_usd, liquidity_usd, price_usd,
                    change_24h_pct, change_6h_pct, change_1h_pct, whale_count, total_whale_usd,
                    top_whale_handles, status, upside_tier, last_scanned_at
             FROM tracked_tokens
             WHERE market_cap_usd >= 50000.0 AND market_cap_usd <= 20000000.0 AND liquidity_usd >= 30000.0 AND last_scanned_at >= ?1
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
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![cutoff, limit], |row| {
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

    pub fn create_execution_intent(
        &self,
        intent_id: &str,
        chain: Chain,
        action: &str,
        token_mint: &str,
        wallet_address: &str,
        amount_native: f64,
    ) -> Result<bool> {
        if intent_id.trim().is_empty()
            || action.trim().is_empty()
            || token_mint.trim().is_empty()
            || wallet_address.trim().is_empty()
            || !amount_native.is_finite()
            || amount_native <= 0.0
        {
            return Err(anyhow::anyhow!("invalid execution intent"));
        }
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        let inserted = conn.execute(
            "INSERT OR IGNORE INTO execution_intents
             (intent_id,chain,action,token_mint,wallet_address,amount_native,state,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,'INTENT',?7,?7)",
            params![intent_id, chain.to_string(), action, token_mint, wallet_address, amount_native, now],
        )?;
        Ok(inserted == 1)
    }

    pub fn update_execution_intent(
        &self,
        intent_id: &str,
        state: &str,
        tx_hash: Option<&str>,
        signature: Option<&str>,
    ) -> Result<()> {
        if intent_id.trim().is_empty() || state.trim().is_empty() {
            return Err(anyhow::anyhow!("invalid execution intent update"));
        }
        let conn = self.conn.lock().unwrap();
        let current: Option<String> = conn
            .query_row(
                "SELECT state FROM execution_intents WHERE intent_id=?1",
                params![intent_id],
                |row| row.get(0),
            )
            .optional()?;
        let current =
            current.ok_or_else(|| anyhow::anyhow!("execution intent not found: {intent_id}"))?;
        let allowed = match (current.as_str(), state) {
            (
                "INTENT",
                "QUOTED" | "SIMULATED" | "SIGNED" | "BROADCASTED" | "FAILED" | "UNKNOWN",
            ) => true,
            ("QUOTED", "SIMULATED" | "SIGNED" | "FAILED" | "UNKNOWN") => true,
            ("SIMULATED", "SIGNED" | "FAILED" | "UNKNOWN") => true,
            ("SIGNED", "BROADCASTED" | "FAILED" | "UNKNOWN") => true,
            ("BROADCASTED", "PENDING_RECONCILIATION" | "CONFIRMED" | "FAILED" | "UNKNOWN") => true,
            ("PENDING_RECONCILIATION", "CONFIRMED" | "RECONCILED" | "FAILED") => true,
            ("CONFIRMED", "RECONCILED" | "FAILED") => true,
            ("RECONCILED", "FAILED") => true,
            ("FAILED" | "CLOSED", _) => false,
            _ => false,
        };
        if !allowed {
            return Err(anyhow::anyhow!(
                "invalid execution intent transition: {current} -> {state}"
            ));
        }
        let existing_hashes: (Option<String>, Option<String>) = conn.query_row(
            "SELECT tx_hash, signature FROM execution_intents WHERE intent_id=?1",
            params![intent_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if tx_hash.is_some_and(|value| existing_hashes.0.as_deref().is_some_and(|old| old != value))
            || signature
                .is_some_and(|value| existing_hashes.1.as_deref().is_some_and(|old| old != value))
        {
            return Err(anyhow::anyhow!(
                "replacement tx/signature ditolak untuk execution intent: {intent_id}"
            ));
        }
        let changed = conn.execute(
            "UPDATE execution_intents
             SET state=?1,tx_hash=coalesce(?2,tx_hash),signature=coalesce(?3,signature),updated_at=?4
             WHERE intent_id=?5",
            params![state, tx_hash, signature, Utc::now().to_rfc3339(), intent_id],
        )?;
        if changed != 1 {
            return Err(anyhow::anyhow!(
                "execution intent update affected no row: {intent_id}"
            ));
        }
        Ok(())
    }

    pub fn update_execution_intent_metadata(
        &self,
        intent_id: &str,
        metadata: &ExecutionIntentMetadata,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE execution_intents
             SET nonce=?1,recent_blockhash=?2,last_valid_block_height=?3,
                 calldata_hash=?4,quote_block=?5,expiry=?6,updated_at=?7
             WHERE intent_id=?8",
            params![
                metadata.nonce,
                metadata.recent_blockhash,
                metadata.last_valid_block_height,
                metadata.calldata_hash,
                metadata.quote_block,
                metadata.expiry,
                Utc::now().to_rfc3339(),
                intent_id,
            ],
        )?;
        if changed != 1 {
            return Err(anyhow::anyhow!("execution intent not found: {intent_id}"));
        }
        Ok(())
    }

    pub fn get_execution_intent_metadata(
        &self,
        intent_id: &str,
    ) -> Result<Option<ExecutionIntentMetadata>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT nonce,recent_blockhash,last_valid_block_height,calldata_hash,quote_block,expiry
                 FROM execution_intents WHERE intent_id=?1",
                params![intent_id],
                |row| {
                    Ok(ExecutionIntentMetadata {
                        nonce: row.get(0)?,
                        recent_blockhash: row.get(1)?,
                        last_valid_block_height: row.get(2)?,
                        calldata_hash: row.get(3)?,
                        quote_block: row.get(4)?,
                        expiry: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn get_execution_intent(&self, intent_id: &str) -> Result<Option<ExecutionIntentSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT state,action,tx_hash,signature FROM execution_intents WHERE intent_id=?1",
        )?;
        Ok(stmt
            .query_row(params![intent_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .optional()?)
    }

    pub fn get_pending_execution_intents(&self) -> Result<Vec<PendingExecutionIntent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT intent_id,chain,action,token_mint,wallet_address,state,tx_hash,signature
             FROM execution_intents
             WHERE state IN ('INTENT','QUOTED','SIMULATED','SIGNED','BROADCASTED','PENDING_RECONCILIATION')
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?;
        let mut intents = Vec::new();
        for row in rows {
            intents.push(row?);
        }
        Ok(intents)
    }
}

#[cfg(test)]
mod scan_address_tests {
    use super::{deduplicate_scan_addresses, WalletScanAddress};
    use crate::models::Chain;

    #[test]
    fn deduplicates_by_chain_and_canonical_address() {
        let items = vec![
            WalletScanAddress {
                chain: Chain::Robinhood,
                address: "0xABC".to_string(),
                source_db: "wallet_registry.db",
                is_candidate: false,
                is_active: true,
                tier: "TIER-S".to_string(),
                execution_track: "FAST_LANE".to_string(),
            },
            WalletScanAddress {
                chain: Chain::Robinhood,
                address: " 0xabc ".to_string(),
                source_db: "candidate_wallet_registry.db",
                is_candidate: true,
                is_active: false,
                tier: "TIER-B".to_string(),
                execution_track: "MONITOR_ONLY".to_string(),
            },
            WalletScanAddress {
                chain: Chain::Solana,
                address: "0xabc".to_string(),
                source_db: "candidate_wallet_registry.db",
                is_candidate: true,
                is_active: false,
                tier: "TIER-B".to_string(),
                execution_track: "MONITOR_ONLY".to_string(),
            },
        ];

        let result = deduplicate_scan_addresses(items);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_db, "wallet_registry.db");
    }

    #[test]
    fn extracts_canonical_whale_alias() {
        use super::extract_whale_alias;

        assert_eq!(
            extract_whale_alias("bunker_unipcs_main_01", "@unipcs"),
            "unipcs"
        );
        assert_eq!(
            extract_whale_alias("bunker_sniper_01", "Unipcs Sniper"),
            "unipcs"
        );
        assert_eq!(
            extract_whale_alias("bunker_solstice_feeder_01", "Solstice Feeder"),
            "solstice"
        );
        assert_eq!(
            extract_whale_alias("bunker_veeman_main_01", "@theveeman"),
            "veeman"
        );
        assert_eq!(
            extract_whale_alias("bunker_detweiler_sat_01", "@0xdetweiler"),
            "detweiler"
        );
    }
}
