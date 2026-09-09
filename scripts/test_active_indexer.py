import urllib.request, json, time

api_key = 'def963d9-3753-4fe1-a609-f0534ead614f'
rpc_url = f'https://mainnet.helius-rpc.com/?api-key={api_key}'

req = urllib.request.Request('https://api.dexscreener.com/token-boosts/top/v1', headers={'User-Agent': 'Mozilla/5.0'})
tokens = []
with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read().decode())
    for item in data:
        if item.get('chainId') == 'solana':
            tokens.append(item.get('tokenAddress'))

print(f"Discovered {len(tokens)} trending Solana tokens from DEX...")

if tokens:
    target_mint = tokens[0]
    print(f"Target token: {target_mint}")
    time.sleep(1.0)
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [target_mint, {"limit": 5}]
    }
    req2 = urllib.request.Request(rpc_url, data=json.dumps(payload).encode('utf-8'), headers={'Content-Type': 'application/json', 'User-Agent': 'Mozilla/5.0'})
    with urllib.request.urlopen(req2, timeout=10) as resp2:
        res2 = json.loads(resp2.read().decode())
        sigs = res2.get('result', [])
        print(f"SUCCESS! Found {len(sigs)} recent on-chain transactions on token:")
        for s in sigs:
            bt = s.get('blockTime', 0)
            diff_m = (time.time() - bt) / 60.0
            print(f"  Tx: {s['signature'][:24]}... | {diff_m:.1f} mins ago")
