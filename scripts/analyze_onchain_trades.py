import urllib.request
import json
import time

RPC = "https://api.mainnet-beta.solana.com"
PUBKEY = "bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk"

def rpc_call(method, params):
    req = urllib.request.Request(
        RPC,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode("utf-8"),
        headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode("utf-8"))["result"]

sigs = rpc_call("getSignaturesForAddress", [PUBKEY, {"limit": 100}])
sigs.reverse()

print("Analyzing on-chain transactions...")

buys = []
sells = []
other = []

for s in sigs:
    sig = s["signature"]
    if s.get("err"):
        continue
    tx = rpc_call("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
    if not tx:
        continue
    meta = tx.get("meta", {})
    fee = meta.get("fee", 0) / 1e9
    account_keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
    if PUBKEY not in account_keys:
        continue
    my_idx = account_keys.index(PUBKEY)
    pre = meta.get("preBalances", [])[my_idx] / 1e9
    post = meta.get("postBalances", [])[my_idx] / 1e9
    delta = post - pre

    # Look at token balance changes for PUBKEY
    pre_tokens = {tb["mint"]: float(tb["uiTokenAmount"]["uiAmount"] or 0)
                  for tb in meta.get("preTokenBalances", []) if tb.get("owner") == PUBKEY}
    post_tokens = {tb["mint"]: float(tb["uiTokenAmount"]["uiAmount"] or 0)
                   for tb in meta.get("postTokenBalances", []) if tb.get("owner") == PUBKEY}

    all_mints = set(pre_tokens.keys()).union(set(post_tokens.keys()))
    token_diffs = {}
    for m in all_mints:
        diff = post_tokens.get(m, 0) - pre_tokens.get(m, 0)
        if abs(diff) > 1e-6:
            token_diffs[m] = diff

    if delta < -0.005: # Outflow (likely Buy)
        buys.append((s.get("blockTime"), delta, fee, token_diffs, sig))
    elif delta > 0.005: # Inflow (likely Sell or Funding)
        sells.append((s.get("blockTime"), delta, fee, token_diffs, sig))
    else:
        other.append((s.get("blockTime"), delta, fee, sig))

print(f"Total Buys (SOL outflows > 0.005): {len(buys)} | Total SOL spent: {sum(b[1] for b in buys):.4f} SOL")
print(f"Total Sells/Inflows: {len(sells)} | Total SOL returned: {sum(s[1] for s in sells):.4f} SOL")
for idx, b in enumerate(buys):
    print(f"BUY #{idx+1}: {b[1]:.4f} SOL | Token: {list(b[3].keys())} | tx: {b[4][:12]}...")

for idx, s in enumerate(sells):
    print(f"SELL/INFLOW #{idx+1}: +{s[1]:.4f} SOL | Token: {list(s[3].keys())} | tx: {s[4][:12]}...")

