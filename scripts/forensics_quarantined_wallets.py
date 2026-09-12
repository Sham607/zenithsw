import sqlite3
import json
import urllib.request
import time
from collections import defaultdict

QUARANTINE_DB = "/home/node/Developments/zenithsw/data/junk_wallets_quarantine.db"
RPC_URLS = [
    "https://api.mainnet-beta.solana.com",
    "https://solana-rpc.publicnode.com"
]

conn = sqlite3.connect(QUARANTINE_DB)
cur = conn.cursor()
# Get quarantined Solana wallets
cur.execute("SELECT id, handle, name, address, original_tier, source, quarantine_reason FROM quarantined_wallets WHERE chain='SOLANA'")
rows = cur.fetchall()
conn.close()

print(f"Total Quarantined Solana Wallets to Investigate: {len(rows)}")

# Let's inspect the top 50 formerly 'TIER-S' or 'TIER-A' quarantined wallets first
tier_s_a = [r for r in rows if r[4] in ('TIER-S', 'TIER-A')]
print(f"Of which formerly Tier-S/A: {len(tier_s_a)}")

endpoint_idx = 0
def rpc_call(method, params):
    global endpoint_idx
    ep = RPC_URLS[endpoint_idx % len(RPC_URLS)]
    endpoint_idx += 1
    req = urllib.request.Request(
        ep,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json"}
    )
    for _ in range(3):
        try:
            with urllib.request.urlopen(req, timeout=8) as resp:
                data = json.loads(resp.read().decode())
                return data.get("result")
        except Exception:
            time.sleep(0.3)
    return None

# Investigate destination clusters
destination_clusters = defaultdict(list)
drain_txs = []

print("\nTracing last drain / transfer transactions for high-priority quarantined wallets...")
for idx, r in enumerate(tier_s_a[:40]):
    w_id, handle, name, addr, orig_tier, src, reason = r
    sigs = rpc_call("getSignaturesForAddress", [addr, {"limit": 5}])
    time.sleep(0.1)
    if not sigs:
        continue

    # Inspect last tx
    last_sig = sigs[0].get("signature")
    tx = rpc_call("getTransaction", [last_sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
    time.sleep(0.12)
    if not tx:
        continue

    meta = tx.get("meta", {})
    account_keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
    if addr not in account_keys:
        continue
    my_idx = account_keys.index(addr)
    pre = meta.get("preBalances", [])[my_idx] / 1e9
    post = meta.get("postBalances", [])[my_idx] / 1e9
    diff = post - pre

    # Look for recipient who gained SOL
    recipients = []
    for k_idx, k in enumerate(account_keys):
        if k_idx == my_idx:
            continue
        k_pre = meta.get("preBalances", [])[k_idx] / 1e9
        k_post = meta.get("postBalances", [])[k_idx] / 1e9
        k_diff = k_post - k_pre
        if k_diff > 0.005: # gained SOL
            recipients.append((k, k_diff))

    if recipients:
        for rec, amt in recipients:
            destination_clusters[rec].append((handle, addr, amt, last_sig))
            drain_txs.append({
                "wallet": handle,
                "address": addr,
                "outflow": diff,
                "destination": rec,
                "received": amt,
                "tx": last_sig
            })
        print(f"[{idx+1:02d}] @{handle:<16} drained -> sent {recipients[0][1]:.3f} SOL to {recipients[0][0][:10]}... (tx: {last_sig[:12]}...)")
    else:
        print(f"[{idx+1:02d}] @{handle:<16} last tx change: {diff:.4f} SOL (no direct recipient identified)")

print("\n=== CLUSTER SUMMARY ===")
print(f"Total Drain Transactions Identified: {len(drain_txs)}")
print(f"Unique Destination Addresses: {len(destination_clusters)}")
for dest, senders in sorted(destination_clusters.items(), key=lambda x: len(x[1]), reverse=True):
    if len(senders) > 1:
        print(f"[CONVERGENCE HUB] Destination {dest} received funds from {len(senders)} quarantined wallets:")
        for s in senders:
            print(f"   - @{s[0]} ({s[1][:8]}...): {s[2]:.3f} SOL")

# Save investigation log
with open("/home/node/Developments/zenithsw/data/quarantined_forensics_sample.json", "w") as f:
    json.dump({"drain_txs": drain_txs, "clusters": {k: v for k, v in destination_clusters.items()}}, f, indent=2)

print("\nForensics sample saved to data/quarantined_forensics_sample.json")
