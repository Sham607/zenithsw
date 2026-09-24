import sqlite3
import json
import urllib.request
import time
import datetime
from collections import defaultdict

DB_PATH = "/home/node/Developments/zenithsw/data/wallet_registry.db"
RPC_URL = "https://public.rpc.solanavibestation.com"

HEADERS = {
    "Content-Type": "application/json",
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
}

def rpc_call(method, params, retries=2):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for attempt in range(retries):
        try:
            req = urllib.request.Request(RPC_URL, data=payload, headers=HEADERS)
            with urllib.request.urlopen(req, timeout=12) as resp:
                data = json.loads(resp.read().decode())
                if "error" in data:
                    time.sleep(0.5)
                    continue
                return data.get("result")
        except Exception:
            time.sleep(0.5)
    return None

conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()
cur.execute("SELECT handle, address FROM wallets WHERE handle LIKE '%main_01';")
main_wallets = cur.fetchall()
cur.execute("SELECT address, handle FROM wallets;")
all_registry = dict(cur.fetchall())
conn.close()

print(f"Tracing {len(main_wallets)} Main Whale Wallets...", flush=True)

findings = []

for handle, main_addr in main_wallets:
    print(f"\n=======================================================", flush=True)
    print(f"Inspecting Main Wallet: {handle} ({main_addr})", flush=True)
    print(f"=======================================================", flush=True)
    
    sigs = rpc_call("getSignaturesForAddress", [main_addr, {"limit": 15}])
    time.sleep(0.3)
    if not sigs:
        print(f"  [!] Gagal mengambil signatures untuk {handle}", flush=True)
        continue
    
    print(f"  Menganalisis {len(sigs)} transaksi terbaru...", flush=True)
    
    outbound_fundings = []
    inbound_token_transfers = []
    direct_swaps = []
    
    for s in sigs:
        sig = s.get("signature")
        if s.get("err"):
            continue
        
        tx = rpc_call("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 1}])
        time.sleep(0.25)
        if not tx or not tx.get("meta"):
            continue
        
        meta = tx["meta"]
        block_time = tx.get("blockTime")
        dt_str = datetime.datetime.fromtimestamp(block_time, datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%S") if block_time else "N/A"
        msg = tx.get("transaction", {}).get("message", {})
        
        # 1. Check SOL Outbound Transfer
        # Gather all instructions
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
                if src == main_addr and dst != main_addr and sol >= 0.1:
                    is_known = dst in all_registry
                    known_name = all_registry.get(dst, "UNKNOWN / BARU")
                    outbound_fundings.append({
                        "sig": sig,
                        "time": dt_str,
                        "amount_sol": round(sol, 4),
                        "destination": dst,
                        "is_known_in_registry": is_known,
                        "registry_name": known_name
                    })
                    print(f"  [OUTBOUND SOL] {dt_str} -> {sol:.3f} SOL dikirim ke {dst} ({known_name}) | tx: {sig[:16]}...", flush=True)

        # 2. Check Token Balance Deltas (Inbound Token Accumulation)
        pre_tokens = {t.get("mint"): t for t in meta.get("preTokenBalances", []) if t.get("owner") == main_addr}
        post_tokens = {t.get("mint"): t for t in meta.get("postTokenBalances", []) if t.get("owner") == main_addr}
        
        all_mints = set(pre_tokens.keys()).union(set(post_tokens.keys()))
        for m in all_mints:
            pre_amt = float(pre_tokens.get(m, {}).get("uiTokenAmount", {}).get("uiAmount") or 0.0)
            post_amt = float(post_tokens.get(m, {}).get("uiTokenAmount", {}).get("uiAmount") or 0.0)
            delta = post_amt - pre_amt
            if delta > 0.0001:
                # Inflow token detected
                inbound_token_transfers.append({
                    "sig": sig,
                    "time": dt_str,
                    "mint": m,
                    "delta_amount": delta
                })
                print(f"  [TOKEN INFLOW 💎] {dt_str} -> +{delta:,.2f} token mint {m[:10]}... | tx: {sig[:16]}...", flush=True)

    findings.append({
        "handle": handle,
        "main_address": main_addr,
        "outbound_fundings": outbound_fundings,
        "inbound_token_transfers": inbound_token_transfers
    })

with open("/home/node/Developments/zenithsw/data/trace_main_wallets_forensics.json", "w") as f:
    json.dump(findings, f, indent=2)

print("\nForensic Tracing Selesai. Hasil disimpan di data/trace_main_wallets_forensics.json", flush=True)
