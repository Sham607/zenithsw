import re

db_path = "/home/node/Developments/zenithsw/src/db.rs"
with open(db_path) as f:
    content = f.read()

# 1. Update wallet SELECTs to include execution_track
# get_active_wallets
content = content.replace(
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at\n             FROM wallets WHERE is_active = 1 ORDER BY pnl_24h_usd DESC, rank ASC',
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track\n             FROM wallets WHERE is_active = 1 ORDER BY pnl_24h_usd DESC, rank ASC'
)

# get_wallets_by_tier
content = content.replace(
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at\n             FROM wallets WHERE is_active = 1 AND (tier = ?1 OR tier = ?2 OR tier LIKE ?3) ORDER BY pnl_24h_usd DESC, reputation_score DESC',
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track\n             FROM wallets WHERE is_active = 1 AND (tier = ?1 OR tier = ?2 OR tier LIKE ?3) ORDER BY pnl_24h_usd DESC, reputation_score DESC'
)

# get_qualified_wallets
content = content.replace(
    "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at\n             FROM wallets WHERE is_active = 1 AND tier IN ('TIER-S', 'TIER-A') ORDER BY pnl_24h_usd DESC, reputation_score DESC",
    "SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track\n             FROM wallets WHERE is_active = 1 AND tier IN ('TIER-S', 'TIER-A') ORDER BY pnl_24h_usd DESC, reputation_score DESC"
)

# get_wallet_by_address
content = content.replace(
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at\n             FROM wallets WHERE address = ?1 LIMIT 1',
    'SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at, execution_track\n             FROM wallets WHERE address = ?1 LIMIT 1'
)

# 2. Add execution_track extraction to SmartWallet mapping in query_map
old_wallet_map = """            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier"""

new_wallet_map = """            let updated_at = DateTime::parse_from_rfc3339(&updated_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let track_str: String = row.get(15).unwrap_or_else(|_| "FAST_LANE".to_string());
            let execution_track = ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

            Ok(SmartWallet {
                id,
                rank,
                name,
                handle,
                chain,
                address,
                tier"""

content = content.replace(old_wallet_map, new_wallet_map)

# Also add execution_track field in SmartWallet struct creation
old_wallet_end = """                source,
                reputation_score,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })"""

new_wallet_end = """                source,
                reputation_score,
                execution_track,
                is_active: is_active_int == 1,
                last_seen_at,
                updated_at,
            })"""

content = content.replace(old_wallet_end, new_wallet_end)

# 3. Update insert_signal & get_recent_signals
content = content.replace(
    'chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp\n             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)',
    'chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, track, timestamp\n             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)'
)

content = content.replace(
    'sig.confluence_count,\n                ts_str,',
    'sig.confluence_count,\n                sig.track.to_string(),\n                ts_str,'
)

content = content.replace(
    'SELECT chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp\n             FROM shadow_signals ORDER BY id DESC LIMIT ?1',
    'SELECT chain, token_symbol, token_mint, whale_handle, whale_address, whale_tier, whale_entry_price, current_pool_price, price_delta_pct, pool_liquidity_usd, is_passed, rejection_reason, confluence_count, timestamp, track\n             FROM shadow_signals ORDER BY id DESC LIMIT ?1'
)

old_sig_map = """            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ShadowSignal {
                chain,
                token_symbol,
                token_mint,
                whale_handle,
                whale_address,
                whale_tier,"""

new_sig_map = """            let timestamp = DateTime::parse_from_rfc3339(&ts_str)
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
                track,"""

content = content.replace(old_sig_map, new_sig_map)

# 4. Update upsert_position & get_active_positions
content = content.replace(
    'id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status\n             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)',
    'id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, track, entry_time, last_update, status\n             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)'
)

content = content.replace(
    'realized_pnl_usd = excluded.realized_pnl_usd,\n                last_update = excluded.last_update,',
    'realized_pnl_usd = excluded.realized_pnl_usd,\n                track = excluded.track,\n                last_update = excluded.last_update,'
)

content = content.replace(
    'pos.realized_pnl_usd,\n                entry_str,\n                update_str,\n                pos.status,',
    'pos.realized_pnl_usd,\n                pos.track.to_string(),\n                entry_str,\n                update_str,\n                pos.status,'
)

content = content.replace(
    'SELECT id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status\n             FROM shadow_positions WHERE status != \'CLOSED\' ORDER BY entry_time DESC',
    'SELECT id, chain, token_symbol, token_mint, target_whale_handle, target_whale_address, entry_price, current_price, peak_price, amount_native, amount_usd, remaining_portion_pct, unrealized_pnl_pct, realized_pnl_usd, entry_time, last_update, status, track\n             FROM shadow_positions WHERE status != \'CLOSED\' ORDER BY entry_time DESC'
)

old_pos_map = """            let last_update = DateTime::parse_from_rfc3339(&update_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(ShadowPosition {
                id,
                chain,
                token_symbol,
                token_mint,
                target_whale_handle,
                target_whale_address,"""

new_pos_map = """            let last_update = DateTime::parse_from_rfc3339(&update_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let track_str: String = row.get(17).unwrap_or_else(|_| "FAST_LANE".to_string());
            let track = ExecutionTrack::from_str(&track_str).unwrap_or(ExecutionTrack::FastLane);

            Ok(ShadowPosition {
                id,
                chain,
                token_symbol,
                token_mint,
                target_whale_handle,
                target_whale_address,
                track,"""

content = content.replace(old_pos_map, new_pos_map)

# 5. Update close_position_portion & get_recent_trades
content = content.replace(
    'SELECT entry_price, amount_usd, remaining_portion_pct, realized_pnl_usd, entry_time, chain, token_symbol, token_mint, target_whale_handle FROM shadow_positions WHERE id = ?1',
    'SELECT entry_price, amount_usd, remaining_portion_pct, realized_pnl_usd, entry_time, chain, token_symbol, token_mint, target_whale_handle, track FROM shadow_positions WHERE id = ?1'
)

old_close_opt = """                let mint: String = row.get(7)?;
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
                ))"""

new_close_opt = """                let mint: String = row.get(7)?;
                let target_whale: String = row.get(8)?;
                let track_s: String = row.get(9).unwrap_or_else(|_| "FAST_LANE".to_string());
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
                ))"""

content = content.replace(old_close_opt, new_close_opt)

old_close_unpack = """            symbol,
            mint,
            target_whale,
        )) = pos_opt"""

new_close_unpack = """            symbol,
            mint,
            target_whale,
            track_s,
        )) = pos_opt"""

content = content.replace(old_close_unpack, new_close_unpack)

content = content.replace(
    'timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs\n                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)',
    'timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs, track\n                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)'
)

content = content.replace(
    'duration_secs,\n                ],',
    'duration_secs,\n                    track_s,\n                ],'
)

content = content.replace(
    'SELECT timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs\n             FROM trade_history ORDER BY id DESC LIMIT ?1',
    'SELECT timestamp, chain, action, symbol, mint, target_whale, exit_reason, portion_sold_pct, entry_price, exit_price, pnl_pct, pnl_usd, duration_secs, track\n             FROM trade_history ORDER BY id DESC LIMIT ?1'
)

old_trade_rec = """            let duration_secs: u64 = row.get(12)?;

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
                exit_reason,"""

new_trade_rec = """            let duration_secs: u64 = row.get(12)?;
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
                track,"""

content = content.replace(old_trade_rec, new_trade_rec)

with open(db_path, "w") as f:
    f.write(content)
print("Updated src/db.rs successfully!")
