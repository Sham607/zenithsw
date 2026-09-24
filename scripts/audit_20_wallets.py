import sqlite3
import json
import urllib.request
import time
import datetime
from collections import defaultdict

DB_PATH = "/home/node/Developments/zenithsw/data/wallet_registry.db"
RPC_URL = "https://solana-rpc.publicnode.com"

NOW_TS = int(time.time())
# Cutoff 14 hari: 14 * 86400 detik
TWO_WEEKS_AGO_TS = NOW_TS - (14 * 86400)

HEADERS = {
    "Content-Type": "application/json",
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
}

def rpc_call(method, params, retries=3):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(RPC_URL, data=payload, headers=HEADERS)
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                data = json.loads(resp.read().decode())
                if "error" in data:
                    time.sleep(1.0)
                    continue
                return data.get("result")
        except Exception as e:
            time.sleep(1.0 * (attempt + 1))
    return None

conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()
cur.execute("SELECT id, rank, handle, name, address, tier, is_active FROM wallets ORDER BY tier DESC, rank ASC;")
wallets = cur.fetchall()
conn.close()

print(f"Total wallets in registry: {len(wallets)}")
registry_addresses = {w[4] for w in wallets}

results = []
main_wallets_to_trace = []

for w in wallets:
    w_id, rank, handle, name, addr, tier, is_active = w
    
    # 1. getBalance
    bal_res = rpc_call("getBalance", [addr])
    sol_balance = 0.0
    if bal_res and "value" in bal_res:
        sol_balance = bal_res["value"] / 1e9
    time.sleep(0.15)
    
    # 2. getSignaturesForAddress
    sigs = rpc_call("getSignaturesForAddress", [addr, {"limit": 30}])
    time.sleep(0.2)
    
    latest_ts = None
    latest_sig = None
    tx_count_14d = 0
    total_found_sigs = len(sigs) if sigs else 0
    
    if sigs:
        for s in sigs:
            bt = s.get("blockTime")
            if bt:
                if latest_ts is None:
                    latest_ts = bt
                    latest_sig = s.get("signature")
                if bt >= TWO_WEEKS_AGO_TS:
                    tx_count_14d += 1
    
    latest_dt_str = "TIDAK ADA / TIDAK TERDETEKSI"
    days_since_active = 999.0
    if latest_ts:
        latest_dt = datetime.datetime.fromtimestamp(latest_ts, datetime.timezone.utc)
        latest_dt_str = latest_dt.strftime("%Y-%m-%d %H:%M:%S UTC")
        days_since_active = round((NOW_TS - latest_ts) / 86400.0, 1)
    
    is_dead_14d = (tx_count_14d == 0)
    
    res = {
        "handle": handle,
        "name": name,
        "address": addr,
        "tier": tier,
        "sol_balance": round(sol_balance, 3),
        "latest_sig": latest_sig,
        "latest_activity_utc": latest_dt_str,
        "days_inactive": days_since_active,
        "txs_sample_14d": tx_count_14d,
        "status_eval": "INACTIVE (>14 hari)" if is_dead_14d else "AKTIF"
    }
    results.append(res)
    print(f"[{res['status_eval']}] {handle}: Last active {latest_dt_str} ({days_since_active} hari lalu) | Saldo {round(sol_balance, 3)} SOL | TXs 14d: {tx_count_14d}")
    
    if "main_01" in handle:
        main_wallets_to_trace.append((handle, addr, sigs[:15] if sigs else []))

print("\n--- HASIL EVALUASI 20 WALLET ---")
with open("/home/node/Developments/zenithsw/data/audit_20_wallets_eval.json", "w") as f:
    json.dump(results, f, indent=2)

print("\n--- TRACING OUTBOUND FUNDS DARI MAIN WALLETS ---")
new_candidate_recipients = defaultdict(lambda: {"total_sol": 0.0, "txs": [], "sender": ""})

for handle, main_addr, sigs in main_wallets_to_trace:
    print(f"\nTracing recent outbound from {handle} ({main_addr}) across {len(sigs)} signatures...")
    for s in sigs:
        sig = s.get("signature")
        if s.get("err"):
            continue
        tx = rpc_call("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1}])
        time.sleep(0.2)
        if not tx or not tx.get("meta"):
            continue
        
        meta = tx["meta"]
        msg = tx.get("transaction", {}).get("message", {})
        
        # Check system transfers in instructions & innerInstructions
        all_ixs = []
        if "instructions" in msg:
            all_ixs.extend(msg["instructions"])
        for inner in meta.get("innerInstructions", []):
            all_ixs.extend(inner.get("instructions", []))
            
        for ix in all_ixs:
            parsed = ix.get("parsed")
            if isinstance(parsed, dict) and parsed.get("type") == "transfer":
                info = parsed.get("info", {})
                src = info.get("source")
                dst = info.get("destination")
                lamports = info.get("lamports", 0)
                sol = lamports / 1e9
                if src == main_addr and dst and dst not in registry_addresses and sol >= 0.05:
                    new_candidate_recipients[dst]["total_sol"] += sol
                    new_candidate_recipients[dst]["txs"].append({"sig": sig, "sol": sol, "blockTime": tx.get("blockTime")})
                    new_candidate_recipients[dst]["sender"] = handle

print("\n--- RECIPIENT BARU DARI MAIN WALLETS (KANDIDAT SHADOW WALLET) ---")
trace_results = []
for dst, info in new_candidate_recipients.items():
    print(f"Target: {dst} | Sender: {info['sender']} | Total SOL: {info['total_sol']} | TXs: {len(info['txs'])}")
    trace_results.append({
        "recipient": dst,
        "sender": info["sender"],
        "total_sol": round(info["total_sol"], 3),
        "txs": info["txs"]
    })

with open("/home/node/Developments/zenithsw/data/audit_main_wallets_trace.json", "w") as f:
    json.dump(trace_results, f, indent=2)

print("\nAudit and Tracing complete.")
