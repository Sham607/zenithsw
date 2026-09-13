import urllib.request, json, sqlite3, time
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

now_iso = datetime.now(timezone.utc).isoformat()
print(f"[{now_iso}] Starting Mass Whale Cluster Detective Ingestion...")

# 1. Ambil seluruh Top Tier-S Wallets (Solana)
cur.execute("""
    SELECT handle, address, pnl_24h_usd, win_rate_pct
    FROM wallets
    WHERE chain='SOLANA' AND is_active=1 AND tier='TIER-S'
    ORDER BY pnl_24h_usd DESC
""")
sol_masters = cur.fetchall()

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
total_sol_satellites_added = 0

print(f"Scanning inbound clusters for {len(sol_masters)} Solana Tier-S Masters...\n")

for handle, vault, pnl, wr in sol_masters:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [vault, {"limit": 15}]
    }
    req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

        discovered_satellites = set()

        for s in sigs:
            sig = s["signature"]
            tx_payload = {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "getTransaction",
                "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
            }
            tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
            try:
                with urllib.request.urlopen(tx_req) as tx_resp:
                    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
                    if not tx:
                        continue
                    meta = tx.get("meta", {})
                    pre = meta.get("preTokenBalances", [])
                    post = meta.get("postTokenBalances", [])
                    for p in post:
                        if p.get("owner") == vault:
                            mint = p.get("mint")
                            post_amt = (p.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                            pre_amt = 0.0
                            for pr in pre:
                                if pr.get("owner") == vault and pr.get("mint") == mint:
                                    pre_amt = (pr.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                                    break
                            if post_amt > pre_amt:
                                for pr in pre:
                                    if pr.get("mint") == mint and pr.get("owner") != vault:
                                        p_owner = pr.get("owner")
                                        if p_owner and len(p_owner) > 30 and not p_owner.startswith("1111") and p_owner != vault:
                                            discovered_satellites.add(p_owner)
                time.sleep(0.12)
            except Exception:
                pass

        for sat in discovered_satellites:
            short = sat[:6] + "_" + sat[-4:]
            sat_handle = f"{handle[:12]}_sat_{short}"
            name = f"{handle} Satellite ({sat[:6]}...)"

            cur.execute("""
                INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
                VALUES (1, ?, ?, 'SOLANA', ?, 'TIER-S', ?, ?, 50, 'CLUSTER_DETECTIVE', 95.0, 1, 'VAULT_LANE', ?, ?)
                ON CONFLICT(handle) DO UPDATE SET
                    address = excluded.address,
                    tier = 'TIER-S',
                    is_active = 1,
                    execution_track = 'VAULT_LANE',
                    source = 'CLUSTER_DETECTIVE',
                    updated_at = excluded.updated_at
            """, (name, sat_handle, sat, pnl, wr or 75.0, now_iso, now_iso))
            total_sol_satellites_added += 1
            print(f"  🎯 [SOLANA] Linked Satellite: @{sat_handle} -> {sat} (Parent: @{handle})")

    except Exception as e:
        print(f"Err {handle}: {e}")

conn.commit()

# 2. Ambil seluruh Top Tier-S Wallets (Robinhood)
cur.execute("""
    SELECT handle, address, pnl_24h_usd, win_rate_pct
    FROM wallets
    WHERE chain='ROBINHOOD' AND is_active=1 AND tier='TIER-S'
    ORDER BY pnl_24h_usd DESC LIMIT 20
""")
rh_masters = cur.fetchall()

rh_headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
total_rh_satellites_added = 0

print(f"\nScanning inbound clusters for {len(rh_masters)} Robinhood Tier-S Masters...\n")

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
        time.sleep(0.2)
    except Exception as e:
        pass

conn.commit()

print(f"\n==========================================")
print(f"Ingestion Completed:")
print(f"  - Solana Satellites Ingested: {total_sol_satellites_added}")
print(f"  - Robinhood Satellites Ingested: {total_rh_satellites_added}")

active_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1").fetchone()[0]
vault_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1 AND execution_track = 'VAULT_LANE'").fetchone()[0]
fast_count = cur.execute("SELECT COUNT(*) FROM wallets WHERE is_active = 1 AND execution_track = 'FAST_LANE'").fetchone()[0]
print(f"Current Total Active Wallets: {active_count} (Vault Lane: {vault_count}, Fast Lane: {fast_count})")
print(f"==========================================")

conn.close()
