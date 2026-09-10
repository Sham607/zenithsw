import requests

# Test newly launched pump.fun tokens
r = requests.get("https://frontend-api.pump.fun/coins?offset=0&limit=10&sort=last_trade_timestamp&order=DESC&includeNsfw=false").json()
for c in r:
    mint = c.get("mint")
    symbol = c.get("symbol")
    market_cap = c.get("usd_market_cap", 0)
    v_sol = c.get("virtual_sol_reserves", 0)
    # Check DexScreener
    dex_r = requests.get(f"https://api.dexscreener.com/latest/dex/tokens/{mint}").json()
    dex_pairs = dex_r.get("pairs")
    print(f"Token: {symbol} ({mint[:8]}...) | MC: ${market_cap:,.2f} | vSol: {v_sol}")
    if dex_pairs:
        for p in dex_pairs:
            dex_id = p.get("dexId")
            liq = p.get("liquidity", {}).get("usd", 0)
            print(f"   DexScreener {dex_id}: Liq = ${liq}")
    else:
        print("   DexScreener: No pairs indexed yet (Bonding curve)")
