import urllib.request, json

evm_addresses = [
    ("unipcs", "0x2ac082e252143c89c6e3bcb972e49855f9f6711c"),
    ("Salem1299534", "0xa14b38a3dfd05c089bae5d60e9d42af3924e5da0"),
    ("picadura", "0x155ed6e7524dd0bbdfb972b003dec52f876b0e1a"),
    ("DumbCrayonEater", "0xb48ae67b008443eaa5b5f20b7e9da4fb39f8e167")
]

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
    "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
}

for handle, addr in evm_addresses:
    url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{addr}"
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            d = json.loads(resp.read().decode("utf-8"))
            print(f"@{handle} on Robinhood Blockscout:")
            print("  Hash:", d.get("hash"))
            print("  Is Contract:", d.get("is_contract"))
            print("  Coin Balance:", d.get("coin_balance"))
            print("  Counters:", d.get("counters"))
    except Exception as e:
        print(f"@{handle} Err: {e}")
