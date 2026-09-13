import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
target = "CKuBTzPwzrCisqQMQfAjNn6jKcfny8ny4rJVN3cxLjcf"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [target, {"limit": 25}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

print(f"Checking {len(sigs)} recent signatures for CKuB ({target[:8]}...)\n")

related_addresses = set()
for s in sigs[:10]:
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
            pre = meta.get("preTokenBalances", [])
            post = meta.get("postTokenBalances", [])

            # Cek pengirim atau penerima token langsung
            for pr in pre:
                owner = pr.get("owner")
                if owner and owner != target and not owner.startswith("1111") and len(owner) > 30:
                    related_addresses.add(owner)
            for po in post:
                owner = po.get("owner")
                if owner and owner != target and not owner.startswith("1111") and len(owner) > 30:
                    related_addresses.add(owner)
    except Exception:
        pass

print(f"Found {len(related_addresses)} on-chain addresses interacting with CKuB in sample:")
for a in list(related_addresses)[:10]:
    print(" ", a)
