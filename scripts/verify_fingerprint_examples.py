import json
import urllib.request

RPCS = [
    "https://api.mainnet-beta.solana.com",
    "https://rpc.solanatracker.io/public?advanced=true",
]
CASES = [
    {
        "handle": "pointfarmcap",
        "token_a": "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx",
        "amount_a": 37067363.11991306,
        "token_b": "6V7cF52p3eSkRuoVXnAryWVwoyS775DERenYPMMYBPxS",
        "amount_b": 726328445.618303,
        "claimed_wallet": "Beqv6dzTcjV2eodo8RRXCiCcnSYrS1vkQKhfqwHXqeit",
    },
    {
        "handle": "The__Solstice",
        "token_a": "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx",
        "amount_a": 20853447.061540816,
        "token_b": "0x5ab000ff9b9ffe0349ce5ffa5fd86f217c3680f5",
        "amount_b": 21445828.364752743,
        "claimed_wallet": "4ugDhHJ8XDXAeABmrNmGffFaLbJb9BkPyiFGVSV9ocwo",
    },
    {
        "handle": "theveeman",
        "token_a": "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx",
        "amount_a": 18070442.30034204,
        "token_b": "9cRCn9rGT8V2imeM2BaKs13yhMEais3ruM3rPvTGpump",
        "amount_b": 2521107.05944,
        "claimed_wallet": "F5hkYsi8JxjyA2JHN5CA7MbnnhWubkXB2ZQB7Gkaxqs6",
    },
]

def call(rpc, method, params):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode()
    req = urllib.request.Request(rpc, data=body, headers={"Content-Type":"application/json","User-Agent":"zenithsw-readonly"})
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())

def largest(rpc, mint):
    result = call(rpc, "getTokenLargestAccounts", [mint])
    return result.get("result", {}).get("value", []), result

def main():
    for case in CASES:
        print("CASE", case["handle"])
        used = None
        holders = []
        for rpc in RPCS:
            try:
                holders, raw = largest(rpc, case["token_a"])
                print("  rpc", rpc, "entries", len(holders), "error", raw.get("error"))
                if holders:
                    used = rpc
                    break
            except Exception as e:
                print("  rpc", rpc, "error", type(e).__name__)
        if not holders:
            print("  status NO_TOP_HOLDERS")
            continue
        near = []
        for h in holders:
            amount = h.get("uiAmount") or 0
            if case["amount_a"] and abs(amount-case["amount_a"])/case["amount_a"] <= 0.02:
                near.append((h.get("address"), amount))
        print("  near_token_a", near)
        print("  claimed_wallet", case["claimed_wallet"])
        print("  status", "CANDIDATE_FOUND" if near else "NO_MATCH_IN_TOP20")

if __name__ == "__main__":
    main()
