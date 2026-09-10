import urllib.request
import urllib.error
import json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwNDQxODUsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTA0Nzc4NX0.0JdOEwAYN2B4lOvC0WYOuhzCi40bVNv_I-To30DykiMRU5f8OfrlRw5tf_snTKAJje_btcKmqv6yLwBv2zxsKQ"

headers = {
    "Authorization": f"Bearer {bearer}",
    "privy-app-id": "cm6h485o300n3zj9yl6vpedq7",
    "Content-Type": "application/json",
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Accept": "application/json"
}

endpoints = [
    "https://api.fomo.family/traders?timeframe=7d&limit=50",
    "https://api.fomo.family/leaderboard?timeframe=7d&limit=50",
    "https://api.fomo.family/api/traders?timeframe=7d&limit=50",
    "https://fomo.family/api/traders?timeframe=7d&limit=50",
    "https://api.fomo.family/traders/top?timeframe=7d",
    "https://api.fomo.family/tokens"
]

for ep in endpoints:
    print(f"\nTesting endpoint: {ep}")
    req = urllib.request.Request(ep, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            status = resp.status
            raw = resp.read().decode()
            data = json.loads(raw)
            print(f"Status {status} SUCCESS! Type: {type(data)}")
            if isinstance(data, list):
                print(f"Items count: {len(data)}, Sample: {json.dumps(data[:2], indent=2)[:300]}")
            elif isinstance(data, dict):
                print(f"Keys: {list(data.keys())}, Sample: {json.dumps(data, indent=2)[:300]}")
    except urllib.error.HTTPError as e:
        print(f"HTTP Error {e.code}: {e.reason}")
    except Exception as e:
        print(f"Error: {e}")
