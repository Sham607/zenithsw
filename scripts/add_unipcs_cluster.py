import sqlite3
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

now_iso = datetime.now(timezone.utc).isoformat()

cluster_wallets = [
    {
        "name": "Unipcs Master Vault",
        "handle": "unipcs_sol_vault",
        "address": "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF",
        "tier": "TIER-S",
        "pnl_usd": 3865000.0,
        "win_rate": 98.3,
        "trades": 60,
        "source": "UNIPCS_SOL_CLUSTER",
        "reputation": 98.0
    },
    {
        "name": "Unipcs USDC Buyer",
        "handle": "unipcs_usdc_buyer",
        "address": "4tWHFGC6iwzZm2VZaHZEiyvKMKjBArK424vMejgUUvTp",
        "tier": "TIER-A",
        "pnl_usd": 103635.0,
        "win_rate": 65.0,
        "trades": 169,
        "source": "UNIPCS_SOL_CLUSTER",
        "reputation": 88.0
    },
    {
        "name": "Unipcs Execution Arm",
        "handle": "unipcs_sniper",
        "address": "GZjYfGyUNQfDChcQ66Gc3ZMcQqPEisyRYe1nPyQhP9bp",
        "tier": "TIER-A",
        "pnl_usd": 818.0,
        "win_rate": 75.0,
        "trades": 50,
        "source": "UNIPCS_SOL_CLUSTER",
        "reputation": 88.0
    }
]

for idx, w in enumerate(cluster_wallets):
    rank = idx + 1
    cur.execute("""
        INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at)
        VALUES (?, ?, ?, 'SOLANA', ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
        ON CONFLICT(handle) DO UPDATE SET
            name = excluded.name,
            address = excluded.address,
            tier = excluded.tier,
            pnl_24h_usd = excluded.pnl_24h_usd,
            win_rate_pct = excluded.win_rate_pct,
            total_trades = excluded.total_trades,
            source = excluded.source,
            reputation_score = excluded.reputation_score,
            is_active = 1,
            updated_at = excluded.updated_at
    """, (
        rank,
        w["name"],
        w["handle"],
        w["address"],
        w["tier"],
        w["pnl_usd"],
        w["win_rate"],
        w["trades"],
        w["source"],
        w["reputation"],
        now_iso,
        now_iso
    ))
    print(f"Registered {w['name']} (@{w['handle']}) -> {w['tier']} ({w['address'][:8]}...)")

conn.commit()

cur.execute("SELECT tier, count(*) FROM wallets WHERE chain='SOLANA' GROUP BY tier")
print("\nFinal Solana Wallet Counts in DB:", cur.fetchall())
conn.close()
