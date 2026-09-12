import os
from pathlib import Path

def load_env_val(key, default=""):
    env_file = Path(__file__).resolve().parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.strip().startswith(f"{key}="):
                return line.strip().split("=", 1)[1].strip('"'')
    return os.getenv(key, default)

import urllib.request
import json

DATA_API_KEY = load_env_val("SOLANATRACKER_API_KEY")
RPC_URL = load_env_val("SOLANA_RPC_URL", "https://api.mainnet-beta.solana.com")

url = "https://data.solanatracker.io/top-traders/all"
req = urllib.request.Request(url, headers={"x-api-key": DATA_API_KEY, "User-Agent": "Mozilla/5.0"})

with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read().decode())

wallets = data.get("wallets", [])
print(f"Fetched {len(wallets)} traders from SolanaTracker Top-Traders (Cost: 1 API credit)\n")

# CEX & System Known Addresses / Signatures
KNOWN_CEX_PREFIXES = [
    "5tzFkiKscXHK5ZXCGbXZxdw7gTjjKAggm7Ygy2S58GyA", # Binance Hot Wallet
    "2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S", # Binance
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", # Binance
    "H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS", # Coinbase
    "2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPics", # Coinbase
    "AobVTdHnRyaVJvaVvf3NuLTW5gcqLnHuET9eBp6tqdKx", # Bybit
]

def check_sol_rpc_balance(addr):
    payload = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [addr]}).encode()
    req = urllib.request.Request(RPC_URL, data=payload, headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            res = json.loads(r.read().decode())
            return res.get("result", {}).get("value", 0) / 1e9
    except Exception:
        return 0.0

classified = []
for idx, w in enumerate(wallets):
    addr = w.get("wallet")
    sumry = w.get("summary", {})
    realized_usd = sumry.get("realized", 0.0)
    win_rate = sumry.get("winPercentage", 0.0)
    wins = sumry.get("totalWins", 0)
    losses = sumry.get("totalLosses", 0)
    avg_buy = sumry.get("averageBuyAmount", 0.0)

    # Check RPC balance using SolanaTracker free RPC (does NOT burn Data API credits)
    bal_sol = check_sol_rpc_balance(addr)

    # Anti-CEX heuristics
    is_cex = False
    cex_reason = ""
    if addr in KNOWN_CEX_PREFIXES:
        is_cex = True
        cex_reason = "KNOWN_CEX_HOT_WALLET"
    elif bal_sol > 100000.0:
        is_cex = True
        cex_reason = f"EXCESSIVE_CEX_LIQUIDITY ({bal_sol:,.0f} SOL)"
    elif wins + losses > 500000:
        is_cex = True
        cex_reason = "EXCHANGE_MARKET_MAKER_VOLUME"

    status = "CEX / BOT ❌" if is_cex else "ORGANIC WHALE 🟢"
    classified.append({
        "rank": idx + 1,
        "address": addr,
        "balance_sol": bal_sol,
        "realized_pnl": realized_usd,
        "win_rate": win_rate,
        "wins": wins,
        "losses": losses,
        "avg_buy": avg_buy,
        "is_cex": is_cex,
        "cex_reason": cex_reason,
        "status": status
    })

    print(f"#{idx+1:02d} [{status}] {addr[:8]}...{addr[-6:]} | Bal: {bal_sol:>8.1f} SOL | WinRate: {win_rate:>5.1f}% | PnL: ${realized_usd:>11,.0f} | AvgBuy: ${avg_buy:>6.0f} {cex_reason}")

print(f"\n--- AUDIT SUMMARY ---")
organic = [c for c in classified if not c["is_cex"]]
cex_bots = [c for c in classified if c["is_cex"]]
print(f"Organic Whale Traders: {len(organic)} / {len(classified)}")
print(f"CEX / System Wallets Flagged: {len(cex_bots)}")

with open("/home/node/Developments/zenithsw/data/solanatracker_harvest_sample.json", "w") as f:
    json.dump(classified, f, indent=2)
print("Saved sample to data/solanatracker_harvest_sample.json")
