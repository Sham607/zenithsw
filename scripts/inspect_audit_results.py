import json

with open("/home/node/Developments/zenithsw/data/onchain_solana_audit_results.json") as f:
    data = json.load(f)

cands = data["qualified_candidates"]
print(f"Total Qualified: {len(cands)}")

print("\n--- TOP 25 QUALIFIED TRADERS BY SOL BALANCE ---")
for idx, c in enumerate(cands[:25]):
    h = c.get("handle", "N/A")
    addr = c.get("address", "N/A")
    tier = c.get("tier", "N/A")
    src = c.get("source", "N/A")
    bal = c.get("sol_balance", 0.0)
    days = c.get("days_inactive", 0.0)
    tx_s = c.get("max_tx_per_second", 0)
    print(f"#{idx+1:02d} @{h:<18} ({addr[:6]}...{addr[-4:]}) | Old: {tier:<6} | Src: {src:<20} | Bal: {bal:>8.2f} SOL | Last Active: {days:>4.1f}d | MaxTx/s: {tx_s}")

print("\n--- WHAT HAPPENED TO THE 87 TIER-S WALLETS? ---")
dead_tiers = {}
for w in data["dead_wallets_sample"] + data["disqualified_alive"]:
    t = w.get("tier")
    dead_tiers[t] = dead_tiers.get(t, 0) + 1

# Let's check all 87 Tier-S specifically
all_sol_s = [w for w in data["qualified_candidates"] if w["tier"] == "TIER-S"]
print(f"Surviving Tier-S: {len(all_sol_s)} / 87")
for s in all_sol_s:
    print(f"  - @{s['handle']} ({s['address'][:8]}...): {s['sol_balance']:.2f} SOL | Source: {s['source']}")
