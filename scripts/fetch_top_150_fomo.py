import urllib.request
import json
import time

auth_token = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwMjI0OTMsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTAyNjA5M30.AsmnTTX6egwG1MFpUbIlgCVVLVgKPJrXnbbo336-WGaIzQjN3eIoiTn7SISgHVmu5c9_GgBBaZij8wtkIj07hg"

urls = [
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=ALL",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=30d",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=24h",
    "https://prod-api.fomo.family/v2/traders?limit=100",
]

for url in urls:
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {auth_token}",
            "Accept": "application/json",
            "Origin": "https://fomo.family",
            "Referer": "https://fomo.family/"
        }
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            count = len(data) if isinstance(data, list) else len(data.get("traders", data.get("data", [])))
            print(f"URL {url} -> SUCCESS! Returned {count} items. Sample type: {type(data)}")
            if count > 0:
                sample = data[0] if isinstance(data, list) else data
                print(f"Sample keys/entry: {sample}")
    except Exception as e:
        print(f"URL {url} -> Error: {e}")
