import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sig = "21A4282v3wPbHxb13uh6f21qjCjE288eJ1fN5N2f5X7p9m2k6d4g5j7h8l9m1k2j3h4g5f6d7s8a9"

# Ambil sig yang valid
payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getSignaturesForAddress",
    "params": ["5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6", {"limit": 1}]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    valid_sig = json.loads(resp.read().decode("utf-8"))["result"][0]["signature"]

print("Fetching tx:", valid_sig)
tx_payload = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "getTransaction",
    "params": [valid_sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(tx_req) as tx_resp:
    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")

for ix in tx.get("transaction", {}).get("message", {}).get("instructions", []):
    print("Program:", ix.get("program"), ix.get("programId"))
    if ix.get("parsed"):
        print("  Parsed:", ix.get("parsed"))
