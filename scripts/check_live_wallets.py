import urllib.request, json, time

api_key = 'def963d9-3753-4fe1-a609-f0534ead614f'
rpc_url = f'https://mainnet.helius-rpc.com/?api-key={api_key}'

with open('/home/node/Developments/zenithsw/data/verified_smart_wallets.json') as f:
    wallets = json.load(f)

now = time.time()
print(f"Timestamp: {time.strftime('%Y-%m-%d %H:%M:%S', time.localtime())}")
print(f"{'HANDLE':<18} | {'TIER':<6} | {'LAST TX (WIB)':<20} | {'MINS AGO':<10} | {'SIGNATURE'}")
print("-" * 85)

for w in wallets:
    if w['chain'] != 'SOLANA':
        continue
    time.sleep(0.3) # 300ms delay to stay well under Helius 10 RPS limit
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [w['address'], {"limit": 1}]
    }
    req = urllib.request.Request(
        rpc_url,
        data=json.dumps(payload).encode('utf-8'),
        headers={'Content-Type': 'application/json', 'User-Agent': 'Mozilla/5.0'}
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read().decode())
            res = data.get('result', [])
            if res:
                bt = res[0].get('blockTime', 0)
                mins_ago = (now - bt) / 60.0
                tx_time_str = time.strftime('%Y-%m-%d %H:%M:%S', time.localtime(bt + 7*3600))
                print(f"@{w['handle']:<17} | {w['tier']:<6} | {tx_time_str:<20} | {mins_ago:<10.1f} | {res[0]['signature'][:16]}...")
            else:
                print(f"@{w['handle']:<17} | {w['tier']:<6} | {'NO TX':<20} | {'N/A':<10} | -")
    except Exception as e:
        print(f"@{w['handle']:<17} | Error: {e}")
