#!/usr/bin/env python3
"""
Verifikasi on-chain wallet candidates dari whale trace.
Cek: getAccountInfo (program vs wallet), saldo SOL, jumlah tx terbaru, ritme tx.
RPC: publik Solana mainnet ONLY.
"""
import urllib.request, urllib.error, json, time, os
from datetime import datetime, timezone

RPC = "https://api.mainnet-beta.solana.com"
DATA_DIR = "/home/node/Developments/zenithsw/data/whale_traces"
RESULT_FILE = f"{DATA_DIR}/verification_results.json"

# System program owner = wallet asli
SYSTEM_PROGRAM = "11111111111111111111111111111111"

CANDIDATES = [
    # Tier-S
    ("48M3tRdbVYmEbf5rCTFVAgqCCaZdChVmeg3VPBrmgT8m", "S1", "0xdetweiler", "STONK $4.67M"),
    ("AfrddTGYwCVEQB1gxCAhR8i48o6qtxYqksdVkeLudEhg", "S2", "0xdetweiler", "STONK $3.59M"),
    ("3C6qVymTAwWNKCSspmd1qbUH9avaqhsjgW2yntvEYBXt", "S3", "0xdetweiler", "STONK $1.68M"),
    ("zxTpi4BtaWX3mgdAPoezkMD1hxx8CdeCfrqXMWvSCLX", "S4", "0xdetweiler", "STONK $1.37M"),
    ("7a8xxAJBELDo6P9dikSYctdw6ce8F4mWr3ahcAD8Ao49", "S5", "0xdetweiler", "STONK $1.15M"),
    ("HMzvsEEmtzHhvZNw9uwbaG85HCTmFnkbhzUx16cy7ca3", "S6", "theveeman", "CATE $336K"),
    ("EgB1PqsGFbj3u7jGfwwx5V9CVoc4BGjyfgnag7BJcQjB", "S7", "0xdetweiler", "STONK $314K"),
    ("AgEZLwFKa3nHtu9Xhz9MQhBBHHseDtVrUmfzsNSowbrW", "S8", "0xdetweiler", "STONK $169K"),
    # S9 skipped — address truncated in source data, cannot verify
    ("Dzmkq4wo1xZ2sAXhMvSg5GbcoSPiQ4JHL3aoVvjeuA5J", "S10", "0xdetweiler", "STONK $115K"),
    ("3KAXs9Qhz1wPE5bXKx65iZAPv2WUaabuitnTxseuMoyj", "S11", "0xdetweiler", "STONK $86K"),
    ("6ash7NazTfoeszf7UkKufy8JLTZyaJJtCdkWvsjwpb1d", "S12", "0xdetweiler", "STONK $85K"),
    ("8S2XFMPBNnUNBuzKTb1ZFC1B2DMJSRUFZQsbpSwFXhkx", "S13", "0xdetweiler", "GP $59K"),
    # Tier-A
    ("qhJ7kLjrE68sbDF11CpwMt3P9rqiwZtretM5ZzeAX6o", "A1", "theveeman", "CATE $331K"),
    ("CfqYM8CfuWr3qL67hwnDyJDqWDb8FU7Nrexga1sjTHEV", "A2", "0xdetweiler", "STONK $144K"),
    ("7iBuJoX4nHkqpBxC7ZiRxzrhEHnfT3jUmtzUC61F1vnc", "A3", "0xdetweiler", "STONK $104K"),
    ("36SpkkrsnyUgjPC24KU993r8T3whkJaPgoEeAmbBj9uB", "A4", "0xdetweiler", "STONK $42K"),
    ("EAf6shtt8QGJ7UiSRrDc6pzwXKEmb5s7tCCpSDe5zpzZ", "A5", "theveeman", "BUTTHOLE $10K"),
    ("HX72xZ1CHg7cWPhGGJyHbLp3sLrpznTYmWn5AY3FVwUj", "A6", "theveeman", "BUTTHOLE $10K"),
    ("HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC", "A7", "shared", "LFOWN+SME $5.9K"),
]

def log(msg):
    print(f"[{datetime.now(timezone.utc).strftime('%H:%M:%S')}] {msg}", flush=True)

def rpc_call(method, params, max_retries=8):
    delay = 3.0
    body = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    for attempt in range(max_retries):
        try:
            req = urllib.request.Request(RPC, data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=30) as r:
                resp = json.loads(r.read().decode())
                return resp.get("result")
        except urllib.error.HTTPError as e:
            if e.code == 429:
                log(f"  429 backoff {delay:.0f}s")
                time.sleep(delay)
                delay = min(delay * 2, 120)
            else:
                log(f"  HTTP {e.code}, retry {attempt+1}")
                time.sleep(2)
        except Exception as e:
            log(f"  net err: {str(e)[:60]}, retry {attempt+1}")
            time.sleep(2)
    return None

