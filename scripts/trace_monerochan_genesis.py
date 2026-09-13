import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
mint = "39HqgQ18g9R33sUfKq1TjX4sbKrgTb4YntnBCFhURJvy"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [mint, {"limit": 100}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    sigs = data.get("result", [])
    print(f"Total signatures fetched: {len(sigs)}")
    if sigs:
        oldest_sig = sigs[-1]["signature"]
        print("Oldest signature (Genesis):", oldest_sig)

        tx_payload = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getTransaction",
            "params": [oldest_sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
        }
        tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(tx_req) as tx_resp:
            tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
            for ix in tx.get("transaction", {}).get("message", {}).get("instructions", []):
                print("Genesis IX Program:", ix.get("program"), ix.get("programId"))
