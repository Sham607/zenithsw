import urllib.request, json

alchemy_url = "https://arb-mainnet.g.alchemy.com/v2/44zT2Kj3U48u2K1U6z70Wv"  # let's get exact URL from .env
with open('/home/node/Developments/zenithsw/.env') as f:
    for line in f:
        if 'ARBITRUM_RPC_URL' in line:
            alchemy_url = line.split('=', 1)[1].strip().strip('"')

print("Using Arbitrum/Robinhood RPC URL:", alchemy_url[:40] + "...")

# Let's test alchemy_getAssetTransfers on a sample Robinhood token from DexScreener
# Example token from dexscreener top boosts:
url = 'https://api.dexscreener.com/token-boosts/top/v1'
req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
resp = urllib.request.urlopen(req, timeout=10)
data = json.loads(resp.read().decode())

rh_tokens = [d for d in data if d.get('chainId') in ('robinhood', 'arbitrum')]
print("Found Robinhood/Arbitrum boosted tokens:", len(rh_tokens))

if rh_tokens:
    sample_token = rh_tokens[0]['tokenAddress']
    print("Testing token:", sample_token)
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "alchemy_getAssetTransfers",
        "params": [{
            "fromBlock": "0x0",
            "toBlock": "latest",
            "contractAddresses": [sample_token],
            "category": ["erc20"],
            "order": "asc",
            "maxCount": "0xa"
        }]
    }
    req = urllib.request.Request(alchemy_url, data=json.dumps(payload).encode('utf-8'), headers={'Content-Type': 'application/json'})
    try:
        res = urllib.request.urlopen(req, timeout=10)
        res_data = json.loads(res.read().decode())
        print("Alchemy transfers result:", json.dumps(res_data, indent=2)[:500])
    except Exception as e:
        print("Alchemy transfers failed:", e)
