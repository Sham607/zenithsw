import json, sqlite3
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

with open("/home/node/Developments/zenithsw/data/fomo_my_following_raw.json") as f:
    raw_data = json.load(f)

users = raw_data.get("responseObject", {}).get("users", [])
now_iso = datetime.now(timezone.utc).isoformat()

print(f"Total followed traders to ingest: {len(users)}\n")

sol_added = 0
evm_added = 0

for u in users:
    handle = u.get("userHandle")
    name = u.get("displayName") or handle
    pnl = float(u.get("pnl24h") or 0.0)
    trades = int(u.get("numTrades") or 0)
    sol_addr = u.get("address")
    evm_addr = u.get("evmAddress")

    # Tentukan tier dasar (Following user = Prioritas Tinggi / Tier-S atau Tier-A)
    tier = "TIER-S" if abs(pnl) > 100_000 or trades > 1000 else "TIER-A"

    # Ingest Solana Wallet jika ada
    if sol_addr and len(sol_addr) > 30 and not sol_addr.startswith("0x"):
        sol_handle = f"fomo_sol_{handle}"
        cur.execute("""
            INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
            VALUES (1, ?, ?, 'SOLANA', ?, ?, ?, 80.0, ?, 'FOMO_FOLLOWING', 95.0, 1, 'VAULT_LANE', ?, ?)
            ON CONFLICT(handle) DO UPDATE SET
                address = excluded.address,
                tier = excluded.tier,
                pnl_24h_usd = excluded.pnl_24h_usd,
                total_trades = excluded.total_trades,
                is_active = 1,
                execution_track = 'VAULT_LANE',
                source = 'FOMO_FOLLOWING',
                updated_at = excluded.updated_at
        """, (f"{name} (Solana)", sol_handle, sol_addr, tier, pnl, trades, now_iso, now_iso))
        sol_added += 1
        print(f"✅ Solana Ingested: @{sol_handle} -> {sol_addr[:8]}... ({tier})")

    # Ingest Robinhood/EVM Wallet jika ada
    if evm_addr and evm_addr.startswith("0x") and len(evm_addr) == 42:
        rh_handle = f"fomo_rh_{handle}"
        cur.execute("""
            INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
            VALUES (1, ?, ?, 'ROBINHOOD', ?, ?, ?, 80.0, ?, 'FOMO_FOLLOWING', 95.0, 1, 'VAULT_LANE', ?, ?)
            ON CONFLICT(handle) DO UPDATE SET
                address = excluded.address,
                tier = excluded.tier,
                pnl_24h_usd = excluded.pnl_24h_usd,
                total_trades = excluded.total_trades,
                is_active = 1,
                execution_track = 'VAULT_LANE',
                source = 'FOMO_FOLLOWING',
                updated_at = excluded.updated_at
        """, (f"{name} (Robinhood)", rh_handle, evm_addr, tier, pnl, trades, now_iso, now_iso))
        evm_added += 1
        print(f"✅ Robinhood Ingested: @{rh_handle} -> {evm_addr[:8]}... ({tier})")

conn.commit()

active_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1").fetchone()[0]
following_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE source = 'FOMO_FOLLOWING' AND is_active = 1").fetchone()[0]

print(f"\n==========================================")
print(f"Following Ingestion Completed Successfully:")
print(f"  - Solana Wallets Ingested: {sol_added}")
print(f"  - Robinhood Wallets Ingested: {evm_added}")
print(f"  - Total Following in DB: {following_count}")
print(f"  - Total Active Wallets in DB: {active_count}")
print(f"==========================================")

conn.close()
