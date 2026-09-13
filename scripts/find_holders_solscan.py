import urllib.request, json

# Kita cari via Solscan / SolanaTracker token holders API
mint = "5dvXTZ5qwgafnHtwu3Ls3QrWx1U4LQsFeCuJgkk4QEC6"
url = f"https://data.solanatracker.io/tokens/{mint}/holders"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0", "x-api-key": "61cf3d81-8eb7-47b2-ac63-f2fa1597fbb3"})
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("Total holders returned:", len(data))
        for h in data[:10]:
            print("  Holder:", h.get("wallet"), "Amount:", h.get("amount"), "Pct:", h.get("percentage"))
except Exception as e:
    print("Err Solanatracker holders:", e)
