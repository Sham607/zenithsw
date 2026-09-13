import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
target = "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [target, {"limit": 20}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        sigs = json.loads(resp.read().decode("utf-8")).get("result", [])
        print(f"Signatures for Unipcs Solana Address ({target}): {len(sigs)}")
        for s in sigs[:5]:
            print(s["signature"][:20], "time:", s.get("blockTime"))
except Exception as e:
    print("Err:", e)
