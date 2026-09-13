import urllib.request
import json

DATA_API_KEY = "fc7e6533-da19-48bb-b80f-565a104d341f"
RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"
ADDR = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

# Check PnL on SolanaTracker API
print("Checking wallet on SolanaTracker PnL V2 API...")
url = f"https://data.solanatracker.io/v2/pnl/wallets/{ADDR}"
req = urllib.request.Request(url, headers={"x-api-key": DATA_API_KEY, "User-Agent": "Mozilla/5.0"})

try:
    with urllib.request.urlopen(req, timeout=10) as resp:
        data = json.loads(resp.read().decode())
        print("Summary PnL:", data.get("summary", {}).get("pnl"))
        print("Identity:", data.get("identity"))
        print("Analysis:", data.get("analysis"))
except Exception as e:
    print("SolanaTracker PnL error:", e)

# Also check top positions / holdings around 15.8M
with open("/home/node/Developments/zenithsw/data/unipcs_sol_audit.json") as f:
    audit = json.load(f)

tokens = audit.get("tokens", [])
print(f"\nLooking for 15.8M tokens in {len(tokens)} token accounts...")
matches = [t for t in tokens if 15_000_000 <= t[1] <= 16_500_000]
print(f"Exact range matches (~15.8M): {len(matches)}")
for m in matches:
    print(f"  Mint: {m[0]} -> Amount: {m[1]:,}")
    # check symbol via Dexscreener
    try:
        u = f"https://api.dexscreener.com/latest/dex/tokens/{m[0]}"
        r = urllib.request.Request(u, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(r, timeout=5) as res:
            dex = json.loads(res.read().decode())
            pairs = dex.get("pairs", [])
            if pairs:
                p = pairs[0]
                sym = p.get("baseToken", {}).get("symbol")
                price = float(p.get("priceUsd", 0) or 0)
                val = m[1] * price
                print(f"    Symbol: {sym} | Price: ${price:.6f} | Value: ${val:,.2f}")
    except Exception as e:
        print("    Dexscreener error:", e)
