import sqlite3
import json

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
conn.row_factory = sqlite3.Row
cur = conn.cursor()

cur.execute("""
    SELECT target_whale,
           count(*) as total_trades,
           sum(case when pnl_usd > 0 then 1 else 0 end) as wins,
           sum(case when pnl_usd < 0 then 1 else 0 end) as losses,
           sum(pnl_usd) as net_pnl_usd,
           avg(pnl_pct) as avg_pnl_pct,
           group_concat(distinct symbol) as tokens_traded,
           group_concat(distinct exit_reason) as exit_reasons
    FROM trade_history
    GROUP BY target_whale
    ORDER BY net_pnl_usd DESC
""")

whale_stats = [dict(r) for r in cur.fetchall()]

print("=== PROFIT & LOSS BREAKDOWN BY TARGET WHALE ===")
print(f"{'Target Whale':<32} | {'Trades':<6} | {'W/L':<5} | {'Net PnL ($)':<12} | {'Avg PnL %':<10} | {'Tokens'}")
print("-" * 105)

profitable_whales = []
unprofitable_whales = []

for w in whale_stats:
    net = w["net_pnl_usd"]
    wl = f"{w['wins']}/{w['losses']}"
    target = w["target_whale"]
    avg_pct = f"{w['avg_pnl_pct']:+.1f}%"
    net_str = f"${net:+.2f}"
    tokens = w["tokens_traded"]
    print(f"{target:<32} | {w['total_trades']:<6} | {wl:<5} | {net_str:<12} | {avg_pct:<10} | {tokens}")
    if net > 0:
        profitable_whales.append(w)
    else:
        unprofitable_whales.append(w)

print("\n--- SUMMARY OF ALPHA WHALES (PROFIT MAKERS) ---")
total_win_usd = sum(w["net_pnl_usd"] for w in profitable_whales)
print(f"Total Profitable Whales: {len(profitable_whales)} -> Generated +${total_win_usd:.2f} USD")
for w in profitable_whales:
    print(f"  🟢 {w['target_whale']}: +${w['net_pnl_usd']:.2f} (WR: {w['wins']/w['total_trades']*100:.0f}%, Avg: {w['avg_pnl_pct']:+.1f}%) via [{w['tokens_traded']}]")

print("\n--- SUMMARY OF TOXIC / DRAIN WHALES (LOSS MAKERS) ---")
total_loss_usd = sum(w["net_pnl_usd"] for w in unprofitable_whales)
print(f"Total Unprofitable Whales: {len(unprofitable_whales)} -> Lost ${total_loss_usd:.2f} USD")
for w in unprofitable_whales:
    print(f"  🔴 {w['target_whale']}: ${w['net_pnl_usd']:.2f} (WR: {w['wins']/w['total_trades']*100:.0f}%) via [{w['tokens_traded']}] | Reasons: {w['exit_reasons']}")

# Exit reason breakdown
cur.execute("""
    SELECT exit_reason, count(*), sum(pnl_usd), avg(pnl_pct)
    FROM trade_history
    GROUP BY exit_reason
    ORDER BY sum(pnl_usd) DESC
""")
reasons = cur.fetchall()
print("\n--- BREAKDOWN BY EXIT REASON ---")
for r in reasons:
    print(f"  - {r[0]:<30}: count={r[1]:<2} | PnL: ${r[2]:+6.2f} | Avg %: {r[3]:+6.1f}%")

conn.close()
