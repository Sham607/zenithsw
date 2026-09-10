import urllib.request
import json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwMjI0OTMsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTAyNjA5M30.AsmnTTX6egwG1MFpUbIlgCVVLVgKPJrXnbbo336-WGaIzQjN3eIoiTn7SISgHVmu5c9_GgBBaZij8wtkIj07hg"

cookie = f"privy-session=t; privy-token={bearer}; privy-id-token={bearer}"

endpoints = [
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=ALL",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=30d",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d",
    "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=24h",
    "https://prod-api.fomo.family/v2/leaderboard?limit=150",
    "https://prod-api.fomo.family/v2/users/leaderboard?limit=150",
    "https://prod-api.fomo.family/v2/leaderboard/traders?limit=150",
    "https://prod-api.fomo.family/v2/traders?limit=150",
    "https://prod-api.fomo.family/v2/swaps?limit=50",
]

for url in endpoints:
    req = urllib.request.Request(url)
    req.add_header("Authorization", f"Bearer {bearer}")
    req.add_header("Cookie", cookie)
    req.add_header("privy-app-id", "cm6h485v300n3zj9yl6vpedq7")
    req.add_header("privy-ca-id", "cm6h485v300n3zj9yl6vpedq7")
    req.add_header("Origin", "https://fomo.family")
    req.add_header("Referer", "https://fomo.family/")
    req.add_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    req.add_header("Accept", "application/json, text/plain, */*")
    try:
        resp = urllib.request.urlopen(req, timeout=10)
        data = json.loads(resp.read().decode())
        print(f"[SUCCESS 200] {url}")
        print(f"Data type: {type(data)}, length: {len(data) if isinstance(data, (list, dict)) else 0}")
        if isinstance(data, list) and len(data) > 0:
            print("First item keys:", data[0].keys() if isinstance(data[0], dict) else data[0])
            with open(f"/home/node/Developments/zenithsw/data/fomo_live_dump.json", "w") as f:
                json.dump(data, f, indent=2)
            print("Saved dump to data/fomo_live_dump.json")
            break
        elif isinstance(data, dict):
            print("Dict keys:", list(data.keys()))
    except Exception as e:
        print(f"[FAILED] {url} -> {e}")
