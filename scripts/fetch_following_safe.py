import urllib.request, json, time, sys

token = sys.argv[1] if len(sys.argv) > 1 else ""
url = "https://prod-api.fomo.family/v2/users/a649f1f6-e5ee-5732-8e93-b0d0a1522560/followingPaginate"

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Authorization": f"Bearer {token}",
    "Accept": "application/json, text/plain, */*",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/",
    "Sec-Fetch-Dest": "empty",
    "Sec-Fetch-Mode": "cors",
    "Sec-Fetch-Site": "same-site"
}

print(f"Fetching following data from: {url}...")
req = urllib.request.Request(url, headers=headers)
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("✅ SUCCESS! Fetched following data.")
        with open("/home/node/Developments/zenithsw/data/fomo_my_following_raw.json", "w") as f:
            json.dump(data, f, indent=2)

        print("Data type:", type(data))
        if isinstance(data, dict):
            print("Keys:", list(data.keys()))
            items = data.get("data") or data.get("following") or data.get("users") or data.get("items") or []
            print(f"Total following retrieved: {len(items)}")
        elif isinstance(data, list):
            print(f"Total following retrieved: {len(data)}")
except Exception as e:
    print("❌ Error:", e)
