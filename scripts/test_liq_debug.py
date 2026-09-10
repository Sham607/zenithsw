import requests
import json

test_tokens = [
    '61V8vBaqAGMpgDQi4JcAwo1dmBGHsyhzodcPqnEVpump',
    'ED5nyyWEzpPPiWimP8vYm7SXDPUvNLNZRjfubcmCpump',
    'CzLSujWBLFsSjncfkh59rUFqvafWcY5tzedWJSuypump',
    'DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263',
]

for t in test_tokens:
    r = requests.get(f'https://api.dexscreener.com/latest/dex/tokens/{t}').json()
    pairs = r.get('pairs', [])
    if pairs:
        print(f"Token: {t}")
        for i, p in enumerate(pairs[:4]):
            dex = p.get('dexId')
            liq = p.get('liquidity')
            price = p.get('priceUsd')
            print(f"  [{i}] Dex: {dex}, Price: {price}, Liquidity: {liq}")
