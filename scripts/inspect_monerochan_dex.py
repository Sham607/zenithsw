import urllib.request, json

mint = "39HqgQ18g9R33sUfKq1TjX4sbKrgTb4YntnBCFhURJvy"
url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    pairs = data.get("pairs", [])
    print(f"Total pairs for MONEROCHAN: {len(pairs)}")
    for p in pairs[:6]:
        dex = p.get("dexId")
        pair_addr = p.get("pairAddress")
        base = p.get("baseToken", {}).get("symbol")
        quote = p.get("quoteToken", {}).get("symbol")
        quote_addr = p.get("quoteToken", {}).get("address")
        liq = p.get("liquidity", {}).get("usd")
        print(f"DEX: {dex:<10} | Pair: {base}/{quote} | Liq: ${liq:,.2f} | QuoteAddr: {quote_addr}")
