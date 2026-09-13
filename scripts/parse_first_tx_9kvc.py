import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sig = "2VCggDgTAD81dH1tPdit8W12qjH1eWc9zY9773N6YJ2B1W4KzP2n2L1LpLg2mD7oGZ6B9a1r2D3f4G5h6J7k8L9"

# Cek tx pertama sender
payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTransaction",
    "params": ["2VCggDgTAD81dH1tPditLqD5jFm2u1E8qH2C2H2q9s1mKjH4D7n9wXyZ3vB1kM8n7L4pQ2R5tS8wV9yA1bC3dE5f", {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
# ambil signature yang tepat dari list sebelumnya
payload["params"][0] = "2VCggDgTAD81dH1tPdit"
# Kita cari signature ke-2
payload_list = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": ["9kVcp55TnJGqS7VHC3rziDkdmiVnPnBJckPhjNGSt2NW", {"limit": 5}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload_list).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

target_sig = sigs[1]["signature"]
print("Tx Signature 2:", target_sig)

tx_payload = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "getTransaction",
    "params": [target_sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(tx_req) as tx_resp:
    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
    meta = tx.get("meta", {})
    print("Logs:")
    for l in meta.get("logMessages", []):
        if "Instruction" in l or "Program log" in l:
            print(" ", l)
