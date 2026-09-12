import os
from pathlib import Path

def load_env_val(key, default=""):
    env_file = Path(__file__).resolve().parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.strip().startswith(f"{key}="):
                return line.strip().split("=", 1)[1].strip('"'')
    return os.getenv(key, default)

import asyncio
import aiohttp
import urllib.parse
import json
import sqlite3
import time

DATA_API_KEY = load_env_val("SOLANATRACKER_API_KEY")
RPC_URL = load_env_val("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com")
DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"

headers = {
    "x-api-key": DATA_API_KEY,
    "User-Agent": "Mozilla/5.0"
}

async def harvest_100():
    print("=== HARVESTING 100 TOP ALPHA TRADERS FROM SOLANA TRACKER ===")
    all_traders = {}

    timeout = aiohttp.ClientTimeout(total=15)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        # 1. KOL Leaderboard Page 1 & 2 (Top verified organic traders)
        for page_num in [1, 2]:
            url = f"https://data.solanatracker.io/v2/pnl/leaderboard/kols?limit=50&page={page_num}&sort=total&direction=desc"
            print(f"Fetching KOL Leaderboard Page {page_num}...")
            try:
                async with session.get(url, headers=headers) as resp:
                    if resp.status == 200:
                        data = await resp.json()
                        traders = data.get("traders", [])
                        print(f"  Page {page_num} returned {len(traders)} KOLs.")
                        for t in traders:
                            w = t.get("wallet")
                            if not w:
                                continue
                            ident = t.get("identity") or {}
                            name = ident.get("name") or ident.get("twitter") or f"ST-KOL-{w[:4]}"
                            all_traders[w] = {
                                "address": w,
                                "name": name,
                                "handle": ident.get("twitter", f"st_{w[:4]}_{w[-4:]}").replace("@", ""),
                                "source": "SOLANATRACKER_KOL",
                                "win_rate": t.get("winRate", 0.0),
                                "pnl": t.get("summary", {}).get("total", 0.0) or t.get("period", {}).get("realized", 0.0),
                                "trades": t.get("counts", {}).get("trades", 100),
                                "identity": ident
                            }
                await asyncio.sleep(0.5)
            except Exception as e:
                print("Error fetching KOLs:", e)

        # 2. PnL V2 Top Traders (pages 1 to 3)
        cursor = None
        for p in range(1, 4):
            if cursor:
                url = f"https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted&cursor={urllib.parse.quote(cursor)}"
            else:
                url = "https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted"
            print(f"Fetching PnL V2 Page {p}...")
            try:
                async with session.get(url, headers=headers) as resp:
                    if resp.status == 200:
                        data = await resp.json()
                        traders = data.get("traders", [])
                        cursor = data.get("pagination", {}).get("nextCursor")
                        print(f"  Page {p} returned {len(traders)} traders.")
                        for t in traders:
                            w = t.get("wallet")
                            if not w:
                                continue
                            ident = t.get("identity") or {}
                            name = ident.get("name") or ident.get("twitter") or f"ST-Top-{w[:4]}"
                            pnl_val = t.get("period", {}).get("realized", 0.0)
                            wr_val = t.get("winRate", 0.0)
                            if w not in all_traders:
                                all_traders[w] = {
                                    "address": w,
                                    "name": name,
                                    "handle": ident.get("twitter", f"st_{w[:4]}_{w[-4:]}").replace("@", ""),
                                    "source": "SOLANATRACKER_TOP_30D",
                                    "win_rate": wr_val,
                                    "pnl": pnl_val,
                                    "trades": t.get("counts", {}).get("trades", 50),
                                    "identity": ident
                                }
                            else:
                                if wr_val > all_traders[w]["win_rate"]:
                                    all_traders[w]["win_rate"] = wr_val
                                if pnl_val > all_traders[w]["pnl"]:
                                    all_traders[w]["pnl"] = pnl_val
                await asyncio.sleep(0.5)
            except Exception as e:
                print(f"Error fetching PnL V2 page {p}:", e)

        # 3. Top Traders Classic All
        try:
            print("Fetching Classic Top-Traders All...")
            async with session.get("https://data.solanatracker.io/top-traders/all", headers=headers) as resp:
                if resp.status == 200:
                    data = await resp.json()
                    for t in data.get("wallets", []):
                        w = t.get("wallet")
                        if not w:
                            continue
                        s = t.get("summary", {})
                        if w not in all_traders:
                            all_traders[w] = {
                                "address": w,
                                "name": f"ST-Classic-{w[:4]}",
                                "handle": f"st_{w[:4]}_{w[-4:]}",
                                "source": "SOLANATRACKER_CLASSIC",
                                "win_rate": s.get("winPercentage", 0.0),
                                "pnl": s.get("realized", 0.0),
                                "trades": s.get("totalWins", 0) + s.get("totalLosses", 0),
                                "identity": None
                            }
        except Exception as e:
            print("Error classic:", e)

        print(f"\nTotal Unique Candidate Wallets Harvested: {len(all_traders)}")

        # 4. On-Chain SOL Balance Verification via Solana Tracker Free RPC
        print("Verifying on-chain balances via Solana Tracker RPC...")
        sem = asyncio.Semaphore(15)

        async def check_bal(addr, item):
            async with sem:
                payload = {"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [addr]}
                for _ in range(3):
                    try:
                        async with session.post(RPC_URL, json=payload, headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}) as r:
                            res = await r.json()
                            bal = res.get("result", {}).get("value", 0) / 1e9
                            item["sol_balance"] = round(bal, 2)
                            return
                    except Exception:
                        await asyncio.sleep(0.2)
                item["sol_balance"] = 0.0

        tasks = [check_bal(addr, item) for addr, item in all_traders.items()]
        await asyncio.gather(*tasks)

    # 5. Apply Strict Filtering
    print("\nFiltering for 100 Quality Alpha Traders...")
    qualified = []
    disqualified = []

    for addr, item in all_traders.items():
        bal = item.get("sol_balance", 0.0)
        wr = item.get("win_rate", 0.0)
        ident = item.get("identity") or {}
        itype = ident.get("type", "") if isinstance(ident, dict) else ""

        # Reject CEX
        if itype in ("cex", "exchange", "market_maker", "pool"):
            disqualified.append((addr, f"CEX_TYPE_{itype}"))
            continue
        if bal > 50000.0:
            disqualified.append((addr, f"CEX_BALANCE_{bal}"))
            continue
        if bal < 0.5:
            disqualified.append((addr, f"LOW_BALANCE_{bal}"))
            continue
        if wr < 50.0:
            disqualified.append((addr, f"LOW_WINRATE_{wr}"))
            continue

        qualified.append(item)

    print(f"Qualified Alpha Traders Count: {len(qualified)}")
    # Sort by Balance & Win Rate composite score
    qualified.sort(key=lambda x: (x["win_rate"] * 1000.0 + min(x["sol_balance"], 500.0) * 10.0), reverse=True)

    final_100 = qualified[:100]
    print(f"Final Selection: {len(final_100)} Alpha Traders.")

    # 6. Save to JSON report
    with open("/home/node/Developments/zenithsw/data/solanatracker_top100_final.json", "w") as f:
        json.dump(final_100, f, indent=2)

    # 7. Ingest / Upsert into SQLite Database (smart_wallets.db)
    conn = sqlite3.connect(DB_PATH)
    cur = conn.cursor()
    now_iso = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

    ingested_s = 0
    ingested_a = 0

    for idx, t in enumerate(final_100):
        rank = idx + 1
        tier = "TIER-S" if idx < 25 else "TIER-A"
        if tier == "TIER-S":
            ingested_s += 1
        else:
            ingested_a += 1

        rep_score = min(98.0, 80.0 + (t["win_rate"] - 50.0) * 0.4 + min(t["sol_balance"], 100.0) * 0.1)

        cur.execute("""
            INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, reputation_score, is_active, last_seen_at, updated_at)
            VALUES (?, ?, ?, 'SOLANA', ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
            ON CONFLICT(handle) DO UPDATE SET
                rank = excluded.rank,
                name = excluded.name,
                tier = excluded.tier,
                address = excluded.address,
                pnl_24h_usd = excluded.pnl_24h_usd,
                win_rate_pct = excluded.win_rate_pct,
                total_trades = excluded.total_trades,
                source = excluded.source,
                reputation_score = excluded.reputation_score,
                is_active = 1,
                updated_at = excluded.updated_at
        """, (
            rank,
            t["name"][:30],
            t["handle"][:30],
            t["address"],
            tier,
            round(t["sol_balance"] * 102.0, 2),
            round(t["win_rate"], 1),
            max(int(t["trades"]), 1),
            t["source"],
            round(rep_score, 1),
            now_iso,
            now_iso
        ))

    conn.commit()

    # Final tally in DB
    cur.execute("SELECT tier, count(*) FROM wallets WHERE chain='SOLANA' GROUP BY tier")
    counts = cur.fetchall()
    conn.close()

    print(f"\n[DATABASE UPDATED ✅] Ingested {len(final_100)} Alpha Traders into smart_wallets.db:")
    print(f"Tier Breakdown in DB: {counts}")

if __name__ == "__main__":
    asyncio.run(harvest_100())
