import urllib.request, json, time, sqlite3

api_key = 'def963d9-3753-4fe1-a609-f0534ead614f'
rpc_url = f'https://mainnet.helius-rpc.com/?api-key={api_key}'

# 1. Fetch top boosted tokens
req = urllib.request.Request('https://api.dexscreener.com/token-boosts/top/v1', headers={'User-Agent': 'Mozilla/5.0'})
tokens = []
with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read().decode())
    for item in data:
        if item.get('chainId') == 'solana':
            tokens.append(item.get('tokenAddress'))

print(f"Discovered {len(tokens)} trending Solana tokens from DexScreener...")

# Known system programs to ignore
known_programs = {
    "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",
    "srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX",
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",
    "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",
    "11111111111111111111111111111111",
    "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    "So11111111111111111111111111111111111111112",
    "ComputeBudget111111111111111111111111111111"
}

discovered_wallets = []

for mint in tokens[:4]:
    print(f"Indexing token mint: {mint}")
    time.sleep(0.3)
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [mint, {"limit": 10}]
    }
    req2 = urllib.request.Request(rpc_url, data=json.dumps(payload).encode('utf-8'), headers={'Content-Type': 'application/json', 'User-Agent': 'Mozilla/5.0'})
    try:
        with urllib.request.urlopen(req2, timeout=10) as resp2:
            res2 = json.loads(resp2.read().decode())
            sigs = res2.get('result', [])
            for s in sigs[:3]:
                sig = s.get('signature')
                time.sleep(0.2)
                tx_payload = {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getTransaction",
                    "params": [sig, {"encoding": "jsonParsed", "maxSupportedTransactionVersion": 0}]
                }
                tx_req = urllib.request.Request(rpc_url, data=json.dumps(tx_payload).encode('utf-8'), headers={'Content-Type': 'application/json'})
                with urllib.request.urlopen(tx_req, timeout=10) as tx_resp:
                    tx_data = json.loads(tx_resp.read().decode()).get('result', {})
                    msg = tx_data.get('transaction', {}).get('message', {})
                    keys = msg.get('accountKeys', [])
                    for k in keys:
                        pk = k.get('pubkey') if isinstance(k, dict) else k
                        is_signer = k.get('signer', False) if isinstance(k, dict) else False
                        if pk and is_signer and pk not in known_programs:
                            print(f"  -> Discovered Active On-Chain Trader: {pk}")
                            discovered_wallets.append(pk)
                            break
    except Exception as e:
        print(f"  Error indexing {mint}: {e}")

print(f"Total discovered unique active smart traders: {len(set(discovered_wallets))}")
