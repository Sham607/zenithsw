import urllib.request, re

url = "https://www.stonkfun.xyz/rewards"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
with urllib.request.urlopen(req) as resp:
    html = resp.read().decode("utf-8")

# Extract all text inside <p> tags
p_texts = re.findall(r'<p[^>]*>(.*?)</p>', html, re.DOTALL)
for p in p_texts:
    clean = re.sub(r'<[^>]+>', '', p).strip()
    print("-", clean)
