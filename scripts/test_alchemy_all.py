import urllib.request, json

alchemy_url = ""
with open('/home/node/Developments/zenithsw/.env') as f:
    for line in f:
        if 'ARBITRUM_RPC_URL' in line:
            alchemy_url = line.split('=', 1)[1].strip().strip('"')

# Let's test alchemy_getAssetTransfers with multiple categories
token = "0x008Df4b3E857D06c4603Aeb11F267ccD32ce2005"
payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "alchemy_getAssetTransfers",
    "params": [{
        "fromBlock": "0x0",
        "toBlock": "latest",
        "contractAddresses": [token],
        "category": ["external", "erc20"],
        "order": "desc",
        "maxCount": "0x5"
    }]
}
req = urllib.request.Request(alchemy_url, data=json.dumps(payload).encode('utf-8'), headers={'Content-Type': 'application/json'})
try:
    res = urllib.request.urlopen(req, timeout=10)
    data = json.loads(res.read().decode())
    print("Alchemy Transfers:", json.dumps(data, indent=2))
except Exception as e:
    print("Failed:", e)
