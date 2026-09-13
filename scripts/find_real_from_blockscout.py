import urllib.request, json

# Kita cari pemegang token PONS di Robinhood (Chain 4663)
# Di fomo_alltime_holdings.json:
# Unipcs memegang 10,957,904.67 token PONS (0x39dbed3a2bd333467115de45665cc57f813c4571) senilai $6.57M!
url = "https://robinhoodchain.blockscout.com/api/v2/tokens/0x39dbed3a2bd333467115de45665cc57f813c4571/holders"
headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}
req = urllib.request.Request(url, headers=headers)
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        items = data.get("items", [])
        print(f"Top Holders of PONS Token on Robinhood Chain:")
        for it in items[:8]:
            holder = it.get("address", {}).get("hash")
            val = float(it.get("value", 0)) / 1e18
            print(f"  Holder: {holder} | Balance: {val:,.2f} PONS")
except Exception as e:
    print("Err Blockscout holders:", e)
