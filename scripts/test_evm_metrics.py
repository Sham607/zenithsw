import urllib.request, json

alchemy_url = "https://arb-mainnet.g.alchemy.com/v2/44zT2Kj3U48u2K1U6z70Wv"
with open('/home/node/Developments/zenithsw/.env') as f:
    for line in f:
        if 'ARBITRUM_RPC_URL' in line:
            alchemy_url = line.split('=', 1)[1].strip().strip('"')

# Test address: Avast @0xAvast from our DB
test_address = "0x89C7bA7be3451D75283f36a56C71329241F83011"

# 1. eth_getBalance
req1 = urllib.request.Request(
    alchemy_url,
    data=json.dumps({"jsonrpc":"2.0","id":1,"method":"eth_getBalance","params":[test_address, "latest"]}).encode('utf-8'),
    headers={"Content-Type": "application/json"}
)
res1 = json.loads(urllib.request.urlopen(req1).read().decode())
bal_hex = res1.get("result", "0x0")
eth_bal = int(bal_hex, 16) / 10**18
print(f"ETH Balance for {test_address}: {eth_bal:.4f} ETH (${eth_bal * 2500:.2f})")

# 2. eth_getTransactionCount
req2 = urllib.request.Request(
    alchemy_url,
    data=json.dumps({"jsonrpc":"2.0","id":2,"method":"eth_getTransactionCount","params":[test_address, "latest"]}).encode('utf-8'),
    headers={"Content-Type": "application/json"}
)
res2 = json.loads(urllib.request.urlopen(req2).read().decode())
tx_count = int(res2.get("result", "0x0"), 16)
print(f"Transaction Count: {tx_count} txs")