def verify_wallet(addr, tier, whale, desc):
    """Verifikasi satu wallet: account type, saldo SOL, ritme tx."""
    result = {
        "addr": addr,
        "tier": tier,
        "whale": whale,
        "desc": desc,
        "is_wallet": None,
        "owner": None,
        "sol_balance": None,
        "recent_tx_count": None,
        "tx_span_days": None,
        "avg_tx_per_day": None,
        "verdict": None,
        "reason": None,
        "error": None,
    }
    
    # 1. getAccountInfo
    log(f"Verifying {tier} {addr[:16]}...")
    account = rpc_call("getAccountInfo", [addr, {"encoding": "jsonParsed"}])
    time.sleep(1.5)
    
    if account is None:
        result["error"] = "getAccountInfo failed"
        result["verdict"] = "UNKNOWN"
        result["reason"] = "RPC gagal, tidak bisa verifikasi"
        return result
    
    if account.get("value") is None:
        # Account doesn't exist or has 0 balance
        result["is_wallet"] = True  # non-existent accounts default to system program
        result["owner"] = SYSTEM_PROGRAM
        result["sol_balance"] = 0.0
        result["verdict"] = "TIDAK_LAYAK"
        result["reason"] = "Account tidak ada/saldo 0 SOL — kemungkinan sudah ditinggalkan"
        return result
    
    value = account["value"]
    owner = value.get("owner", "")
    lamports = value.get("lamports", 0)
    sol = lamports / 1e9
    
    result["owner"] = owner
    result["sol_balance"] = round(sol, 4)
    result["is_wallet"] = (owner == SYSTEM_PROGRAM)
    
    # Jika bukan wallet (akun program)
    if owner != SYSTEM_PROGRAM:
        result["verdict"] = "TIDAK_LAYAK"
        result["reason"] = f"Akun PROGRAM (owner: {owner[:16]}...). Bukan wallet personal."
        return result
    
    # 2. getSignaturesForAddress — ambil 100 tx terbaru untuk hitung ritme
    sigs = rpc_call("getSignaturesForAddress", [addr, {"limit": 100}])
    time.sleep(1.5)
    
    if sigs and len(sigs) > 0:
        result["recent_tx_count"] = len(sigs)
        timestamps = [s.get("blockTime") for s in sigs if s.get("blockTime")]
        if len(timestamps) >= 2:
            newest = max(timestamps)
            oldest = min(timestamps)
            span_seconds = newest - oldest
            span_days = max(span_seconds / 86400, 0.01)
            result["tx_span_days"] = round(span_days, 1)
            result["avg_tx_per_day"] = round(len(timestamps) / span_days, 1)
            
            # Cek ritme: MM/MEV bot punya >100 tx/hari
            if result["avg_tx_per_day"] > 100:
                result["verdict"] = "TIDAK_LAYAK"
                result["reason"] = f"Bot/MM indicator: {result['avg_tx_per_day']} tx/hari (>100). Ritme terlalu cepat."
                return result
            
            # Cek burst: >20 tx dalam <1 jam
            if span_days < 0.05 and len(timestamps) > 20:
                result["verdict"] = "TIDAK_LAYAK"
                result["reason"] = f"Burst bot: {len(timestamps)} tx dalam {span_days*24:.1f} jam"
                return result
    else:
        result["recent_tx_count"] = 0
    
    # 3. Final verdict berdasarkan saldo + aktivitas
    if sol < 0.01:
        result["verdict"] = "TIDAK_LAYAK"
        result["reason"] = f"Saldo terlalu rendah ({sol:.4f} SOL). Wallet mati/disposable."
    elif sol < 0.2:
        result["verdict"] = "MARGINAL"
        result["reason"] = f"Saldo tipis ({sol:.4f} SOL). Kemungkinan sniper sekali pakai."
    elif result["avg_tx_per_day"] and result["avg_tx_per_day"] > 50:
        result["verdict"] = "TIDAK_LAYAK"
        result["reason"] = f"Terlalu aktif ({result['avg_tx_per_day']} tx/hari). Kemungkinan bot."
    else:
        result["verdict"] = "LAYAK"
        result["reason"] = f"Wallet asli, saldo {sol:.2f} SOL, ritme normal"
    
    return result


# ---- MAIN ----
if __name__ == "__main__":
    log(f"Mulai verifikasi {len(CANDIDATES)} wallet...")
    results = []
    
    for addr, tier, whale, desc in CANDIDATES:
        try:
            r = verify_wallet(addr, tier, whale, desc)
            results.append(r)
            verdict_emoji = {"LAYAK": "✅", "TIDAK_LAYAK": "❌", "MARGINAL": "⚠️", "UNKNOWN": "❓"}.get(r["verdict"], "?")
            log(f"  {verdict_emoji} {tier} {addr[:16]}... → {r['verdict']}: {r['reason']}")
        except Exception as e:
            log(f"  ❗ ERROR {tier} {addr[:16]}...: {e}")
            results.append({"addr": addr, "tier": tier, "whale": whale, "desc": desc, 
                          "verdict": "ERROR", "reason": str(e)})
        time.sleep(1.0)
    
    # Save results
    with open(RESULT_FILE, "w") as f:
        json.dump(results, f, indent=2)
    
    # Summary
    log(f"\n{'='*60}")
    log(f"RINGKASAN VERIFIKASI")
    log(f"{'='*60}")
    
    layak = [r for r in results if r["verdict"] == "LAYAK"]
    tidak = [r for r in results if r["verdict"] == "TIDAK_LAYAK"]
    marginal = [r for r in results if r["verdict"] == "MARGINAL"]
    unknown = [r for r in results if r["verdict"] in ("UNKNOWN", "ERROR")]
    
    log(f"✅ LAYAK: {len(layak)}")
    for r in layak:
        log(f"   {r['tier']} {r['addr'][:20]}... | {r['sol_balance']} SOL | {r.get('avg_tx_per_day','?')} tx/hari | {r['desc']}")
    
    log(f"⚠️  MARGINAL: {len(marginal)}")
    for r in marginal:
        log(f"   {r['tier']} {r['addr'][:20]}... | {r['sol_balance']} SOL | {r['reason']}")
    
    log(f"❌ TIDAK LAYAK: {len(tidak)}")
    for r in tidak:
        log(f"   {r['tier']} {r['addr'][:20]}... | {r['reason']}")
    
    log(f"❓ UNKNOWN/ERROR: {len(unknown)}")
    for r in unknown:
        log(f"   {r['tier']} {r['addr'][:20]}... | {r.get('reason','?')}")
    
    log(f"\nReport: {RESULT_FILE}")
    log(f"=== VERIFIKASI SELESAI ===")
