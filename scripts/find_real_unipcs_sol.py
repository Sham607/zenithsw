import urllib.request, json

rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
# Token USELESS di Solana
useless_mint = "Dz9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk"
expected_amount = 15874969.636746

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenLargestAccounts",
    "params": [useless_mint]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    accounts = data.get("result", {}).get("value", [])
    print(f"Top Holders of USELESS on Solana:")
    for a in accounts[:15]:
        acc_addr = a.get("address")
        amt = a.get("uiAmount")

        # Cek apakah ada yang mendekati expected_amount
        diff = abs(amt - expected_amount)
        print(f"  Account: {acc_addr} | Amount: {amt:,.2f} | Diff: {diff:,.2f}")
