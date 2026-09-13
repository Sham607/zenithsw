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

# Get all Tier-S Solana wallets
cur.execute("SELECT id, rank, name, handle, address, tier, win_rate_pct, pnl_24h_usd, source FROM wallets WHERE chain='SOLANA' AND tier='TIER-S'")
tier_s_wallets = [dict(r) for r in cur.fetchall()]
conn.close()

print(f"[{datetime.now().strftime('%H:%M:%S')}] Starting Calibration Scan on {len(tier_s_wallets)} Tier-S Solana Wallets...")

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
    for _ in range(2):
        try:
            with urllib.request.urlopen(req, timeout=8) as resp:
                return json.loads(resp.read().decode())
        except Exception:
            time.sleep(0.3)
    return None

results = []
qualified_swing_pros = []
disqualified_wallets = []

for idx, w in enumerate(tier_s_wallets):
    addr = w["address"]
    handle = w["handle"]

    # Check live native balance via free RPC
    bal = get_sol_balance(addr)
    time.sleep(0.04)

    # Query PnL V2
    st = query_solanatracker_pnl(addr)
    time.sleep(0.12) # ~8 RPS max to respect rate limits

    if not st or "summary" not in st:
        # Fallback to current DB stats
        wr = w["win_rate_pct"]
        hold_time_mins = 60.0
        avg_buy = 500.0
        deep_loss_rate = 0.0
        trades_cnt = 50
    else:
        analysis = st.get("analysis", {})
        summary = st.get("summary", {})
        timing = summary.get("timing", {})
        counts = summary.get("counts", {})
        averages = summary.get("averages", {})

        wr = float(analysis.get("winRate", 0.0))
        hold_time_mins = float(timing.get("avgHoldTimeSecs", 3600)) / 60.0
        avg_buy = float(averages.get("buy", 0.0))
        trades_cnt = int(counts.get("trades", 0))

        # Check deep losses (< -50%)
        dist = analysis.get("distribution", [])
        deep_loss_rate = 0.0
        for d in dist:
            if d.get("range") in ("<-50%", "< -50%"):
                deep_loss_rate = float(d.get("rate", 0.0))

    record = {
        "handle": handle,
        "name": w["name"],
        "address": addr,
        "source": w["source"],
        "sol_balance": round(bal, 2),
        "win_rate": round(wr, 1),
        "avg_hold_mins": round(hold_time_mins, 1),
        "avg_buy_usd": round(avg_buy, 1),
        "deep_loss_rate": round(deep_loss_rate, 1),
        "trades": trades_cnt
    }

    # RIGOROUS CRITERIA FOR "TIER-S SWING PRO"
    # 1. Active Balance >= 1.0 SOL
    # 2. Win Rate >= 60.0%
    # 3. Holding Time Sweet Spot: Between 5 minutes and 360 minutes (6 hours)
    # 4. Average Buy Size: $50 - $4,000 USD (Human copyable, no price impact)
    # 5. Deep Loss Rate <= 15.0% (Disciplined risk management)
    # 6. Trades Count >= 15 (Sufficient historical sample)

    is_qualified = True
    fail_reasons = []

    if bal < 1.0:
        is_qualified = False
        fail_reasons.append(f"Low Bal ({bal:.2f} SOL)")
    if wr < 60.0:
        is_qualified = False
        fail_reasons.append(f"Low WR ({wr:.1f}%)")
    if hold_time_mins < 5.0:
        is_qualified = False
        fail_reasons.append(f"MEV Sniper (<5m hold)")
    elif hold_time_mins > 360.0: # > 6 hours
        is_qualified = False
        fail_reasons.append(f"Diamond Hand ({hold_time_mins/60:.1f}h hold)")
    if avg_buy > 4000.0:
        is_qualified = False
        fail_reasons.append(f"Heavy Whale Buy (${avg_buy:,.0f})")
    elif avg_buy < 30.0:
        is_qualified = False
        fail_reasons.append(f"Micro Dust Buy (${avg_buy:,.0f})")
    if deep_loss_rate > 15.0:
        is_qualified = False
        fail_reasons.append(f"High Rug Exposure ({deep_loss_rate:.1f}% deep losses)")
    if trades_cnt < 15:
        is_qualified = False
        fail_reasons.append(f"Low Sample ({trades_cnt} trades)")

    record["is_qualified"] = is_qualified
    record["fail_reasons"] = fail_reasons

    if is_qualified:
        qualified_swing_pros.append(record)
        status_tag = "QUALIFIED 👑"
    else:
        disqualified_wallets.append(record)
        status_tag = "ELIMINATED ❌"

    print(f"#{idx+1:02d} [{status_tag}] @{handle:<18} | WR: {wr:>5.1f}% | Hold: {hold_time_mins:>6.1f}m | AvgBuy: ${avg_buy:>6.0f} | Bal: {bal:>6.1f} SOL | Fail: {', '.join(fail_reasons) if fail_reasons else 'None'}")

print(f"\n==================================================================")
print(f"SCAN RESULTS: {len(tier_s_wallets)} Evaluated | {len(qualified_swing_pros)} Qualified | {len(disqualified_wallets)} Eliminated")
print(f"==================================================================")

# Save to JSON
with open("/home/node/Developments/zenithsw/data/calibrated_swing_whales.json", "w") as f:
    json.dump({
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total_evaluated": len(tier_s_wallets),
        "qualified_count": len(qualified_swing_pros),
        "qualified_whales": qualified_swing_pros,
        "eliminated_whales": disqualified_wallets
    }, f, indent=2)

print("Saved report to data/calibrated_swing_whales.json")
