import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
fee_authority = "5KXDF6QnqhBj72hDtJNkkpFaQVUfbFXNybMsp3DiK6tD"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenAccountsByOwner",
    "params": [
        fee_authority,
        {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
        {"encoding": "jsonParsed"}
    ]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    accounts = data.get("result", {}).get("value", [])
    print(f"Fee Authority Token Accounts (Standard SPL): {len(accounts)}")
    for a in accounts:
        info = a["account"]["data"]["parsed"]["info"]
        print("  Mint:", info["mint"], "Amt:", info["tokenAmount"]["uiAmount"])
