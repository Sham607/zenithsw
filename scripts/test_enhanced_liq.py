import requests

def extract_best_pool_and_liquidity(mint):
    url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
    try:
        r = requests.get(url, timeout=5).json()
    except Exception as e:
        return None
    
    pairs = r.get("pairs") or []
    if not pairs:
        return None

    best_pair = None
    max_liq = 0.0
    total_liq = 0.0
    best_symbol = "TOKEN"
    best_price = 0.0

    for p in pairs:
        dex_id = p.get("dexId", "")
        price_str = p.get("priceUsd")
        price = float(price_str) if price_str else 0.0
        sym = p.get("baseToken", {}).get("symbol", "")
        
        # Check standard liquidity
        liq_obj = p.get("liquidity") or {}
        liq_usd = float(liq_obj.get("usd") or 0.0)
        
        # If pumpfun curve with null liquidity, estimate from market cap
        if dex_id == "pumpfun" and liq_usd <= 0.0:
            mcap = float(p.get("marketCap") or p.get("fdv") or 0.0)
            if mcap > 0.0:
                # Bonding curve virtual liquidity ~ 50% of MC
                liq_usd = mcap * 0.50

        total_liq += liq_usd
        if liq_usd >= max_liq:
            max_liq = liq_usd
            best_pair = p
            best_price = price
            if sym:
                best_symbol = sym

    effective_liq = max(max_liq, total_liq)
    return {
        "symbol": best_symbol,
        "price": best_price,
        "max_pool_liq": max_liq,
        "total_liq": total_liq,
        "effective_liq": effective_liq,
        "best_dex": best_pair.get("dexId") if best_pair else "unknown"
    }

# Test with various tokens
tokens = [
    ("GATO", "HsprHxBRgiVaQdRf3TnKNuZZtaX9JUSPgMFdAkk6pump"),
    ("Plumber", "G8dmGbWTEFeK8Xmj5YaukwNsAKXCDEQfm11d5987crmZ"),
    ("Sapijiju", "7thY5PzYdH311NXYE6mecBjUs7wcosk1bWw2qfH4pump"),
    ("AISLOP", "2wzVMXhLypmP92mXNCq4fuFcd9TCC972AbMfuiH3pump"),
    ("EGGS", "BbCZqewwS1jYUe5c5HQnAtV83hPRjvsoSPRotBhRZ1ck"),
    ("Saturween", "5THtNEGTF1y67eRWvcgz3MomDZePCVx1xdskHHeCpump"),
    ("PINTO", "5mepeg141jSVK1TgxhDPUH1rS3DizVQZg8y8gtbmpump"),
]

for name, mint in tokens:
    res = extract_best_pool_and_liquidity(mint)
    if res:
        print(f"[{name}] DEX: {res['best_dex']:<10} | Price: ${res['price']:<10.6f} | Max Pool: ${res['max_pool_liq']:<10,.2f} | Effective Liq: ${res['effective_liq']:<10,.2f}")
