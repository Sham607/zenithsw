import re

# 1. Update src/indexer/winner_profiler.rs
wp_path = "/home/node/Developments/zenithsw/src/indexer/winner_profiler.rs"
with open(wp_path) as f:
    wp = f.read()

wp = wp.replace(
    """                    tier,
                    pnl_24h_usd: estimated_pnl_usd,
                    win_rate_pct,
                    total_trades: tx_count,
                    source: "ONCHAIN_INDEXER".to_string(),
                    reputation_score,""",
    """                    tier,
                    pnl_24h_usd: estimated_pnl_usd,
                    win_rate_pct,
                    total_trades: tx_count,
                    source: "ONCHAIN_INDEXER".to_string(),
                    reputation_score,
                    execution_track: crate::models::ExecutionTrack::FastLane,"""
)
with open(wp_path, "w") as f:
    f.write(wp)
print("Updated winner_profiler.rs")

# 2. Update src/indexer/cluster_detective.rs
cd_path = "/home/node/Developments/zenithsw/src/indexer/cluster_detective.rs"
with open(cd_path) as f:
    cd = f.read()

cd = cd.replace(
    """                    source: "CLUSTER_DETECTIVE".to_string(),
                    reputation_score: parent.reputation_score * 0.90,
                    is_active: true,""",
    """                    source: "CLUSTER_DETECTIVE".to_string(),
                    reputation_score: parent.reputation_score * 0.90,
                    execution_track: parent.execution_track,
                    is_active: true,"""
)
with open(cd_path, "w") as f:
    f.write(cd)
print("Updated cluster_detective.rs")

# 3. Update src/ingestion/fomo_scraper.rs
fs_path = "/home/node/Developments/zenithsw/src/ingestion/fomo_scraper.rs"
with open(fs_path) as f:
    fs = f.read()

fs = fs.replace(
    """                                    source: "FOMO".to_string(),
                                    reputation_score: 85.0,
                                    is_active: true,""",
    """                                    source: "FOMO".to_string(),
                                    reputation_score: 85.0,
                                    execution_track: crate::models::ExecutionTrack::FastLane,
                                    is_active: true,"""
)

fs = fs.replace(
    """                    source: "FOMO".to_string(),
                    reputation_score: 90.0,
                    is_active: true,""",
    """                    source: "FOMO".to_string(),
                    reputation_score: 90.0,
                    execution_track: crate::models::ExecutionTrack::FastLane,
                    is_active: true,"""
)

fs = fs.replace(
    """                source: "FOMO_SNAPSHOT".to_string(),
                reputation_score: 95.0,
                is_active: true,""",
    """                source: "FOMO_SNAPSHOT".to_string(),
                reputation_score: 95.0,
                execution_track: crate::models::ExecutionTrack::VaultLane,
                is_active: true,"""
)
with open(fs_path, "w") as f:
    f.write(fs)
print("Updated fomo_scraper.rs")

# 4. Update src/execution/evm_executor.rs
ee_path = "/home/node/Developments/zenithsw/src/execution/evm_executor.rs"
with open(ee_path) as f:
    ee = f.read()

ee = ee.replace(
    """            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            entry_price,""",
    """            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            track: signal.track,
            entry_price,"""
)
with open(ee_path, "w") as f:
    f.write(ee)
print("Updated evm_executor.rs")

# 5. Update src/execution/solana_executor.rs
se_path = "/home/node/Developments/zenithsw/src/execution/solana_executor.rs"
with open(se_path) as f:
    se = f.read()

se = se.replace(
    """            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            entry_price,""",
    """            target_whale_handle: format!("@{}", signal.whale_handle),
            target_whale_address: signal.whale_address.clone(),
            track: signal.track,
            entry_price,"""
)
with open(se_path, "w") as f:
    f.write(se)
print("Updated solana_executor.rs")

# 6. Update src/gating/confluence.rs
gc_path = "/home/node/Developments/zenithsw/src/gating/confluence.rs"
with open(gc_path) as f:
    gc = f.read()

# Replace all return Ok(ShadowSignal { ... in confluence.rs to include track: wallet.execution_track
gc = re.sub(
    r'(whale_tier:\s*wallet\.tier,)',
    r'\1\n                track: wallet.execution_track,',
    gc
)
with open(gc_path, "w") as f:
    f.write(gc)
print("Updated confluence.rs")
