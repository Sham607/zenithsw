import urllib.request, json, time

txs = [
    "0x716e1004c4e8fc65fb92f2ec0bfbadc8663dca311b12312ac00f02ce329f5773",
    "0x4bada99d318c64539b4e56557af5588c4cd970fb00a3d745a96f5d953a27c88e",
    "0x0677aa1c4973f2881768b131bfff440833c7d162e174b4a2ed6b1c92f85715bf",
    "0xd5b2f0651773bbd649095444dd0c22c0322b7c110be29878c8188997d11e6209",
    "0x5df1a5ea193dc8a05861b23f231324a8281808600aa0a89d7348fc84deb609da",
    "0x04fe0c84f41e71853e8a3e725770dbc490ad585adf257597b594063b4d1e3249"
]

print("Tracing Real Signers who routed tokens into Unipcs RH Vault (0x2ac082...):\n")
for tx in txs:
    url = f"https://robinhoodchain.blockscout.com/api/v2/transactions/{tx}"
    headers = {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)",
        "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
    }
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            initiator = data.get("from", {}).get("hash")
            method = data.get("method")
            print(f"Tx {tx[:16]}... -> Signer: {initiator} | Method: {method}")
        time.sleep(0.3)
    except Exception as e:
        print(f"Err {tx[:10]}: {e}")
