import urllib.request
import urllib.error
import json
import csv
import os
import time

token = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR2bW1oYjIwMzhuMGNsMTk5NHR5eXJlIiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkxODkyMDEsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dm1taHFqMDM4bzBjbDFhYzV4dTlueiIsImV4cCI6MTc4OTE5MjgwMX0.rXZ2Kkvapysx5bEI4mTRp2ZKWRPrTyMhr9g6mia-jqEi4Ai12NVcPKbt6qgN_F8bVpjcg3bbiM3EE9AWGp83Zw"

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36",
    "Accept": "*/*",
    "Accept-Language": "en-US,en;q=0.9,id;q=0.8",
    "App-Language": "en",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/",
    "privy-app-id": "cm6h485v300n3zj9yl6vpedq7",
    "privy-token": token,
    "privy-id-token": token,
    "privy-session": "t",
    "Cookie": f"privy-session=t; privy-token={token}; privy-id-token={token}",
    "Authorization": f"Bearer {token}",
}

url = "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=ALL"
print(f"[{time.strftime('%H:%M:%S')}] Executing gentle single GET request to FOMO All-Time Leaderboard...")

req = urllib.request.Request(url, headers=headers)
with urllib.request.urlopen(req, timeout=12) as resp:
    raw = resp.read().decode()
    data = json.loads(raw)

lb = data.get("responseObject", {}).get("leaderboard", [])
print(f"[{time.strftime('%H:%M:%S')}] Received {len(lb)} Top All-Time Traders!")

# Network map
NETWORK_NAMES = {
    1399811149: "Solana",
    4663: "Robinhood",
    8453: "Base",
    1: "Ethereum",
    42161: "Arbitrum",
    56: "BNB/BSC",
}

out_records = []
flattened_holdings = []

for idx, t in enumerate(lb):
    rank = idx + 1
    uid = t.get("id")
    handle = t.get("userHandle") or t.get("displayName") or uid
    disp = t.get("displayName") or handle
    pnl = float(t.get("totalPnL") or 0.0)
    sol_addr = t.get("address") or ""
    evm_addr = t.get("evmAddress") or ""
    num_trades = int(t.get("numTrades") or 0)
    swaps = int(t.get("swapCount") or 0)
    total_vol = float(t.get("totalVolume") or 0.0)
    holdings = t.get("topHoldings") or []

    trader_info = {
        "rank": rank,
        "id": uid,
        "handle": handle,
        "display_name": disp,
        "total_pnl_usd": pnl,
        "solana_address": sol_addr,
        "evm_address": evm_addr,
        "num_trades": num_trades,
        "swap_count": swaps,
        "total_volume_usd": total_vol,
        "top_holdings": []
    }

    for h in holdings:
        token_addr = h.get("tokenAddress") or h.get("address") or ""
        net_id = h.get("networkId")
        chain_name = NETWORK_NAMES.get(net_id, "Solana" if len(token_addr) > 40 and not token_addr.startswith("0x") else "EVM")

        amount = float(h.get("humanAmount") or h.get("amount") or 0.0)
        price = float(h.get("price") or 0.0)
        value_usd = float(h.get("value") or 0.0)
        pnl_val = float(h.get("pnl") or 0.0)

        h_record = {
            "token_address": token_addr,
            "network_id": net_id,
            "chain": chain_name,
            "amount": amount,
            "price_usd": price,
            "value_usd": value_usd,
            "unrealized_pnl_usd": pnl_val
        }
        trader_info["top_holdings"].append(h_record)

        flattened_holdings.append({
            "rank": rank,
            "trader_handle": handle,
            "trader_display_name": disp,
            "trader_alltime_pnl_usd": pnl,
            "chain": chain_name,
            "network_id": net_id,
            "token_mint_address": token_addr,
            "token_amount": amount,
            "token_price_usd": price,
            "holding_value_usd": value_usd,
            "unrealized_pnl_usd": pnl_val,
            "trader_solana_addr": sol_addr,
            "trader_evm_addr": evm_addr
        })

    out_records.append(trader_info)

out_dir = "/home/node/Developments/zenithsw/data"
json_path = os.path.join(out_dir, "fomo_alltime_holdings.json")
csv_path = os.path.join(out_dir, "fomo_alltime_holdings.csv")

with open(json_path, "w") as f:
    json.dump(out_records, f, indent=2)

if flattened_holdings:
    with open(csv_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=flattened_holdings[0].keys())
        writer.writeheader()
        writer.writerows(flattened_holdings)

print(f"\n[SUKSES 🚀] Tersimpan {len(out_records)} traders dan {len(flattened_holdings)} token holdings:")
print(f" - JSON: {json_path}")
print(f" - CSV:  {csv_path}")

print("\n=== TOP 15 ALL-TIME TRADERS WITH EXACT HOLDING QUANTITIES ===")
for t in out_records[:15]:
    print(f"\n#{t['rank']:02d} @{t['handle']:<18} | All-Time PnL: ${t['total_pnl_usd']:>12,.2f} | Trades: {t['num_trades']}")
    for h in t["top_holdings"]:
        amt = h["amount"]
        val = h["value_usd"]
        p = h["price_usd"]
        ch = h["chain"]
        m = h["token_address"]
        m_short = f"{m[:8]}...{m[-6:]}" if len(m) > 16 else m
        print(f"    • [{ch:<9}] Qty: {amt:>14,.2f} token | Price: ${p:>9.6f} | Val: ${val:>11,.2f} | Contract: {m_short}")
