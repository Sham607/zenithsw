import urllib.request, json, time

signers = [
    "0x62A1b859F37E3121F0CFA61bd6FA7B679391E334",
    "0xcCa77b8C0dF128b8A723E1C0C9De621bDf284576",
    "0xeC1bA2f851b54c55A224d73104934763F26ced21"
]

for s in signers:
    url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{s}/transactions"
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
    }
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            items = data.get("items", [])
            print(f"Signer {s} total txs fetched: {len(items)}")
            first_tx = items[-1] if items else {}
            print(f"  First TX from: {first_tx.get('from', {}).get('hash')} -> To: {first_tx.get('to', {}).get('hash')}")
            print(f"  Value: {first_tx.get('value')} | Time: {first_tx.get('timestamp')}")
        time.sleep(0.3)
    except Exception as e:
        print(f"Err {s}:", e)
