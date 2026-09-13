import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
fee_authority = "5KXDF6QnqhBj72hDtJNkkpFaQVUfbFXNybMsp3DiK6tD"
weth_mint = "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [fee_authority, {"limit": 30}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

print(f"Scanning {len(sigs)} txs for WETH / StonkFun distribution...")

for s in sigs:
    sig = s["signature"]
    tx_payload = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "getTransaction",
        "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
    }
    tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(tx_req) as tx_resp:
            tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
            if not tx:
                continue
            meta = tx.get("meta", {})
            post_tok = meta.get("postTokenBalances", [])
            for p in post_tok:
                if p.get("mint") == weth_mint:
                    print(f"🔥 WETH INTERACTION DETECTED in Tx: {sig[:20]}...")
                    print(f"  Owner: {p.get('owner')} | Amount: {p.get('uiTokenAmount', {}).get('uiAmount')}")
    except Exception:
        pass
