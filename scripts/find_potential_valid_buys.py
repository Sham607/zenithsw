import urllib.request, json, sqlite3, time

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

cur.execute("""
    SELECT id, token_symbol, token_mint, wallet_handle, amount_usd
    FROM wallet_activities
    WHERE activity_type='BUY' AND amount_usd >= 50
    ORDER BY id DESC LIMIT 15
""")
buys = cur.fetchall()
conn.close()

print(f"Testing {len(buys)} recent buys with proper multi-pool aggregation:\n")

for aid, sym, mint, whale, amt in buys:
    url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            pairs = data.get("pairs", [])

            best_liq = 0.0
            best_price = 0.0
            best_mcap = 0.0

            for p in pairs:
                base_addr = (p.get("baseToken") or {}).get("address", "")
                if base_addr.lower() != mint.lower():
                    continue
                dex = p.get("dexId")
                liq = float((p.get("liquidity") or {}).get("usd") or 0.0)
                mcap = float(p.get("marketCap") or p.get("fdv") or 0.0)
                price = float(p.get("priceUsd") or 0.0)

                if dex == "pumpfun" and liq <= 0.0 and mcap > 0.0:
                    liq = mcap * 0.5

                if liq > best_liq:
                    best_liq = liq
                    best_price = price
                    best_mcap = mcap

            print(f"[{sym:<10}] by @{whale} (${amt:,.1f}) -> Best Liq: ${best_liq:,.0f} | MCap: ${best_mcap:,.0f} | Price: ${best_price:.6f}")
        time.sleep(0.15)
    except Exception as e:
        print(f"Err {sym}: {e}")
