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
sigs.reverse() # chronological

total_tx = len(sigs)
failed_tx = sum(1 for s in sigs if s.get("err") is not None)
print(f"Total Transactions: {total_tx}, Failed: {failed_tx}")

total_fees = 0
funding_sol = 0
swaps_buy = 0
swaps_sell = 0
token_accounts_rent = 0

tx_details = []

for idx, s in enumerate(sigs):
    sig = s["signature"]
    # get tx
    tx = None
    for attempt in range(3):
        try:
            tx = rpc_call("getTransaction", [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}])
            break
        except Exception:
            time.sleep(0.5)
    if not tx:
        continue

    meta = tx.get("meta", {})
    fee = meta.get("fee", 0) / 1e9
    total_fees += fee
    err = meta.get("err")

    # Find balance change
    account_keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
    if PUBKEY in account_keys:
        my_idx = account_keys.index(PUBKEY)
        pre = meta.get("preBalances", [])[my_idx] / 1e9
        post = meta.get("postBalances", [])[my_idx] / 1e9
        delta = post - pre
    else:
        delta = 0

    block_time = s.get("blockTime")
    time_str = time.strftime("%Y-%m-%d %H:%M:%S", time.gmtime(block_time)) if block_time else "unknown"

    # Check if funding (received SOL without token swap)
    pre_tokens = meta.get("preTokenBalances", [])
    post_tokens = meta.get("postTokenBalances", [])

    is_swap = len(pre_tokens) > 0 or len(post_tokens) > 0
    if not is_swap and delta > 0.01:
        funding_sol += delta
        print(f"[{time_str}] FUNDING: +{delta:.6f} SOL | tx: {sig[:16]}...")
    elif is_swap:
        if delta < 0:
            swaps_buy += 1
        else:
            swaps_sell += 1

    tx_details.append({
        "time": time_str,
        "sig": sig,
        "delta": delta,
        "fee": fee,
        "err": err,
        "is_swap": is_swap
    })

print(f"\n--- SUMMARY ---")
print(f"Total Funding detected: {funding_sol:.6f} SOL")
print(f"Total Network Fees Paid: {total_fees:.6f} SOL")
print(f"Total Swaps: {swaps_buy + swaps_sell} (Buys/Outflows: {swaps_buy}, Sells/Inflows: {swaps_sell})")
print(f"Failed Transactions: {failed_tx}")

current_balance = rpc_call("getBalance", [PUBKEY])["value"] / 1e9
print(f"Current SOL Balance: {current_balance:.6f} SOL")

