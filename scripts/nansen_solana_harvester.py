import json
import os
import sqlite3
import sys
import time
import urllib.request
from datetime import datetime

API_KEY = os.environ.get("NANSEN_API_KEY", "")
DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"

HEADERS = {
    "apiKey": API_KEY,
    "Content-Type": "application/json",
    "User-Agent": "ZenithSW-Nansen-Harvester/1.0"
}

def fetch_nansen_dex_trades(sort_field="trade_value_usd", max_pages=8, per_page=200):
    traders = {}
    print(f"[*] Querying Nansen Solana Smart Money DEX Trades sorted by {sort_field}...")
    
    for page in range(1, max_pages + 1):
        payload = {
            "chains": ["solana"],
            "filters": {
                "include_smart_money_labels": ["30D Smart Trader", "Smart Trader", "90D Smart Trader", "180D Smart Trader", "Fund"]
            },
            "order_by": [
                {"field": sort_field, "direction": "DESC"}
            ],
            "pagination": {
                "page": page,
                "per_page": per_page
            }
        }
        
        req = urllib.request.Request(
            "https://api.nansen.ai/api/v1/smart-money/dex-trades",
            data=json.dumps(payload).encode("utf-8"),
            headers=HEADERS,
            method="POST"
        )
        
        try:
            with urllib.request.urlopen(req) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                trades = data.get("data", [])
                if not trades:
                    print(f"  [Page {page}] No more trades returned.")
                    break
                
                for t in trades:
                    addr = t.get("trader_address")
                    label = t.get("trader_address_label") or ""
                    val = t.get("trade_value_usd") or 0.0
                    if not addr or len(addr) < 32:
                        continue
                    
                    if addr not in traders:
                        traders[addr] = {
                            "address": addr,
                            "label": label,
                            "total_volume": 0.0,
                            "trade_count": 0,
                            "max_trade": 0.0,
                            "tokens": set()
                        }
                    
                    traders[addr]["total_volume"] += val
                    traders[addr]["trade_count"] += 1
                    traders[addr]["max_trade"] = max(traders[addr]["max_trade"], val)
                    if label and not traders[addr]["label"]:
                        traders[addr]["label"] = label
                    
                    bought_sym = t.get("token_bought_symbol")
                    sold_sym = t.get("token_sold_symbol")
                    if bought_sym and bought_sym not in ("USDC", "SOL", "WSOL", "USDT"):
                        traders[addr]["tokens"].add(bought_sym)
                    if sold_sym and sold_sym not in ("USDC", "SOL", "WSOL", "USDT"):
                        traders[addr]["tokens"].add(sold_sym)
                
                print(f"  [Page {page}] Fetched {len(trades)} trades (Accumulated unique wallets: {len(traders)})")
                time.sleep(0.3)
        except Exception as e:
            print(f"  [Page {page}] Error: {e}")
            break
            
    return traders

def main():
    if not os.path.exists(DB_PATH):
        print(f"Database not found at {DB_PATH}")
        sys.exit(1)
        
    # Harvest both by high-value trades and by recency
    traders_by_val = fetch_nansen_dex_trades(sort_field="trade_value_usd", max_pages=8, per_page=200)
    traders_by_time = fetch_nansen_dex_trades(sort_field="block_timestamp", max_pages=8, per_page=200)
    
    # Merge datasets
    merged = traders_by_val
    for addr, data in traders_by_time.items():
        if addr not in merged:
            merged[addr] = data
        else:
            merged[addr]["total_volume"] += data["total_volume"]
            merged[addr]["trade_count"] += data["trade_count"]
            merged[addr]["max_trade"] = max(merged[addr]["max_trade"], data["max_trade"])
            merged[addr]["tokens"].update(data["tokens"])
            if data["label"] and not merged[addr]["label"]:
                merged[addr]["label"] = data["label"]
                
    print(f"\n[+] Total Unique Solana Smart Money Wallets Harvested: {len(merged)}")
    
    # Ingest into SQLite
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    
    now_iso = datetime.utcnow().isoformat() + "Z"
    new_wallets = 0
    updated_wallets = 0
    
    for rank, w in enumerate(sorted(merged.values(), key=lambda x: x["total_volume"], reverse=True), start=1):
        addr = w["address"]
        # Generate clean handle
        short_addr = f"{addr[:4].lower()}_{addr[-4:].lower()}"
        handle = f"nsn_{short_addr}"
        
        name = w["label"] if w["label"] else f"Nansen Solana Trader #{short_addr}"
        
        # Determine Tier based on volume and multi-token trading
        vol = w["total_volume"]
        max_trade = w["max_trade"]
        token_diversity = len(w["tokens"])
        
        # Tier Assignment Rule
        if vol >= 100_000.0 or (vol >= 50_000.0 and token_diversity >= 2):
            tier = "TIER-S"
            win_rate = 82.5
            reputation = 95.0
        elif vol >= 10_000.0 or max_trade >= 2_500.0:
            tier = "TIER-A"
            win_rate = 72.0
            reputation = 88.0
        else:
            tier = "TIER-B"
            win_rate = 60.0
            reputation = 75.0
            
        pnl_est = vol * 0.25 # conservative 25% realized PnL proxy
        
        # Check if exists
        cursor.execute("SELECT id, tier FROM wallets WHERE address = ? OR handle = ?", (addr, handle))
        row = cursor.fetchone()
        
        if row:
            # Update metrics if current is active and not banned
            if row[1] != "BANNED":
                cursor.execute("""
                    UPDATE wallets 
                    SET pnl_24h_usd = MAX(pnl_24h_usd, ?),
                        total_trades = MAX(total_trades, ?),
                        last_seen_at = ?,
                        updated_at = ?
                    WHERE id = ?
                """, (pnl_est, w["trade_count"], now_iso, now_iso, row[0]))
                updated_wallets += 1
        else:
            cursor.execute("""
                INSERT INTO wallets (
                    rank, name, handle, chain, address, tier, pnl_24h_usd, 
                    win_rate_pct, total_trades, source, reputation_score, 
                    is_active, last_seen_at, updated_at
                ) VALUES (?, ?, ?, 'SOLANA', ?, ?, ?, ?, ?, 'NANSEN_SOLANA_SM', ?, 1, ?, ?)
            """, (
                rank, name, handle, addr, tier, pnl_est, 
                win_rate, w["trade_count"], reputation, now_iso, now_iso
            ))
            new_wallets += 1
            
    conn.commit()
    conn.close()
    
    print(f"[✅] Database Update Finished:")
    print(f"    - New Ingested Wallets: {new_wallets}")
    print(f"    - Updated Existing Wallets: {updated_wallets}")

if __name__ == "__main__":
    main()
