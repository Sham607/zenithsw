import urllib.request, json
from urllib.error import HTTPError

refresh_token = "9zcb8z5j8p251i0ywhf8q91e"
app_id = "cm6h485v300n3zj9yl6vpedq7"

url = "https://auth.privy.io/api/v1/sessions"
payload = json.dumps({"refresh_token": refresh_token}).encode('utf-8')
headers = {
    "Content-Type": "application/json",
    "privy-app-id": app_id,
    "privy-client": "react-auth:1.98.4",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36"
}

req = urllib.request.Request(url, data=payload, headers=headers)
try:
    with urllib.request.urlopen(req, timeout=10) as resp:
        print("Status:", resp.status)
        print("Response:", resp.read().decode())
except HTTPError as e:
    print(f"HTTP Error {e.code}: {e.reason}")
    print("Error Body:", e.read().decode())
except Exception as e:
    print("Error:", e)
