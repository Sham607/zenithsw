import sqlite3

db = sqlite3.connect("/home/node/Developments/zenithsw/data/smart_wallets.db")
cursor = db.cursor()

# Get all trades
cursor.execute("SELECT symbol, exit_reason, pnl_pct, pnl_usd FROM trade_history")
trades = cursor.fetchall()

wins = [t for t in trades if t[2] > 0]
losses = [t for t in trades if t[2] < 0]

total_pnl_usd = sum(t[3] for t in trades)
win_usd = sum(t[3] for t in wins)
loss_usd = sum(t[3] for t in losses)

print(f"Total Closed Trades: {len(trades)}")
print(f"Wins: {len(wins)} (Total Gain: ${win_usd:+.2f})")
print(f"Losses: {len(losses)} (Total Loss: ${loss_usd:+.2f})")
print(f"Net Realized PnL: ${total_pnl_usd:+.2f}")

print("\n--- Breakdown of Exit Reasons ---")
cursor.execute("SELECT exit_reason, count(*), sum(pnl_usd), avg(pnl_pct) FROM trade_history GROUP BY exit_reason")
for row in cursor.fetchall():
    print(f"- {row[0]}: count={row[1]}, total_usd=${row[2]:+.2f}, avg_pnl={row[3]:+.2f}%")

print("\n--- Open Positions ---")
cursor.execute("SELECT token_symbol, token_mint, amount_usd, unrealized_pnl_pct, status FROM shadow_positions WHERE status='OPEN'")
for row in cursor.fetchall():
    print(f"- {row[0]} ({row[1]}): amount=${row[2]:.2f}, unpnl={row[3]:+.2f}%")

