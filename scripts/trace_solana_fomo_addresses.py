import urllib.request, json

# Cek apakah address Solana tersebut memiliki saldo atau transaksi sama sekali
sol_addresses = [
    ("unipcs", "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1"),
    ("Salem1299534", "CDY1JBoi2fKPPsHabyANmp6RP9Lkz8TcrkACsX3rHkka"),
    ("picadura", "GsJhJ9zo19vWbQMBaMGPhMfkNzVAB16bCUWM7D8bD191"),
    ("brrrgrrrz", "9qFZGRCDbPmKH2meHefPK4oUPsqRGzk9Y5tZib261CQW"),
    ("DumbCrayonEater", "43nktK56Bfk3yJK9iqr3Kzyt9npgEB6sPYKAeoYMxeug"),
    ("theveeman", "EYveLtK4M3PsNnzH6zAGQgLnCGb2CqFqkKnEoNtRirDX")
]

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"

for handle, addr in sol_addresses:
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [addr]
    }
    req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
    with urllib.request.urlopen(req) as resp:
        res = json.loads(resp.read().decode("utf-8")).get("result", {})
        sol = res.get("value", 0) / 1e9
        print(f"@{handle} -> SOL Balance: {sol} SOL")
