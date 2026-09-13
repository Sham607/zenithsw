import urllib.request, json, time, sys

token = sys.argv[1] if len(sys.argv) > 1 else ""

# Kita tes pola /v2/users/... atau query param
endpoints = [
    "https://prod-api.fomo.family/v2/users/following?page=1&limit=50",
    "https://prod-api.fomo.family/v2/users/me",
    "https://prod-api.fomo.family/v2/user",
    "https://prod-api.fomo.family/v2/trader/following",
    "https://prod-api.fomo.family/v2/follows",
    "https://prod-api.fomo.family/v2/following",
    "https://prod-api.fomo.family/v2/account",
    "https://prod-api.fomo.family/v2/profile"
]

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Authorization": f"Bearer {token}",
    "Accept": "application/json",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/"
}

for ep in endpoints:
    req = urllib.request.Request(ep, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            print(f"✅ Success on: {ep}")
            print(json.dumps(data, indent=2)[:300])
            break
    except urllib.error.HTTPError as e:
        print(f"❌ {ep} -> HTTP {e.code}: {e.reason}")
        if e.code == 400:
            try:
                print("   Body:", e.read().decode("utf-8"))
            except:
                pass
    except Exception as e:
        print(f"❌ {ep} -> {e}")
    time.sleep(1.2)
