import urllib.request, json, time

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
vault = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [vault, {"limit": 40}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

print(f"Checking {len(sigs)} txs for SOL/USDC funding outbound from Vault...")

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
            tx_data = json.loads(tx_resp.read().decode("utf-8")).get("result")
            if not tx_data:
                continue

            instructions = tx_data.get("transaction", {}).get("message", {}).get("instructions", [])
            for inst in instructions:
                parsed = inst.get("parsed")
                if not parsed:
                    continue
                info = parsed.get("info", {})
                inst_type = parsed.get("type", "")

                # Transfer SOL langsung
                if inst_type == "transfer":
                    src = info.get("source")
                    dst = info.get("destination")
                    lamports = info.get("lamports", 0)
                    sol = lamports / 1e9
                    if src == vault and sol >= 0.5:
                        print(f"🚀 FUNDING OUTBOUND DETECTED (SOL):")
                        print(f"  Tx: {sig}")
                        print(f"  To Satellite: {dst}")
                        print(f"  Amount: {sol:.4f} SOL")
                        print("---")
            time.sleep(0.1)
    except Exception:
        pass
