//! Kontrak fondasi DEX-specific untuk Smart Feed.
//! Modul ini hanya mendefinisikan identity, cursor, dan classifier read-only.
//! Decoder venue tidak boleh diaktifkan tanpa evidence ABI/program dan fixture nyata.

use crate::config::Config;
use crate::models::{ActivityType, Chain, WalletActivity};
use crate::radar_activity::{NormalizedActivity, VerifiedVenue};
use crate::radar_solana_activity::SolanaTransactionReference;
use anyhow::{bail, Result};
use chrono::Utc;
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DexVenue {
    SolanaRaydiumAmm,
    SolanaRaydiumCpmm,
    SolanaRaydiumClmm,
    SolanaMeteora,
    SolanaOrcaWhirlpool,
    SolanaPumpFun,
    SolanaPumpSwap,
    SolanaJupiterRouter,
    RobinhoodUniswapV3,
    RobinhoodUniswapV4,
    RobinhoodPons,
    RobinhoodLong,
    RobinhoodO1,
    RobinhoodPoolsTrade,
    RobinhoodPair,
    RobinhoodBags,
}

impl DexVenue {
    pub const fn chain(self) -> Chain {
        match self {
            Self::SolanaRaydiumAmm
            | Self::SolanaRaydiumCpmm
            | Self::SolanaRaydiumClmm
            | Self::SolanaMeteora
            | Self::SolanaOrcaWhirlpool
            | Self::SolanaPumpFun
            | Self::SolanaPumpSwap
            | Self::SolanaJupiterRouter => Chain::Solana,
            Self::RobinhoodUniswapV3
            | Self::RobinhoodUniswapV4
            | Self::RobinhoodPons
            | Self::RobinhoodLong
            | Self::RobinhoodO1
            | Self::RobinhoodPoolsTrade
            | Self::RobinhoodPair
            | Self::RobinhoodBags => Chain::Robinhood,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::SolanaRaydiumAmm => "SOLANA/RAYDIUM_AMM",
            Self::SolanaRaydiumCpmm => "SOLANA/RAYDIUM_CPMM",
            Self::SolanaRaydiumClmm => "SOLANA/RAYDIUM_CLMM",
            Self::SolanaMeteora => "SOLANA/METEORA",
            Self::SolanaOrcaWhirlpool => "SOLANA/ORCA_WHIRLPOOL",
            Self::SolanaPumpFun => "SOLANA/PUMP_FUN",
            Self::SolanaPumpSwap => "SOLANA/PUMP_SWAP",
            Self::SolanaJupiterRouter => "SOLANA/JUPITER_ROUTER",
            Self::RobinhoodUniswapV3 => "ROBINHOOD/UNISWAP_V3",
            Self::RobinhoodUniswapV4 => "ROBINHOOD/UNISWAP_V4",
            Self::RobinhoodPons => "ROBINHOOD/PONS",
            Self::RobinhoodLong => "ROBINHOOD/LONG",
            Self::RobinhoodO1 => "ROBINHOOD/O1",
            Self::RobinhoodPoolsTrade => "ROBINHOOD/POOLS_TRADE",
            Self::RobinhoodPair => "ROBINHOOD/PAIR",
            Self::RobinhoodBags => "ROBINHOOD/BAGS",
        }
    }

