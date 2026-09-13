import urllib.request, json

# Kita cari address asli Unipcs di FOMO API atau di database backup
url = "https://prod-api.fomo.family/v2/trader/unipcs/positions"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("FOMO Positions response keys:", data.keys() if isinstance(data, dict) else len(data))
        if isinstance(data, dict):
            print("Trader info:", data.get("trader"))
            wallets = data.get("wallets") or data.get("trader", {}).get("wallets")
            print("Wallets:", wallets)
except Exception as e:
    print("Err FOMO:", e)
