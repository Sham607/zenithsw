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
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

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
            meta = tx_data.get("meta", {})
            pre = meta.get("preTokenBalances", [])
            post = meta.get("postTokenBalances", [])
            signers = [acc["pubkey"] for acc in tx_data.get("transaction", {}).get("message", {}).get("accountKeys", []) if acc.get("signer")]

            # Cari pengirim yang tokennya berkurang
            for p in post:
                if p.get("owner") == vault:
                    mint = p.get("mint")
                    post_amt = (p.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                    pre_amt = 0.0
                    for pr in pre:
                        if pr.get("owner") == vault and pr.get("mint") == mint:
                            pre_amt = (pr.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                            break
                    delta = post_amt - pre_amt
                    if delta > 0:
                        # Siapa pengirim token ini?
                        senders = []
                        for pr in pre:
                            if pr.get("mint") == mint and pr.get("owner") != vault:
                                p_owner = pr.get("owner")
                                # cek post
                                post_sender_amt = 0.0
                                for po in post:
                                    if po.get("owner") == p_owner and po.get("mint") == mint:
                                        post_sender_amt = (po.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                                        break
                                sender_delta = post_sender_amt - ((pr.get("uiTokenAmount") or {}).get("uiAmount") or 0.0)
                                if sender_delta < 0:
                                    senders.append((p_owner, abs(sender_delta)))

                        print(f"🔥 INBOUND TOKEN TO VAULT DETECTED!")
                        print(f"  Tx: {sig}")
                        print(f"  Signer: {signers}")
                        print(f"  Mint: {mint}")
                        print(f"  Vault Gained: +{delta:,.2f} tokens")
                        print(f"  Sent From Wallet: {senders}")
                        print("---")
            time.sleep(0.2)
    except Exception as e:
        pass
