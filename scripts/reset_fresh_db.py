import sqlite3

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

# Clean up operational runtime tables
cur.execute("DELETE FROM shadow_positions;")
cur.execute("DELETE FROM trade_history;")
cur.execute("DELETE FROM wallet_activities;")
cur.execute("DELETE FROM shadow_signals;")

conn.commit()

# Verify counts
tables = ["shadow_positions", "trade_history", "wallet_activities", "shadow_signals", "wallets", "tracked_tokens"]
print("=== DATABASE CLEAN & FRESH STATUS ===")
for t in tables:
    cur.execute(f"SELECT count(*) FROM {t}")
    cnt = cur.fetchone()[0]
    print(f"- {t:<20}: {cnt} rows")

conn.close()
print("\nDatabase is now 100% clean and fresh for live execution!")
