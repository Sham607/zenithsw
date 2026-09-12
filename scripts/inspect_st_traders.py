import os
from pathlib import Path

def load_env_val(key, default=""):
    env_file = Path(__file__).resolve().parent.parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            if line.strip().startswith(f"{key}="):
                return line.strip().split("=", 1)[1].strip('"'')
    return os.getenv(key, default)

import urllib.request
import json

DATA_API_KEY = load_env_val("SOLANATRACKER_API_KEY")
url = "https://data.solanatracker.io/top-traders/all"

req = urllib.request.Request(
    url,
    headers={
        "x-api-key": DATA_API_KEY,
        "User-Agent": "Mozilla/5.0"
    }
)

with urllib.request.urlopen(req, timeout=10) as resp:
    data = json.loads(resp.read().decode())

wallets = data.get("wallets", [])
print(f"Total Top Traders returned: {len(wallets)}")
if wallets:
    for idx, w in enumerate(wallets[:5]):
        print(f"\n--- Trader #{idx+1} ---")
        for k, v in w.items():
            print(f"  {k}: {v}")
