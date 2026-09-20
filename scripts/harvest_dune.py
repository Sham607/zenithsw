import requests
import sqlite3
import datetime

API_KEY = "T1BKDNeHx0pwFLqvP07CZTTaswfV4b3F"
QUERY_ID = 6914195
DB_PATH = "/home/node/Developments/zenithsw/data/wallet_registry.db"

# Dune cached results URL with server-side filters
# WR >= 70%, Net PnL > 50 SOL, mints >= 15, avg buy >= 1.0 SOL, total trades <= 50,000
filter_str = "win_rate_closed_pct>=70 AND net_pnl_sol>50 AND mints_traded>=15 AND avg_buy_sol_per_mint>=1.0 AND total_trades<=50000"
url = f"https://api.dune.com/api/v1/query/{QUERY_ID}/results"

headers = {
    "X-Dune-API-Key": API_KEY
}

params = {
    "filters": filter_str,
    "limit": 150,
    "sort_by": "net_pnl_sol desc"
}

print(f"Fetching filtered smart wallets from Dune query {QUERY_ID}...")
res = requests.get(url, headers=headers, params=params, timeout=30)
if res.status_code != 200:
    print(f"Failed to fetch Dune results: {res.status_code} {res.text}")
    exit(1)

data = res.json()
rows = data.get("result", {}).get("rows", [])
print(f"Retrieved {len(rows)} matching smart money wallets!")

if not rows:
    print("No rows returned. Exiting.")
    exit(0)

conn = sqlite3.connect(DB_PATH)
cursor = conn.cursor()

# Get current max rank
cursor.execute("SELECT COALESCE(MAX(rank), 0) FROM wallets")
current_max_rank = cursor.fetchone()[0]

inserted = 0
updated = 0
now_str = datetime.datetime.utcnow().isoformat()

# Estimate SOL price for PnL USD representation ($140 default)
SOL_USD = 140.0

for r in rows:
    address = r["wallet"]
    wr = float(r.get("win_rate_closed_pct", 0.0))
    pnl_sol = float(r.get("net_pnl_sol", 0.0))
    pnl_usd = round(pnl_sol * SOL_USD, 2)
    mints = int(r.get("mints_traded", 0))
    total_trades = int(r.get("total_trades", 0))
    
    # Tiering logic
    # Tier-S: WR >= 80%, PnL > 500 SOL, mints >= 30
    # Tier-A: Remaining smart money
    if wr >= 80.0 and pnl_sol >= 500.0 and mints >= 30:
        tier = "TIER-S"
        rep_score = 90.0
        track = "FAST_LANE"
    else:
        tier = "TIER-A"
        rep_score = 82.0
        track = "FAST_LANE"
        
    handle = f"@dune_{address[:4]}_{address[-4:]}"
    name = f"Dune SM {address[:4]}..{address[-4:]}"
    
    cursor.execute("SELECT id FROM wallets WHERE address = ?", (address,))
    existing = cursor.fetchone()
    
    if existing:
        cursor.execute("""
            UPDATE wallets 
            SET win_rate_pct = ?, pnl_24h_usd = ?, total_trades = ?, tier = ?, reputation_score = ?, updated_at = ?
            WHERE address = ?
        """, (wr, pnl_usd, total_trades, tier, rep_score, now_str, address))
        updated += 1
    else:
        current_max_rank += 1
        cursor.execute("""
            INSERT INTO wallets (
                rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct,
                total_trades, source, reputation_score, is_active, updated_at, execution_track
            ) VALUES (?, ?, ?, 'Solana', ?, ?, ?, ?, ?, 'DUNE_SMART_MONEY', ?, 1, ?, ?)
        """, (current_max_rank, name, handle, address, tier, pnl_usd, wr, total_trades, rep_score, now_str, track))
        inserted += 1

conn.commit()
conn.close()

print(f"Harvest complete: {inserted} new smart wallets inserted, {updated} updated.")
