import asyncio
import aiohttp
import sqlite3
import json
import time
import os
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
OUTPUT_JSON = "/home/node/Developments/zenithsw/data/dual_track_onchain_audit.json"
DATA_API_KEY = "fc7e6533-da19-48bb-b80f-565a104d341f"
RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"

# Load active Solana wallets
conn = sqlite3.connect(DB_PATH)
conn.row_factory = sqlite3.Row
cur = conn.cursor()
cur.execute("SELECT id, rank, name, handle, address, tier, win_rate_pct, pnl_24h_usd, total_trades, source, execution_track FROM wallets WHERE chain='SOLANA' AND is_active=1")
wallets = [dict(r) for r in cur.fetchall()]
conn.close()

print(f"[{datetime.now().strftime('%H:%M:%S')}] Loaded {len(wallets)} active Solana wallets for forensic on-chain investigation.")

async def fetch_balance(session, address):
    payload = {"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [address]}
    try:
        async with session.post(RPC_URL, json=payload, timeout=aiohttp.ClientTimeout(total=6)) as resp:
            if resp.status == 200:
                data = await resp.json()
                return data.get("result", {}).get("value", 0) / 1e9
    except Exception:
        pass
    return 0.0

async def fetch_pnl_data(session, address, semaphore):
    url = f"https://data.solanatracker.io/v2/pnl/wallets/{address}"
    headers = {"x-api-key": DATA_API_KEY, "User-Agent": "Mozilla/5.0"}
    async with semaphore:
        for attempt in range(3):
            try:
                async with session.get(url, headers=headers, timeout=aiohttp.ClientTimeout(total=10)) as resp:
                    if resp.status == 429:
                        await asyncio.sleep(1.0 + attempt * 0.5)
                        continue
                    if resp.status == 200:
                        return await resp.json()
                    elif resp.status == 404:
                        return None
                    else:
                        await asyncio.sleep(0.3)
            except Exception:
                await asyncio.sleep(0.5)
        return None

async def main():
    semaphore = asyncio.Semaphore(5) # max 5 concurrent requests to respect rate limits
    connector = aiohttp.TCPConnector(limit=10)
    async with aiohttp.ClientSession(connector=connector) as session:
        tasks = []
        for w in wallets:
            tasks.append((w, fetch_balance(session, w["address"]), fetch_pnl_data(session, w["address"], semaphore)))

        results = []
        for idx, (w, bal_coro, pnl_coro) in enumerate(tasks):
            bal, pnl = await asyncio.gather(bal_coro, pnl_coro)

            addr = w["address"]
            handle = w["handle"]
            current_tier = w["tier"]
            current_track = w["execution_track"]

            record = {
                "id": w["id"],
                "handle": handle,
                "name": w["name"],
                "address": addr,
                "current_tier": current_tier,
                "current_track": current_track,
                "source": w["source"],
                "sol_balance": round(bal, 4),
                "has_pnl_data": False,
                "is_arbitrage": False,
                "win_rate": round(w["win_rate_pct"], 2),
                "total_trades": w["total_trades"],
                "realized_pnl": 0.0,
                "unrealized_pnl": 0.0,
                "total_pnl": 0.0,
                "avg_hold_secs": 0,
                "avg_hold_mins": 0.0,
                "avg_buy_usd": 0.0,
                "classification": "UNVERIFIED",
                "recommended_track": current_track,
                "recommended_tier": current_tier,
                "reason": ""
            }

            if pnl and "summary" in pnl:
                record["has_pnl_data"] = True
                summary = pnl.get("summary", {})
                analysis = pnl.get("analysis", {})
                tags = pnl.get("tags", {})

                is_arb = tags.get("isArbitrage", False) if isinstance(tags, dict) else False
                record["is_arbitrage"] = is_arb

                win_rate = analysis.get("winRate", w["win_rate_pct"])
                record["win_rate"] = round(win_rate, 2)

                timing = summary.get("timing", {})
                avg_hold_secs = timing.get("avgHoldTimeSecs", 0)
                record["avg_hold_secs"] = avg_hold_secs
                record["avg_hold_mins"] = round(avg_hold_secs / 60, 1)

                pnl_obj = summary.get("pnl", {})
                record["realized_pnl"] = round(pnl_obj.get("realized", 0.0), 2)
                record["unrealized_pnl"] = round(pnl_obj.get("unrealized", 0.0), 2)
                record["total_pnl"] = round(pnl_obj.get("total", 0.0), 2)

                counts = summary.get("counts", {})
                record["total_trades"] = counts.get("trades", w["total_trades"])

                avg_buy = summary.get("averages", {}).get("buy", 0.0)
                record["avg_buy_usd"] = round(avg_buy, 2)

                # Evaluation logic
                # 1. Arbitrage/Bot disqualification
                if is_arb or record["total_trades"] > 50000:
                    record["classification"] = "DISQUALIFIED_MEV_BOT"
                    record["recommended_track"] = "DISQUALIFIED"
                    record["reason"] = f"Tagged Arbitrage ({is_arb}) or hyper-frequency trades ({record['total_trades']})"
                # 2. Dead wallet check
                elif bal < 0.05 and record["total_pnl"] <= 0:
                    record["classification"] = "DEAD_WALLET"
                    record["recommended_track"] = "DISQUALIFIED"
                    record["reason"] = f"Zero/dust SOL balance ({bal:.3f} SOL) with non-positive PnL"
                # 3. Vault Lane check (Whale Conviction)
                # Criteria: avg_hold_secs >= 14400 (4h+), win_rate >= 55%, total_pnl > $25k or sol_balance >= 5 SOL
                elif avg_hold_secs >= 14400 and record["win_rate"] >= 55.0 and (record["total_pnl"] >= 20000 or bal >= 5.0):
                    record["classification"] = "QUALIFIED_VAULT"
                    record["recommended_track"] = "VAULT_LANE"
                    record["recommended_tier"] = "TIER-S" if record["total_pnl"] >= 100000 else "TIER-A"
                    record["reason"] = f"Conviction swing hold: {record['avg_hold_mins']}m avg hold, win_rate {record['win_rate']}%, PnL ${record['total_pnl']:,.0f}"
                # 4. Fast Lane check (Scalp / Quick Momentum)
                # Criteria: avg_hold_secs < 14400 (under 4h), win_rate >= 52%, total_pnl > 0 or bal >= 1.0 SOL
                elif avg_hold_secs < 14400 and record["win_rate"] >= 52.0 and (record["total_pnl"] >= 5000 or bal >= 1.0):
                    record["classification"] = "QUALIFIED_FAST"
                    record["recommended_track"] = "FAST_LANE"
                    record["recommended_tier"] = "TIER-S" if record["total_pnl"] >= 100000 else ("TIER-A" if record["total_pnl"] >= 25000 else "TIER-B")
                    record["reason"] = f"Fast scalp/momentum: {record['avg_hold_mins']}m avg hold, win_rate {record['win_rate']}%, PnL ${record['total_pnl']:,.0f}"
                # 5. Low win rate or losing wallet
                elif record["win_rate"] < 48.0 or record["total_pnl"] < -10000:
                    record["classification"] = "DISQUALIFIED_POOR_PERFORMANCE"
                    record["recommended_track"] = "DISQUALIFIED"
                    record["reason"] = f"Poor performance: win_rate {record['win_rate']}%, total_pnl ${record['total_pnl']:,.0f}"
                else:
                    # Marginal / Needs observation
                    record["classification"] = "MARGINAL_MONITOR"
                    record["recommended_track"] = "FAST_LANE"
                    record["recommended_tier"] = "TIER-B"
                    record["reason"] = f"Marginal metrics: win_rate {record['win_rate']}%, hold {record['avg_hold_mins']}m, bal {bal:.2f} SOL"
            else:
                # No PnL data returned by API: fallback to balance and existing DB stats
                if bal < 0.05:
                    record["classification"] = "DEAD_WALLET"
                    record["recommended_track"] = "DISQUALIFIED"
                    record["reason"] = f"No on-chain PnL data & dead balance ({bal:.3f} SOL)"
                elif w["win_rate_pct"] >= 65.0 and bal >= 5.0:
                    record["classification"] = "ORGANIC_WHALE_FALLBACK"
                    record["recommended_track"] = "VAULT_LANE"
                    record["reason"] = f"Fallback organic whale: bal {bal:.2f} SOL, DB win rate {w['win_rate_pct']}%"
                else:
                    record["classification"] = "FAST_LANE_FALLBACK"
                    record["recommended_track"] = "FAST_LANE"
                    record["reason"] = f"Fallback active wallet: bal {bal:.2f} SOL"

            results.append(record)
            if (idx + 1) % 50 == 0 or (idx + 1) == len(wallets):
                print(f"[{datetime.now().strftime('%H:%M:%S')}] Audited {idx + 1}/{len(wallets)} wallets...")

            # Small delay between batches
            if idx % 5 == 0:
                await asyncio.sleep(0.1)

    # Summary
    vault_count = sum(1 for r in results if r["recommended_track"] == "VAULT_LANE")
    fast_count = sum(1 for r in results if r["recommended_track"] == "FAST_LANE")
    disqualified_count = sum(1 for r in results if r["recommended_track"] == "DISQUALIFIED")

    output_payload = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total_audited": len(results),
        "summary": {
            "vault_lane": vault_count,
            "fast_lane": fast_count,
            "disqualified": disqualified_count
        },
        "details": results
    }

    with open(OUTPUT_JSON, "w") as f:
        json.dump(output_payload, f, indent=2)

    print(f"\nAudit complete! Saved to {OUTPUT_JSON}")
    print(f"Summary: VAULT_LANE = {vault_count}, FAST_LANE = {fast_count}, DISQUALIFIED = {disqualified_count}")

if __name__ == "__main__":
    asyncio.run(main())
