import urllib.request, json, time, sys

# Token dari user (diproteksi agar tidak terekspos)
token = sys.argv[1] if len(sys.argv) > 1 else ""

endpoints = [
    "https://prod-api.fomo.family/v2/user/following",
    "https://prod-api.fomo.family/v2/users/following",
    "https://prod-api.fomo.family/v2/me/following",
    "https://prod-api.fomo.family/v2/traders/following",
    "https://prod-api.fomo.family/v1/user/following",
    "https://prod-api.fomo.family/v2/me"
]

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Authorization": f"Bearer {token}",
    "Accept": "application/json, text/plain, */*",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/"
}

print("Testing FOMO following endpoints with gentle rate limit...\n")

for ep in endpoints:
    req = urllib.request.Request(ep, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            print(f"✅ Success on: {ep}")
            if isinstance(data, dict):
                print("Keys:", list(data.keys())[:10])
                if "following" in data:
                    print(f"Found following count: {len(data['following'])}")
            elif isinstance(data, list):
                print(f"Returned array of {len(data)} items")

            with open("/home/node/Developments/zenithsw/data/fomo_my_following.json", "w") as f:
                json.dump(data, f, indent=2)
            break
    except urllib.error.HTTPError as e:
        print(f"❌ {ep} -> HTTP {e.code}: {e.reason}")
    except Exception as e:
        print(f"❌ {ep} -> {e}")
    time.sleep(1.0)
