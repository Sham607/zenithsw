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

async def harvest_exact_100():
    print("=== EXPANDING HARVEST TO SECURE 100 TOP QUALIFIED ALPHA TRADERS ===")
    all_traders = {}

    timeout = aiohttp.ClientTimeout(total=20)
    async with aiohttp.ClientSession(timeout=timeout) as session:
        # 1. KOL Leaderboard Pages 1 to 5 (250 Top KOLs)
        for page_num in range(1, 6):
            url = f"https://data.solanatracker.io/v2/pnl/leaderboard/kols?limit=50&page={page_num}&sort=total&direction=desc"
            try:
                async with session.get(url, headers=headers) as resp:
                    if resp.status == 200:
                        data = await resp.json()
                        traders = data.get("traders", [])
                        print(f"KOL Page {page_num}: {len(traders)} traders")
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
                await asyncio.sleep(0.3)
            except Exception as e:
                print(f"KOL Page {page_num} error: {e}")

        # 2. PnL V2 Top Traders (pages 1 to 8, 400 traders)
        cursor = None
        for p in range(1, 9):
            if cursor:
                url = f"https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted&cursor={urllib.parse.quote(cursor)}"
            else:
                url = "https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted"
            try:
                async with session.get(url, headers=headers) as resp:
                    if resp.status == 200:
                        data = await resp.json()
                        traders = data.get("traders", [])
                        cursor = data.get("pagination", {}).get("nextCursor")
                        print(f"Top 30D Page {p}: {len(traders)} traders")
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
                await asyncio.sleep(0.3)
                if not cursor:
                    break
            except Exception as e:
                print(f"Top 30D Page {p} error: {e}")

        print(f"\nTotal Unique Candidates Collected: {len(all_traders)}")

        # 3. Fast Concurrent Verification via Solana Tracker Free RPC
        print("Verifying on-chain balances on Solana Tracker RPC...")
        sem = asyncio.Semaphore(20)

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
                        await asyncio.sleep(0.15)
                item["sol_balance"] = 0.0

        tasks = [check_bal(addr, item) for addr, item in all_traders.items()]
        await asyncio.gather(*tasks)

    # 4. Filter Quality Alpha Traders
    print("\nFiltering Quality Alpha Traders (Anti-CEX & Positive WinRate)...")
    qualified = []
    disqualified = []

    for addr, item in all_traders.items():
        bal = item.get("sol_balance", 0.0)
        wr = item.get("win_rate", 0.0)
        ident = item.get("identity") or {}
        itype = ident.get("type", "") if isinstance(ident, dict) else ""

        # Reject CEX
        if itype in ("cex", "exchange", "market_maker", "pool"):
            disqualified.append((addr, f"CEX_{itype}"))
            continue
        if bal > 50000.0:
            disqualified.append((addr, f"CEX_BALANCE_{bal}"))
            continue
        # Allow traders who have >= 0.2 SOL active balance
        if bal < 0.2:
            disqualified.append((addr, f"LOW_BALANCE_{bal}"))
            continue
        # Quality Win Rate >= 48.0%
        if wr < 48.0:
            disqualified.append((addr, f"LOW_WINRATE_{wr}"))
            continue

        qualified.append(item)

    print(f"Total Qualified Alpha Traders: {len(qualified)}")

    # Sort by quality score (Win Rate primary, balance secondary)
    qualified.sort(key=lambda x: (x["win_rate"] * 100.0 + min(x["sol_balance"], 500.0)), reverse=True)

    # Pick exactly Top 100
    final_100 = qualified[:100]
    print(f"Selected Top 100 Alpha Traders (Count: {len(final_100)})")

    # 5. Ingest into smart_wallets.db
    conn = sqlite3.connect(DB_PATH)
    cur = conn.cursor()
    now_iso = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())

    ingested_s = 0
    ingested_a = 0

    for idx, t in enumerate(final_100):
        rank = idx + 1
        # Top 30 with highest Win Rate & capital -> TIER-S, rest -> TIER-A
        tier = "TIER-S" if idx < 30 and t["sol_balance"] >= 2.0 else "TIER-A"
        if tier == "TIER-S":
            ingested_s += 1
        else:
            ingested_a += 1

        rep_score = min(98.0, 80.0 + (t["win_rate"] - 48.0) * 0.35 + min(t["sol_balance"], 100.0) * 0.1)

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

    cur.execute("SELECT tier, count(*) FROM wallets WHERE chain='SOLANA' GROUP BY tier")
    db_counts = cur.fetchall()
    conn.close()

    # Save final 100 report
    with open("/home/node/Developments/zenithsw/data/solanatracker_100_alpha_final.json", "w") as f:
        json.dump(final_100, f, indent=2)

    print(f"\n[SUKSES 🚀] Database smart_wallets.db kini memegang:")
    print(f"Tier Breakdown Solana: {db_counts}")
    print(f"Top 100 Alpha Report tersimpan di data/solanatracker_100_alpha_final.json")

if __name__ == "__main__":
    asyncio.run(harvest_exact_100())
