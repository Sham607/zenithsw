import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
genesis_sig = "29nZvy35rfqbs8xStwBtwz2nK8m8cM1jXvL1f1N4m2sK3L4P5Q6R7S8T9U1V2W3X4Y5Z6A7B8C9D1E2F3G4H"

payload_list = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": ["2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF", {"limit": 20}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload_list).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

genesis_sig = sigs[-1]["signature"]
print("Genesis Signature:", genesis_sig)

tx_payload = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "getTransaction",
    "params": [genesis_sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(tx_req) as tx_resp:
    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")

msg = tx.get("transaction", {}).get("message", {})
for ix in msg.get("instructions", []):
    print("IX:", ix.get("program"), ix.get("parsed"))