    pub const fn is_router(self) -> bool {
        matches!(self, Self::SolanaJupiterRouter)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolanaProgramSpec {
    pub venue: DexVenue,
    pub program_id: &'static str,
    pub status: &'static str,
}

pub const SOLANA_PROGRAM_SPECS: &[SolanaProgramSpec] = &[
    SolanaProgramSpec {
        venue: DexVenue::SolanaJupiterRouter,
        program_id: "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
        status: "VERIFIED_ALLOWLIST",
    },
    SolanaProgramSpec {
        venue: DexVenue::SolanaRaydiumAmm,
        program_id: "675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
        status: "VERIFIED_ALLOWLIST",
    },
    SolanaProgramSpec {
        venue: DexVenue::SolanaRaydiumClmm,
        program_id: "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",
        status: "VERIFIED_ALLOWLIST",
    },
    SolanaProgramSpec {
        venue: DexVenue::SolanaRaydiumCpmm,
        program_id: "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C",
        status: "VERIFIED_ALLOWLIST",
    },
    SolanaProgramSpec {
        venue: DexVenue::SolanaPumpFun,
        program_id: "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
        status: "VERIFIED_ALLOWLIST",
    },
    SolanaProgramSpec {
        venue: DexVenue::SolanaPumpSwap,
        program_id: "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA",
        status: "VERIFIED_ALLOWLIST_PENDING_SEPARATE_DECODER",
    },
];

pub fn solana_program_spec(program_id: &str) -> Option<&'static SolanaProgramSpec> {
    SOLANA_PROGRAM_SPECS
        .iter()
        .find(|spec| spec.program_id == program_id)
}

impl fmt::Display for DexVenue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VenueSpec {
    pub venue: DexVenue,
    pub chain: Chain,
    pub status: &'static str,
    pub requires_verified_program_or_contract: bool,
}

pub fn venue_specs() -> Vec<VenueSpec> {
    [
        DexVenue::SolanaRaydiumAmm,
        DexVenue::SolanaRaydiumCpmm,
        DexVenue::SolanaRaydiumClmm,
        DexVenue::SolanaMeteora,
        DexVenue::SolanaOrcaWhirlpool,
        DexVenue::SolanaPumpFun,
        DexVenue::SolanaPumpSwap,
        DexVenue::SolanaJupiterRouter,
        DexVenue::RobinhoodUniswapV3,
        DexVenue::RobinhoodUniswapV4,
        DexVenue::RobinhoodPons,
        DexVenue::RobinhoodLong,
        DexVenue::RobinhoodO1,
        DexVenue::RobinhoodPoolsTrade,
        DexVenue::RobinhoodPair,
        DexVenue::RobinhoodBags,
    ]
    .into_iter()
    .map(|venue| VenueSpec {
        venue,
        chain: venue.chain(),
        status: "UNVERIFIED",
        requires_verified_program_or_contract: true,
    })
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanCursor {
    Solana {
        signature: String,
        slot: u64,
    },
    Robinhood {
        block: u64,
        transaction_index: u32,
        log_index: u32,
    },
}

impl ScanCursor {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Solana { signature, slot } => {
                if signature.trim().is_empty() || *slot == 0 {
                    bail!("cursor Solana tidak valid");
                }
            }
            Self::Robinhood { block, .. } if *block == 0 => {
                bail!("cursor Robinhood tidak valid");
            }
            Self::Robinhood { .. } => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolanaPollBatch {
    pub references: Vec<SolanaTransactionReference>,
    pub next_cursor: Option<ScanCursor>,
}

/// Membatasi satu batch signature tanpa network atau persistence.
/// Cursor hanya maju ke reference yang benar-benar dikembalikan dalam batch.
pub fn plan_solana_poll_batch(
    references: &[SolanaTransactionReference],
    cursor: Option<&ScanCursor>,
    max_batch: usize,
) -> Result<SolanaPollBatch> {
    if max_batch == 0 {
        bail!("batas batch Solana harus lebih besar dari nol");
    }
    let cursor = match cursor {
        Some(ScanCursor::Solana { signature, slot }) => Some((signature.as_str(), *slot)),
        Some(ScanCursor::Robinhood { .. }) => bail!("cursor chain tidak cocok untuk Solana"),
        None => None,
    };
    let mut seen = HashSet::new();
    let mut selected = Vec::new();
    let mut previous_slot = None;
    for reference in references {
        if reference.chain != Chain::Solana
            || reference.wallet_address.trim().is_empty()
            || reference.signature.trim().is_empty()
            || reference.slot == 0
        {
            bail!("signature reference Solana tidak valid");
        }
        if previous_slot.is_some_and(|slot| reference.slot < slot) {
            bail!("signature references tidak monotonik berdasarkan slot");
        }
        previous_slot = Some(reference.slot);
        if !seen.insert(reference.signature.as_str()) {
            continue;
        }
        if cursor.is_some_and(|(signature, slot)| {
            reference.slot < slot || (reference.slot == slot && reference.signature == signature)
        }) {
            continue;
        }
        if selected.len() == max_batch {
            break;
        }
        selected.push(reference.clone());
    }
    let next_cursor = selected.last().map(|reference| ScanCursor::Solana {
        signature: reference.signature.clone(),
        slot: reference.slot,
    });
    Ok(SolanaPollBatch {
        references: selected,
        next_cursor,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RobinhoodBlockRangePlan {
    pub from_block: u64,
    pub to_block: u64,
    pub next_cursor: ScanCursor,
}

pub fn plan_robinhood_block_range(
    latest_block: u64,
    cursor: Option<&ScanCursor>,
    max_chunk: u64,
    max_tail_blocks: u64,
) -> Result<RobinhoodBlockRangePlan> {
    if latest_block == 0 {
        bail!("latest block tidak boleh nol");
    }
    if max_chunk == 0 {
        bail!("max chunk Robinhood harus lebih besar dari nol");
    }
    let cursor_block = match cursor {
        Some(ScanCursor::Robinhood { block, .. }) => Some(*block),
        Some(ScanCursor::Solana { .. }) => bail!("cursor chain tidak cocok untuk Robinhood"),
        None => None,
    };

    let from_block = match cursor_block {
        Some(b) => {
            if b >= latest_block {
                return Ok(RobinhoodBlockRangePlan {
                    from_block: latest_block,
                    to_block: latest_block,
                    next_cursor: ScanCursor::Robinhood {
                        block: latest_block,
                        transaction_index: 0,
                        log_index: 0,
                    },
                });
            }
            b + 1
        }
        None => latest_block.saturating_sub(max_tail_blocks).max(1),
    };

    let to_block = (from_block + max_chunk - 1).min(latest_block);
    let next_cursor = ScanCursor::Robinhood {
        block: to_block,
        transaction_index: 0,
        log_index: 0,
    };

    Ok(RobinhoodBlockRangePlan {
        from_block,
        to_block,
        next_cursor,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RobinhoodLogRecordKey {
    pub chain_id: u64,
    pub tx_hash: String,
    pub log_index: u32,
}

impl RobinhoodLogRecordKey {
    pub fn new(chain_id: u64, tx_hash: &str, log_index: u32) -> Result<Self> {
        if chain_id == 0 {
            bail!("chain_id tidak boleh nol");
        }
        let clean = tx_hash.trim();
        if clean.is_empty() {
            bail!("tx_hash tidak boleh kosong");
        }
        Ok(Self {
            chain_id,
            tx_hash: clean.to_lowercase(),
            log_index,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexSwapObservation {
    pub chain: Chain,
    pub venue: DexVenue,
    pub wallet: String,
    pub token: String,
    pub quote_asset: String,
    pub pool: String,
    pub tx_id: String,
    pub event_index: u32,
    pub token_delta: i128,
    pub quote_delta: i128,
    pub token_balance_before: u128,
    pub token_balance_after: u128,
    pub success: bool,
    pub qualified_wallet: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WalletActivityMetadata {
    pub wallet_handle: String,
    pub token_address: String,
    pub quote_asset: String,
    pub token_symbol: String,
    pub token_decimals: u8,
    pub quote_decimals: u8,
    pub quote_price_usd: f64,
    pub observed_at: chrono::DateTime<Utc>,
}

pub fn normalized_to_observation(
    activity: &NormalizedActivity,
    pool: &str,
    qualified_wallet: bool,
) -> Result<DexSwapObservation> {
    if activity.chain != Chain::Solana || !qualified_wallet {
        bail!("normalized activity belum memenuhi chain/qualified gate");
    }
    let venue = match activity.venue {
        Some(VerifiedVenue::SolanaRaydiumAmm) => DexVenue::SolanaRaydiumAmm,
        Some(VerifiedVenue::SolanaRaydiumCpmm) => DexVenue::SolanaRaydiumCpmm,
        Some(VerifiedVenue::SolanaRaydiumClmm) => DexVenue::SolanaRaydiumClmm,
        Some(VerifiedVenue::SolanaPumpFun) => DexVenue::SolanaPumpFun,
        Some(VerifiedVenue::SolanaPumpSwap) => DexVenue::SolanaPumpSwap,
        Some(VerifiedVenue::SolanaJupiter) => DexVenue::SolanaJupiterRouter,
        _ => bail!("venue Solana tidak memiliki mapping DEX yang verified"),
    };
    let quote_asset = activity
        .quote_asset
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("quote asset tidak tersedia"))?;
    if pool.trim().is_empty() || pool.chars().any(char::is_whitespace) {
        bail!("pool identity tidak valid");
    }
    let token_delta = i128::try_from(activity.token_out_amount)
        .map_err(|_| anyhow::anyhow!("token delta overflow"))?;
    let quote_delta = i128::try_from(activity.token_in_amount)
        .map_err(|_| anyhow::anyhow!("quote delta overflow"))?
        .checked_neg()
        .ok_or_else(|| anyhow::anyhow!("quote delta overflow"))?;
    Ok(DexSwapObservation {
        chain: Chain::Solana,
        venue,
        wallet: activity.wallet_address.clone(),
        token: activity.token_address.clone(),
        quote_asset: quote_asset.to_string(),
        pool: pool.to_string(),
        tx_id: activity.tx_id.clone(),
        event_index: activity.event_index,
        token_delta,
        quote_delta,
        token_balance_before: activity.balance_before,
        token_balance_after: activity.balance_after,
        success: true,
        qualified_wallet: true,
    })
}

pub struct WalletActivityMetadataInput<'a> {
    pub token_address: &'a str,
    pub quote_asset: &'a str,
    pub token_symbol: &'a str,
    pub token_decimals: u8,
    pub quote_decimals: u8,
    pub quote_price_usd: f64,
    pub observed_at: chrono::DateTime<Utc>,
}

pub fn metadata_from_wallet(
    wallet: &crate::models::SmartWallet,
    input: WalletActivityMetadataInput<'_>,
) -> Result<WalletActivityMetadata> {
    let WalletActivityMetadataInput {
        token_address,
        quote_asset,
        token_symbol,
        token_decimals,
        quote_decimals,
        quote_price_usd,
        observed_at,
    } = input;
    if wallet.handle.trim().is_empty()
        || wallet.handle.chars().any(char::is_whitespace)
        || wallet.address.trim().is_empty()
        || wallet.address.chars().any(char::is_whitespace)
    {
        bail!("wallet registry identity tidak valid");
    }
    if !wallet.is_active {
        bail!("wallet tidak aktif");
    }
    if !matches!(
        wallet.tier,
        crate::models::WalletTier::TierS | crate::models::WalletTier::TierA
    ) {
        bail!("wallet belum qualified");
    }
    for (value, label) in [
        (token_address, "token address"),
        (quote_asset, "quote asset"),
        (token_symbol, "token symbol"),
    ] {
        if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
            bail!("{label} tidak valid");
        }
    }
    if token_decimals > 38 || quote_decimals > 38 {
        bail!("decimals di luar batas aman");
    }
    if !quote_price_usd.is_finite() || quote_price_usd <= 0.0 {
        bail!("quote price USD tidak valid");
    }
    Ok(WalletActivityMetadata {
        wallet_handle: wallet.handle.clone(),
        token_address: token_address.to_string(),
        quote_asset: quote_asset.to_string(),
        token_symbol: token_symbol.to_string(),
        token_decimals,
        quote_decimals,
        quote_price_usd,
        observed_at,
    })
}

/// Menyelesaikan metadata activity Solana dari registry authoritative dan Config.
/// Resolver ini read-only: ia tidak menulis DB, memanggil executor, atau memakai fallback.
pub fn resolve_solana_activity_metadata(
    db: &crate::db::SmartWalletDb,
    config: &Config,
    wallet_address: &str,
    observed_at: chrono::DateTime<Utc>,
) -> Result<(crate::models::SmartWallet, WalletActivityMetadata)> {
    if !config.enable_solana_dex_worker {
        bail!("Solana DEX worker belum diaktifkan");
    }
    let token_address = config
        .solana_dex_token_address
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("token address DEX Solana belum tersedia"))?;
    if token_address.trim().is_empty() || token_address.chars().any(char::is_whitespace) {
        bail!("token address DEX Solana tidak valid");
    }
    let quote_asset = config
        .solana_dex_quote_asset
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("quote asset DEX Solana belum tersedia"))?;
    if quote_asset.trim().is_empty() || quote_asset.chars().any(char::is_whitespace) {
        bail!("quote asset DEX Solana tidak valid");
    }
    let token_symbol = config
        .solana_dex_token_symbol
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("token symbol DEX Solana belum tersedia"))?;
    let token_decimals = config
        .solana_dex_token_decimals
        .ok_or_else(|| anyhow::anyhow!("token decimals DEX Solana belum tersedia"))?;
    let quote_decimals = config
        .solana_dex_quote_decimals
        .ok_or_else(|| anyhow::anyhow!("quote decimals DEX Solana belum tersedia"))?;
    let quote_price_usd = config
        .solana_dex_quote_price_usd
        .ok_or_else(|| anyhow::anyhow!("quote price USD DEX Solana belum tersedia"))?;
    let wallet = db
        .get_wallet_by_address(wallet_address)?
        .ok_or_else(|| anyhow::anyhow!("wallet tidak ditemukan di registry authoritative"))?;
    if wallet.chain != Chain::Solana {
        bail!("wallet metadata bukan wallet Solana");
    }
    let metadata = metadata_from_wallet(
        &wallet,
        WalletActivityMetadataInput {
            token_address,
            quote_asset,
            token_symbol,
            token_decimals,
            quote_decimals,
            quote_price_usd,
            observed_at,
        },
    )?;
    Ok((wallet, metadata))
}

pub fn persist_and_broadcast_activity(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    activity: &WalletActivity,
) -> Result<()> {
    db.insert_activity_if_new(activity)?;
    if db.enqueue_activity_ipc(activity)? {
        ipc.broadcast(&crate::ipc::IpcMessage::ActivityBroadcast {
            activity: activity.clone(),
        })?;
        db.mark_activity_ipc_published(activity)?;
    }
    Ok(())
}

/// Memulihkan activity yang sudah durable tetapi belum dipublish ke IPC.
/// `limit` membatasi kerja per pemanggilan; error pertama menghentikan drain.
/// Status `PUBLISHED_TO_IPC` tidak berarti TUI mengirim ACK.
pub fn drain_activity_ipc_outbox(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    limit: usize,
) -> Result<usize> {
    if limit == 0 {
        bail!("limit drain outbox harus lebih besar dari nol");
    }
    let queued = db.queued_activity_ipc(limit)?;
    let mut published = 0;
    for activity in queued {
        ipc.broadcast(&crate::ipc::IpcMessage::ActivityBroadcast {
            activity: activity.clone(),
        })?;
        if db.mark_activity_ipc_published(&activity)? {
            published += 1;
        }
    }
    Ok(published)
}

/// Menyelesaikan satu event verified sebelum cursor dimajukan.
///
/// Urutan wajib: dedupe/persist → broadcast bila row baru → save cursor.
/// Jika broadcast atau save cursor gagal, caller menerima error dan tidak boleh
/// menganggap batch sudah selesai.
pub struct NormalizedActivityMaterializationContext<'a> {
    pub wallet: &'a crate::models::SmartWallet,
    pub token_symbol: &'a str,
    pub token_decimals: u8,
    pub quote_decimals: u8,
    pub quote_price_usd: f64,
}

pub fn materialize_and_persist_normalized_activity(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    activity: &NormalizedActivity,
    context: NormalizedActivityMaterializationContext<'_>,
    scope_key: &str,
    cursor: &ScanCursor,
) -> Result<bool> {
    let pool = activity
        .pool_address
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("pool authoritative tidak tersedia"))?;
    let observation = normalized_to_observation(activity, pool, true)?;
    let quote_asset = activity
        .quote_asset
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("quote asset tidak tersedia"))?;
    let metadata = metadata_from_wallet(
        context.wallet,
        WalletActivityMetadataInput {
            token_address: &activity.token_address,
            quote_asset,
            token_symbol: context.token_symbol,
            token_decimals: context.token_decimals,
            quote_decimals: context.quote_decimals,
            quote_price_usd: context.quote_price_usd,
            observed_at: activity.observed_at,
        },
    )?;
    let wallet_activity = materialize_wallet_activity(&observation, &metadata)?;
    persist_activity_and_advance_cursor(
        db,
        ipc,
        activity.chain,
        observation.venue,
        scope_key,
        cursor,
        &wallet_activity,
    )
}

/// Mematerialisasi seluruh batch lebih dahulu, lalu melakukan satu boundary side effect.
/// Kegagalan pada pool, metadata, direction, atau balance terjadi sebelum DB/IPC/cursor.
pub fn materialize_and_persist_normalized_batch(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    activities: &[NormalizedActivity],
    context: NormalizedActivityMaterializationContext<'_>,
    scope_key: &str,
    cursor: &ScanCursor,
) -> Result<usize> {
    if activities.is_empty() {
        bail!("normalized activity batch kosong");
    }
    let first = activities
        .first()
        .ok_or_else(|| anyhow::anyhow!("normalized activity batch kosong"))?;
    let mut materialized = Vec::with_capacity(activities.len());
    let mut batch_venue = None;
    for activity in activities {
        if activity.chain != first.chain {
            bail!("normalized activity batch mencampur chain");
        }
        let pool = activity
            .pool_address
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("pool authoritative tidak tersedia"))?;
        let observation = normalized_to_observation(activity, pool, true)?;
        if let Some(expected) = batch_venue {
            if expected != observation.venue {
                bail!("normalized activity batch mencampur venue");
            }
        } else {
            batch_venue = Some(observation.venue);
        }
        let quote_asset = activity
            .quote_asset
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("quote asset tidak tersedia"))?;
        let metadata = metadata_from_wallet(
            context.wallet,
            WalletActivityMetadataInput {
                token_address: &activity.token_address,
                quote_asset,
                token_symbol: context.token_symbol,
                token_decimals: context.token_decimals,
                quote_decimals: context.quote_decimals,
                quote_price_usd: context.quote_price_usd,
                observed_at: activity.observed_at,
            },
        )?;
        materialized.push(materialize_wallet_activity(&observation, &metadata)?);
    }
    persist_materialized_batch_and_advance_cursor(
        db,
        ipc,
        first.chain,
        batch_venue.ok_or_else(|| anyhow::anyhow!("venue batch kosong"))?,
        scope_key,
        cursor,
        &materialized,
    )
}

/// Menyimpan batch activity yang sudah tervalidasi, lalu memajukan cursor sekali.
/// Materialisasi harus dilakukan caller sebelum side effect agar kegagalan validasi
/// tidak meninggalkan side effect parsial.
pub fn persist_materialized_batch_and_advance_cursor(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    chain: Chain,
    venue: DexVenue,
    scope_key: &str,
    cursor: &ScanCursor,
    activities: &[WalletActivity],
) -> Result<usize> {
    if activities.is_empty() {
        bail!("batch activity kosong");
    }
    if chain != venue.chain() {
        bail!("chain dan venue batch tidak cocok");
    }
    cursor.validate()?;
    for activity in activities {
        if activity.chain != chain || activity.tx_signature.trim().is_empty() {
            bail!("activity batch memiliki provenance tidak valid");
        }
    }
    let mut inserted = 0;
    for activity in activities {
        let row_inserted = db.insert_activity_if_new(activity)?;
        let queued = db.enqueue_activity_ipc(activity)?;
        if queued {
            ipc.broadcast(&crate::ipc::IpcMessage::ActivityBroadcast {
                activity: activity.clone(),
            })?;
            db.mark_activity_ipc_published(activity)?;
        }
        if row_inserted {
            inserted += 1;
        }
    }
    db.save_dex_cursor(chain, venue, scope_key, cursor)?;
    Ok(inserted)
}

pub fn persist_activity_and_advance_cursor(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    chain: Chain,
    venue: DexVenue,
    scope_key: &str,
    cursor: &ScanCursor,
    activity: &WalletActivity,
) -> Result<bool> {
    if activity.chain != chain {
        bail!("activity dan cursor chain tidak cocok");
    }
    if activity.tx_signature.trim().is_empty() {
        bail!("activity transaction signature tidak valid");
    }
    persist_activity_and_advance_cursor_inner(db, ipc, chain, venue, scope_key, cursor, activity)
}

fn persist_activity_and_advance_cursor_inner(
    db: &crate::db::SmartWalletDb,
    ipc: &crate::ipc::IpcServer,
    chain: Chain,
    venue: DexVenue,
    scope_key: &str,
    cursor: &ScanCursor,
    activity: &WalletActivity,
) -> Result<bool> {
    if chain != venue.chain() {
        bail!("chain dan venue cursor tidak cocok");
    }
    cursor.validate()?;
    let inserted = db.insert_activity_if_new(activity)?;
    if db.enqueue_activity_ipc(activity)? {
        ipc.broadcast(&crate::ipc::IpcMessage::ActivityBroadcast {
            activity: activity.clone(),
        })?;
        db.mark_activity_ipc_published(activity)?;
    }
    db.save_dex_cursor(chain, venue, scope_key, cursor)?;
    Ok(inserted)
}

pub fn classify_swap(observation: &DexSwapObservation) -> Result<SwapDirection> {
    if observation.chain != observation.venue.chain() {
        bail!("chain dan venue tidak cocok");
    }
    for (name, value) in [
        ("wallet", observation.wallet.as_str()),
        ("token", observation.token.as_str()),
        ("quote asset", observation.quote_asset.as_str()),
        ("pool", observation.pool.as_str()),
        ("tx id", observation.tx_id.as_str()),
    ] {
        if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
            bail!("{name} identity tidak valid");
        }
    }
    if !observation.success || !observation.qualified_wallet {
        bail!("swap belum memenuhi success/qualified gate");
    }
    if observation.chain == Chain::Robinhood {
        match observation.venue {
            DexVenue::RobinhoodUniswapV4
            | DexVenue::RobinhoodPons
            | DexVenue::RobinhoodUniswapV3 => {}
            DexVenue::RobinhoodLong
            | DexVenue::RobinhoodO1
            | DexVenue::RobinhoodPoolsTrade
            | DexVenue::RobinhoodPair
            | DexVenue::RobinhoodBags => {
                bail!(
                    "venue Robinhood '{}' belum memiliki deployment contract atau event ABI terverifikasi: UNVERIFIED",
                    observation.venue
                );
            }
            _ => bail!("venue bukan bagian dari Robinhood"),
        }
    }
    if observation.token_delta > 0 && observation.quote_delta < 0 {
        Ok(SwapDirection::Buy)
    } else if observation.token_delta < 0 && observation.quote_delta > 0 {
        Ok(SwapDirection::Sell)
    } else {
        bail!("delta tidak membuktikan buy atau sell");
    }
}

pub fn materialize_wallet_activity(
    observation: &DexSwapObservation,
    metadata: &WalletActivityMetadata,
) -> Result<WalletActivity> {
    let direction = classify_swap(observation)?;
    if metadata.wallet_handle.trim().is_empty()
        || metadata.wallet_handle.chars().any(char::is_whitespace)
        || metadata.token_symbol.trim().is_empty()
        || metadata.token_symbol.chars().any(char::is_whitespace)
    {
        bail!("wallet activity metadata identity tidak valid");
    }
    if metadata.token_decimals > 38 || metadata.quote_decimals > 38 {
        bail!("decimals di luar batas aman");
    }
    if !metadata.quote_price_usd.is_finite() || metadata.quote_price_usd <= 0.0 {
        bail!("quote price USD tidak valid");
    }
    let balance_delta = observation
        .token_balance_after
        .checked_sub(observation.token_balance_before)
        .map(|value| value as i128)
        .or_else(|| {
            observation
                .token_balance_before
                .checked_sub(observation.token_balance_after)
                .and_then(|value| i128::try_from(value).ok().map(|value| -value))
        })
        .ok_or_else(|| anyhow::anyhow!("balance delta overflow"))?;
    if balance_delta != observation.token_delta {
        bail!("balance delta tidak cocok dengan token delta");
    }

    let token_amount = (observation.token_delta.unsigned_abs() as f64)
        / 10_f64.powi(i32::from(metadata.token_decimals));
    let quote_amount = (observation.quote_delta.unsigned_abs() as f64)
        / 10_f64.powi(i32::from(metadata.quote_decimals));
    let amount_usd = quote_amount * metadata.quote_price_usd;
    let execution_price_usd = amount_usd / token_amount;
    if !token_amount.is_finite()
        || !amount_usd.is_finite()
        || !execution_price_usd.is_finite()
        || token_amount <= 0.0
        || amount_usd <= 0.0
        || execution_price_usd <= 0.0
    {
        bail!("nilai activity hasil materialisasi tidak valid");
    }

    Ok(WalletActivity {
        id: None,
        wallet_handle: metadata.wallet_handle.clone(),
        wallet_address: observation.wallet.clone(),
        chain: observation.chain,
        activity_type: match direction {
            SwapDirection::Buy => ActivityType::Buy,
            SwapDirection::Sell => ActivityType::Sell,
        },
        token_symbol: metadata.token_symbol.clone(),
        token_mint: observation.token.clone(),
        amount_usd,
        token_amount,
        execution_price_usd,
        whale_balance_before: observation.token_balance_before as f64
            / 10_f64.powi(i32::from(metadata.token_decimals)),
        whale_balance_after: observation.token_balance_after as f64
            / 10_f64.powi(i32::from(metadata.token_decimals)),
        sell_ratio_pct: match direction {
            SwapDirection::Buy => 0.0,
            SwapDirection::Sell => {
                token_amount
                    / ((observation.token_balance_before as f64)
                        / 10_f64.powi(i32::from(metadata.token_decimals)))
                    * 100.0
            }
        },
        tx_signature: observation.tx_id.clone(),
        timestamp: metadata.observed_at,
        source_reference: Some(observation.tx_id.clone()),
        parser_version: Some("dex-materializer-v1".to_string()),
        finality: Some("UNKNOWN".to_string()),
    })
}

pub fn deduplicate_event_keys(events: &[DexSwapObservation]) -> Result<usize> {
    let mut keys = HashSet::new();
    for event in events {
        classify_swap(event)?;
        let tx = match event.chain {
            Chain::Solana => event.tx_id.clone(),
            Chain::Robinhood => event.tx_id.to_ascii_lowercase(),
            _ => bail!("chain DEX tidak didukung"),
        };
        keys.insert((event.chain, tx, event.event_index));
    }
    Ok(keys.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(venue: DexVenue) -> DexSwapObservation {
        DexSwapObservation {
            chain: venue.chain(),
            venue,
            wallet: "wallet".to_string(),
            token: "token".to_string(),
            quote_asset: "quote".to_string(),
            pool: "pool".to_string(),
            tx_id: "tx".to_string(),
            event_index: 1,
            token_delta: 10,
            quote_delta: -20,
            token_balance_before: 10,
            token_balance_after: 20,
            success: true,
            qualified_wallet: true,
        }
    }

    fn metadata() -> WalletActivityMetadata {
        WalletActivityMetadata {
            wallet_handle: "whale".to_string(),
            token_address: "Token111".to_string(),
            quote_asset: "USDC111".to_string(),
            token_symbol: "TOK".to_string(),
            token_decimals: 2,
            quote_decimals: 2,
            quote_price_usd: 2.0,
            observed_at: Utc::now(),
        }
    }

    fn wallet(tier: crate::models::WalletTier, is_active: bool) -> crate::models::SmartWallet {
        crate::models::SmartWallet {
            id: None,
            rank: 1,
            name: "Whale".to_string(),
            handle: "whale".to_string(),
            chain: Chain::Solana,
            address: "Wallet111".to_string(),
            tier,
            pnl_24h_usd: 1000.0,
            win_rate_pct: 60.0,
            total_trades: 10,
            source: "ONCHAIN_INDEXER".to_string(),
            reputation_score: 90.0,
            execution_track: crate::models::ExecutionTrack::VaultLane,
            is_active,
            last_seen_at: Some(Utc::now()),
            updated_at: Utc::now(),
        }
    }

    fn reference(signature: &str, slot: u64) -> SolanaTransactionReference {
        SolanaTransactionReference {
            chain: Chain::Solana,
            wallet_address: "wallet".to_string(),
            signature: signature.to_string(),
            slot,
            block_time: None,
        }
    }

    #[test]
    fn durable_cursor_roundtrips_each_chain_scope() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let solana_cursor = ScanCursor::Solana {
            signature: "sig-42".to_string(),
            slot: 42,
        };
        db.save_dex_cursor(
            Chain::Solana,
            DexVenue::SolanaRaydiumAmm,
            "wallet-1",
            &solana_cursor,
        )
        .unwrap();
        assert_eq!(
            db.load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "wallet-1")
                .unwrap(),
            Some(solana_cursor)
        );

        let evm_cursor = ScanCursor::Robinhood {
            block: 4663,
            transaction_index: 2,
            log_index: 7,
        };
        db.save_dex_cursor(
            Chain::Robinhood,
            DexVenue::RobinhoodUniswapV4,
            "global",
            &evm_cursor,
        )
        .unwrap();
        assert_eq!(
            db.load_dex_cursor(Chain::Robinhood, DexVenue::RobinhoodUniswapV4, "global")
                .unwrap(),
            Some(evm_cursor)
        );
        assert!(db
            .load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "missing")
            .unwrap()
            .is_none());
    }

