import asyncio
import aiohttp
import sqlite3
import json
import time
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
OUTPUT_JSON = "/home/node/Developments/zenithsw/data/robinhood_onchain_audit_verified.json"
BLOCKSCOUT_API_KEY = "proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
BASE_URL = "https://robinhoodchain.blockscout.com/api/v2"

conn = sqlite3.connect(DB_PATH)
conn.row_factory = sqlite3.Row
cur = conn.cursor()
cur.execute("SELECT id, rank, name, handle, address, tier, win_rate_pct, pnl_24h_usd, total_trades, source, execution_track, is_active FROM wallets WHERE chain='ROBINHOOD'")
wallets = [dict(r) for r in cur.fetchall()]
conn.close()

print(f"[{datetime.now().strftime('%H:%M:%S')}] Total Robinhood wallets to audit: {len(wallets)}")

async def fetch_counters(session, wallet, semaphore):
    addr = wallet["address"]
    url = f"{BASE_URL}/addresses/{addr}/counters"
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Authorization": f"Bearer {BLOCKSCOUT_API_KEY}"
    }
    async with semaphore:
        for attempt in range(3):
            try:
                async with session.get(url, headers=headers, timeout=aiohttp.ClientTimeout(total=8)) as resp:
                    if resp.status == 200:
                        text = await resp.text()
                        data = json.loads(text)
                        return (wallet, data, None)
                    elif resp.status in (404, 422):
                        return (wallet, {"transactions_count": "0", "token_transfers_count": "0"}, "invalid_or_not_found")
                    elif resp.status == 429:
                        await asyncio.sleep(0.5 * (attempt + 1))
            except Exception as e:
                await asyncio.sleep(0.3 * (attempt + 1))
        return (wallet, {"transactions_count": "0", "token_transfers_count": "0"}, "error_timeout")

async def main():
    semaphore = asyncio.Semaphore(12) # conservative 12 concurrency
    connector = aiohttp.TCPConnector(limit=20)
    async with aiohttp.ClientSession(connector=connector) as session:
        batch_size = 50
        results = []
        for i in range(0, len(wallets), batch_size):
            batch = wallets[i:i+batch_size]
            tasks = [fetch_counters(session, w, semaphore) for w in batch]
            responses = await asyncio.gather(*tasks)

            for w, counters, err in responses:
                wid = w["id"]
                handle = w["handle"]
                addr = w["address"]
                source = w["source"]
                current_tier = w["tier"]
                current_pnl = w["pnl_24h_usd"]
                current_wr = w["win_rate_pct"]
                reported_trades = w["total_trades"]

                tx_count = int(counters.get("transactions_count", 0))
                tt_count = int(counters.get("token_transfers_count", 0))

                # Evaluation
                if len(addr) != 42 or err == "invalid_or_not_found" and len(addr) != 42:
                    rec_track = "DISQUALIFIED"
                    classification = "INVALID_ADDRESS"
                    reason = f"Invalid address format or length ({len(addr)})"
                elif tx_count > 50000 or reported_trades > 50000 or tt_count > 100000:
                    rec_track = "DISQUALIFIED"
                    classification = "DISQUALIFIED_HFT_BOT"
                    reason = f"Hyper-frequency bot: {tx_count} on-chain txs, {tt_count} token transfers, {reported_trades} DB trades"
                elif tx_count == 0 and tt_count == 0:
                    rec_track = "DISQUALIFIED"
                    classification = "DEAD_WALLET"
                    reason = "Zero transactions and zero token transfers on Robinhood Chain"
                elif (current_pnl >= 500000 or source in ("FOMO", "FOMO_SWING", "ONCHAIN")) and tt_count >= 10 and tx_count <= 25000 and current_wr >= 65.0:
                    rec_track = "VAULT_LANE"
                    classification = "QUALIFIED_VAULT"
                    reason = f"High conviction swing whale: {tt_count} token transfers, PnL ${current_pnl:,.0f}, WR {current_wr:.1f}%"
                elif tt_count >= 1 or (tx_count >= 1 and tx_count <= 25000):
                    rec_track = "FAST_LANE"
                    classification = "QUALIFIED_FAST"
                    reason = f"Active on-chain mover: {tx_count} txs, {tt_count} token transfers"
                else:
                    rec_track = "DISQUALIFIED"
                    classification = "MARGINAL_INACTIVE"
                    reason = "Insufficient on-chain proof"

                if current_pnl >= 1000000 or (tt_count >= 15 and current_wr >= 80.0):
                    rec_tier = "TIER-S"
                elif current_pnl >= 100000 or tt_count >= 5:
                    rec_tier = "TIER-A"
                else:
                    rec_tier = "TIER-B"

                results.append({
                    "id": wid,
                    "handle": handle,
                    "address": addr,
                    "source": source,
                    "current_tier": current_tier,
                    "recommended_tier": rec_tier,
                    "recommended_track": rec_track,
                    "classification": classification,
                    "onchain_txs": tx_count,
                    "onchain_token_transfers": tt_count,
                    "db_trades": reported_trades,
                    "db_pnl": current_pnl,
                    "db_win_rate": current_wr,
                    "reason": reason
                })

            print(f"[{datetime.now().strftime('%H:%M:%S')}] Audited {min(i+batch_size, len(wallets))}/{len(wallets)}...")
            await asyncio.sleep(0.1)

    vault_count = sum(1 for r in results if r["recommended_track"] == "VAULT_LANE")
    fast_count = sum(1 for r in results if r["recommended_track"] == "FAST_LANE")
    disq_count = sum(1 for r in results if r["recommended_track"] == "DISQUALIFIED")

    with open(OUTPUT_JSON, "w") as f:
        json.dump({
            "timestamp": datetime.now(timezone.utc).isoformat(),
            "total_audited": len(results),
            "summary": {
                "vault_lane": vault_count,
                "fast_lane": fast_count,
                "disqualified": disq_count
            },
            "details": results
        }, f, indent=2)

    print(f"\nAudit complete! Saved to {OUTPUT_JSON}")
    print(f"Summary: VAULT_LANE = {vault_count}, FAST_LANE = {fast_count}, DISQUALIFIED = {disq_count}")

if __name__ == "__main__":
    asyncio.run(main())
