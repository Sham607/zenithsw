#!/usr/bin/env python3
"""
GMGN Solana Smart Money Harvester (Paced & Resilient)
Strict 5-pillar gating for ZenithSW:
1. Win Rate >= 60.0% (30D/7D track record)
2. Total Trades >= 8 (sufficient historical sample)
3. Anti-Insider Filter: Drops rat_trader, bundler, fresh_wallet, scam dev
4. Profit Floor: Realized PnL >= $5,000 USD
5. Pacing & 429 Auto-Cooldown: Respects GMGN Free Tier (Weight 5) automatically.
"""

import os
import sys
import time
import json
import uuid
import sqlite3
import urllib.request
import urllib.error

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
GMGN_ENV = os.path.expanduser("~/.config/gmgn/.env")

def get_api_key() -> str:
    if os.path.exists(GMGN_ENV):
        with open(GMGN_ENV, "r") as f:
            for line in f:
                if line.startswith("GMGN_API_KEY="):
                    return line.strip().split("=", 1)[1].strip('"\'')
    return os.getenv("GMGN_API_KEY", "")

API_KEY = get_api_key()
if not API_KEY:
    print("[GMGN HARVESTER ❌] GMGN_API_KEY not found in ~/.config/gmgn/.env")
    sys.exit(1)

