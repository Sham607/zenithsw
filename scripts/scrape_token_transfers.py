import urllib.request, json

# Ambil transaksi terbaru dari mint USELESS untuk melihat siapa yang memegang jutaan token
rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
mint = "Dz9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk"

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

print(f"Checking {len(sigs)} recent txs on USELESS mint...")
for s in sigs[:5]:
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
            meta = tx.get("meta", {})
            for p in meta.get("postTokenBalances", []):
                amt = (p.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                owner = p.get("owner")
                if amt > 1_000_000 and owner:
                    print(f"  Whale Holder in Tx: {owner} | Amount: {amt:,.2f} USELESS")
    except Exception:
        pass
