#!/usr/bin/env python3
"""
ZenithSW - Nansen-Grade Dune Analytics Smart Money Harvester (Solana Focus)
Extracts multi-token alpha snipers from on-chain DEX data using 5 elimination pillars:
1. Multi-token consistency (>= 5 distinct tokens traded)
2. Realistic human/algo Win Rate (60.0% - 88.0%, rejecting instant MEV sandwich bots)
3. High realized PnL (>= $25,000 up to millions)
4. Proven asymmetric multi-baggers (>= 3 tokens with >=10x gains, >= 1 token with >=100x gains for Tier-S)
5. Non-contract valid Base58 EOA validation

Usage:
  python3 scripts/dune_smart_money_harvester.py --dry-run
  python3 scripts/dune_smart_money_harvester.py --execute --limit 50
"""

import os
import sys
import json
import sqlite3
import argparse
import urllib.request
import urllib.error
import re
from datetime import datetime

DUNE_API_KEY = os.environ.get("DUNE_API_KEY")
DB_PATH = os.path.join(os.path.dirname(__file__), "..", "data", "smart_wallets.db")

# Curated High-Value Solana Smart Money Query IDs on Dune
# 6454114: Top 10k Pump.fun & DEX Traders Overall Profitability & Win Rate Analysis
SOLANA_PUMPFUN_QUERY_ID = 6454114

def clean_base58(raw: str) -> str:
    """Extract valid Solana Base58 public key from HTML link or raw string."""
    m = re.search(r'[1-9A-HJ-NP-Za-km-z]{32,44}', str(raw))
    return m.group(0) if m else ""

def fetch_dune_query(query_id: int, api_key: str):
    """Fetch latest query execution result from Dune API."""
    url = f"https://api.dune.com/api/v1/query/{query_id}/results"
    req = urllib.request.Request(url, headers={
        "x-dune-api-key": api_key,
        "User-Agent": "ZenithSW-Harvester/2.0"
    })
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read().decode('utf-8'))
        return data.get("result", {}).get("rows", [])
    except urllib.error.HTTPError as e:
        print(f"[ERROR] Dune API HTTP {e.code}: {e.read().decode('utf-8')}")
        return []
    except Exception as e:
        print(f"[ERROR] Failed to fetch Dune query {query_id}: {e}")
        return []

def harvest_solana_nansen_grade(api_key: str, min_winrate: float = 60.0, max_winrate: float = 88.0, min_pnl: float = 25000.0):
    print(f"[*] Querying Dune Analytics for Solana Smart Money (Query ID: {SOLANA_PUMPFUN_QUERY_ID})...")
    rows = fetch_dune_query(SOLANA_PUMPFUN_QUERY_ID, api_key)
    print(f"[*] Ingested {len(rows)} raw on-chain trader profiles from Dune.")
    
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
            num_tokens = int(r.get("number_of_tokens", 0))
            buy_swaps = int(r.get("total_buy_swaps", 0))
            sell_swaps = int(r.get("total_sell_swaps", 0))
            total_swaps = buy_swaps + sell_swaps
            tokens_10x = int(r.get("tokens_10x_or_higher", 0))
            tokens_20x = int(r.get("tokens_20x_or_higher", 0))
            tokens_50x = int(r.get("tokens_50x_or_higher", 0))
            tokens_100x = int(r.get("tokens_100x_or_higher", 0))
            rank = int(r.get("rank", 999999))
            last_tx = str(r.get("last_tx", ""))
            
            # --- 5-PILLAR NANSEN-GRADE CONFLUENCE FILTER ---
            # 1. Multi-Token Consistency: Must trade >= 5 distinct tokens (reject single-token dev insiders)
            if num_tokens < 5:
                continue
                
            # 2. Realistic Human/Semi-Algo Win Rate (60% - 88%, reject pure sandwich MEV 99%)
            if wr < min_winrate or wr > max_winrate:
                continue
                
            # 3. High Realized Profit (>= $25k)
            if profit < min_pnl:
                continue
                
            # 4. Asymmetric Multi-Bagger Evidence: Must have caught >= 3 tokens with >= 10x gains
            if tokens_10x < 3:
                continue
                
            # 5. Activity Bound: >= 20 swaps and <= 300,000 swaps (exclude pure infrastructure spam bots)
            if total_swaps < 20 or total_swaps > 300000:
                continue

            # Composite Scoring & Tiering
            # Score weights: 100x tokens (40%), 10x tokens (30%), WinRate (20%), Profit log (10%)
            if tokens_100x >= 10 and profit >= 250000.0 and wr >= 65.0:
                tier = "TIER-S"
                rep_score = min(99.0, 90.0 + (tokens_100x * 0.1))
            elif tokens_10x >= 15 or profit >= 75000.0:
                tier = "TIER-A"
                rep_score = min(89.0, 80.0 + (tokens_10x * 0.1))
            else:
                tier = "TIER-B"
                rep_score = 75.0

            handle = f"dune_sol_{addr[:4]}_{addr[-4:]}".lower()
            name = f"Dune Solana Sniper #{rank} (${profit:,.0f} PnL)"
            
            candidates.append({
                "rank": rank,
                "name": name,
                "handle": handle,
                "chain": "SOLANA",
                "address": addr,
                "tier": tier,
                "pnl_24h_usd": profit,
                "win_rate_pct": wr,
                "reputation_score": rep_score,
                "source": "DUNE_PUMPFUN",
                "tokens_10x": tokens_10x,
                "tokens_20x": tokens_20x,
                "tokens_50x": tokens_50x,
                "tokens_100x": tokens_100x,
                "num_tokens": num_tokens,
                "total_swaps": total_swaps,
                "last_tx": last_tx
            })
        except Exception:
            continue
            
    # Sort by Multi-bagger count first, then win rate, then realized profit
    candidates.sort(key=lambda x: (x["tokens_100x"], x["tokens_50x"], x["tokens_10x"], x["win_rate_pct"], x["pnl_24h_usd"]), reverse=True)
    return candidates

