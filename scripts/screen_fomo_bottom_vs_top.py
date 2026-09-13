import urllib.request
import urllib.error
import json
import time

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwNDQxODUsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTA0Nzc4NX0.0JdOEwAYN2B4lOvC0WYOuhzCi40bVNv_I-To30DykiMRU5f8OfrlRw5tf_snTKAJje_btcKmqv6yLwBv2zxsKQ"

headers = {
    "Authorization": f"Bearer {bearer}",
    "privy-token": bearer,
    "privy-app-id": "cm6h485o300n3zj9yl6vpedq7",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/",
    "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
}

url = "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d"
req = urllib.request.Request(url, headers=headers)
resp = urllib.request.urlopen(req, timeout=10)
data = json.loads(resp.read().decode())
lb = data.get("responseObject", {}).get("leaderboard", [])[:50]

print(f"Analyzing Top {len(lb)} 7D Whales on FOMO.family...\n")

token_whale_map = {} # token_addr -> list of {whale, value, pnl, network}

for t in lb:
    handle = t.get("userHandle") or "unknown"
    pnl_7d = float(t.get("totalPnL") or 0.0)
    top_holdings = t.get("topHoldings", [])

    for h in top_holdings:
        token_addr = h.get("tokenAddress")
        val = float(h.get("value") or 0.0)
        pnl = float(h.get("pnl") or 0.0)
        net_id = h.get("networkId")

        if token_addr and val > 10000.0: # Filter holdings > $10k
            if token_addr not in token_whale_map:
                token_whale_map[token_addr] = []
            token_whale_map[token_addr].append({
                "whale": handle,
                "whale_7d_pnl": pnl_7d,
                "holding_value": val,
                "position_pnl": pnl,
                "network_id": net_id
            })

print(f"Discovered {len(token_whale_map)} Unique High-Conviction Tokens held by Top 50 Whales.\n")

# Rank tokens by total whale value and holder count
ranked_tokens = sorted(
    token_whale_map.items(),
    key=lambda item: (len(item[1]), sum(x["holding_value"] for x in item[1])),
    reverse=True
)

for token_addr, holders in ranked_tokens[:15]:
    total_val = sum(x["holding_value"] for x in holders)
    whales_list = ", ".join([f"@{x['whale']} (${x['holding_value']:,.0f})" for x in holders[:3]])

    # Query DexScreener for Market Structure (Bottom vs Pucuk)
    dex_url = f"https://api.dexscreener.com/latest/dex/tokens/{token_addr}"
    dex_req = urllib.request.Request(dex_url, headers={"User-Agent": "Mozilla/5.0"})

    sym = "UNKNOWN"
    price = 0.0
    liq = 0.0
    mcap = 0.0
    chg_24h = 0.0
    chg_6h = 0.0
    chg_1h = 0.0
    status_tag = "UNKNOWN"

    try:
        with urllib.request.urlopen(dex_req, timeout=5) as dresp:
            ddata = json.loads(dresp.read().decode())
            pairs = ddata.get("pairs", [])
            if pairs:
                p = pairs[0]
                sym = p.get("baseToken", {}).get("symbol", "UNKNOWN")
                price = float(p.get("priceUsd") or 0.0)
                liq = float(p.get("liquidity", {}).get("usd") or 0.0)
                mcap = float(p.get("marketCap") or p.get("fdv") or 0.0)
                chg_24h = float(p.get("priceChange", {}).get("h24") or 0.0)
                chg_6h = float(p.get("priceChange", {}).get("h6") or 0.0)
                chg_1h = float(p.get("priceChange", {}).get("h1") or 0.0)

                # Bottom vs Pucuk Decision Engine
                if chg_24h > 150.0 or chg_6h > 50.0:
                    status_tag = "🔴 LOCAL_TOP / PUCUK (Wait for Pullback)"
                elif -30.0 <= chg_24h <= 25.0 and liq >= 20000.0:
                    status_tag = "🟢 ACCUMULATION_SUPPORT / BOTTOM (Ready to Buy)"
                elif chg_24h < -30.0:
                    status_tag = "🟡 DUMP_RECOVERY / OVERSOLD (High Risk Bottom)"
                else:
                    status_tag = "⚪ NEUTRAL_EXPANSION"
    except Exception as e:
        status_tag = f"API_ERROR ({e})"

    print(f"Token: {sym} ({token_addr[:10]}...{token_addr[-6:]})")
    print(f"  • Whales ({len(holders)} holders): {whales_list} | Total USD: ${total_val:,.0f}")
    print(f"  • Price: ${price:.6f} | Liq: ${liq:,.0f} | MCap: ${mcap:,.0f}")
    print(f"  • Momentum 24h: {chg_24h:+.1f}% | 6h: {chg_6h:+.1f}% | 1h: {chg_1h:+.1f}%")
    print(f"  • Status: {status_tag}\n")
    time.sleep(0.5)
