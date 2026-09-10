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

url = "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=7d"
req = urllib.request.Request(url, headers=headers)
resp = urllib.request.urlopen(req, timeout=10)
data = json.loads(resp.read().decode())
lb = data.get("responseObject", {}).get("leaderboard", [])

print(f"Total 7D leaderboard traders: {len(lb)}")
if lb:
    print("Full fields of Rank 1:", json.dumps(lb[0], indent=2))
    print("\nTop 10 Traders 7D:")
    for idx, t in enumerate(lb[:10]):
        print(f"#{idx+1}: @{t.get('userHandle')} ({t.get('displayName')}) | SOL: {t.get('address')} | EVM: {t.get('evmAddress')} | PnL: ${float(t.get('totalPnL') or 0):,.2f}")
