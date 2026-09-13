import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
vault = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [vault, {"limit": 100}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

print(f"Total signatures in history for Unipcs Vault ({vault}): {len(sigs)}")
for s in sigs:
    print(s["signature"][:20], "time:", s.get("blockTime"))
