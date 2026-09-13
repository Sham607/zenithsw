import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sender = "9kVcp55TnJGqS7VHC3rziDkdmiVnPnBJckPhjNGSt2NW"

# Cek 10 signature dari sender ini
payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [sender, {"limit": 10}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

print(f"Total signatures for 9kVcp55Tn...: {len(sigs)}")
for s in sigs:
    sig = s["signature"]
    # get tx
    tx_payload = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "getTransaction",
        "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
    }
    tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(tx_req) as tx_resp:
        tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
        meta = tx.get("meta", {})
        pre = meta.get("preBalances", [])
        post = meta.get("postBalances", [])
        print(f"Tx: {sig[:20]}... | Log: {meta.get('logMessages', [])[:2]}")
