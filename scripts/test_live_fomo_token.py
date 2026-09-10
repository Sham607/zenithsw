import urllib.request, json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODg5Njg2MDEsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4ODk3MjIwMX0.rZ_R5-e5CO0sgWZoLb_gHkYur3y1QzpcdJg8-_yJYAcWSuK7r-v1h5tMQuv0u93nGy-bGZhgBzYzmJulmsfQ1g"
cookie = "privy-session=t; privy-token=" + bearer

endpoints = [
    "https://prod-api.fomo.family/v2/leaderboard?limit=20",
    "https://prod-api.fomo.family/v2/users/leaderboard?limit=20",
    "https://prod-api.fomo.family/v2/leaderboard/traders?limit=20",
    "https://prod-api.fomo.family/v1/leaderboard",
    "https://prod-api.fomo.family/v2/swaps?limit=10"
]

for url in endpoints:
    req = urllib.request.Request(url)
    req.add_header("Authorization", f"Bearer {bearer}")
    req.add_header("Cookie", cookie)
    req.add_header("privy-app-id", "cm6h485v300n3zj9yl6vpedq7")
    req.add_header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
    try:
        resp = urllib.request.urlopen(req, timeout=10)
        data = json.loads(resp.read().decode())
        print(f"[SUCCESS 200] {url}")
        print(f"Data preview: {str(data)[:200]}")
    except Exception as e:
        print(f"[FAILED] {url} -> {e}")
