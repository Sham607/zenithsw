import urllib.request, json

# Kita ambil 3 sampel trader: Unipcs, Salem1299534, DumbCrayonEater
samples = [
    ("unipcs", "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1", "0x2ac082e252143c89c6e3bcb972e49855f9f6711c"),
    ("Salem1299534", "CDY1JBoi2fKPPsHabyANmp6RP9Lkz8TcrkACsX3rHkka", "0xa14b38a3dfd05c089bae5d60e9d42af3924e5da0"),
    ("DumbCrayonEater", "43nktK56Bfk3yJK9iqr3Kzyt9npgEB6sPYKAeoYMxeug", "0xb48ae67b008443eaa5b5f20b7e9da4fb39f8e167")
]

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"

print("Auditing On-Chain Legitimacy of FOMO Following Addresses:\n")

for handle, sol_addr, evm_addr in samples:
    print(f"=== Trader: @{handle} ===")

    # 1. Cek aktivitas Solana
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getSignaturesForAddress",
        "params": [sol_addr, {"limit": 5}]
    }
    req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            sigs = json.loads(resp.read().decode("utf-8")).get("result", [])
            print(f"  [SOLANA] {sol_addr[:10]}... -> Signatures in ledger: {len(sigs)}")
            if sigs:
                print(f"    Latest Tx: {sigs[0]['signature'][:20]}... | BlockTime: {sigs[0].get('blockTime')}")
    except Exception as e:
        print("  [SOLANA] Err:", e)

    # 2. Cek aktivitas Robinhood via Blockscout
    rh_url = f"https://robinhoodchain.blockscout.com/api/v2/addresses/{evm_addr}/counters"
    headers = {
        "User-Agent": "Mozilla/5.0",
        "Authorization": "Bearer proapi_gP08rH4YIy0Sv2Bi5MaCEX2qFBxT0FuNYKFnVwr55RVkCeJVx8G1LE5PPEVvxewG9_eePH2M"
    }
    req_rh = urllib.request.Request(rh_url, headers=headers)
    try:
        with urllib.request.urlopen(req_rh) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            print(f"  [ROBINHOOD] {evm_addr[:10]}... -> Transfers: {data.get('token_transfers_count')} | Txs: {data.get('transactions_count')}")
    except Exception as e:
        print("  [ROBINHOOD] Err:", e)
    print("-" * 50)
