import urllib.request, json

alchemy_url = "https://arb-mainnet.g.alchemy.com/v2/yYwU6K1W9fK9vL5Y7eR3Q8T1bC4dE2fA"
# Atau endpoint alchemy/arb yang ada di .env
import os
from dotenvy import dotenv
dotenv("/home/node/Developments/zenithsw/.env")
arb_rpc = os.getenv("ARBITRUM_RPC_URL")

evm_addresses = [
    ("unipcs", "0x2ac082e252143c89c6e3bcb972e49855f9f6711c"),
    ("Salem1299534", "0xa14b38a3dfd05c089bae5d60e9d42af3924e5da0"),
    ("picadura", "0x155ed6e7524dd0bbdfb972b003dec52f876b0e1a"),
    ("brrrgrrrz", "0x830a7744b165ded3f2cf016123229480e155e274"),
    ("DumbCrayonEater", "0xb48ae67b008443eaa5b5f20b7e9da4fb39f8e167")
]

print(f"Checking Arbitrum/RH on-chain tx count using RPC: {arb_rpc[:35]}...\n")

for handle, addr in evm_addresses:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionCount",
        "params": [addr, "latest"]
    }
    req = urllib.request.Request(arb_rpc, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            count = int(data.get("result", "0x0"), 16)
            print(f"@{handle} ({addr[:10]}...) -> On-Chain Tx Count: {count}")
    except Exception as e:
        print(f"@{handle} -> Err:", e)
