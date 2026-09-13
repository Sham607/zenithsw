import urllib.request, json, sqlite3, time

DB_PATH = "/home/node/Developments/zenithsw/data/smart_wallets.db"
conn = sqlite3.connect(DB_PATH)
cur = conn.cursor()

# Ambil 10 whale Tier-S Solana non-unipcs dengan PnL tertinggi
cur.execute("""
    SELECT handle, address, pnl_24h_usd
    FROM wallets
    WHERE chain='SOLANA' AND is_active=1 AND tier='TIER-S' AND handle NOT LIKE '%unipcs%'
    ORDER BY pnl_24h_usd DESC LIMIT 10
""")
whales = cur.fetchall()
conn.close()

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"

print("Auditing Inbound Token Transfers for Top 10 Solana Tier-S Whales:\n")

for handle, vault, pnl in whales:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [vault, {"limit": 10}]
    }
    req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            sigs = json.loads(resp.read().decode("utf-8")).get("result", [])

        inbound_count = 0
        satellites = set()

        for s in sigs[:5]:
            sig = s["signature"]
            tx_payload = {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "getTransaction",
                "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
            }
            tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
            try:
                with urllib.request.urlopen(tx_req) as tx_resp:
                    tx = json.loads(tx_resp.read().decode("utf-8")).get("result")
                    if not tx:
                        continue
                    meta = tx.get("meta", {})
                    pre = meta.get("preTokenBalances", [])
                    post = meta.get("postTokenBalances", [])
                    for p in post:
                        if p.get("owner") == vault:
                            mint = p.get("mint")
                            post_amt = (p.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                            pre_amt = 0.0
                            for pr in pre:
                                if pr.get("owner") == vault and pr.get("mint") == mint:
                                    pre_amt = (pr.get("uiTokenAmount") or {}).get("uiAmount") or 0.0
                                    break
                            if post_amt > pre_amt:
                                for pr in pre:
                                    if pr.get("mint") == mint and pr.get("owner") != vault:
                                        p_owner = pr.get("owner")
                                        if p_owner and len(p_owner) > 30 and not p_owner.startswith("1111"):
                                            satellites.add(p_owner)
                                            inbound_count += 1
                time.sleep(0.15)
            except Exception:
                pass

        print(f"@{handle} ({vault[:6]}...{vault[-4:]}) | PnL: +${pnl:,.0f}")
        print(f"  Sample Txs Checked: {len(sigs[:5])} | Inbound Token Transfers Detected: {inbound_count}")
        if satellites:
            print(f"  ⚠️ DETECTED SATELLITES: {list(satellites)[:3]}")
        else:
            print(f"  ✅ Normal direct trader (no satellite deposit in sample)")
        print("-" * 50)
    except Exception as e:
        print(f"Err @{handle}: {e}")
