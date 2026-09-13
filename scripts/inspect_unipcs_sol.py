import urllib.request
import json
import time

RPC_URL = "https://rpc-mainnet.solanatracker.io/?api_key=4d65775f-e88a-49f2-be37-3480ba522e76"
ADDR = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"

def rpc(method, params):
    req = urllib.request.Request(
        RPC_URL,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"}
    )
    with urllib.request.urlopen(req, timeout=10) as r:
        return json.loads(r.read().decode())["result"]

print(f"=== AUDITING SOLANA WALLET: {ADDR} ===")

# 1. Native Balance
bal = rpc("getBalance", [ADDR])["value"] / 1e9
print(f"Native Balance: {bal:.4f} SOL (${bal * 102:.2f} USD)")

# 2. Token Accounts (SPL Token)
tok_spl = rpc("getTokenAccountsByOwner", [ADDR, {"programId": "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}, {"encoding": "jsonParsed"}])
tok_2022 = rpc("getTokenAccountsByOwner", [ADDR, {"programId": "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"}, {"encoding": "jsonParsed"}])

tokens = []
for t in tok_spl.get("value", []) + tok_2022.get("value", []):
    info = t["account"]["data"]["parsed"]["info"]
    mint = info["mint"]
    amt = float(info["tokenAmount"]["uiAmount"] or 0)
    if amt > 0:
        tokens.append((mint, amt))

print(f"\nNon-zero Token Accounts Found: {len(tokens)}")
for mint, amt in sorted(tokens, key=lambda x: x[1], reverse=True)[:15]:
    print(f"  Mint: {mint} | Amount: {amt:,.2f}")

# 3. Signatures
sigs = rpc("getSignaturesForAddress", [ADDR, {"limit": 25}])
print(f"\nTotal Recent Signatures in Batch: {len(sigs)}")
if sigs:
    print(f"Latest Tx Signature: {sigs[0]['signature']}")
    bt = sigs[0].get("blockTime")
    time_str = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(bt)) if bt else "unknown"
    print(f"Latest Tx Time: {time_str}")

with open("/home/node/Developments/zenithsw/data/unipcs_sol_audit.json", "w") as f:
    json.dump({
        "address": ADDR,
        "balance_sol": bal,
        "tokens": tokens,
        "recent_signatures": sigs[:10]
    }, f, indent=2)
