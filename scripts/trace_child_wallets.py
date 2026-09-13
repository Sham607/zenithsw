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
# Fetch wallets quarantined today with DEAD_EMPTY_BALANCE
cur.execute("""
    SELECT id, handle, address, original_tier, source
    FROM quarantined_wallets
    WHERE quarantined_at LIKE '2026-09-12%' AND quarantine_reason LIKE 'DEAD_EMPTY_BALANCE%'
    LIMIT 60
""")
sample_wallets = cur.fetchall()
conn.close()

print(f"Investigating {len(sample_wallets)} empty wallets for fund transfers / child wallet migrations...")

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
            with urllib.request.urlopen(req, timeout=6) as resp:
                data = json.loads(resp.read().decode())
                return data.get("result")
        except Exception:
            time.sleep(0.2)
    return None

fund_transfers = []
recipient_counts = defaultdict(list)
zero_history_count = 0

for idx, w in enumerate(sample_wallets):
    w_id, handle, addr, orig_tier, src = w
    sigs = rpc_call("getSignaturesForAddress", [addr, {"limit": 10}])
    time.sleep(0.08)

    if not sigs:
        zero_history_count += 1
        continue

    # Find the largest outflow tx
    found_outflow = False
    for s in sigs:
        sig = s.get("signature")
        if s.get("err"):
            continue
        tx = rpc_call("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
        time.sleep(0.08)
        if not tx or "meta" not in tx:
            continue

        meta = tx["meta"]
        account_keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
        if addr not in account_keys:
            continue
        my_idx = account_keys.index(addr)
        pre = meta["preBalances"][my_idx] / 1e9
        post = meta["postBalances"][my_idx] / 1e9
        delta = post - pre

        if delta < -0.05: # Outflow > 0.05 SOL
            # Find recipient who gained SOL
            for k_idx, k in enumerate(account_keys):
                if k_idx == my_idx:
                    continue
                k_gain = (meta["postBalances"][k_idx] - meta["preBalances"][k_idx]) / 1e9
                if k_gain >= 0.04: # received funds
                    recipient_counts[k].append((handle, addr, k_gain, sig))
                    fund_transfers.append({
                        "sender_handle": handle,
                        "sender_addr": addr,
                        "sent_sol": abs(delta),
                        "recipient": k,
                        "received_sol": k_gain,
                        "tx": sig,
                        "block_time": s.get("blockTime")
                    })
                    print(f"[{idx+1:02d}] @{handle:<16} transferred {abs(delta):.3f} SOL -> {k[:10]}... (tx: {sig[:12]}...)")
                    found_outflow = True
                    break
        if found_outflow:
            break

print(f"\n--- INVESTIGATION RESULTS (Sample of {len(sample_wallets)}) ---")
print(f"Wallets with 0 on-chain signatures (Pure Phantom Accounts): {zero_history_count}")
print(f"Wallets with confirmed fund drain/transfer: {len(fund_transfers)}")
print(f"Unique destination recipients found: {len(recipient_counts)}")

print("\n--- IDENTIFIED DESTINATION CLUSTERS / RECIPIENTS ---")
for dest, senders in sorted(recipient_counts.items(), key=lambda x: len(x[1]), reverse=True):
    print(f"\nDestination: {dest} (Received from {len(senders)} wallets):")
    for s in senders:
        print(f"   <- From @{s[0]} ({s[1][:8]}...): {s[2]:.3f} SOL | tx: {s[3][:12]}...")

# Save to json
with open("/home/node/Developments/zenithsw/data/child_wallet_investigation_sample.json", "w") as f:
    json.dump({
        "sample_size": len(sample_wallets),
        "zero_history_phantom_accounts": zero_history_count,
        "fund_transfers": fund_transfers,
        "recipient_clusters": {k: v for k, v in recipient_counts.items()}
    }, f, indent=2)
print("\nLog saved to data/child_wallet_investigation_sample.json")