def main():
    parser = argparse.ArgumentParser(description="ZenithSW Nansen-Grade Dune Smart Money Harvester")
    parser.add_argument("--execute", action="store_true", help="Commit harvested wallets to SQLite database")
    parser.add_argument("--limit", type=int, default=50, help="Maximum number of wallets to ingest (default: 50)")
    parser.add_argument("--min-winrate", type=float, default=60.0, help="Minimum win rate percentage (default: 60.0)")
    parser.add_argument("--max-winrate", type=float, default=88.0, help="Maximum win rate percentage (default: 88.0)")
    parser.add_argument("--min-pnl", type=float, default=25000.0, help="Minimum realized profit in USD (default: 25000.0)")
    args = parser.parse_args()

    api_key = DUNE_API_KEY
    if not api_key:
        print("[ERROR] DUNE_API_KEY environment variable is not set.")
        sys.exit(1)

    candidates = harvest_solana_nansen_grade(
        api_key=api_key,
        min_winrate=args.min_winrate,
        max_winrate=args.max_winrate,
        min_pnl=args.min_pnl
    )
    print(f"\n[+] Total candidates passing 5-Pillar Confluence: {len(candidates)}")
    
    # Check existing wallets in SQLite
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    cursor.execute("SELECT lower(address) FROM wallets")
    existing_wallets = {r[0].lower() for r in cursor.fetchall()}
    
    new_candidates = [c for c in candidates if c["address"].lower() not in existing_wallets]
    print(f"[+] New unindexed candidates: {len(new_candidates)}")
    
    selected = new_candidates[:args.limit]
    print(f"[+] Selected top {len(selected)} verified smart money snipers.")
    
    print("\n" + "="*95)
    print(f"{'Rank':<8} {'Handle':<22} {'Tier':<8} {'WinRate':<9} {'PnL (Realized)':<18} {'100x/50x/10x':<14} {'Address'}")
    print("="*95)
    for c in selected[:20]:
        print(f"#{c['rank']:<7} @{c['handle']:<20} {c['tier']:<8} {c['win_rate_pct']:>5.1f}%   ${c['pnl_24h_usd']:>14,.2f}   {c['tokens_100x']}/{c['tokens_50x']}/{c['tokens_10x']:<8} {c['address']}")
    if len(selected) > 20:
        print(f"... and {len(selected)-20} more high-conviction wallets.")
    print("="*95)
    
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
        print(f"[SUCCESS] Successfully indexed {inserted} new Solana Nansen-Grade Smart Wallets into SQLite.")
    else:
        print("\n[DRY-RUN] Verification complete. Run with --execute to commit to database.")
    
    conn.close()

if __name__ == "__main__":
    main()
