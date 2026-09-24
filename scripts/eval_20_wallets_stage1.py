import sqlite3
import json
import urllib.request
import time
import datetime
import sys

DB_PATH = "/home/node/Developments/zenithsw/data/wallet_registry.db"
RPC_URLS = [
    "https://public.rpc.solanavibestation.com",
    "https://solana-rpc.publicnode.com"
]

NOW_TS = 1790233200  # 2026-09-24 07:00:00 UTC
TWO_WEEKS_AGO_TS = NOW_TS - (14 * 86400) # 1789023600 (10 Sep 2026)

HEADERS = {
    "Content-Type": "application/json",
    "User-Agent": "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
}

def rpc_call(method, params, max_retries=3):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    for attempt in range(max_retries):
        url = RPC_URLS[attempt % len(RPC_URLS)]
        req = urllib.request.Request(url, data=payload, headers=HEADERS)
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                data = json.loads(resp.read().decode())
                if "error" in data:
                    time.sleep(1.0)
                    continue
                return data.get("result")
        except Exception as e:
            time.sleep(1.0)
    return None

conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()
cur.execute("SELECT id, rank, handle, name, address, tier, is_active FROM wallets ORDER BY tier DESC, rank ASC;")
wallets = cur.fetchall()
conn.close()

print(f"Memeriksa {len(wallets)} wallet di wallet_registry.db...", flush=True)

results = []

for idx, w in enumerate(wallets, 1):
    w_id, rank, handle, name, addr, tier, is_active = w
    
    sigs = rpc_call("getSignaturesForAddress", [addr, {"limit": 10}])
    time.sleep(0.4)
    
    latest_ts = None
    latest_sig = None
    tx_count_14d = 0
    
    if sigs:
        for s in sigs:
            bt = s.get("blockTime")
            if bt:
                if latest_ts is None:
                    latest_ts = bt
                    latest_sig = s.get("signature")
                if bt >= TWO_WEEKS_AGO_TS:
                    tx_count_14d += 1
    
    latest_dt_str = "TIDAK ADA AKTIVITAS"
    days_since_active = 999.0
    if latest_ts:
        latest_dt = datetime.datetime.fromtimestamp(latest_ts, datetime.timezone.utc)
        latest_dt_str = latest_dt.strftime("%Y-%m-%d %H:%M:%S UTC")
        days_since_active = round((NOW_TS - latest_ts) / 86400.0, 1)
    
    is_dead_14d = (tx_count_14d == 0)
    status_label = "INACTIVE (>14 HARI)" if is_dead_14d else "AKTIF"
    
    res = {
        "rank": rank,
        "handle": handle,
        "name": name,
        "address": addr,
        "tier": tier,
        "latest_sig": latest_sig,
        "latest_activity_utc": latest_dt_str,
        "days_inactive": days_since_active,
        "txs_sample_14d": tx_count_14d,
        "status_eval": status_label
    }
    results.append(res)
    print(f"[{idx:02d}/20] [{status_label}] {handle} ({tier}) -> Terakhir: {latest_dt_str} ({days_since_active} hari lalu) | 14d TXs: {tx_count_14d}", flush=True)

with open("/home/node/Developments/zenithsw/data/audit_20_wallets_eval.json", "w") as f:
    json.dump(results, f, indent=2)

print("\nAudit 20 Wallet Selesai. Hasil tersimpan di data/audit_20_wallets_eval.json", flush=True)
