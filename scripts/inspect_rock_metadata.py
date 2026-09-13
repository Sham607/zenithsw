import urllib.request, json

url = "https://gateway.irys.xyz/BdSAHY4iQw3Fk4bQKyEWBgsP5Za1bGW49iurgZUXTbSM"
req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
try:
    with urllib.request.urlopen(req) as resp:
        data = json.loads(resp.read().decode("utf-8"))
        print("Metadata JSON:")
        print(json.dumps(data, indent=2))
except Exception as e:
    print("Err metadata:", e)
