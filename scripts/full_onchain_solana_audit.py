import asyncio
import aiohttp
import sqlite3
import json
import time
import os
from datetime import datetime, timezone

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
OUTPUT_JSON = "/home/node/Developments/zenithsw/data/onchain_solana_audit_results.json"
OUTPUT_MD = "/home/node/Developments/zenithsw/data/solana_audit_report.md"

ENDPOINTS = [
    "https://api.mainnet-beta.solana.com",
    "https://solana-rpc.publicnode.com"
]

class Auditor:
    def __init__(self, wallets):
        self.wallets = wallets
        self.endpoint_idx = 0
        self.lock = asyncio.Lock()
        self.results = {}

    async def get_next_endpoint(self):
        async with self.lock:
            ep = ENDPOINTS[self.endpoint_idx % len(ENDPOINTS)]
            self.endpoint_idx += 1
            return ep

    async def rpc_call(self, session, method, params, retries=3):
        payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
        for attempt in range(retries):
            ep = await self.get_next_endpoint()
            try:
                async with session.post(ep, json=payload, timeout=6) as resp:
                    if resp.status == 429:
                        await asyncio.sleep(0.5 * (attempt + 1))
                        continue
                    if resp.status != 200:
                        await asyncio.sleep(0.3)
                        continue
                    data = await resp.json()
                    if "result" in data:
                        return data["result"]
            except Exception:
                await asyncio.sleep(0.4 * (attempt + 1))
        return None

