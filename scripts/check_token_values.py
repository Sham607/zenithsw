import urllib.request
import json

PUBKEY = "bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk"

# Token accounts identified earlier:
# 5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6 | Amount: 294.75773
# MukLDtJ8Cx9DxLbeyLRSWPSposTMWuwHANbuaudpump | Amount: 412.174795
# 4tkupXEGfbYPCVN37CagviJd3tgWnudT9avKZvouT3Dm | Amount: 2195.271184565
# 7LxC96Ag4DnBotK6bMMUj4kdneAo1kdV1xHnzVF5s7W2 | Amount: 1762.260538
# Aw6fiDPWLUnjSsJQtsyEMSaoPaKAUUrStAsYPiPwpump | Amount: 39607.396514
# DrHDDvxdmLEpb4d1DiCgiAnLNATiw2MC7EEuiyqfHGpg | Amount: 10806.197368
# HunmXDXMNQYVoDnUL6PNnSYEtFaTZA2WzGh1HJTW7aoV | Amount: 1619.540185

mints = [
    ("5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6", 294.75773),
    ("MukLDtJ8Cx9DxLbeyLRSWPSposTMWuwHANbuaudpump", 412.174795),
    ("4tkupXEGfbYPCVN37CagviJd3tgWnudT9avKZvouT3Dm", 2195.271184565),
    ("7LxC96Ag4DnBotK6bMMUj4kdneAo1kdV1xHnzVF5s7W2", 1762.260538),
    ("Aw6fiDPWLUnjSsJQtsyEMSaoPaKAUUrStAsYPiPwpump", 39607.396514),
    ("DrHDDvxdmLEpb4d1DiCgiAnLNATiw2MC7EEuiyqfHGpg", 10806.197368),
    ("HunmXDXMNQYVoDnUL6PNnSYEtFaTZA2WzGh1HJTW7aoV", 1619.540185),
]

print("Checking token values via DexScreener...")
total_token_usd = 0
for mint, amt in mints:
    try:
        url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read().decode())
            pairs = data.get("pairs", [])
            if pairs:
                p0 = pairs[0]
                symbol = p0.get("baseToken", {}).get("symbol", "???")
                price = float(p0.get("priceUsd", 0) or 0)
                val = amt * price
                total_token_usd += val
                print(f"{symbol} ({mint[:8]}...): {amt:.2f} tokens @ ${price:.6f} = ${val:.3f}")
            else:
                print(f"Unknown ({mint[:8]}...): {amt:.2f} tokens (no pair found)")
    except Exception as e:
        print(f"Error fetching {mint[:8]}: {e}")

print(f"\nTotal Token Value: ${total_token_usd:.2f}")
