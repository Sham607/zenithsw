import urllib.request, json

url = "https://www.stonkfun.xyz/_next/data/build-TfctsWXpff2fKS/index.json"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("Data keys:", data.keys())
        tokens = data.get("pageProps", {}).get("tokens", [])
        print(f"Total tokens listed on StonkFun: {len(tokens)}")
        for t in tokens:
            if t.get("mint") == "39HqgQ18g9R33sUfKq1TjX4sbKrgTb4YntnBCFhURJvy" or "monero" in t.get("name", "").lower():
                print("🔥 FOUND ON STONKFUN:")
                print(json.dumps(t, indent=2))
except Exception as e:
    print("Err:", e)
