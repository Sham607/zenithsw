import sqlite3
import csv
import json
import shutil
import os
from datetime import datetime, timezone

SRC_DB = "/home/node/Developments/zenithsw/data/smart_wallets.db"
BACKUP_DIR = "/home/node/Developments/zenithsw/data/backups/historical_trades_audit"
os.makedirs(BACKUP_DIR, exist_ok=True)

# 1. Full SQLite binary backup
timestamp_str = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
raw_db_backup = os.path.join(BACKUP_DIR, f"smart_wallets_pre_fresh_{timestamp_str}.db")
shutil.copy2(SRC_DB, raw_db_backup)
print(f"[BACKUP 1/4] Full SQLite DB snapshot saved to: {raw_db_backup}")

conn = sqlite3.connect(SRC_DB)
conn.row_factory = sqlite3.Row
cur = conn.cursor()

# 2. Export trade_history to JSON and CSV
cur.execute("SELECT * FROM trade_history ORDER BY id ASC")
trades = [dict(row) for row in cur.fetchall()]

trades_json_path = os.path.join(BACKUP_DIR, "trade_history.json")
with open(trades_json_path, "w") as f:
    json.dump(trades, f, indent=2)

trades_csv_path = os.path.join(BACKUP_DIR, "trade_history.csv")
if trades:
    with open(trades_csv_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=trades[0].keys())
        writer.writeheader()
        writer.writerows(trades)
print(f"[BACKUP 2/4] Exported {len(trades)} trade_history records to JSON & CSV.")

# 3. Export shadow_positions to JSON and CSV
cur.execute("SELECT * FROM shadow_positions")
positions = [dict(row) for row in cur.fetchall()]

positions_json_path = os.path.join(BACKUP_DIR, "shadow_positions.json")
with open(positions_json_path, "w") as f:
    json.dump(positions, f, indent=2)

positions_csv_path = os.path.join(BACKUP_DIR, "shadow_positions.csv")
if positions:
    with open(positions_csv_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=positions[0].keys())
        writer.writeheader()
        writer.writerows(positions)
print(f"[BACKUP 3/4] Exported {len(positions)} shadow_positions to JSON & CSV.")

# 4. Export wallet_activities
cur.execute("SELECT * FROM wallet_activities ORDER BY id DESC")
acts = [dict(row) for row in cur.fetchall()]
acts_json_path = os.path.join(BACKUP_DIR, "wallet_activities_sample.json")
with open(acts_json_path, "w") as f:
    json.dump(acts[:100], f, indent=2)
print(f"[BACKUP 4/4] Exported wallet_activities records ({len(acts)} total).")

conn.close()
print("\nAll historical trade backups successfully created in:", BACKUP_DIR)
