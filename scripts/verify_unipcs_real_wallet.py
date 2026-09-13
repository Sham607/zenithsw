import urllib.request, json

url = "https://robinhoodchain.blockscout.com/api/v2/addresses/0x0a6EBEd0155EDB4b21D92AD02897A626CD90119E"
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
req = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    print("Unipcs Real Robinhood Wallet Details:")
    print("  Hash:", data.get("hash"))
    print("  Coin Balance:", float(data.get("coin_balance", 0)) / 1e18, "ETH")
    print("  Counters:", data.get("counters"))
