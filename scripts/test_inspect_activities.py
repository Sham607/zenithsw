import sqlite3
import requests

conn = sqlite3.connect('/home/node/Developments/zenithsw/data/smart_wallets.db')
c = conn.cursor()
c.execute("SELECT id, token_symbol, token_mint, wallet_handle FROM wallet_activities ORDER BY id DESC LIMIT 10")
rows = c.fetchall()
print(f"Total recorded activities in DB: {len(rows)}")
for r in rows:
    act_id, sym, mint, whale = r
    print(f"\nActivity #{act_id} - {sym} ({mint}) from {whale}")
    res = requests.get(f"https://api.dexscreener.com/latest/dex/tokens/{mint}").json()
    pairs = res.get("pairs") or []
    print(f"  Total pairs returned by DexScreener: {len(pairs)}")
    if pairs:
        first_liq = float(pairs[0].get("liquidity", {}).get("usd") or 0.0)
        first_dex = pairs[0].get("dexId")
        
        max_liq = 0.0
        best_dex = None
        total_liq = 0.0
        for p in pairs:
            liq = float(p.get("liquidity", {}).get("usd") or 0.0)
            total_liq += liq
            if liq > max_liq:
                max_liq = liq
                best_dex = p.get("dexId")
        
        print(f"  First Pool ({first_dex}): ${first_liq:,.2f}")
        print(f"  Max Pool ({best_dex}):   ${max_liq:,.2f}")
        print(f"  Total Token Liq:         ${total_liq:,.2f}")
