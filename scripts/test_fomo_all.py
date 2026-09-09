import urllib.request, json

refresh_token = "9zcb8z5j8p251i0ywhf8q91e"
app_id = "cm6h485v300n3zj9yl6vpedq7"

# 1. Refresh Privy Session
refresh_url = "https://auth.privy.io/api/v1/sessions/refresh"
refresh_payload = json.dumps({"refresh_token": refresh_token}).encode('utf-8')
refresh_req = urllib.request.Request(
    refresh_url,
    data=refresh_payload,
    headers={
        "Content-Type": "application/json",
        "privy-app-id": app_id,
        "Origin": "https://fomo.family"
    }
)

new_token = None
try:
    with urllib.request.urlopen(refresh_req, timeout=10) as resp:
        res = json.loads(resp.read().decode())
        new_token = res.get("token")
        print(f"Privy Token refreshed successfully! Token length: {len(new_token) if new_token else 0}")
except Exception as e:
    print(f"Privy refresh error: {e}")

if new_token:
    # 2. Fetch FOMO Leaderboard
    lb_url = "https://prod-api.fomo.family/v2/leaderboard?type=TOTAL_PNL&timeframe=ALL"
    lb_req = urllib.request.Request(
        lb_url,
        headers={
            "Authorization": f"Bearer {new_token}",
            "Accept": "application/json"
        }
    )
    try:
        with urllib.request.urlopen(lb_req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            print(f"Leaderboard type: {type(data)}")
            if isinstance(data, list):
                print(f"SUCCESS! Fetched {len(data)} total smart wallets from FOMO Leaderboard!")
                # Save to full json
                with open('/home/node/Developments/zenithsw/data/fomo_all_100_wallets.json', 'w') as f:
                    json.dump(data, f, indent=2)
                print("Saved full FOMO leaderboard to data/fomo_all_100_wallets.json")
            elif isinstance(data, dict):
                print("Keys:", list(data.keys()))
    except Exception as e:
        print(f"FOMO Leaderboard error: {e}")
