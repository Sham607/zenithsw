import urllib.request, json

url = "https://api.dexscreener.com/latest/dex/tokens/BGU4TfDLpV1JeJGXVgSbZkLSRxgAzcsQci1vEbaWJVcm"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    for p in data.get("pairs", []):
        print("dexId:", p.get("dexId"), "pairAddress:", p.get("pairAddress"))
        print("  base:", p.get("baseToken", {}).get("symbol"), p.get("baseToken", {}).get("address")[:8])
        print("  quote:", p.get("quoteToken", {}).get("symbol"))
        print("  liq:", p.get("liquidity", {}).get("usd"), "quoteLiq:", p.get("liquidity", {}).get("quote"))
        print("  price:", p.get("priceUsd"))
