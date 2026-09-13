import urllib.request, json, sqlite3, time
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

now_iso = datetime.now(timezone.utc).isoformat()

# Robinhood Masters
cur.execute("""
    SELECT handle, address, pnl_24h_usd, win_rate_pct
    FROM wallets
    WHERE chain='ROBINHOOD' AND is_active=1 AND tier='TIER-S'
    ORDER BY pnl_24h_usd DESC LIMIT 15
""")
rh_masters = cur.fetchall()

rh_headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
total_rh_satellites_added = 0

print(f"Scanning inbound clusters for {len(rh_masters)} Robinhood Tier-S Masters...\n")

for handle, vault, pnl, wr in rh_masters:
    url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{vault}/token-transfers"
    req = urllib.request.Request(url, headers=rh_headers)
    try:
        with urllib.request.urlopen(req) as resp:
            items = json.loads(resp.read().decode("utf-8")).get("items", [])

        discovered_rh = set()
        for it in items[:15]:
            from_addr = it.get("from", {}).get("hash")
            to_addr = it.get("to", {}).get("hash")
            if to_addr and to_addr.lower() == vault.lower() and from_addr and from_addr.lower() != vault.lower():
                if not from_addr.startswith("0x0000000000000000"):
                    discovered_rh.add(from_addr)

        for sat in discovered_rh:
            short = sat[:6] + "_" + sat[-4:]
            sat_handle = f"{handle[:12]}_sat_{short}"
            name = f"{handle} RH Satellite ({sat[:6]}...)"

            cur.execute("""
                INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
                VALUES (1, ?, ?, 'ROBINHOOD', ?, 'TIER-S', ?, ?, 50, 'CLUSTER_DETECTIVE', 95.0, 1, 'VAULT_LANE', ?, ?)
                ON CONFLICT(handle) DO UPDATE SET
                    address = excluded.address,
                    tier = 'TIER-S',
                    is_active = 1,
                    execution_track = 'VAULT_LANE',
                    source = 'CLUSTER_DETECTIVE',
                    updated_at = excluded.updated_at
            """, (name, sat_handle, sat, pnl, wr or 75.0, now_iso, now_iso))
            total_rh_satellites_added += 1
            print(f"  🎯 [ROBINHOOD] Linked Satellite: @{sat_handle} -> {sat} (Parent: @{handle})")
        time.sleep(0.15)
    except Exception as e:
        pass

conn.commit()

active_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1").fetchone()[0]
vault_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1 AND execution_track = 'VAULT_LANE'").fetchone()[0]
fast_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1 AND execution_track = 'FAST_LANE'").fetchone()[0]
satellites = cur.execute("SELECT COUNT(*) FROM wallets WHERE source = 'CLUSTER_DETECTIVE' AND is_active = 1").fetchone()[0]

print(f"\n==========================================")
print(f"Total Active Wallets: {active_count}")
print(f" - VAULT_LANE: {vault_count}")
print(f" - FAST_LANE:  {fast_count}")
print(f" - TOTAL SATELLITES INGESTED: {satellites}")
print(f"==========================================")

conn.close()
