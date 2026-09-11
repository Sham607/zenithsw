import json
import os
import sqlite3
import sys
import time
import urllib.request

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
RPC_URL = "https://solana-rpc.publicnode.com"

def get_token_accounts(wallet_addr):
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountsByOwner",
        "params": [
            wallet_addr,
            {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
            {"encoding": "jsonParsed"}
        ]
    }
    try:
        req = urllib.request.Request(
            RPC_URL,
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}
        )
        with urllib.request.urlopen(req, timeout=8) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            return data.get("result", {}).get("value", [])
    except Exception:
        return []

def get_token_metadata(mints):
    if not mints:
        return {}
    results = {}
    chunk = list(mints)[:30]
    url = f"https://api.dexscreener.com/latest/dex/tokens/{','.join(chunk)}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=8) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            pairs = data.get("pairs", [])
            for p in pairs:
                base = p.get("baseToken", {})
                mint = base.get("address")
                if mint and mint not in results:
                    results[mint] = {
                        "symbol": base.get("symbol", "UNKNOWN"),
                        "name": base.get("name", "Unknown"),
                        "price_usd": float(p.get("priceUsd") or 0.0),
                        "mcap_usd": float(p.get("marketCap") or p.get("fdv") or 0.0),
                        "liquidity_usd": float(p.get("liquidity", {}).get("usd") or 0.0)
                    }
    except Exception:
        pass
    return results

def main():
    conn = sqlite3.connect(DB_PATH)
    cursor = conn.cursor()
    
    # Query Nansen, Dune, and FOMO Solana active wallets
    cursor.execute("""
        SELECT id, rank, name, handle, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source 
        FROM wallets 
        WHERE chain = 'SOLANA' AND is_active = 1
        ORDER BY 
            CASE source 
                WHEN 'NANSEN_SOLANA_SM' THEN 1 
                WHEN 'DUNE_SOLANA_PRO' THEN 2 
                WHEN 'DUNE_PUMPFUN_SNIPERS' THEN 3
                ELSE 4 
            END,
            pnl_24h_usd DESC
    """)
    wallets = cursor.fetchall()
    conn.close()
    
    print(f"[*] Scanning on-chain active token portfolios across Nansen & Dune Solana Smart Wallets...")
    
    found_with_holdings = 0
    scanned = 0
    
    for w in wallets:
        w_id, rank, name, handle, addr, tier, pnl, wr, trades, source = w
        token_accs = get_token_accounts(addr)
        scanned += 1
        
        # Filter non-zero altcoins
        holding_mints = {}
        for acc in token_accs:
            info = acc["account"]["data"]["parsed"]["info"]
            mint = info["mint"]
            amount = info["tokenAmount"]["uiAmount"]
            if amount and amount > 0 and mint not in ("So11111111111111111111111111111111111111112", "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB"):
                holding_mints[mint] = amount
                
        if holding_mints:
            found_with_holdings += 1
            meta = get_token_metadata(list(holding_mints.keys()))
            valid_holdings = []
            for mint, amt in holding_mints.items():
                if mint in meta:
                    m = meta[mint]
                    val_usd = amt * m["price_usd"]
                    if val_usd >= 10.0: # Filter dust tokens
                        valid_holdings.append((m["symbol"], m["name"], amt, val_usd, m["price_usd"], m["mcap_usd"], mint))
                        
            valid_holdings.sort(key=lambda x: x[3], reverse=True)
            
            if valid_holdings:
                print(f"\n🐋 [{source}] {name} (@{handle})")
                print(f"   Address: {addr} ({tier}) | WinRate: {wr:.1f}%")
                print(f"   Tracked Significant Holdings:")
                for sym, tname, amt, val_usd, price, mcap, mint in valid_holdings[:4]:
                    print(f"     • {sym} ({tname}) | Value: ${val_usd:,.2f} | Balance: {amt:,.1f} @ ${price:.6f} | MCap: ${mcap:,.0f} | Mint: {mint}")
                    
        if found_with_holdings >= 8 or scanned >= 35:
            break
        time.sleep(0.3)

if __name__ == "__main__":
    main()
