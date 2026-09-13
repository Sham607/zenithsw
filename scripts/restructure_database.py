import sqlite3
import json
import shutil
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
QUARANTINE_DB = "/home/node/Developments/zenithsw/data/junk_wallets_quarantine.db"
AUDIT_JSON = "/home/node/Developments/zenithsw/data/onchain_solana_audit_results.json"
BACKUP_PATH = "/home/node/Developments/zenithsw/data/smart_wallets_before_restructure.db"

# 1. Backup
shutil.copy2(DB_PATH, BACKUP_PATH)
print(f"[BACKUP] Created backup at {BACKUP_PATH}")

with open(AUDIT_JSON, "r") as f:
    audit_data = json.load(f)

qualified = audit_data["qualified_candidates"]
disqualified_alive = audit_data["disqualified_alive"]
# dead wallets can be identified by: address in solana wallets not in qualified and not in disqualified_alive
qualified_addrs = set(w["address"] for w in qualified)
disqualified_alive_dict = {w["address"]: w for w in disqualified_alive}

conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

q_conn = sqlite3.connect(QUARANTINE_DB)
q_cur = q_conn.cursor()

now_iso = datetime.now(timezone.utc).isoformat()

# Fetch all Solana wallets from db
cur.execute("SELECT id, rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score FROM wallets WHERE chain='SOLANA'")
sol_wallets = cur.fetchall()

quarantined_count = 0
retained_count = 0

for row in sol_wallets:
    w_id, rank, name, handle, chain, address, tier, pnl, wr, trades, source, rep = row
    if address in qualified_addrs:
        retained_count += 1
        continue

    # Determine reason
    if address in disqualified_alive_dict:
        info = disqualified_alive_dict[address]
        reason = info.get("classification", "DISQUALIFIED_ONCHAIN_FORENSICS")
    else:
        reason = "DEAD_EMPTY_BALANCE (<0.05 SOL on-chain)"

    # Insert into quarantine
    q_cur.execute("""
        INSERT OR REPLACE INTO quarantined_wallets
        (original_id, rank, name, handle, chain, address, original_tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, quarantine_reason, quarantined_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    """, (w_id, rank, name, handle, chain, address, tier, pnl, wr, trades, source, rep, reason, now_iso))

    # Delete from main db
    cur.execute("DELETE FROM wallets WHERE id = ?", (w_id,))
    quarantined_count += 1

q_conn.commit()
q_conn.close()

print(f"[QUARANTINE] Moved {quarantined_count} dead/bot wallets to quarantine.")

# Restructure the 212 qualified wallets
# Sort by SOL balance descending
qualified.sort(key=lambda x: x["sol_balance"], reverse=True)

# Top 25 (minimum 30 SOL) -> TIER-S
# Remaining -> TIER-A
tier_s_count = 0
tier_a_count = 0

for idx, w in enumerate(qualified):
    addr = w["address"]
    sol_bal = w["sol_balance"]
    usd_equity = round(sol_bal * 102.0, 2)
    new_rank = idx + 1

    if idx < 25 and sol_bal >= 30.0:
        new_tier = "TIER-S"
        rep_score = min(98.0, 90.0 + (sol_bal / 500.0) * 8.0)
        tier_s_count += 1
    else:
        new_tier = "TIER-A"
        rep_score = min(88.0, 75.0 + (sol_bal / 50.0) * 10.0)
        tier_a_count += 1

    cur.execute("""
        UPDATE wallets
        SET rank = ?, tier = ?, pnl_24h_usd = ?, reputation_score = ?, updated_at = ?, is_active = 1
        WHERE address = ? AND chain = 'SOLANA'
    """, (new_rank, new_tier, usd_equity, round(rep_score, 1), now_iso, addr))

conn.commit()

# Check final counts
cur.execute("SELECT tier, count(*) FROM wallets WHERE chain='SOLANA' GROUP BY tier")
final_tiers = cur.fetchall()
conn.close()

print(f"[RESTRUCTURED] Solana Wallets in DB: {final_tiers}")
print(f"Tier-S: {tier_s_count} | Tier-A: {tier_a_count}")
