import urllib.request, json

url = "https://robinhoodchain.blockscout.com/api/v2/addresses/0x2ac082e252143c89c6e3bcb972e49855f9f6711c/token-transfers"
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
req = urllib.request.Request(url, headers=headers)
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        items = data.get("items", [])
        print(f"Robinhood Unipcs Transfers Count: {len(items)}")
        for it in items[:10]:
            from_addr = it.get("from", {}).get("hash")
            to_addr = it.get("to", {}).get("hash")
            tok = it.get("token", {}).get("symbol")
            val = it.get("total", {}).get("value")
            tx = it.get("transaction_hash")
            print(f"  From: {from_addr} -> To: {to_addr}")
            print(f"  Token: {tok} | Amount: {val} | Tx: {tx}")
            print("---")
except Exception as e:
    print("Err Blockscout transfers:", e)
