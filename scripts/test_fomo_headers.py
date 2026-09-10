import urllib.request
import urllib.error
import json

bearer = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR0dmNsamgwMjU2MGNpNWQ2MDk5ZHM4IiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkwNDQxODUsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dHZjbHQ2MDI1NzBjaTU3b2V4ZTVtZiIsImV4cCI6MTc4OTA0Nzc4NX0.0JdOEwAYN2B4lOvC0WYOuhzCi40bVNv_I-To30DykiMRU5f8OfrlRw5tf_snTKAJje_btcKmqv6yLwBv2zxsKQ"

headers_variants = [
    {
        "Authorization": f"Bearer {bearer}",
        "privy-token": bearer,
        "privy-app-id": "cm6h485o300n3zj9yl6vpedq7",
        "Origin": "https://fomo.family",
        "Referer": "https://fomo.family/",
        "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
    },
    {
        "x-privy-token": bearer,
        "Authorization": f"Bearer {bearer}",
        "Origin": "https://fomo.family",
        "Referer": "https://fomo.family/",
        "User-Agent": "Mozilla/5.0"
    }
]

url = "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d"

for idx, h in enumerate(headers_variants):
    req = urllib.request.Request(url, headers=h)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            print(f"Variant {idx} SUCCESS! Count: {len(data)}")
    except urllib.error.HTTPError as e:
        print(f"Variant {idx} HTTP Error {e.code}: {e.read().decode()[:300]}")
    except Exception as e:
        print(f"Variant {idx} Error: {e}")
