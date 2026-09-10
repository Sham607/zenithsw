import urllib.request, json

url = 'https://api.dexscreener.com/latest/dex/tokens/0xd7321801caae694090694ff55a9323139f043b88'
req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
resp = urllib.request.urlopen(req, timeout=10)
data = json.loads(resp.read().decode())
pairs = data.get('pairs', [])
print(f"Total pairs found: {len(pairs)}")
for p in pairs[:5]:
    print("Chain:", p.get('chainId'), "DEX:", p.get('dexId'), "PairAddr:", p.get('pairAddress'), "Base:", p.get('baseToken'), "Quote:", p.get('quoteToken'), "Liq USD:", p.get('liquidity', {}).get('usd'))
