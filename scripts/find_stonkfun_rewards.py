import urllib.request, json

url = "https://www.stonkfun.xyz/rewards"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        html = resp.read().decode("utf-8")
        print("Rewards HTML found! Length:", len(html))
        for line in html.split("<"):
            if any(k in line.lower() for k in ["weth", "claim", "yield", "dividend", "revenue", "reward", "flywheel"]):
                print(" ", line.strip()[:120])
except Exception as e:
    print("Err Rewards:", e)
