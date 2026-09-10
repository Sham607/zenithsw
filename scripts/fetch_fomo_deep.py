import urllib.request
import urllib.error
import json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwNDQxODUsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTA0Nzc4NX0.0JdOEwAYN2B4lOvC0WYOuhzCi40bVNv_I-To30DykiMRU5f8OfrlRw5tf_snTKAJje_btcKmqv6yLwBv2zxsKQ"

headers = {
    "Authorization": f"Bearer {bearer}",
    "privy-token": bearer,
    "privy-app-id": "cm6h485o300n3zj9yl6vpedq7",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/",
    "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
}

endpoints = [
    ("7D Leaderboard", "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d"),
    ("30D Leaderboard", "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=30d"),
    ("ALL Leaderboard", "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=ALL"),
    ("Top Traders", "https://prod-api.fomo.family/v2/traders?limit=50"),
]

for label, url in endpoints:
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            print(f"\n=== {label} ({url}) ===")
            if isinstance(data, list):
                print(f"Total count: {len(data)}")
                if data:
                    print("Sample item:", json.dumps(data[0], indent=2))
            elif isinstance(data, dict):
                print("Dict keys:", list(data.keys()))
                print("Sample data:", json.dumps(data, indent=2)[:500])
    except Exception as e:
        print(f"Error on {label}: {e}")
