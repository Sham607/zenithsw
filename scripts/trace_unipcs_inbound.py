import urllib.request, json, time

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
vault = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": [vault, {"limit": 15}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    res = json.loads(resp.read().decode("utf-8"))
    sigs = res.get("result", [])

print(f"Analyzing {len(sigs)} transactions for Unipcs Vault ({vault[:8]}...)\n")

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

            block_time = tx_data.get("blockTime")
            msg = tx_data.get("transaction", {}).get("message", {})
            instructions = msg.get("instructions", [])

            # Cari transfer token SPL atau SOL
            for inst in instructions:
                parsed = inst.get("parsed")
                if not parsed:
                    continue
                inst_type = parsed.get("type")
                info = parsed.get("info", {})

                # Check SPL token transfer / transferChecked
                if "transfer" in inst_type.lower():
                    dest = info.get("destination") or info.get("wallet") or info.get("account")
                    src = info.get("source") or info.get("authority") or info.get("multisigAuthority")
                    amt = info.get("amount") or info.get("tokenAmount", {}).get("uiAmount")
                    mint = info.get("mint")
                    authority = info.get("authority")

                    print(f"Tx: {sig[:16]}... | Type: {inst_type}")
                    print(f"  From/Authority: {authority or src}")
                    print(f"  Destination: {dest}")
                    print(f"  Token/Amount: Mint: {mint} | Amt: {amt}")
                    print("---")
            time.sleep(0.3)
    except Exception as e:
        print(f"Err on {sig[:10]}: {e}")
