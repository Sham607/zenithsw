import urllib.request
import json

RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"
ADDR = "4tWHFGC6iwzZm2VZaHZEiyvKMKjBArK424vMejgUUvTp"

def rpc(method, params):
    req = urllib.request.Request(
        RPC_URL,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read().decode())["result"]

# 1. Check account info (is it a system account or program?)
info = rpc("getAccountInfo", [ADDR, {"encoding": "jsonParsed"}])
print("Account Info:")
print("  Owner Program:", info.get("value", {}).get("owner"))
print("  Executable:", info.get("value", {}).get("executable"))
print("  Lamports:", info.get("value", {}).get("lamports"))

# 2. Check token accounts
toks = rpc("getTokenAccountsByOwner", [ADDR, {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}, {"encoding": "jsonParsed"}])
print(f"\nToken Accounts count: {len(toks.get('value', []))}")
for t in toks.get("value", []):
    parsed = t["account"]["data"]["parsed"]["info"]
    amt = parsed["tokenAmount"]["uiAmount"]
    if amt and amt > 0:
        print(f"  Mint: {parsed['mint'][:12]}... | Amount: {amt}")

# 3. Check recent signatures and transaction details
sigs = rpc("getSignaturesForAddress", [ADDR, {"limit": 10}])
print(f"\nRecent Signatures ({len(sigs)}):")
for idx, s in enumerate(sigs[:5]):
    sig = s["signature"]
    tx = rpc("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
    if not tx:
        continue
    keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
    ixs = tx["transaction"]["message"]["instructions"]
    types = [ix.get("parsed", {}).get("type") or ix.get("programId") for ix in ixs]
    fee_payer = keys[0] if keys else "unknown"
    is_signer = ADDR in keys and keys.index(ADDR) == 0
    print(f"  Tx #{idx+1}: {sig[:16]}... | Payer: {fee_payer[:10]}... | IsSigner: {is_signer} | Instructions: {types}")
