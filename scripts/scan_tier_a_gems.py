import urllib.request
import json
import sqlite3
import time
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
DATA_API_KEY = "fc7e6533-da19-48bb-b80f-565a104d341f"
RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"

conn = sqlite3.connect(DB_PATH)
conn.row_factory = sqlite3.Row
cur = conn.cursor()
cur.execute("SELECT id, rank, name, handle, address, tier, win_rate_pct, pnl_24h_usd, source FROM wallets WHERE chain='SOLANA' AND tier='TIER-A'")
tier_a = [dict(r) for r in cur.fetchall()]
conn.close()

print(f"Scanning {len(tier_a)} Tier-A wallets for Swing Pro Gems...")

def get_sol_balance(addr):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [addr]}).encode()
    req = urllib.request.Request(RPC_URL, data=payload, headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            res = json.loads(r.read().decode())
            return res.get("result", {}).get("value", 0) / 1e9
    except Exception:
        return 0.0

def query_solanatracker_pnl(addr):
    url = f"https://data.solanatracker.io/v2/pnl/wallets/{addr}"
    req = urllib.request.Request(url, headers={"x-api-key": DATA_API_KEY, "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=8) as resp:
            return json.loads(resp.read().decode())
    except Exception:
        return None

qualified_a = []

for idx, w in enumerate(tier_a[:100]): # sample 100
    addr = w["address"]
    handle = w["handle"]

    st = query_solanatracker_pnl(addr)
    time.sleep(0.08)
    if not st or "summary" not in st:
        continue

    analysis = st.get("analysis", {})
    summary = st.get("summary", {})
    timing = summary.get("timing", {})
    averages = summary.get("averages", {})
    counts = summary.get("counts", {})

    wr = float(analysis.get("winRate", 0.0))
    hold_time_mins = float(timing.get("avgHoldTimeSecs", 0)) / 60.0
    avg_buy = float(averages.get("buy", 0.0))
    trades = int(counts.get("trades", 0))

    dist = analysis.get("distribution", [])
    deep_loss_rate = 0.0
    for d in dist:
        if d.get("range") in ("<-50%", "< -50%"):
            deep_loss_rate = float(d.get("rate") or 0.0)

    # Criteria:
    # 1. WR >= 60%
    # 2. Hold time: 10m to 720m (12 hours)
    # 3. Avg Buy: $50 - $3,000
    # 4. Deep loss rate <= 20%
    # 5. Trades >= 15
    if wr >= 60.0 and 10.0 <= hold_time_mins <= 720.0 and 50.0 <= avg_buy <= 3000.0 and deep_loss_rate <= 20.0 and trades >= 15:
        bal = get_sol_balance(addr)
        if bal >= 1.0:
            rec = {
                "handle": handle,
                "address": addr,
                "win_rate": wr,
                "hold_mins": hold_time_mins,
                "avg_buy": avg_buy,
                "balance_sol": bal,
                "trades": trades
            }
            qualified_a.append(rec)
            print(f"[GEM FOUND 💎] @{handle:<16} | WR: {wr:>5.1f}% | Hold: {hold_time_mins:>5.1f}m | AvgBuy: ${avg_buy:>5.0f} | Bal: {bal:>6.1f} SOL")

print(f"\nFound {len(qualified_a)} Swing Pro Gems in sampled Tier-A wallets!")
with open("/home/node/Developments/zenithsw/data/tier_a_swing_gems.json", "w") as f:
    json.dump(qualified_a, f, indent=2)
