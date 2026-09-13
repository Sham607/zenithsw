import urllib.request, json, sqlite3, time

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

# Ambil semua mint yang dibeli dengan amount_usd >= 150
cur.execute("""
    SELECT DISTINCT token_mint, token_symbol, wallet_handle, amount_usd
    FROM wallet_activities
    WHERE activity_type='BUY' AND amount_usd >= 150
    ORDER BY id DESC LIMIT 15
""")
buys = cur.fetchall()
conn.close()

print(f"Auditing DexScreener Real Liquidity for {len(buys)} High-Conviction Buys (>= $150 USD):\n")

for mint, sym, whale, amt in buys:
    url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            pairs = data.get("pairs", [])
            total_liq = sum([float(p.get("liquidity", {}).get("usd") or 0) for p in pairs])
            max_liq = max([float(p.get("liquidity", {}).get("usd") or 0) for p in pairs]) if pairs else 0
            mcap = pairs[0].get("marketCap") or pairs[0].get("fdv") if pairs else 0
            print(f"Token: {sym:<12} | Whale: @{whale} (${amt:,.0f})")
            print(f"  Mint: {mint}")
            print(f"  Pairs: {len(pairs)} | Max Pool Liq: ${max_liq:,.0f} | Total Liq: ${total_liq:,.0f} | MCap: ${float(mcap or 0):,.0f}")
            print("-" * 50)
        time.sleep(0.2)
    except Exception as e:
        print(f"Err {sym}: {e}")
