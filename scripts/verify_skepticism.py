import urllib.request
import json
import sqlite3

addrs = [
    ("unipcs", "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1"),
    ("DumbCrayonEater", "43nktK56Bfk3yJK9iqr3Kzyt9npgEB6sPYKAeoYMxeug"),
    ("pointfarmcap", "6cerGpSGzCrA1NeHUNtyoNm4rWQx5SnK3DSAXb4e615t"),
    ("Salem1299534", "CDY1JBoi2fKPPsHabyANmp6RP9Lkz8TcrkACsX3rHkka"),
    ("The__Solstice", "GmfY52CC5BKt43xt9GSmZWNzsAv4ywE6qgguRM5fvAtN"),
    ("theveeman", "EYveLtK4M3PsNnzH6zAGQgLnCGb2CqFqkKnEoNtRirDX"),
    ("ogle", "HvdahwUMiFStRDFkNi2VQVjkY6tvsi5FZ3bnBH5LSK7d"),
    ("frankdegods", "A5SEXYJY4jTEi6sjMLfZs5KAP8SVFvLDPDV67GgSSZSk"),
]

print("=== IN-DEPTH AUDIT OF HIGH-PROFILE QUARANTINED WALLETS ===")
for name, addr in addrs:
    req = urllib.request.Request("https://api.mainnet-beta.solana.com", data=json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress", "params": [addr, {"limit": 1000}]
    }).encode(), headers={"Content-Type": "application/json"})
    
    with urllib.request.urlopen(req) as resp:
        sigs = json.loads(resp.read().decode())["result"]
    
    # Check balance
    req_bal = urllib.request.Request("https://api.mainnet-beta.solana.com", data=json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "getBalance", "params": [addr]
    }).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req_bal) as resp:
        bal = json.loads(resp.read().decode())["result"]["value"] / 1e9

    print(f"Wallet @{name:<16} ({addr[:8]}...):")
    print(f"  - Total Lifetime Signatures: {len(sigs)}")
    print(f"  - Current SOL Balance: {bal:.6f} SOL")

    # Inspect first 3 transactions
    for idx, s in enumerate(sigs[:3]):
        sig = s["signature"]
        req_tx = urllib.request.Request("https://api.mainnet-beta.solana.com", data=json.dumps({
            "jsonrpc": "2.0", "id": 1, "method": "getTransaction", "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
        }).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req_tx) as resp:
            tx = json.loads(resp.read().decode()).get("result")
            if tx:
                meta = tx.get("meta", {})
                pre = meta.get("preBalances", [])
                post = meta.get("postBalances", [])
                keys = [k["pubkey"] if isinstance(k, dict) else k for k in tx["transaction"]["message"]["accountKeys"]]
                if addr in keys:
                    my_idx = keys.index(addr)
                    delta = (post[my_idx] - pre[my_idx]) / 1e9
                    print(f"    Tx #{idx+1}: {sig[:14]}... delta={delta:.6f} SOL | postTokenCount={len(meta.get('postTokenBalances', []))}")
                else:
                    print(f"    Tx #{idx+1}: {sig[:14]}... (read-only/lookup)")
