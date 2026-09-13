import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sig = "3uQAwSMW6UFcchePAuaZkH2QxK1t3Vq9P7n4m5L2sK3L4P5Q6R7S8T9U1V2W3X4Y5Z6A7B8C9D1E2F3G4H"

payload_list = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": ["5KXDF6QnqhBj72hDtJNkkpFaQVUfbFXNybMsp3DiK6tD", {"limit": 1}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload_list).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sig = json.loads(resp.read().decode("utf-8"))["result"][0]["signature"]

print("Fetching tx for fee authority:", sig)
tx_payload = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "getTransaction",
    "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(tx_req) as tx_resp:
    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")

for ix in tx.get("transaction", {}).get("message", {}).get("instructions", []):
    print("Program:", ix.get("program"), ix.get("programId"))
    if ix.get("parsed"):
        print("  Parsed:", ix.get("parsed"))
