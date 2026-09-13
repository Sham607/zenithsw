import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
fee_authority = "5KXDF6QnqhBj72hDtJNkkpFaQVUfbFXNybMsp3DiK6tD"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [fee_authority, {"limit": 10}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    sigs = data.get("result", [])
    print(f"Total signatures for Fee Authority ({fee_authority[:8]}...): {len(sigs)}")
    for s in sigs[:5]:
        print(s["signature"][:20], s.get("blockTime"))
