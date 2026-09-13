import asyncio
import aiohttp
import sqlite3
import json
import time

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
OUTPUT_JSON = "/home/node/Developments/zenithsw/data/robinhood_fomo_audit.json"
BLOCKSCOUT_API_KEY = "proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
BASE_URL = "https://robinhoodchain.blockscout.com/api/v2"

conn = sqlite3.connect(DB_PATH)
conn.row_factory = sqlite3.Row
cur = conn.cursor()
cur.execute("SELECT id, rank, name, handle, address, tier, win_rate_pct, pnl_24h_usd, total_trades, source, execution_track, is_active FROM wallets WHERE chain='ROBINHOOD' AND source IN ('FOMO', 'FOMO_SWING')")
fomo_wallets = [dict(r) for r in cur.fetchall()]
conn.close()

print(f"Loaded {len(fomo_wallets)} FOMO Robinhood wallets to audit with gentle rate limit...")

async def main():
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Authorization": f"Bearer {BLOCKSCOUT_API_KEY}"
    }
    connector = aiohttp.TCPConnector(limit=5)
    results = []

    async with aiohttp.ClientSession(connector=connector) as session:
        for idx, w in enumerate(fomo_wallets):
            addr = w["address"]
            url = f"{BASE_URL}/addresses/{addr}/counters"

            data = None
            for attempt in range(5):
                try:
                    async with session.get(url, headers=headers, timeout=aiohttp.ClientTimeout(total=6)) as resp:
                        if resp.status == 200:
                            data = await resp.json()
                            break
                        elif resp.status == 429:
                            await asyncio.sleep(1.0 * (attempt + 1))
                            continue
                        elif resp.status in (404, 422):
                            data = {"transactions_count": "0", "token_transfers_count": "0", "invalid": True}
                            break
                except Exception:
                    await asyncio.sleep(0.5)

            tx_c = int(data.get("transactions_count", 0)) if data else 0
            tt_c = int(data.get("token_transfers_count", 0)) if data else 0

            results.append({
                "id": w["id"],
                "handle": w["handle"],
                "address": addr,
                "tx_count": tx_c,
                "token_transfers_count": tt_c,
                "db_trades": w["total_trades"],
                "db_pnl": w["pnl_24h_usd"],
                "db_win_rate": w["win_rate_pct"],
                "source": w["source"]
            })

            if (idx + 1) % 20 == 0 or (idx + 1) == len(fomo_wallets):
                print(f"Audited {idx + 1}/{len(fomo_wallets)} FOMO wallets...")

            await asyncio.sleep(0.2) # 5 req/s max

    with open(OUTPUT_JSON, "w") as f:
        json.dump(results, f, indent=2)
    print(f"Saved FOMO audit to {OUTPUT_JSON}")

if __name__ == "__main__":
    asyncio.run(main())
