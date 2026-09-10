import requests, json

test_mints = [
    'BbCZqewwS1jYUe5c5HQnAtV83hPRjvsoSPRotBhRZ1ck', # EGGS
    '5THtNEGTF1y67eRWvcgz3MomDZePCVx1xdskHHeCpump', # Saturween
    'HsprHxBRgiVaQdRf3TnKNuZZtaX9JUSPgMFdAkk6pump', # GATO
]

for mint in test_mints:
    r = requests.get(f"https://api.dexscreener.com/latest/dex/tokens/{mint}").json()
    pairs = r.get("pairs") or []
    print(f"\n================ Mint: {mint} ================")
    for p in pairs:
        print(f"DEX: {p.get('dexId')} | Pair: {p.get('pairAddress')}")
        print(f"  Price USD: {p.get('priceUsd')}")
        print(f"  FDV: {p.get('fdv')} | Market Cap: {p.get('marketCap')}")
        print(f"  Liquidity Object: {p.get('liquidity')}")
        print(f"  Volume Object: {p.get('volume')}")
