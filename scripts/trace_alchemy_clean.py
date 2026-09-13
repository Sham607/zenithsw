import urllib.request, json

arb_rpc = ""
with open("/home/node/Developments/zenithsw/.env") as f:
    for line in f:
        if line.startswith("ARBITRUM_RPC_URL="):
            arb_rpc = line.strip().split("=", 1)[1].strip("\"'")

evm_addresses = [
    ("unipcs", "0x2ac082e252143c89c6e3bcb972e49855f9f6711c"),
    ("Salem1299534", "0xa14b38a3dfd05c089bae5d60e9d42af3924e5da0"),
    ("picadura", "0x155ed6e7524dd0bbdfb972b003dec52f876b0e1a"),
    ("brrrgrrrz", "0x830a7744b165ded3f2cf016123229480e155e274"),
    ("DumbCrayonEater", "0xb48ae67b008443eaa5b5f20b7e9da4fb39f8e167")
]

print("Checking Arbitrum/RH on-chain tx count...\n")

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
            print(f"@{handle} ({addr[:10]}...) -> On-Chain Tx Count (Arbitrum): {count}")
    except Exception as e:
        print(f"@{handle} -> Err:", e)
