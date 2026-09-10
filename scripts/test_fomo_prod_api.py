import urllib.request
import urllib.error
import json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwNDQxODUsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTA0Nzc4NX0.0JdOEwAYN2B4lOvC0WYOuhzCi40bVNv_I-To30DykiMRU5f8OfrlRw5tf_snTKAJje_btcKmqv6yLwBv2zxsKQ"

urls = [
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=30d",
    "https://prod-api.fomo.family/v2/traders?limit=50",
    "https://prod-api.fomo.family/v2/trader/Aurelius0121",
    "https://prod-api.fomo.family/v2/trader/unipcs/positions",
]

for url in urls:
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {bearer}",
            "Accept": "application/json",
            "Origin": "https://fomo.family",
            "Referer": "https://fomo.family/",
            "User-Agent": "Mozilla/5.0"
        }
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            print(f"\nSUCCESS: {url}")
            if isinstance(data, list):
                print(f"List length: {len(data)}")
                if data:
                    print("Sample item:", json.dumps(data[0], indent=2)[:400])
            elif isinstance(data, dict):
                print(f"Dict keys: {list(data.keys())}")
                print("Sample dict:", json.dumps(data, indent=2)[:400])
    except urllib.error.HTTPError as e:
        print(f"HTTP Error {e.code} on {url}: {e.read().decode()[:200]}")
    except Exception as e:
        print(f"Error on {url}: {e}")
