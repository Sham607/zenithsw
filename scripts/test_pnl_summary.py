import urllib.request
import urllib.error
import json
from datetime import datetime, timedelta, timezone

api_key = "nsn_c92defed8b1988c00a545597c0feeb42"

now = datetime.now(timezone.utc)
date_to = now.strftime("%Y-%m-%d")
date_from = (now - timedelta(days=7)).strftime("%Y-%m-%d")

payload = {
    "address": "Ev1AaPu4c6L1QEXZiLv9DkNBa3ZigddiXYFTk9tN2DoK",
    "chain": "solana",
    "date": {
        "from": date_from,
        "to": date_to
    }
}

req = urllib.request.Request(
    "https://api.nansen.ai/api/v1/profiler/address/pnl-summary",
    data=json.dumps(payload).encode('utf-8'),
    headers={
        "apikey": api_key,
        "Content-Type": "application/json",
        "Accept": "application/json"
    }
)

try:
    with urllib.request.urlopen(req, timeout=10) as resp:
        body = json.loads(resp.read().decode())
        print(f"[SUCCESS 200] PnL Summary (7D: {date_from} to {date_to}):")
        print(json.dumps(body, indent=2))
except urllib.error.HTTPError as e:
    print(f"[HTTP {e.code}] -> {e.read().decode()}")
except Exception as e:
    print(f"[ERROR] -> {e}")
