#!/usr/bin/env python3
"""
ZenithSW - Dune Analytics Smart Money Harvester
Fetches high-conviction smart wallets (Pump.fun, Solana DEX, EVM) from curated Dune queries.
Usage:
  python3 scripts/dune_smart_money_harvester.py --dry-run
  python3 scripts/dune_smart_money_harvester.py --execute
"""

import os
import sys
import json
import sqlite3
import argparse
import urllib.request
import re
from datetime import datetime

DUNE_API_KEY = os.environ.get("DUNE_API_KEY")
DB_PATH = os.path.join(os.path.dirname(__file__), "..", "data", "smart_wallets.db")

# Curated High-Value Query IDs on Dune
# 6454114: Top 10k Pump.fun Traders Overall Profitability & Win Rate Analysis
PUMPFUN_QUERY_ID = 6454114

def clean_base58(raw: str) -> str:
    m = re.search(r'[1-9A-HJ-NP-Za-km-z]{32,44}', str(raw))
    return m.group(0) if m else ""

def clean_evm_addr(raw: str) -> str:
    m = re.search(r'0x[a-fA-F0-9]{40}', str(raw))
    return m.group(0).lower() if m else ""

def fetch_dune_query(query_id: int, api_key: str):
    url = f"https://api.dune.com/api/v1/query/{query_id}/results"
    req = urllib.request.Request(url, headers={
        "x-dune-api-key": api_key,
        "User-Agent": "ZenithSW-Harvester/1.0"
    })
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode('utf-8'))
    return data.get("result", {}).get("rows", [])

def harvest_pumpfun_smart_money(api_key: str):
    print(f"[*] Fetching Dune Query {PUMPFUN_QUERY_ID} (Pump.fun Smart Money)...")
    rows = fetch_dune_query(PUMPFUN_QUERY_ID, api_key)
    print(f"[*] Received {len(rows)} raw entries from Dune.")
    
    candidates = []
    for r in rows:
        raw_w = r.get("wallet", "")
        addr = clean_base58(raw_w)
        if not addr or len(addr) < 32:
            continue
        try:
            wr_raw = float(r.get("WinRate", 0))
            wr = wr_raw * 100.0 if wr_raw <= 1.0 else wr_raw
            profit = float(r.get("realized_profit", 0))
            buy_swaps = int(r.get("total_buy_swaps", 0))
            sell_swaps = int(r.get("total_sell_swaps", 0))
            total_swaps = buy_swaps + sell_swaps
            tokens_10x = int(r.get("tokens_10x_or_higher", 0))
            tokens_100x = int(r.get("tokens_100x_or_higher", 0))
            rank = int(r.get("rank", 999999))
            
            # Quality Gate:
            # - Win Rate >= 55.0%
            # - Realized Profit >= $10,000
            # - Total Swaps between 20 and 300,000 (exclude non-human pure arb bots)
            # - Must have caught at least 3 tokens with >=10x gains
            if wr >= 55.0 and profit >= 10000.0 and 20 <= total_swaps <= 300000 and tokens_10x >= 3:
                # Tiering logic
                if profit >= 500000.0 and wr >= 65.0 and tokens_100x >= 10:
                    tier = "TIER-S"
                    rep = 98.0
                elif profit >= 100000.0 and wr >= 60.0:
                    tier = "TIER-A"
                    rep = 88.0
                else:
                    tier = "TIER-B"
                    rep = 78.0
                    
                handle = f"dune_pf_{addr[:4]}_{addr[-4:]}".lower()
                name = f"Dune PumpFun #{rank} (${profit:,.0f} PnL)"
                
                candidates.append({
                    "rank": rank,
                    "name": name,
                    "handle": handle,
                    "chain": "SOLANA",
                    "address": addr,
                    "tier": tier,
                    "pnl_24h_usd": profit,
                    "win_rate_pct": wr,
                    "reputation_score": rep,
                    "source": "DUNE_PUMPFUN",
                    "tokens_10x": tokens_10x,
                    "tokens_100x": tokens_100x,
                    "total_swaps": total_swaps
                })
        except Exception:
            continue
            
    # Sort by quality score (100x count, 10x count, profit)
    candidates.sort(key=lambda x: (x["tokens_100x"], x["tokens_10x"], x["pnl_24h_usd"]), reverse=True)
    return candidates

def main():
    parser = argparse.ArgumentParser(description="ZenithSW Dune Smart Money Harvester")
    parser.add_argument("--execute", action="store_true", help="Insert harvested wallets into SQLite (default: dry-run)")
    parser.add_argument("--limit", type=int, default=100, help="Max wallets to ingest (default: 100)")
    args = parser.parse_args()

    api_key = DUNE_API_KEY
    if not api_key:
        print("[ERROR] DUNE_API_KEY environment variable is not set.")
        sys.exit(1)

    candidates = harvest_pumpfun_smart_money(api_key)
    print(f"\n[+] Total candidates passing strict quality filters: {len(candidates)}")
    
    # Check against existing DB
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    cursor.execute("SELECT lower(address), chain FROM wallets")
    existing_wallets = {r[0].lower() for r in cursor.fetchall()}
    
    new_candidates = [c for c in candidates if c["address"].lower() not in existing_wallets]
    print(f"[+] New unique candidates not in DB: {len(new_candidates)}")
    
    selected = new_candidates[:args.limit]
    print(f"[+] Selected top {len(selected)} wallets for processing.")
    
    print("\n" + "="*80)
    print(f"{'Rank':<8} {'Handle':<22} {'Tier':<8} {'WinRate':<9} {'PnL (Est)':<16} {'10x/100x':<10} {'Address'}")
    print("="*80)
    for c in selected[:15]:
        print(f"#{c['rank']:<7} @{c['handle']:<20} {c['tier']:<8} {c['win_rate_pct']:>5.1f}%   ${c['pnl_24h_usd']:>12,.2f}   {c['tokens_10x']}/{c['tokens_100x']:<6} {c['address']}")
    if len(selected) > 15:
        print(f"... and {len(selected)-15} more wallets.")
    print("="*80)
    
    if args.execute:
        print("\n[*] Ingesting into SQLite database...")
        inserted = 0
        now_ts = datetime.utcnow().isoformat()
        for c in selected:
            cursor.execute("""
                INSERT OR IGNORE INTO wallets (
                    rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct,
                    total_trades, source, reputation_score, is_active, last_seen_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
            """, (
                c["rank"], c["name"], c["handle"], c["chain"], c["address"], c["tier"],
                c["pnl_24h_usd"], c["win_rate_pct"], c["total_swaps"], c["source"],
                c["reputation_score"], now_ts, now_ts
            ))
            if cursor.rowcount > 0:
                inserted += 1
        conn.commit()
        print(f"[SUCCESS] Ingested {inserted} new smart wallets from Dune into SQLite database.")
    else:
        print("\n[DRY-RUN] No changes committed to database. Run with --execute to commit.")
    
    conn.close()

if __name__ == "__main__":
    main()
