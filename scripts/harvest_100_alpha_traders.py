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
import time

DATA_API_KEY = load_env_val("SOLANATRACKER_API_KEY")
RPC_URL = load_env_val("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com")

headers = {
    "x-api-key": DATA_API_KEY,
    "User-Agent": "Mozilla/5.0"
}

async def fetch_data():
    all_raw = {}
    cursor = None
    target_qualified = 100
    page = 1

    print(f"=== Harvesting until ~100 verified Alpha Traders are collected ===")

    timeout_client = aiohttp.ClientTimeout(total=12)
    async with aiohttp.ClientSession(timeout=timeout_client) as session:
        # Classic top-traders
        try:
            async with session.get("https://data.solanatracker.io/top-traders/all", headers=headers) as r:
                d_classic = await r.json()
            for t in d_classic.get("wallets", []):
                w = t.get("wallet")
                if w:
                    s = t.get("summary", {})
                    all_raw[w] = {
                        "address": w,
                        "source": "SOLANATRACKER_TOP_TRADERS",
                        "realized_pnl": s.get("realized", 0.0),
                        "win_rate": s.get("winPercentage", 0.0),
                        "total_trades": s.get("totalWins", 0) + s.get("totalLosses", 0),
                        "tokens_traded": 0,
                        "avg_buy": s.get("averageBuyAmount", 0.0),
                        "identity": None
                    }
            print(f"Classic top-traders fetched {len(d_classic.get('wallets', []))} traders.")
        except Exception as e:
            print("Error fetching classic top-traders:", e)

        # PnL V2 pages
        while len(all_raw) < 300 and page <= 6:
            if cursor:
                url = f"https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted&cursor={urllib.parse.quote(cursor)}"
            else:
                url = "https://data.solanatracker.io/v2/pnl/leaderboard/top?limit=50&days=30&pnlMode=adjusted"
            
            print(f"Fetching Page {page} from PnL V2...")
            try:
                async with session.get(url, headers=headers) as resp:
                    data = await resp.json()
                traders = data.get("traders", [])
                cursor = data.get("pagination", {}).get("nextCursor")
                print(f"  Page {page} returned {len(traders)} traders. (Cursor available: {bool(cursor)})")

                for t in traders:
                    w = t.get("wallet")
                    if not w:
                        continue
                    all_raw[w] = {
                        "address": w,
                        "source": "SOLANATRACKER_PNL_V2",
                        "realized_pnl": t.get("period", {}).get("realized", 0.0),
                        "win_rate": t.get("winRate", 0.0),
                        "total_trades": t.get("counts", {}).get("trades", 0),
                        "tokens_traded": t.get("tokens", {}).get("total", 0),
                        "avg_buy": t.get("averages", {}).get("buy", 0.0),
                        "identity": t.get("identity")
                    }

                page += 1
                await asyncio.sleep(0.4)
                if not cursor:
                    break
            except Exception as e:
                print(f"Error on page {page}:", e)
                break

        print(f"\nTotal Candidate Wallets: {len(all_raw)} (API credits spent: {page}).")

        # 2. Fast Concurrent RPC Verification
        print("Verifying on-chain balances with concurrent RPC calls...")
        sem = asyncio.Semaphore(15) # 15 concurrent RPC calls to SolanaTracker RPC

        async def check_balance(addr, item):
            async with sem:
                payload = {"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [addr]}
                for attempt in range(3):
                    try:
                        async with session.post(RPC_URL, json=payload, headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}) as r:
                            res = await r.json()
                            bal = res.get("result", {}).get("value", 0) / 1e9
                            item["sol_balance"] = round(bal, 2)
                            return
                    except Exception:
                        await asyncio.sleep(0.2)
                item["sol_balance"] = 0.0

        tasks = [check_balance(addr, item) for addr, item in all_raw.items()]
        await asyncio.gather(*tasks)

    # 3. Filter
    print("\nApplying Anti-CEX & Quality Filters...")
    verified_alpha = []
    disqualified = []

    for addr, data in all_raw.items():
        bal = data.get("sol_balance", 0.0)

        # Anti-CEX Identity Check
        ident = data.get("identity") or {}
        ident_type = ident.get("type", "") if isinstance(ident, dict) else ""
        if ident_type in ("cex", "exchange", "market_maker", "pool"):
            disqualified.append((addr, f"IDENTIFIED_AS_{ident_type.upper()}"))
            continue

        # Reject CEX hot wallets (huge pool)
        if bal > 50000.0:
            disqualified.append((addr, f"CEX_EXCESSIVE_LIQUIDITY ({bal:,.0f} SOL)"))
            continue

        # Reject empty wallets
        if bal < 1.0:
            disqualified.append((addr, f"LOW_BALANCE ({bal:.2f} SOL)"))
            continue

        # Reject low win rate
        if data["win_rate"] < 50.0:
            disqualified.append((addr, f"LOW_WIN_RATE ({data['win_rate']:.1f}%)"))
            continue

        # Reject abnormal buy sizes
        if data["avg_buy"] > 25000.0 or (data["avg_buy"] > 0 and data["avg_buy"] < 10.0):
            disqualified.append((addr, f"ABNORMAL_BUY_SIZE (${data['avg_buy']:,.0f})"))
            continue

        verified_alpha.append(data)

    print(f"\n=== VERIFICATION COMPLETE ===")
    print(f"Total Evaluated: {len(all_raw)}")
    print(f"Disqualified: {len(disqualified)}")
    print(f"Total Qualified Alpha Traders: {len(verified_alpha)}")

    # Sort by Realized PnL descending
    verified_alpha.sort(key=lambda x: x["realized_pnl"], reverse=True)

    # Take Top 100 (or all qualified)
    final_selected = verified_alpha[:100]
    print(f"Final Selected Alpha Traders: {len(final_selected)}")

    # Save to JSON
    with open("/home/node/Developments/zenithsw/data/solanatracker_top100_alpha.json", "w") as f:
        json.dump({
            "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "total_harvested": len(all_raw),
            "total_qualified": len(verified_alpha),
            "selected_count": len(final_selected),
            "selected_traders": final_selected,
            "disqualified_sample": disqualified[:30]
        }, f, indent=2)

    print("Saved to data/solanatracker_top100_alpha.json")

if __name__ == "__main__":
    asyncio.run(fetch_data())
