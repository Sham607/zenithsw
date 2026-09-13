import urllib.request
import json
import time

RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"
DATA_API_KEY = "fc7e6533-da19-48bb-b80f-565a104d341f"

targets = [
    {
        "handle": "pointfarmcap",
        "address": "Beqv6dzTcjV2eodo8RRXCiCcnSYrS1vkQKhfqwHXqeit",
        "expected_tokens": ["6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx", "6V7cF52p3eSkRuoVXnAryWVwoyS775DERenYPMMYBPxS"]
    },
    {
        "handle": "The__Solstice",
        "address": "4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo",
        "expected_tokens": ["6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx"]
    },
    {
        "handle": "theveeman",
        "address": "F5hkYsi8JxjyA2JHN5CA7MbnnhWubkXB2ZQB7Gkaxqs6",
        "expected_tokens": ["6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx"]
    }
]

def rpc(method, params):
    req = urllib.request.Request(
        RPC_URL,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read().decode())["result"]

def get_dex_info(mint):
    try:
        url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=5) as resp:
            data = json.loads(resp.read().decode())
            pairs = data.get("pairs", [])
            if pairs:
                p = pairs[0]
                return {
                    "symbol": p.get("baseToken", {}).get("symbol", "???"),
                    "price_usd": float(p.get("priceUsd", 0) or 0),
                    "liquidity_usd": float(p.get("liquidity", {}).get("usd", 0) or 0),
                    "market_cap": float(p.get("marketCap", 0) or p.get("fdv", 0) or 0)
                }
    except Exception:
        pass
    return {"symbol": "UNKNOWN", "price_usd": 0.0, "liquidity_usd": 0.0, "market_cap": 0.0}

results = []

for t in targets:
    h = t["handle"]
    addr = t["address"]
    print(f"\n==================================================================")
    print(f"🕵️ AUDITING: @{h} ({addr})")
    print(f"==================================================================")

    # 1. Native Balance
    bal_sol = rpc("getBalance", [addr])["value"] / 1e9
    print(f"1. Native Balance: {bal_sol:.4f} SOL (${bal_sol * 102:,.2f} USD)")

    # 2. Token Holdings
    spl_accs = rpc("getTokenAccountsByOwner", [addr, {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}, {"encoding": "jsonParsed"}])
    spl_22 = rpc("getTokenAccountsByOwner", [addr, {"programId": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"}, {"encoding": "jsonParsed"}])

    token_list = []
    total_token_val_usd = 0.0

    for item in spl_accs.get("value", []) + spl_22.get("value", []):
        info = item["account"]["data"]["parsed"]["info"]
        m = info["mint"]
        amt = float(info["tokenAmount"]["uiAmount"] or 0)
        if amt > 0:
            token_list.append({"mint": m, "amount": amt})

    print(f"2. Non-Zero Token Accounts Held: {len(token_list)}")

    # Sort tokens by expected or large amount
    enriched_tokens = []
    for tok in sorted(token_list, key=lambda x: x["amount"], reverse=True)[:10]:
        dex = get_dex_info(tok["mint"])
        val_usd = tok["amount"] * dex["price_usd"]
        total_token_val_usd += val_usd
        enriched_tokens.append({
            "mint": tok["mint"],
            "symbol": dex["symbol"],
            "amount": tok["amount"],
            "price_usd": dex["price_usd"],
            "value_usd": val_usd,
            "liquidity_usd": dex["liquidity_usd"]
        })
        print(f"   • {dex['symbol']:<10} | Qty: {tok['amount']:>16,.2f} | Price: ${dex['price_usd']:>9.6f} | Value: ${val_usd:>12,.2f} ({tok['mint'][:8]}...)")

    # 3. Check SolanaTracker PnL V2 Identity & Stats
    print(f"3. SolanaTracker PnL V2 Intelligence:")
    st_data = {}
    try:
        u_st = f"https://data.solanatracker.io/v2/pnl/wallets/{addr}"
        r_st = urllib.request.Request(u_st, headers={"x-api-key": DATA_API_KEY, "User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(r_st, timeout=8) as resp_st:
            st_data = json.loads(resp_st.read().decode())
            ident = st_data.get("identity")
            pnl_sumry = st_data.get("summary", {}).get("pnl", {})
            analysis = st_data.get("analysis", {})
            counts = st_data.get("summary", {}).get("counts", {})
            timing = st_data.get("summary", {}).get("timing", {})
            print(f"   Identity: {ident}")
            print(f"   Realized PnL: ${pnl_sumry.get('realized', 0):,.2f} | Unrealized: ${pnl_sumry.get('unrealized', 0):,.2f} | Total: ${pnl_sumry.get('total', 0):,.2f}")
            print(f"   Win Rate: {analysis.get('winRate', 0)}% | Trades: {counts.get('trades', 0)} (Buys: {counts.get('buys', 0)}, Sells: {counts.get('sells', 0)})")
            print(f"   Avg Hold Time: {timing.get('avgHoldTimeSecs', 0) / 60:.1f} minutes")
    except Exception as e:
        print(f"   SolanaTracker query error: {e}")

    # 4. Signatures & On-Chain Role Check
    print(f"4. On-Chain Role & Transaction Forensics:")
    sigs = rpc("getSignaturesForAddress", [addr, {"limit": 30}])
    print(f"   Recent Signatures Returned: {len(sigs)}")
    is_signer_count = 0
    programs_used = set()

    for s in sigs[:10]:
        sig = s["signature"]
        tx = rpc("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
        if not tx:
            continue
        keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
        if keys and keys[0] == addr:
            is_signer_count += 1
        for ix in tx["transaction"]["message"]["instructions"]:
            p = ix.get("program") or ix.get("programId")
            if p:
                programs_used.add(p)

    latest_time_str = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(sigs[0]["blockTime"])) if sigs and sigs[0].get("blockTime") else "N/A"
    print(f"   Latest Activity: {latest_time_str}")
    print(f"   Transactions as Fee Payer/Signer: {is_signer_count} / 10")
    print(f"   Programs Interacted With: {list(programs_used)[:6]}")

    results.append({
        "handle": h,
        "address": addr,
        "balance_sol": bal_sol,
        "tokens_held_count": len(token_list),
        "top_holdings": enriched_tokens,
        "solanatracker": st_data,
        "latest_activity": latest_time_str,
        "is_signer_count": is_signer_count,
        "programs_used": list(programs_used)
    })

# Save report
out_path = "/home/node/Developments/zenithsw/data/three_fomo_whales_audit.json"
with open(out_path, "w") as f:
    json.dump(results, f, indent=2)

print(f"\n[DONE ✅] Comprehensive report saved to: {out_path}")