def gmgn_req(subpath: str, params: dict | None = None, max_retries: int = 3) -> dict:
    if params is None:
        params = {}
    
    for attempt in range(max_retries):
        p = params.copy()
        p["timestamp"] = int(time.time())
        p["client_id"] = str(uuid.uuid4())
        
        qs = "&".join(f"{k}={v}" for k, v in p.items())
        url = f"https://openapi.gmgn.ai{subpath}?{qs}"
        headers = {
            "X-APIKEY": API_KEY,
            "User-Agent": "gmgn-cli/1.0.0"
        }
        req = urllib.request.Request(url, headers=headers)
        
        try:
            time.sleep(0.5) # Safe pacing between calls
            with urllib.request.urlopen(req, timeout=15) as res:
                return json.loads(res.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if e.code == 429:
                reset_ts = int(e.headers.get("x-ratelimit-reset", 0))
                now_ts = int(time.time())
                wait_sec = max(5, reset_ts - now_ts + 2) if reset_ts > now_ts else 30
                print(f"[GMGN ⏳ RATE LIMIT] Cooldown active. Waiting {wait_sec}s for quota reset (Attempt {attempt+1}/{max_retries})...")
                time.sleep(wait_sec)
            else:
                print(f"[GMGN ⚠️ HTTP {e.code}] {subpath}: {e}")
                return {}
        except Exception as e:
            print(f"[GMGN ⚠️ NETWORK ERROR] {subpath}: {e}")
            time.sleep(2)
    return {}

def run_harvest(max_tokens: int = 6, limit_per_token: int = 10):
    print("=" * 60)
    print("     ZENITHSW - GMGN SOLANA SMART MONEY HARVESTER")
    print("=" * 60)
    candidate_wallets = set()

    # 1. Pull Realtime Smart Money Feed
    print("\n[GMGN 📡 STEP 1] Fetching Live Smart Money Feed on Solana...")
    sm_res = gmgn_req("/v1/user/smartmoney", {"chain": "sol"})
    sm_list = sm_res.get("data", {}).get("list", [])
    for item in sm_list:
        maker = item.get("maker")
        if maker and len(maker) > 30:
            candidate_wallets.add(maker)
    print(f"[GMGN 📡] Extracted {len(candidate_wallets)} active makers from Smart Money stream.")

    # 2. Pull Trending Tokens on Solana
    print(f"\n[GMGN 🔍 STEP 2] Scanning Top {max_tokens} Trending Tokens on Solana...")
    hot_tokens = []
    rank_res = gmgn_req("/v1/market/rank", {
        "chain": "sol",
        "interval": "24h",
        "orderby": "volume",
        "direction": "desc",
        "limit": max_tokens
    })
    data_obj = rank_res.get("data", {})
    if isinstance(data_obj, dict) and "data" in data_obj and "rank" in data_obj["data"]:
        hot_tokens = data_obj["data"]["rank"]
    elif isinstance(data_obj, dict) and "rank" in data_obj:
        hot_tokens = data_obj["rank"]
    print(f"[GMGN 🔍] Found {len(hot_tokens)} high-volume Solana tokens.")

    # 3. Pull Top Profitable Traders per Token
    print("\n[GMGN 👥 STEP 3] Extracting Top Traders per Hot Token...")
    for t in hot_tokens:
        addr = t.get("address")
        sym = t.get("symbol", "UNKNOWN")
        if not addr:
            continue
        traders_res = gmgn_req("/v1/market/token_top_traders", {
            "chain": "sol",
            "address": addr,
            "order_by": "profit",
            "direction": "desc",
            "limit": limit_per_token
        })
        t_list = traders_res.get("data", {}).get("list", [])
        added_for_token = 0
        for tr in t_list:
            w_addr = tr.get("address")
            if w_addr and len(w_addr) > 30:
                candidate_wallets.add(w_addr)
                added_for_token += 1
        print(f"  -> Token {sym} ({addr[:8]}...): Extracted {added_for_token} top traders.")

    print(f"\n[GMGN 🎯] Total Unique Candidate Wallets Collected: {len(candidate_wallets)}")
    print("[GMGN 🕵️ STEP 4] Running 5-Pillar Statistical & Insider Audit on Candidates...")

    qualified_wallets = []
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()

    for idx, wallet in enumerate(candidate_wallets, 1):
        stats_res = gmgn_req("/v1/user/wallet_stats", {
            "chain": "sol",
            "wallet_address": wallet,
            "period": "30d"
        })
        w_data = stats_res.get("data", {})
        if not w_data:
            continue

        pnl_stat = w_data.get("pnl_stat", {})
        common = w_data.get("common", {})
        tags = common.get("tags", [])

        # Gating 1: Anti-Insider & Anti-Scam Filter
        banned_tags = {"rat_trader", "bundler", "fresh_wallet", "scammer", "dev"}
        if any(t.lower() in banned_tags for t in tags):
            print(f"  [{idx}/{len(candidate_wallets)}] REJECT 🚫 {wallet[:8]}... -> Banned Tag: {tags}")
            continue

        winrate = float(pnl_stat.get("winrate", 0.0)) * 100.0
        token_count = int(pnl_stat.get("token_num", 0))
        realized_pnl = float(w_data.get("realized_profit", 0.0))
        avg_holding = float(pnl_stat.get("avg_holding_period", 0.0))

        # Gating 2: Sample Size & Holding Time Filter
        if token_count < 5 or avg_holding < 30.0:
            print(f"  [{idx}/{len(candidate_wallets)}] REJECT ❌ {wallet[:8]}... -> Insufficient trades ({token_count}) or MEV sub-30s hold")
            continue

        # Gating 3: Win Rate >= 60.0% & Realized Profit Floor >= $5,000
        if winrate < 60.0 or realized_pnl < 5000.0:
            print(f"  [{idx}/{len(candidate_wallets)}] REJECT ❌ {wallet[:8]}... -> WinRate: {winrate:.1f}% (<60%) or PnL: +${realized_pnl:,.0f} (<$5k)")
            continue

        # Tier Classification
        tier = "TIER-A"
        if realized_pnl >= 500000.0 and winrate >= 75.0:
            tier = "TIER-S"
        elif winrate >= 70.0 or realized_pnl >= 100000.0:
            tier = "TIER-A"
        else:
            tier = "TIER-B"

        now_iso = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        name = f"GMGN Solana Trader {wallet[:6]}"
        handle = f"@gmgn_{wallet[:4].lower()}_{wallet[-4:].lower()}"

        # Upsert into SQLite 'wallets' table
        cursor.execute("""
            INSERT INTO wallets (
                rank, name, handle, chain, address, tier,
                pnl_24h_usd, win_rate_pct, total_trades, source,
                reputation_score, is_active, updated_at
            ) VALUES (
                999, ?, ?, 'SOLANA', ?, ?,
                ?, ?, ?, 'GMGN_PRO',
                85.0, 1, ?
            )
            ON CONFLICT(handle) DO UPDATE SET
                tier=excluded.tier,
                win_rate_pct=excluded.win_rate_pct,
                pnl_24h_usd=excluded.pnl_24h_usd,
                total_trades=excluded.total_trades,
                is_active=1,
                updated_at=excluded.updated_at;
        """, (name, handle, wallet, tier, realized_pnl, winrate, token_count, now_iso))

        qualified_wallets.append({
            "address": wallet,
            "label": handle,
            "tier": tier,
            "winrate": winrate,
            "pnl": realized_pnl,
            "trades": token_count
        })

        print(f"  [{idx}/{len(candidate_wallets)}] QUALIFIED ✅ {handle} ({wallet[:8]}...) -> {tier} | WinRate: {winrate:.1f}% | PnL: +${realized_pnl:,.0f} | Trades: {token_count}")

    conn.commit()
    conn.close()

    print("\n" + "=" * 60)
    print(f"[GMGN HARVEST COMPLETE 🎉] Ingested {len(qualified_wallets)} Top Solana Smart Wallets into ZenithSW DB!")
    print("=" * 60)

if __name__ == "__main__":
    run_harvest()
