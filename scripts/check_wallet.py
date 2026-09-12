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

# 1. Check SPL token accounts
tokens = rpc_call("getTokenAccountsByOwner", [
    PUBKEY,
    {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},
    {"encoding": "jsonParsed"}
])

print(f"=== SPL TOKEN ACCOUNTS: {len(tokens.get('value', []))} ===")
for t in tokens.get("value", []):
    info = t["account"]["data"]["parsed"]["info"]
    mint = info["mint"]
    amount = info["tokenAmount"]["uiAmount"]
    decimals = info["tokenAmount"]["decimals"]
    if amount > 0:
        print(f"Mint: {mint} | Amount: {amount}")

# Also check Token-2022
tokens22 = rpc_call("getTokenAccountsByOwner", [
    PUBKEY,
    {"programId": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"},
    {"encoding": "jsonParsed"}
])
print(f"=== TOKEN-2022 ACCOUNTS: {len(tokens22.get('value', []))} ===")
for t in tokens22.get("value", []):
    info = t["account"]["data"]["parsed"]["info"]
    mint = info["mint"]
    amount = info["tokenAmount"]["uiAmount"]
    if amount > 0:
        print(f"Token-2022 Mint: {mint} | Amount: {amount}")

# 2. Get all signatures
sigs = rpc_call("getSignaturesForAddress", [PUBKEY, {"limit": 100}])
print(f"\n=== TOTAL TX SIGNATURES: {len(sigs)} ===")
sigs.reverse() # chronological order

total_fees = 0
inflows = 0
outflows = 0

# Sample some transactions
print("\nFirst tx (funding?):")
first_tx = rpc_call("getTransaction", [sigs[0]["signature"], {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
if first_tx:
    meta = first_tx.get("meta", {})
    fee = meta.get("fee", 0) / 1e9
    pre = meta.get("preBalances", [])
    post = meta.get("postBalances", [])
    keys = [k["pubkey"] if isinstance(k, dict) else k for k in first_tx["transaction"]["message"]["accountKeys"]]
    my_idx = keys.index(PUBKEY) if PUBKEY in keys else -1
    if my_idx >= 0:
        diff = (post[my_idx] - pre[my_idx]) / 1e9
        print(f"First tx change for wallet: {diff:.6f} SOL (pre: {pre[my_idx]/1e9:.6f}, post: {post[my_idx]/1e9:.6f})")