async def run_audit():
    print(f"[{datetime.now().strftime('%H:%M:%S')}] Connecting to database...")
    conn = sqlite3.connect(DB_PATH)
    cur = conn.cursor()
    cur.execute("""
        SELECT id, handle, name, address, tier, source, win_rate_pct, pnl_24h_usd 
        FROM wallets 
        WHERE chain='SOLANA'
        ORDER BY 
            CASE tier WHEN 'TIER-S' THEN 1 WHEN 'TIER-A' THEN 2 ELSE 3 END,
            rank ASC
    """)
    rows = cur.fetchall()
    conn.close()

    total_wallets = len(rows)
    print(f"[{datetime.now().strftime('%H:%M:%S')}] Found {total_wallets} Solana wallets to audit.")

    wallets = []
    for r in rows:
        wallets.append({
            "id": r[0],
            "handle": r[1],
            "name": r[2],
            "address": r[3],
            "tier": r[4],
            "source": r[5],
            "db_win_rate": r[6],
            "db_pnl": r[7],
        })

    auditor = Auditor(wallets)

    # 1. Stage 1: Balance Check (Parallel with semaphore)
    print(f"[{datetime.now().strftime('%H:%M:%S')}] === STAGE 1: Checking On-Chain SOL Balances (Liveness Gate) ===")
    sem = asyncio.Semaphore(6) # 6 concurrent RPC requests across 2 endpoints = ~3 RPS per endpoint

    async with aiohttp.ClientSession() as session:
        async def check_balance(w, idx):
            async with sem:
                res = await auditor.rpc_call(session, "getBalance", [w["address"]])
                lamports = res.get("value", 0) if isinstance(res, dict) else 0
                sol_bal = lamports / 1e9
                w["sol_balance"] = sol_bal
                await asyncio.sleep(0.12) # ~8 RPS total rate control

                if (idx + 1) % 100 == 0 or (idx + 1) == total_wallets:
                    print(f"  Processed {idx + 1}/{total_wallets} wallets...")

        tasks = [check_balance(w, i) for i, w in enumerate(wallets)]
        await asyncio.gather(*tasks)

    # Filter Stage 1
    alive_wallets = [w for w in wallets if w["sol_balance"] >= 0.05]
    dead_wallets = [w for w in wallets if w["sol_balance"] < 0.05]
    print(f"[{datetime.now().strftime('%H:%M:%S')}] STAGE 1 RESULT: {len(alive_wallets)} ALIVE (>=0.05 SOL), {len(dead_wallets)} DEAD/EMPTY (<0.05 SOL)")

    # 2. Stage 2: Activity & Signature Forensics for Alive Wallets
    print(f"\n[{datetime.now().strftime('%H:%M:%S')}] === STAGE 2: Deep Transaction Forensics on {len(alive_wallets)} Active Wallets ===")
    now_ts = int(time.time())
    fourteen_days_ago = now_ts - (14 * 86400)

    sem2 = asyncio.Semaphore(4)

    async with aiohttp.ClientSession() as session:
        async def check_signatures(w, idx):
            async with sem2:
                sigs = await auditor.rpc_call(session, "getSignaturesForAddress", [w["address"], {"limit": 30}])
                await asyncio.sleep(0.18)

                if not sigs or not isinstance(sigs, list):
                    w["status"] = "NO_SIGNATURES_RETURNED"
                    w["is_qualified"] = False
                    return

                w["recent_tx_count"] = len(sigs)
                failed_txs = sum(1 for s in sigs if s.get("err") is not None)
                w["failed_tx_pct"] = (failed_txs / len(sigs)) * 100.0

                block_times = [s.get("blockTime") for s in sigs if s.get("blockTime")]
                if block_times:
                    w["latest_tx_time"] = block_times[0]
                    days_inactive = (now_ts - block_times[0]) / 86400.0
                    w["days_inactive"] = round(days_inactive, 1)
                else:
                    w["latest_tx_time"] = 0
                    w["days_inactive"] = 999.0

                # Check multi-tx within same second (Sniper bot signature)
                time_counts = {}
                for bt in block_times:
                    time_counts[bt] = time_counts.get(bt, 0) + 1
                max_same_sec = max(time_counts.values()) if time_counts else 0
                w["max_tx_per_second"] = max_same_sec

                # Classify
                if w["days_inactive"] > 14.0:
                    w["classification"] = "ZOMBIE_INACTIVE (>14 days inactive)"
                    w["is_qualified"] = False
                elif max_same_sec >= 3:
                    w["classification"] = "MEV_SNIPER_BOT (Burst >=3 tx/sec)"
                    w["is_qualified"] = False
                elif w["failed_tx_pct"] >= 50.0:
                    w["classification"] = "SPAM_FAIL_BOT (>50% failed txs)"
                    w["is_qualified"] = False
                else:
                    w["classification"] = "ORGANIC_SWING_CANDIDATE"
                    w["is_qualified"] = True

                if (idx + 1) % 25 == 0 or (idx + 1) == len(alive_wallets):
                    print(f"  Analyzed {idx + 1}/{len(alive_wallets)} active wallets...")

        tasks2 = [check_signatures(w, i) for i, w in enumerate(alive_wallets)]
        await asyncio.gather(*tasks2)

    # 3. Compile Report
    candidates = [w for w in alive_wallets if w.get("is_qualified")]
    disqualified_alive = [w for w in alive_wallets if not w.get("is_qualified")]

    print(f"\n[{datetime.now().strftime('%H:%M:%S')}] === AUDIT COMPLETED ===")
    print(f"Total Solana Wallets Evaluated: {total_wallets}")
    print(f"1. Dead/Empty Balances (<0.05 SOL): {len(dead_wallets)}")
    print(f"2. Alive but Disqualified (Zombies/Sniper Bots/Spammers): {len(disqualified_alive)}")
    print(f"3. Qualified Organic Trader Candidates: {len(candidates)}")

    # Sort Candidates by SOL Balance & Activity
    candidates.sort(key=lambda x: x["sol_balance"], reverse=True)

    # Save to JSON
    report_data = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "total_solana_wallets": total_wallets,
        "dead_empty_count": len(dead_wallets),
        "disqualified_alive_count": len(disqualified_alive),
        "qualified_candidates_count": len(candidates),
        "qualified_candidates": candidates,
        "disqualified_alive": disqualified_alive,
        "dead_wallets_sample": dead_wallets[:20]
    }

    with open(OUTPUT_JSON, "w") as f:
        json.dump(report_data, f, indent=2)

    # Generate Markdown Report
    with open(OUTPUT_MD, "w") as f:
        f.write("# Laporan Audit Forensik On-Chain 934 Wallet Solana\n\n")
        f.write(f"- **Waktu Audit:** {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')} UTC\n")
        f.write(f"- **Total Wallet Dievaluasi:** {total_wallets}\n")
        f.write(f"- **Wallet Mati/Kosong (<0.05 SOL):** {len(dead_wallets)} ({len(dead_wallets)/total_wallets*100:.1f}%)\n")
        f.write(f"- **Wallet Terdiskualifikasi (Bot/Zombie):** {len(disqualified_alive)}\n")
        f.write(f"- **Wallet Lolos Kualifikasi Organik:** {len(candidates)}\n\n")

        f.write("## Daftar Wallet yang Lolos Kualifikasi Organik\n\n")
        f.write("| No | Handle / Name | Address | Old Tier | Source | SOL Balance | Inaktif (Hari) | Max TX/Detik | Klasifikasi |\n")
        f.write("|---|---|---|---|---|---|---|---|---|\n")
        for i, c in enumerate(candidates):
            f.write(f"| {i+1} | `{c['handle']}` | `{c['address'][:6]}...{c['address'][-4:]}` | {c['tier']} | {c['source']} | {c['sol_balance']:.2f} SOL | {c['days_inactive']} hr | {c['max_tx_per_second']} | {c['classification']} |\n")

    print(f"[{datetime.now().strftime('%H:%M:%S')}] Results saved to {OUTPUT_JSON} and {OUTPUT_MD}")

if __name__ == "__main__":
    asyncio.run(run_audit())
