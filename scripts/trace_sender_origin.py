import urllib.request, json

# Ambil sample pengirim token ke Robinhood Unipcs:
# 0xD8350Af75c144c95bfead7A8A66996353E734004 (Token VIRUS)
# 0x7277eBDEFdc4B71c4d584F734f0f1A2c7628F064 (Token WIN98)

senders = [
    "0xD8350Af75c144c95bfead7A8A66996353E734004",
    "0x7277eBDEFdc4B71c4d584F734f0f1A2c7628F064",
    "0xB1E0E65a27509ae2f11feb284FE4c006Edf39184"
]

for s in senders:
    url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{s}"
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
    }
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            print(f"Address {s}:")
            print("  Is Contract:", data.get("is_contract"))
            print("  Coin Balance:", data.get("coin_balance"))
            print("  Counters:", data.get("counters"))
    except Exception as e:
        print(f"Err {s}:", e)
