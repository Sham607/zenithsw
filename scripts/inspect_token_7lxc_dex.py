import urllib.request, json

mint = "7LxC96Ag4DnBotK6bMMUj4kdneAo1kdV1xHnzVF5s7W2"
url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        pairs = data.get("pairs", [])
        print(f"Total pairs on DexScreener: {len(pairs)}")
        for p in pairs[:5]:
            print("DEX:", p.get("dexId"))
            print("  Pair Address:", p.get("pairAddress"))
            print("  Base Token:", p.get("baseToken"))
            print("  Quote Token:", p.get("quoteToken"))
            print("  Price USD:", p.get("priceUsd"))
            print("  Liquidity USD:", p.get("liquidity", {}).get("usd"))
            print("  FDV / MCap:", p.get("marketCap") or p.get("fdv"))
            print("  Pair Created At:", p.get("pairCreatedAt"))
            print("---")
except Exception as e:
    print("Err Dex:", e)
