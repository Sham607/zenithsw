import json
import os
import urllib.request
import urllib.error

RPC = os.environ.get("SOLANA_RPC_URL", "")
API_KEY = os.environ.get("SOLANA_TRACKER_API_KEY", "")
CASES = [
    ("pointfarmcap", "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx", 37067363.11991306),
    ("The__Solstice", "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx", 20853447.061540816),
    ("theveeman", "6GmAFSYs4gk3FDao5FzzySQpPZaWsa4rUJHacpMpUNgx", 18070442.30034204),
]

def call(method, params):
    body = json.dumps({"jsonrpc":"2.0", "id":1, "method":method, "params":params}).encode()
    req = urllib.request.Request(RPC, data=body, headers={"Content-Type":"application/json", "User-Agent":"zenithsw-readonly", "x-api-key":API_KEY})
    with urllib.request.urlopen(req, timeout=30) as response:
        return json.loads(response.read())

def main():
    if not RPC:
        raise SystemExit("SOLANA_RPC_URL tidak tersedia")
    for handle, mint, expected in CASES:
        try:
            data = call("getTokenLargestAccounts", [mint])
            if data.get("error"):
                print(handle, "RPC_ERROR", data["error"].get("message", "unknown"))
                continue
            holders = data.get("result", {}).get("value", [])
            matches = []
            for holder in holders:
                amount = holder.get("uiAmount") or 0
                if expected and abs(amount - expected) / expected <= 0.02:
                    matches.append({"token_account":holder.get("address"), "amount":amount, "difference_pct":round((amount-expected)/expected*100, 5)})
            print(handle, "top20", len(holders), "matches", json.dumps(matches, separators=(",", ":")))
        except urllib.error.HTTPError as exc:
            print(handle, "HTTP_ERROR", exc.code)
        except Exception as exc:
            print(handle, "ERROR", type(exc).__name__)

if __name__ == "__main__":
    main()
