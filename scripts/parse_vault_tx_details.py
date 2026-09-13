import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sig = "622DkMRkHn9U5kr7ikXuLoaYSxBF4umnRDScaPhmwPkhURSAvbE2xN8rTxNwgeem6sfNNE63i4q68PM8SdopYApU"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    tx = json.loads(resp.read().decode("utf-8")).get("result")

meta = tx.get("meta", {})
pre = meta.get("preTokenBalances", [])
post = meta.get("postTokenBalances", [])
print("Pre Token Balances:")
for p in pre:
    print(" ", p.get("owner"), p.get("mint")[:8], p.get("uiTokenAmount", {}).get("uiAmount"))
print("Post Token Balances:")
for p in post:
    print(" ", p.get("owner"), p.get("mint")[:8], p.get("uiTokenAmount", {}).get("uiAmount"))
