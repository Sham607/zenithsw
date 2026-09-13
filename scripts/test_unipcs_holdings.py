import urllib.request, json

# Kita coba ambil holdings / positions dari Unipcs
urls = [
    "https://prod-api.fomo.family/v2/users/36adb85a-c0fd-5fa8-916d-8fdc32fe4237/positions",
    "https://prod-api.fomo.family/v2/users/36adb85a-c0fd-5fa8-916d-8fdc32fe4237/holdings",
    "https://prod-api.fomo.family/v2/trader/unipcs/positions"
]

token = "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6IkI4SXlObWU1V0lZRnJIclJRVDBLdFlPRlFIUjFKVXFnaGVTMHhSZHR1QVkifQ.eyJzaWQiOiJjbXR2bW1oYjIwMzhuMGNsMTk5NHR5eXJlIiwiaXNzIjoicHJpdnkuaW8iLCJpYXQiOjE3ODkyMjYwMDgsImF1ZCI6ImNtNmg0ODVvMzAwbjN6ajl5bDZ2cGVkcTciLCJzdWIiOiJkaWQ6cHJpdnk6Y210dm1taHFqMDM4bzBjbDFhYzV4dTlueiIsImV4cCI6MTc4OTIyOTYwOH0.CMXISHxlp04CLN4SVC0ul0ZFC-rNjAGWum2b1ar6r3DTLSA-G_gj_t20EcfEjrCa-17Rw6EE8JwtNGUhMWw93Q"

headers = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
    "Authorization": f"Bearer {token}",
    "Accept": "application/json, text/plain, */*",
    "Origin": "https://fomo.family",
    "Referer": "https://fomo.family/"
}

for url in urls:
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            print(f"✅ Success on: {url}")
            print(json.dumps(data, indent=2)[:400])
            with open("/home/node/Developments/zenithsw/data/unipcs_holdings_sample.json", "w") as f:
                json.dump(data, f, indent=2)
            break
    except urllib.error.HTTPError as e:
        print(f"❌ {url} -> HTTP {e.code}: {e.reason}")
    except Exception as e:
        print(f"❌ {url} -> {e}")
