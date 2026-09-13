import urllib.request, json

# Kita cari pemegang token 5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6 (EMBER)
# Di swap tadi: Unipcs membeli $34,918 dan $14,761 EMBER (total ~$50k) pada 11 Sep
rpc_url = "https://rpc.solanatracker.io/public?advanced=true"
ember_mint = "5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6"

payload = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "getTokenLargestAccounts",
    "params": [ember_mint]
}
req = urllib.request.Request(rpc_url, data=json.dumps(payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    data = json.loads(resp.read().decode("utf-8"))
    accounts = data.get("result", {}).get("value", [])
    print(f"Top 10 EMBER Holder Accounts:")
    for a in accounts[:10]:
        acc_addr = a.get("address")
        amt = a.get("uiAmount")

        # Cari owner dari token account ini
        owner_payload = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getAccountInfo",
            "params": [acc_addr, {"encoding": "jsonParsed"}]
        }
        owner_req = urllib.request.Request(rpc_url, data=json.dumps(owner_payload).encode("utf-8"), headers={"Content-Type": "application/json", "User-Agent": "Mozilla/5.0"})
        try:
            with urllib.request.urlopen(owner_req) as o_resp:
                o_data = json.loads(o_resp.read().decode("utf-8"))
                owner = o_data.get("result", {}).get("value", {}).get("data", {}).get("parsed", {}).get("info", {}).get("owner")
                print(f"  Owner: {owner} | Token Account: {acc_addr[:8]}... | Amount: {amt:,.0f} EMBER")
        except Exception:
            pass
