import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
sig = "2VCggDgTAD81dH1tPdity7nv3Aq6jsxu39JbN983TevBn7D7Nkz5SZBGvkQET18XFCVM5HjkTgmRT6Q1Yt73AVRe"

tx_payload = {
    "jsonrpc": "2.0",
    "id": 2,
    "method": "getTransaction",
    "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
}
tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(tx_req) as tx_resp:
    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")

msg = tx.get("transaction", {}).get("message", {})
for ix in msg.get("instructions", []):
    print("IX Program:", ix.get("program"), "Parsed:", ix.get("parsed"))
