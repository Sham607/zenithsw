import urllib.request, json, sqlite3, time
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

now_iso = datetime.now(timezone.utc).isoformat()
print(f"[{now_iso}] Starting Unipcs Cluster Ingestion (Solana + Robinhood)...")

# 1. Solana Inbound Senders to Unipcs Vault (2heJbC32...)
rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sol_vault = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [sol_vault, {"limit": 50}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

discovered_sol_satellites = set()

# Pre-known cluster wallets for Solana Unipcs
discovered_sol_satellites.add("4tWHFGC6iwzZm2VZaHZEiyvKMKjBArK424vMejgUUvTp") # USDC Buyer
discovered_sol_satellites.add("GZjYfGyUNQfDChcQ66Gc3ZMcQqPEisyRYe1nPyQhP9bp") # Sniper

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
            tx_data = json.loads(tx_resp.read().decode("utf-8")).get("result")
            if not tx_data:
                continue
            meta = tx_data.get("meta", {})
            pre = meta.get("preTokenBalances", [])
            post = meta.get("postTokenBalances", [])
            for p in post:
                if p.get("owner") == sol_vault:
                    mint = p.get("mint")
                    post_amt = (p.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                    pre_amt = 0.0
                    for pr in pre:
                        if pr.get("owner") == sol_vault and pr.get("mint") == mint:
                            pre_amt = (pr.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                            break
                    delta = post_amt - pre_amt
                    if delta > 0:
                        for pr in pre:
                            if pr.get("mint") == mint and pr.get("owner") != sol_vault:
                                p_owner = pr.get("owner")
                                if p_owner and len(p_owner) > 30 and not p_owner.startswith("1111"):
                                    discovered_sol_satellites.add(p_owner)
            time.sleep(0.15)
    except Exception:
        pass

print(f"Discovered {len(discovered_sol_satellites)} Solana Unipcs Satellites: {discovered_sol_satellites}")

# Register Solana Satellites
for addr in discovered_sol_satellites:
    short = addr[:6] + "_" + addr[-4:]
    handle = f"unipcs_sol_{short}"
    name = f"Unipcs SOL Satellite ({addr[:6]}...)"

    cur.execute("""
        INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
        VALUES (1, ?, ?, 'SOLANA', ?, 'TIER-S', 150000.0, 85.0, 50, 'UNIPCS_CLUSTER', 95.0, 1, 'VAULT_LANE', ?, ?)
        ON CONFLICT(handle) DO UPDATE SET
            address = excluded.address,
            tier = 'TIER-S',
            is_active = 1,
            execution_track = 'VAULT_LANE',
            source = 'UNIPCS_CLUSTER',
            updated_at = excluded.updated_at
    """, (name, handle, addr, now_iso, now_iso))
    print(f"✅ Ingested Solana Satellite: @{handle} -> {addr}")

# 2. Robinhood Inbound Senders / Signers to Unipcs RH Vault (0x2ac082...)
rh_vault = "0x2ac082e252143c89c6e3bcb972e49855f9f6711c"
url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{rh_vault}/token-transfers"
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
req = urllib.request.Request(url, headers=headers)
discovered_rh_satellites = set()

try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        items = data.get("items", [])
        tx_hashes = [it.get("transaction_hash") for it in items if it.get("transaction_hash")]
        for tx in tx_hashes[:25]:
            tx_url = f"https://robinhoodchain.blockscout.com/api/v2/transactions/{tx}"
            tx_req = urllib.request.Request(tx_url, headers=headers)
            try:
                with urllib.request.urlopen(tx_req) as tx_resp:
                    t_data = json.loads(tx_resp.read().decode("utf-8"))
                    initiator = t_data.get("from", {}).get("hash")
                    if initiator and initiator.lower() != rh_vault.lower():
                        discovered_rh_satellites.add(initiator)
                time.sleep(0.15)
            except Exception:
                pass
except Exception as e:
    print(f"Err fetching RH transfers: {e}")

print(f"\nDiscovered {len(discovered_rh_satellites)} Robinhood Unipcs Satellites: {discovered_rh_satellites}")

for addr in discovered_rh_satellites:
    short = addr[:6] + "_" + addr[-4:]
    handle = f"unipcs_rh_{short}"
    name = f"Unipcs RH Satellite ({addr[:6]}...)"

    cur.execute("""
        INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, execution_track, last_seen_at, updated_at)
        VALUES (1, ?, ?, 'ROBINHOOD', ?, 'TIER-S', 250000.0, 88.0, 60, 'UNIPCS_CLUSTER', 95.0, 1, 'VAULT_LANE', ?, ?)
        ON CONFLICT(handle) DO UPDATE SET
            address = excluded.address,
            tier = 'TIER-S',
            is_active = 1,
            execution_track = 'VAULT_LANE',
            source = 'UNIPCS_CLUSTER',
            updated_at = excluded.updated_at
    """, (name, handle, addr, now_iso, now_iso))
    print(f"✅ Ingested Robinhood Satellite: @{handle} -> {addr}")

# Update also existing unipcs cluster handles if any
cur.execute("UPDATE wallets SET is_active = 1, execution_track = 'VAULT_LANE', tier = 'TIER-S' WHERE handle LIKE '%unipcs%'")

conn.commit()

cur.execute("SELECT handle, chain, address, tier, execution_track, is_active FROM wallets WHERE handle LIKE '%unipcs%'")
print("\n=== Current Unipcs Cluster in Database ===")
for r in cur.fetchall():
    print(r)

conn.close()
