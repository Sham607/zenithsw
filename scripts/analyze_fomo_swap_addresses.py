import json
from collections import Counter, defaultdict

path = "/home/node/Developments/zenithsw/data/fomo_following_holdings_audit.json"
with open(path, encoding="utf-8") as f:
    data = json.load(f)

rows = []
for user in data.get("users", []):
    profile = {user.get("profile_solana_address"), user.get("profile_evm_address")}
    for swap in user.get("swaps", []):
        rows.append({
            "handle": user.get("handle"),
            "profile_match": swap.get("address") in profile,
            "address": swap.get("address"),
            "network_id": swap.get("networkId"),
            "created_at": swap.get("createdAt"),
            "in_token": swap.get("inTokenAddress"),
            "out_token": swap.get("outTokenAddress"),
            "in_amount": swap.get("inHumanAmount"),
            "out_amount": swap.get("outHumanAmount"),
            "off_platform": swap.get("isOffPlatform"),
            "recipient": swap.get("recipient"),
        })

by_handle = defaultdict(set)
for row in rows:
    if row["address"]:
        by_handle[row["handle"]].add(row["address"])
print("total_swaps", len(rows))
print("unique_swap_addresses", len({r["address"] for r in rows if r["address"]}))
print("swap_address_equals_profile", sum(r["profile_match"] for r in rows))
print("off_platform", sum(bool(r["off_platform"]) for r in rows))
print("recipient_present", sum(bool(r["recipient"]) for r in rows))
print("network_ids", dict(Counter(r["network_id"] for r in rows)))
for handle, addresses in sorted(by_handle.items()):
    print(f"{handle}: {len(addresses)} swap address(es): {', '.join(sorted(addresses))}")
