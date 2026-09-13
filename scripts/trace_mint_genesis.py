import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
mint = "5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [mint, {"limit": 10}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    sigs = data.get("result", [])
    print(f"Signatures count for mint: {len(sigs)}")
    for s in sigs[:5]:
        print(s["signature"][:20], s.get("blockTime"))
