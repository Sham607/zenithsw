import urllib.request
import urllib.error
import json

api_key = "nsn_c92defed8b1988c00a545597c0feeb42"

test_endpoints = [
    {
        "url": "https://api.nansen.ai/api/v1/smart-money/dex-trades",
        "payload": {
            "chains": ["solana", "arbitrum", "base"],
            "pagination": {"page": 1, "per_page": 100},
            "order_by": [{"field": "trade_value_usd", "direction": "DESC"}]
        }
    },
    {
        "url": "https://api.nansen.ai/api/v1/token-screener",
        "payload": {
            "chains": ["solana", "arbitrum"],
            "timeframe": "7d",
            "order_by": [{"field": "net_inflow_usd", "direction": "DESC"}],
            "pagination": {"page": 1, "per_page": 10}
        }
    },
    {
        "url": "https://api.nansen.ai/api/v1/smart-money/netflow",
        "payload": {
            "chains": ["solana", "arbitrum"],
            "order_by": [{"field": "net_flow_7d_usd", "direction": "DESC"}],
            "pagination": {"page": 1, "per_page": 20}
        }
    }
]

for ep in test_endpoints:
    url = ep["url"]
    data = json.dumps(ep["payload"]).encode('utf-8')
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "apikey": api_key,
            "Content-Type": "application/json",
            "Accept": "application/json",
            "User-Agent": "ZenithEngine/1.0"
        }
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            body = json.loads(resp.read().decode())
            print(f"[SUCCESS 200] {url}")
            if "data" in body:
                print(f"Items count: {len(body['data'])}")
                if len(body["data"]) > 0:
                    print("Sample item 0:", json.dumps(body["data"][0], indent=2))
    except urllib.error.HTTPError as e:
        print(f"[HTTP {e.code}] {url} -> {e.read().decode()[:300]}")
    except Exception as e:
        print(f"[ERROR] {url} -> {e}")
