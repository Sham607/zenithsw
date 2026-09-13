import urllib.request, json

mints = [
    ("Titcoin", "8WgQ9XpJYCfnwmSmMTdVioUuxxwnMdYuRvSn9osXSTNK"),
    ("VAGINA", "9oYK1mhLmV283Gggi2vVR4JJC9bcymp7otNtcu5hvDwc")
]

for sym, mint in mints:
    url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        pairs = data.get("pairs", [])
        print(f"Token {sym} ({mint[:8]}...): Total pairs = {len(pairs)}")
        for p in pairs[:5]:
            dex = p.get('dexId')
            liq = p.get('liquidity', {}).get('usd')
            price = p.get('priceUsd')
            mcap = p.get('marketCap')
            print(f"  dex: {dex} | liq: ${liq} | price: ${price} | mcap: ${mcap}")
