import urllib.request, json
from urllib.error import HTTPError

app_id = "cm6h485o300n3zj9yl6vpedq7"
refresh_token = "9zcb8z5j8p251i0ywhf8q91e"

# Let's test privy refresh endpoint with correct app ID
url = "https://auth.privy.io/api/v1/sessions/refresh"
payload = json.dumps({"refresh_token": refresh_token}).encode('utf-8')

req = urllib.request.Request(url, data=payload, method="POST")
req.add_header("Content-Type", "application/json")
req.add_header("privy-app-id", app_id)
req.add_header("privy-client", "react-auth:1.98.4")
req.add_header("Origin", "https://fomo.family")
req.add_header("Referer", "https://fomo.family/")
req.add_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")

try:
    resp = urllib.request.urlopen(req, timeout=10)
    data = json.loads(resp.read().decode())
    print("[SUCCESS PRIVY REFRESH!]")
    print(json.dumps(data, indent=2))
except HTTPError as e:
    print(f"HTTP Error {e.code}: {e.reason}")
    print("Error Body:", e.read().decode())
except Exception as e:
    print(f"Exception: {e}")
