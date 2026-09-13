import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"

# Dua wallet pengirim utama dengan token jutaan
candidates = [
    ("9kVcp55TnJGqS7VHC3rziDkdmiVnPnBJckPhjNGSt2NW", "5pAUGrxzF7eo21XrBjWT3x7QLgTHoBPJyY6q11qih6rn", 719009.33),
    ("BSM2yepG35TX7EQbpdHr4WgnP6nmpVXaXKFYmeatQcEJ", "EodgqTJiS2ni2ehoVKz1cLqiMB3oKLXMCHvjoSixjcKe", 2000000.0)
]

for wallet, mint, amt in candidates:
    print(f"=== Inspecting Satellite Wallet: {wallet} ===")

    # 1. Fetch Token Info from Dexscreener
    url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            pairs = data.get("pairs", [])
            if pairs:
                p = pairs[0]
                sym = p.get("baseToken", {}).get("symbol")
                price = p.get("priceUsd")
                usd_val = float(price or 0) * amt
                print(f"  Token: {sym} | Price: ${price} | Transfer Value: ~${usd_val:,.2f}")
    except Exception as e:
        print("  Dex err:", e)

    # 2. Fetch Recent Signatures of this satellite
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [wallet, {"limit": 5}]
    }
    req2 = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req2) as resp2:
        sigs = json.loads(resp2.read().decode("utf-8")).get("result", [])
        print(f"  Total recent txs: {len(sigs)}")
        for s in sigs[:3]:
            print(f"    Tx: {s['signature'][:20]}... | Time: {s.get('blockTime')}")
    print()
