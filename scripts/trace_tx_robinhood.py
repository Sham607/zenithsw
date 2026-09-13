import urllib.request, json

tx_hash = "0x716e1004c4e8fc65fb92f2ec0bfbadc8663dca311b12312ac00f02ce329f5773"
url = f"https://robinhoodchain.blockscout.com/api/v2/transactions/{tx_hash}"
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
req = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    print("Tx From (Signer/Initiator):", data.get("from", {}).get("hash"))
    print("Tx To:", data.get("to", {}).get("hash"))
    print("Value:", data.get("value"))
    print("Method / Type:", data.get("method"))
