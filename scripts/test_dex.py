import urllib.request, json

req = urllib.request.Request('https://api.dexscreener.com/token-boosts/top/v1', headers={'User-Agent': 'Mozilla/5.0'})
with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read().decode())
    print(f'Top boosted tokens: {len(data)}')
    for item in data[:6]:
        if item.get('chainId') == 'solana':
            print(f"Solana Token: {item.get('tokenAddress')}")