    #[test]
    fn durable_cursor_rejects_mismatched_scope_or_cursor_kind() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let solana_cursor = ScanCursor::Solana {
            signature: "sig-1".to_string(),
            slot: 1,
        };
        assert!(db
            .save_dex_cursor(
                Chain::Robinhood,
                DexVenue::RobinhoodUniswapV4,
                "global",
                &solana_cursor,
            )
            .is_err());
        assert!(db
            .save_dex_cursor(
                Chain::Solana,
                DexVenue::RobinhoodUniswapV4,
                "wallet-1",
                &solana_cursor,
            )
            .is_err());
        assert!(db
            .load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "")
            .is_err());
    }

    #[test]
    fn solana_poll_plan_is_bounded_deduplicated_and_cursored() {
        let references = [
            reference("sig-1", 10),
            reference("sig-1", 10),
            reference("sig-2", 11),
            reference("sig-3", 12),
        ];
        let first = plan_solana_poll_batch(&references, None, 2).unwrap();
        assert_eq!(first.references.len(), 2);
        assert_eq!(first.references[0].signature, "sig-1");
        assert_eq!(first.references[1].signature, "sig-2");
        assert_eq!(
            first.next_cursor,
            Some(ScanCursor::Solana {
                signature: "sig-2".to_string(),
                slot: 11,
            })
        );
        let second = plan_solana_poll_batch(&references, first.next_cursor.as_ref(), 2).unwrap();
        assert_eq!(second.references.len(), 1);
        assert_eq!(second.references[0].signature, "sig-3");
    }

    #[test]
    fn solana_poll_plan_rejects_invalid_order_and_bounds() {
        assert!(plan_solana_poll_batch(&[reference("sig", 1)], None, 0).is_err());
        assert!(
            plan_solana_poll_batch(&[reference("sig-2", 2), reference("sig-1", 1)], None, 2,)
                .is_err()
        );
        assert!(plan_solana_poll_batch(
            &[reference("sig", 1)],
            Some(&ScanCursor::Robinhood {
                block: 1,
                transaction_index: 0,
                log_index: 0,
            }),
            2,
        )
        .is_err());
    }

    fn normalized_activity() -> NormalizedActivity {
        NormalizedActivity {
            chain: Chain::Solana,
            wallet_address: "wallet".to_string(),
            token_address: "token".to_string(),
            tx_id: "tx".to_string(),
            event_index: 1,
            venue: Some(VerifiedVenue::SolanaRaydiumAmm),
            pool_address: Some("Pool111".to_string()),
            quote_asset: Some("USDC".to_string()),
            token_in_amount: 20,
            token_out_amount: 10,
            balance_before: 10,
            balance_after: 20,
            recipient: Some("wallet".to_string()),
            observed_at: Utc::now(),
        }
    }

    #[test]
    fn normalized_activity_maps_to_verified_observation() {
        let observation = normalized_to_observation(&normalized_activity(), "pool", true).unwrap();
        assert_eq!(observation.venue, DexVenue::SolanaRaydiumAmm);
        assert_eq!(observation.token_delta, 10);
        assert_eq!(observation.quote_delta, -20);
        assert_eq!(observation.token_balance_before, 10);
        assert_eq!(observation.token_balance_after, 20);
        assert!(classify_swap(&observation).is_ok());
    }

    #[test]
    fn normalized_activity_rejects_missing_pool_or_unmapped_venue() {
        assert!(normalized_to_observation(&normalized_activity(), "", true).is_err());
        let mut router = normalized_activity();
        router.venue = None;
        assert!(normalized_to_observation(&router, "pool", true).is_err());
        assert!(normalized_to_observation(&normalized_activity(), "pool", false).is_err());
    }

    #[test]
    fn metadata_resolver_requires_active_qualified_wallet() {
        let resolved = metadata_from_wallet(
            &wallet(crate::models::WalletTier::TierS, true),
            WalletActivityMetadataInput {
                token_address: "Token111",
                quote_asset: "USDC111",
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
                observed_at: Utc::now(),
            },
        )
        .unwrap();
        assert_eq!(resolved.wallet_handle, "whale");
        assert!(metadata_from_wallet(
            &wallet(crate::models::WalletTier::TierB, true),
            WalletActivityMetadataInput {
                token_address: "Token111",
                quote_asset: "USDC111",
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
                observed_at: Utc::now(),
            },
        )
        .is_err());
        assert!(metadata_from_wallet(
            &wallet(crate::models::WalletTier::TierA, false),
            WalletActivityMetadataInput {
                token_address: "Token111",
                quote_asset: "USDC111",
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
                observed_at: Utc::now(),
            },
        )
        .is_err());
    }

    #[test]
    fn persist_and_broadcast_is_idempotent_on_isolated_boundaries() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-dex-adapter-test.sock");
        let mut receiver = ipc.get_sender().subscribe();
        let activity =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();

        persist_and_broadcast_activity(&db, &ipc, &activity).unwrap();
        persist_and_broadcast_activity(&db, &ipc, &activity).unwrap();

        let persisted = db.get_recent_activities(10).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].tx_signature, "tx");
        let message = receiver.try_recv().unwrap();
        let decoded: crate::ipc::IpcMessage = serde_json::from_str(&message).unwrap();
        match decoded {
            crate::ipc::IpcMessage::ActivityBroadcast { activity: received } => {
                assert_eq!(received.tx_signature, "tx");
                assert_eq!(received.amount_usd, activity.amount_usd);
            }
            other => panic!("pesan IPC tidak sesuai: {other:?}"),
        }
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn materialized_batch_is_idempotent_and_advances_cursor_after_delivery() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-batch-test.sock");
        let mut receiver = ipc.get_sender().subscribe();
        let first =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();
        let mut second = first.clone();
        second.tx_signature = "tx-2".into();
        second.timestamp += chrono::Duration::seconds(1);
        let cursor = ScanCursor::Solana {
            signature: "tx-2".into(),
            slot: 43,
        };

        assert_eq!(
            persist_materialized_batch_and_advance_cursor(
                &db,
                &ipc,
                Chain::Solana,
                DexVenue::SolanaRaydiumAmm,
                "batch-scope",
                &cursor,
                &[first.clone(), second.clone()],
            )
            .unwrap(),
            2
        );
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 2);
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_ok());
        assert_eq!(
            db.load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "batch-scope")
                .unwrap(),
            Some(cursor.clone())
        );

        assert_eq!(
            persist_materialized_batch_and_advance_cursor(
                &db,
                &ipc,
                Chain::Solana,
                DexVenue::SolanaRaydiumAmm,
                "batch-scope",
                &cursor,
                &[first, second],
            )
            .unwrap(),
            0
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 2);
    }

    #[test]
    fn file_backed_batch_survives_restart_and_retry_is_idempotent() {
        let path = format!(
            "/tmp/zenithsw-restart-harness-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let first =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();
        let cursor = ScanCursor::Solana {
            signature: first.tx_signature.clone(),
            slot: 42,
        };
        {
            let db = crate::db::SmartWalletDb::new(&path).unwrap();
            let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-restart-harness.sock");
            assert_eq!(
                persist_materialized_batch_and_advance_cursor(
                    &db,
                    &ipc,
                    Chain::Solana,
                    DexVenue::SolanaRaydiumAmm,
                    "restart-scope",
                    &cursor,
                    std::slice::from_ref(&first),
                )
                .unwrap(),
                1
            );
        }
        {
            let db = crate::db::SmartWalletDb::new(&path).unwrap();
            let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-restart-harness-2.sock");
            let recovered = db.get_recent_activities(10).unwrap();
            assert_eq!(recovered.len(), 1);
            assert_eq!(
                recovered[0].source_reference,
                Some(first.tx_signature.clone())
            );
            assert_eq!(
                recovered[0].parser_version.as_deref(),
                Some("dex-materializer-v1")
            );
            assert_eq!(recovered[0].finality.as_deref(), Some("UNKNOWN"));
            assert_eq!(
                db.load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "restart-scope")
                    .unwrap(),
                Some(cursor.clone())
            );
            assert_eq!(
                persist_materialized_batch_and_advance_cursor(
                    &db,
                    &ipc,
                    Chain::Solana,
                    DexVenue::SolanaRaydiumAmm,
                    "restart-scope",
                    &cursor,
                    std::slice::from_ref(&first),
                )
                .unwrap(),
                0
            );
            assert_eq!(db.get_recent_activities(10).unwrap().len(), 1);
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn outbox_recovers_queued_activity_after_restart_before_cursor_advance() {
        let path = format!(
            "/tmp/zenithsw-outbox-harness-{}-{}.db",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let activity =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();
        let cursor = ScanCursor::Solana {
            signature: activity.tx_signature.clone(),
            slot: 42,
        };
        {
            let db = crate::db::SmartWalletDb::new(&path).unwrap();
            assert!(db.insert_activity_if_new(&activity).unwrap());
            assert!(db.enqueue_activity_ipc(&activity).unwrap());
            assert!(db
                .load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "outbox-scope")
                .unwrap()
                .is_none());
        }
        {
            let db = crate::db::SmartWalletDb::new(&path).unwrap();
            let queued = db.queued_activity_ipc(10).unwrap();
            assert_eq!(queued.len(), 1);
            assert_eq!(queued[0].tx_signature, activity.tx_signature);
            assert_eq!(queued[0].wallet_address, activity.wallet_address);
            assert_eq!(queued[0].token_mint, activity.token_mint);
            let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-outbox-harness.sock");
            ipc.broadcast(&crate::ipc::IpcMessage::ActivityBroadcast {
                activity: queued[0].clone(),
            })
            .unwrap();
            assert!(db.mark_activity_ipc_published(&queued[0]).unwrap());
            assert!(db.queued_activity_ipc(10).unwrap().is_empty());
            db.save_dex_cursor(
                Chain::Solana,
                DexVenue::SolanaRaydiumAmm,
                "outbox-scope",
                &cursor,
            )
            .unwrap();
            assert_eq!(
                db.load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "outbox-scope")
                    .unwrap(),
                Some(cursor)
            );
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn drain_outbox_is_bounded_and_replays_only_pending_items() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-drain-outbox-test.sock");
        let mut receiver = ipc.get_sender().subscribe();
        let first =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();
        let mut second = first.clone();
        second.tx_signature = "tx-drain-2".into();
        assert!(db.insert_activity_if_new(&first).unwrap());
        assert!(db.insert_activity_if_new(&second).unwrap());
        assert!(db.enqueue_activity_ipc(&first).unwrap());
        assert!(db.enqueue_activity_ipc(&second).unwrap());

        assert_eq!(drain_activity_ipc_outbox(&db, &ipc, 1).unwrap(), 1);
        assert!(receiver.try_recv().is_ok());
        assert_eq!(db.queued_activity_ipc(10).unwrap().len(), 1);
        assert_eq!(drain_activity_ipc_outbox(&db, &ipc, 1).unwrap(), 1);
        assert!(receiver.try_recv().is_ok());
        assert!(db.queued_activity_ipc(10).unwrap().is_empty());
        assert_eq!(drain_activity_ipc_outbox(&db, &ipc, 1).unwrap(), 0);
        assert!(receiver.try_recv().is_err());
        assert!(drain_activity_ipc_outbox(&db, &ipc, 0).is_err());
    }

    #[test]
    fn normalized_batch_materializes_before_side_effects() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-normalized-batch-test.sock");
        let wallet = wallet(crate::models::WalletTier::TierS, true);
        let first = normalized_activity();
        let mut second = first.clone();
        second.tx_id = "tx-batch-2".into();
        second.event_index = 2;
        let cursor = ScanCursor::Solana {
            signature: second.tx_id.clone(),
            slot: 43,
        };
        let context = NormalizedActivityMaterializationContext {
            wallet: &wallet,
            token_symbol: "TOK",
            token_decimals: 2,
            quote_decimals: 2,
            quote_price_usd: 2.0,
        };
        assert_eq!(
            materialize_and_persist_normalized_batch(
                &db,
                &ipc,
                &[first.clone(), second.clone()],
                context,
                "normalized-batch",
                &cursor,
            )
            .unwrap(),
            2
        );
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 2);
        assert_eq!(db.queued_activity_ipc(10).unwrap().len(), 0);
        assert_eq!(
            db.load_dex_cursor(
                Chain::Solana,
                DexVenue::SolanaRaydiumAmm,
                "normalized-batch"
            )
            .unwrap(),
            Some(cursor)
        );

        let mut invalid = normalized_activity();
        invalid.tx_id = "tx-invalid-batch".into();
        invalid.pool_address = None;
        let invalid_cursor = ScanCursor::Solana {
            signature: invalid.tx_id.clone(),
            slot: 44,
        };
        assert!(materialize_and_persist_normalized_batch(
            &db,
            &ipc,
            &[normalized_activity(), invalid],
            NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
            },
            "invalid-normalized-batch",
            &invalid_cursor,
        )
        .is_err());
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 2);
        assert!(db
            .load_dex_cursor(
                Chain::Solana,
                DexVenue::SolanaRaydiumAmm,
                "invalid-normalized-batch"
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn normalized_activity_materializes_persists_broadcasts_and_advances_cursor() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-normalized-persist-test.sock");
        let mut receiver = ipc.get_sender().subscribe();
        let normalized = normalized_activity();
        let wallet = wallet(crate::models::WalletTier::TierS, true);
        let cursor = ScanCursor::Solana {
            signature: normalized.tx_id.clone(),
            slot: 42,
        };

        let inserted = materialize_and_persist_normalized_activity(
            &db,
            &ipc,
            &normalized,
            NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
            },
            "wallet-scope",
            &cursor,
        )
        .unwrap();
        assert!(inserted);
        let persisted = db.get_recent_activities(10).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].tx_signature, normalized.tx_id);
        assert_eq!(
            db.load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "wallet-scope")
                .unwrap(),
            Some(cursor.clone())
        );
        let message = receiver.try_recv().unwrap();
        let decoded: crate::ipc::IpcMessage = serde_json::from_str(&message).unwrap();
        assert!(matches!(
            decoded,
            crate::ipc::IpcMessage::ActivityBroadcast { .. }
        ));

        let duplicate = materialize_and_persist_normalized_activity(
            &db,
            &ipc,
            &normalized,
            NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
            },
            "wallet-scope",
            &cursor,
        )
        .unwrap();
        assert!(!duplicate);
        assert!(receiver.try_recv().is_err());
        assert_eq!(db.get_recent_activities(10).unwrap().len(), 1);
    }

    #[test]
    fn normalized_activity_without_pool_is_rejected_before_persistence() {
        let db = crate::db::SmartWalletDb::new_in_memory().unwrap();
        let ipc = crate::ipc::IpcServer::new("/tmp/zenithsw-normalized-missing-pool-test.sock");
        let normalized = normalized_activity();
        let wallet = wallet(crate::models::WalletTier::TierS, true);
        let mut missing_pool = normalized.clone();
        missing_pool.pool_address = None;
        let cursor = ScanCursor::Solana {
            signature: missing_pool.tx_id.clone(),
            slot: 42,
        };
        assert!(materialize_and_persist_normalized_activity(
            &db,
            &ipc,
            &missing_pool,
            NormalizedActivityMaterializationContext {
                wallet: &wallet,
                token_symbol: "TOK",
                token_decimals: 2,
                quote_decimals: 2,
                quote_price_usd: 2.0,
            },
            "wallet-scope",
            &cursor,
        )
        .is_err());
        assert!(db.get_recent_activities(10).unwrap().is_empty());
        assert!(db
            .load_dex_cursor(Chain::Solana, DexVenue::SolanaRaydiumAmm, "wallet-scope")
            .unwrap()
            .is_none());
    }

    #[test]
    fn program_registry_maps_each_allowlisted_program_exactly() {
        assert_eq!(
            solana_program_spec("675kPX9MHTjS2t1qfr1NYHuzeLXfQM9H24wFSUt1Mp8").map(|s| s.venue),
            Some(DexVenue::SolanaRaydiumAmm)
        );
        assert_eq!(
            solana_program_spec("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C").map(|s| s.venue),
            Some(DexVenue::SolanaRaydiumCpmm)
        );
        assert_eq!(
            solana_program_spec("pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA").map(|s| s.venue),
            Some(DexVenue::SolanaPumpSwap)
        );
        assert!(solana_program_spec("unknown-program").is_none());
    }

    #[test]
    fn registry_keeps_each_chain_and_venue_separate() {
        let specs = venue_specs();
        assert_eq!(specs.len(), 16);
        assert_eq!(specs.iter().filter(|s| s.chain == Chain::Solana).count(), 8);
        assert_eq!(
            specs.iter().filter(|s| s.chain == Chain::Robinhood).count(),
            8
        );
        assert!(specs.iter().all(|s| s.status == "UNVERIFIED"));
    }

    #[test]
    fn materializer_builds_verified_buy_activity() {
        let activity =
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &metadata()).unwrap();
        assert_eq!(activity.activity_type, ActivityType::Buy);
        assert_eq!(activity.wallet_handle, "whale");
        assert_eq!(activity.token_amount, 0.1);
        assert_eq!(activity.amount_usd, 0.4);
        assert_eq!(activity.whale_balance_before, 0.1);
        assert_eq!(activity.whale_balance_after, 0.2);
        assert_eq!(activity.sell_ratio_pct, 0.0);
    }

    #[test]
    fn materializer_builds_verified_sell_activity() {
        let mut observation = event(DexVenue::SolanaRaydiumAmm);
        observation.token_delta = -5;
        observation.quote_delta = 10;
        observation.token_balance_before = 10;
        observation.token_balance_after = 5;
        let activity = materialize_wallet_activity(&observation, &metadata()).unwrap();
        assert_eq!(activity.activity_type, ActivityType::Sell);
        assert_eq!(activity.token_amount, 0.05);
        assert_eq!(activity.sell_ratio_pct, 50.0);
    }

    #[test]
    fn materializer_rejects_balance_or_price_mismatch() {
        let mut observation = event(DexVenue::SolanaRaydiumAmm);
        observation.token_balance_after = 19;
        assert!(materialize_wallet_activity(&observation, &metadata()).is_err());
        let mut invalid_metadata = metadata();
        invalid_metadata.quote_price_usd = 0.0;
        assert!(
            materialize_wallet_activity(&event(DexVenue::SolanaRaydiumAmm), &invalid_metadata)
                .is_err()
        );
    }

    #[test]
    fn classifier_requires_matching_chain() {
        let mut value = event(DexVenue::SolanaRaydiumAmm);
        value.chain = Chain::Robinhood;
        assert!(classify_swap(&value).is_err());
    }

    #[test]
    fn classifier_distinguishes_buy_and_sell() {
        assert_eq!(
            classify_swap(&event(DexVenue::SolanaRaydiumAmm)).unwrap(),
            SwapDirection::Buy
        );
        let mut sell = event(DexVenue::RobinhoodUniswapV4);
        sell.token_delta = -10;
        sell.quote_delta = 20;
        assert_eq!(classify_swap(&sell).unwrap(), SwapDirection::Sell);
    }

    #[test]
    fn transfer_only_is_rejected() {
        let mut value = event(DexVenue::SolanaMeteora);
        value.quote_delta = 0;
        assert!(classify_swap(&value).is_err());
    }

    #[test]
    fn duplicate_key_is_counted_once() {
        let first = event(DexVenue::SolanaOrcaWhirlpool);
        assert_eq!(deduplicate_event_keys(&[first.clone(), first]).unwrap(), 1);
    }

    #[test]
    fn cursors_are_fail_closed() {
        assert!(ScanCursor::Solana {
            signature: String::new(),
            slot: 1
        }
        .validate()
        .is_err());
        assert!(ScanCursor::Robinhood {
            block: 0,
            transaction_index: 0,
            log_index: 0
        }
        .validate()
        .is_err());
    }

    #[test]
    fn robinhood_block_range_plan_without_cursor_bounds_to_tail() {
        let plan = plan_robinhood_block_range(1000, None, 50, 100).expect("plan block range");
        assert_eq!(plan.from_block, 900);
        assert_eq!(plan.to_block, 949);
        assert_eq!(
            plan.next_cursor,
            ScanCursor::Robinhood {
                block: 949,
                transaction_index: 0,
                log_index: 0,
            }
        );
    }

    #[test]
    fn robinhood_block_range_plan_advances_from_existing_cursor() {
        let cursor = ScanCursor::Robinhood {
            block: 949,
            transaction_index: 2,
            log_index: 5,
        };
        let plan =
            plan_robinhood_block_range(1000, Some(&cursor), 50, 100).expect("plan block range");
        assert_eq!(plan.from_block, 950);
        assert_eq!(plan.to_block, 999);
        assert_eq!(
            plan.next_cursor,
            ScanCursor::Robinhood {
                block: 999,
                transaction_index: 0,
                log_index: 0,
            }
        );
    }

    #[test]
    fn robinhood_block_range_plan_handles_up_to_date_cursor() {
        let cursor = ScanCursor::Robinhood {
            block: 1000,
            transaction_index: 0,
            log_index: 0,
        };
        let plan =
            plan_robinhood_block_range(1000, Some(&cursor), 50, 100).expect("plan block range");
        assert_eq!(plan.from_block, 1000);
        assert_eq!(plan.to_block, 1000);
    }

    #[test]
    fn robinhood_block_range_plan_rejects_wrong_cursor_or_zero_latest() {
        let sol_cursor = ScanCursor::Solana {
            signature: "sig123".to_string(),
            slot: 100,
        };
        assert!(plan_robinhood_block_range(1000, Some(&sol_cursor), 50, 100).is_err());
        assert!(plan_robinhood_block_range(0, None, 50, 100).is_err());
        assert!(plan_robinhood_block_range(1000, None, 0, 100).is_err());
    }

    #[test]
    fn robinhood_log_record_key_deduplication_and_validation() {
        let key1 = RobinhoodLogRecordKey::new(4663, "0xAbCdEf", 3).expect("valid key");
        let key2 = RobinhoodLogRecordKey::new(4663, "0xabcdef", 3).expect("valid key");
        assert_eq!(key1, key2);

        let mut set = HashSet::new();
        assert!(set.insert(key1));
        assert!(!set.insert(key2));

        assert!(RobinhoodLogRecordKey::new(0, "0xabcdef", 1).is_err());
        assert!(RobinhoodLogRecordKey::new(4663, "  ", 1).is_err());
    }

    #[test]
    fn unverified_robinhood_venues_long_and_o1_are_rejected_fail_closed() {
        let long_event = event(DexVenue::RobinhoodLong);
        assert!(classify_swap(&long_event).is_err());
        let err_msg = classify_swap(&long_event).unwrap_err().to_string();
        assert!(err_msg.contains("UNVERIFIED"));

        let o1_event = event(DexVenue::RobinhoodO1);
        assert!(classify_swap(&o1_event).is_err());
        let err_msg_o1 = classify_swap(&o1_event).unwrap_err().to_string();
        assert!(err_msg_o1.contains("UNVERIFIED"));

        let pools_event = event(DexVenue::RobinhoodPoolsTrade);
        assert!(classify_swap(&pools_event).is_err());
        let pair_event = event(DexVenue::RobinhoodPair);
        assert!(classify_swap(&pair_event).is_err());
        let bags_event = event(DexVenue::RobinhoodBags);
        assert!(classify_swap(&bags_event).is_err());

        // Verified Robinhood venues must be accepted
        assert!(classify_swap(&event(DexVenue::RobinhoodUniswapV4)).is_ok());
        assert!(classify_swap(&event(DexVenue::RobinhoodPons)).is_ok());
        assert!(classify_swap(&event(DexVenue::RobinhoodUniswapV3)).is_ok());
    }
}
