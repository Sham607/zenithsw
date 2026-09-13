import json
from collections import Counter

path = "/home/node/Developments/zenithsw/data/fomo_following_holdings_audit.json"
with open(path, encoding="utf-8") as f:
    data = json.load(f)
users = data.get("users", [])
swaps = [s for u in users for s in u.get("swaps", [])]
print("users", len(users))
print("statuses", dict(Counter(u.get("swaps_status") for u in users)))
print("total_swaps", len(swaps))
print("profile_wallet_fields", sorted({k for u in users for k in u if "address" in k}))
print("swap_keys", sorted({k for s in swaps for k in s.keys()}))
for u in users[:3]:
    print("USER", u.get("handle"), "profile_sol", u.get("profile_solana_address"), "profile_evm", u.get("profile_evm_address"))
    if u.get("swaps"):
        print("SWAP_SAMPLE", json.dumps(u["swaps"][0], ensure_ascii=False)[:1500])
