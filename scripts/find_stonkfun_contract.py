import urllib.request, json

# Ambil metadata stonkfun atau website stonkfun
url = "https://www.stonkfun.xyz/"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        html = resp.read().decode("utf-8")
        print("HTML length:", len(html))
        for line in html.split("\n")[:25]:
            if "weth" in line.lower() or "dividend" in line.lower() or "reward" in line.lower() or "fee" in line.lower():
                print(" ", line.strip())
except Exception as e:
    print("Err:", e)
