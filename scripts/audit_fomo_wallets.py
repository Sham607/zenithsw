import requests
import json
import time

helius_url = 'https://mainnet.helius-rpc.com/?api-key=def963d9-3753-4fe1-a609-f0534ead614f'
arb_url = 'https://arb-mainnet.g.alchemy.com/v2/alch_PS21nEmHejPxYhypdUH1A'

wallets = json.load(open('/home/node/Developments/zenithsw/data/verified_smart_wallets.json'))
print(f"Checking {len(wallets)} FOMO seed wallets on-chain...")

results = []
for w in wallets:
    addr = w['address']
    chain = w['chain']
    handle = w['handle']
    claimed_pnl = w.get('pnl_24h_usd', 0.0)

    if chain == 'SOLANA':
        payload_bal = {'jsonrpc': '2.0', 'id': 1, 'method': 'getBalance', 'params': [addr]}
        try:
            r = requests.post(helius_url, json=payload_bal, timeout=5).json()
            lamports = r.get('result', {}).get('value', 0)
        except Exception:
            lamports = 0
        sol_bal = lamports / 1e9

        payload_sig = {'jsonrpc': '2.0', 'id': 2, 'method': 'getSignaturesForAddress', 'params': [addr, {'limit': 100}]}
        try:
            r_sig = requests.post(helius_url, json=payload_sig, timeout=5).json()
            sigs = r_sig.get('result', [])
        except Exception:
            sigs = []
        sig_count = len(sigs)
        last_time = sigs[0].get('blockTime', 0) if sigs else 0

        results.append({
            'handle': handle,
            'name': w.get('name', handle),
            'chain': chain,
            'address': addr,
            'claimed_pnl': claimed_pnl,
            'real_bal_native': sol_bal,
            'native_sym': 'SOL',
            'real_bal_usd': sol_bal * 140.0,
            'recent_tx_count': sig_count,
            'last_tx_ts': last_time
        })
    else:
        payload_bal = {'jsonrpc': '2.0', 'id': 1, 'method': 'eth_getBalance', 'params': [addr, 'latest']}
        try:
            r = requests.post(arb_url, json=payload_bal, timeout=5).json()
            hex_val = r.get('result', '0x0')
            eth_bal = int(hex_val, 16) / 1e18
        except Exception:
            eth_bal = 0.0

        payload_tc = {'jsonrpc': '2.0', 'id': 2, 'method': 'eth_getTransactionCount', 'params': [addr, 'latest']}
        try:
            r_tc = requests.post(arb_url, json=payload_tc, timeout=5).json()
            tc = int(r_tc.get('result', '0x0'), 16)
        except Exception:
            tc = 0

        results.append({
            'handle': handle,
            'name': w.get('name', handle),
            'chain': chain,
            'address': addr,
            'claimed_pnl': claimed_pnl,
            'real_bal_native': eth_bal,
            'native_sym': 'ETH',
            'real_bal_usd': eth_bal * 2500.0,
            'recent_tx_count': tc,
            'last_tx_ts': 0
        })
    time.sleep(0.1)

print("\n--- ON-CHAIN AUDIT REPORT FOR FOMO SEED WALLETS ---")
for res in results:
    print(f"@{res['handle']:<16} [{res['chain']:<9}] FOMO Claimed: ${res['claimed_pnl']:>11,.0f} | Real Bal: {res['real_bal_native']:>8.2f} {res['native_sym']} (${res['real_bal_usd']:>10,.0f}) | TXs: {res['recent_tx_count']:>3}")

with open('/home/node/Developments/zenithsw/data/fomo_onchain_audit.json', 'w') as f:
    json.dump(results, f, indent=2)
