#!/usr/bin/env python3
"""
RE-FILTER: Ambil semua counterparty unik dari trace events, verifikasi via getAccountInfo,
pisahkan wallet personal vs program account.
Lalu rank wallet personal berdasarkan volume + frekuensi.
RPC: publik Solana mainnet ONLY.
"""
import json, time, os, urllib.request, urllib.error
from datetime import datetime, timezone

RPC = "https://api.mainnet-beta.solana.com"
DATA_DIR = "/home/node/Developments/zenithsw/data/whale_traces"
SYSTEM_PROGRAM = "11111111111111111111111111111111"

TARGETS = {
    "theveeman": "F5hkYsi8JxjyA2JHN5CA7MbnnhWubkXB2ZQB7Gkaxqs6",
    "0xdetweiler": "EC2f5DnHzuNRit1ExqghSifDbp1wgrzktsRRZCtU92MJ",
}

# Known blacklist from memory
KNOWN_BLACKLIST = {
    "GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL": "MM/MEV bot",
}

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
                return json.loads(r.read().decode())
        except urllib.error.HTTPError as e:
            if e.code == 429:
                time.sleep(delay)
                delay = min(delay * 2, 120)
            else:
                time.sleep(2)
        except Exception:
            time.sleep(2)
    return None

def load_report(label):
    path = os.path.join(DATA_DIR, f"{label}_report.json")
    with open(path) as f:
        return json.load(f)

def collect_all_counterparties():
    """Collect semua counterparty unik dari kedua report (feeders + outbound)."""
    all_cp = {}  # addr -> {whales, total_usd_in, total_usd_out, tokens_in, tokens_out, tx_count_in, tx_count_out, first, last, sigs}
    
    for label in TARGETS:
        report = load_report(label)
        target = report["target"]
        
        # Feeders = inbound
        for addr, info in report["verified_feeders"]:
            if addr == target:
                continue
            if addr not in all_cp:
                all_cp[addr] = {"whales": set(), "total_usd_in": 0, "total_usd_out": 0,
                                "tokens_in": {}, "tokens_out": {}, "tx_count_in": 0, "tx_count_out": 0,
                                "first": info["first"], "last": info["last"], "sigs": []}
            cp = all_cp[addr]
            cp["whales"].add(label)
            cp["total_usd_in"] += abs(info["total_usd"])
            cp["tx_count_in"] += info["count"]
            cp["first"] = min(cp["first"], info["first"])
            cp["last"] = max(cp["last"], info["last"])
            for sym, t in info["tokens"].items():
                if sym != "UNKNOWN":
                    cp["tokens_in"][sym] = cp["tokens_in"].get(sym, 0) + abs(t["usd"])
            for s in info["sigs"][:2]:
                if s not in cp["sigs"]:
                    cp["sigs"].append(s)
        
        # Outbound destinations
        for addr, info in report["outbound_destinations"]:
            if addr == target:
                continue
            if addr not in all_cp:
                all_cp[addr] = {"whales": set(), "total_usd_in": 0, "total_usd_out": 0,
                                "tokens_in": {}, "tokens_out": {}, "tx_count_in": 0, "tx_count_out": 0,
                                "first": info["first"], "last": info["last"], "sigs": []}
            cp = all_cp[addr]
            cp["whales"].add(label)
            cp["total_usd_out"] += abs(info["total_usd"])
            cp["tx_count_out"] += info["count"]
            cp["first"] = min(cp["first"], info["first"])
            cp["last"] = max(cp["last"], info["last"])
            for sym, t in info["tokens"].items():
                if sym != "UNKNOWN":
                    cp["tokens_out"][sym] = cp["tokens_out"].get(sym, 0) + abs(t["usd"])
    
    return all_cp

def verify_account(addr):
    """Return (is_wallet, owner, sol_balance) via getAccountInfo."""
    resp = rpc_call("getAccountInfo", [addr, {"encoding": "jsonParsed"}])
    if resp is None:
        return None, None, None
    result = resp.get("result")
    if result is None:
        return None, None, None
    value = result.get("value")
    if value is None:
        return True, SYSTEM_PROGRAM, 0.0  # non-existent = system-owned, 0 SOL
    owner = value.get("owner", "")
    sol = value.get("lamports", 0) / 1e9
    return (owner == SYSTEM_PROGRAM), owner, sol

def check_tx_rhythm(addr):
    """Ambil 100 tx terbaru, hitung avg tx/day."""
    resp = rpc_call("getSignaturesForAddress", [addr, {"limit": 100}])
    if resp is None:
        return None, None
    result = resp.get("result") or []
    if len(result) < 2:
        return len(result), 0
    timestamps = [s.get("blockTime") for s in result if s.get("blockTime")]
    if len(timestamps) < 2:
        return len(result), 0
    span = max(timestamps) - min(timestamps)
    days = max(span / 86400, 0.01)
    return len(result), round(len(timestamps) / days, 1)


def main():
    log("Collecting semua counterparties dari kedua report...")
    all_cp = collect_all_counterparties()
    
    # Filter: minimal $100 inbound ATAU outbound, skip target wallets
    targets_set = set(TARGETS.values())
    candidates = {addr: info for addr, info in all_cp.items()
                  if addr not in targets_set
                  and (info["total_usd_in"] >= 100 or info["total_usd_out"] >= 100)}
    
    log(f"Total counterparties: {len(all_cp)}")
    log(f"Candidates (>=$100): {len(candidates)}")
    log(f"Mulai verifikasi getAccountInfo...")
    
    wallets = []      # verified personal wallets
    programs = []     # program accounts (LP/pool)
    dead = []         # 0 SOL / non-existent
    
    checked = 0
    for addr, info in sorted(candidates.items(), key=lambda x: x[1]["total_usd_in"] + x[1]["total_usd_out"], reverse=True):
        checked += 1
        if checked % 10 == 0:
            log(f"  Progress: {checked}/{len(candidates)} checked...")
        
        is_wallet, owner, sol = verify_account(addr)
        time.sleep(1.2)
        
        if is_wallet is None:
            log(f"  ❓ {addr[:16]}... — RPC failed, skipping")
            continue
        
        if not is_wallet:
            programs.append({"addr": addr, "owner": owner, **info})
            continue
        
        if sol is not None and sol < 0.001:
            dead.append({"addr": addr, "sol": sol, **info})
            continue
        
        # Known blacklist check
        if addr in KNOWN_BLACKLIST:
            log(f"  ⛔ {addr[:16]}... — {KNOWN_BLACKLIST[addr]}, skipping")
            continue
        
        # Check tx rhythm for bot detection
        tx_count, avg_per_day = check_tx_rhythm(addr)
        time.sleep(1.2)
        
        if avg_per_day and avg_per_day > 100:
            log(f"  ⛔ {addr[:16]}... — Bot ({avg_per_day} tx/day), skipping")
            continue
        
        wallets.append({
            "addr": addr,
            "sol": round(sol, 4) if sol else 0,
            "total_usd_in": info["total_usd_in"],
            "total_usd_out": info["total_usd_out"],
            "tokens_in": info["tokens_in"],
            "tokens_out": info["tokens_out"],
            "tx_count_in": info["tx_count_in"],
            "tx_count_out": info["tx_count_out"],
            "whales": sorted(info["whales"]),
            "first": info["first"],
            "last": info["last"],
            "sigs": info["sigs"][:3],
            "recent_tx": tx_count,
            "avg_tx_day": avg_per_day,
        })
    
    # Sort wallets by total volume (in + out)
    wallets.sort(key=lambda w: w["total_usd_in"] + w["total_usd_out"], reverse=True)
    
    # Save
    result = {
        "generated": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC"),
        "verified_wallets": wallets,
        "program_accounts_count": len(programs),
        "dead_accounts_count": len(dead),
        "total_checked": checked,
    }
    out_path = os.path.join(DATA_DIR, "verified_wallets.json")
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)
    
    # Print summary
    log(f"\n{'='*65}")
    log(f"  HASIL VERIFIKASI ON-CHAIN")
    log(f"{'='*65}")
    log(f"Checked: {checked} | Program accounts: {len(programs)} | Dead: {len(dead)} | Wallet personal: {len(wallets)}")
    
    if wallets:
        log(f"\n✅ WALLET PERSONAL TERVERIFIKASI (owner=SystemProgram):")
        for i, w in enumerate(wallets, 1):
            total = w["total_usd_in"] + w["total_usd_out"]
            direction = ""
            if w["total_usd_in"] > 0 and w["total_usd_out"] > 0:
                direction = f"IN ${w['total_usd_in']:,.0f} / OUT ${w['total_usd_out']:,.0f}"
            elif w["total_usd_in"] > 0:
                direction = f"IN ${w['total_usd_in']:,.0f}"
            else:
                direction = f"OUT ${w['total_usd_out']:,.0f}"
            
            top_tokens_in = sorted(w["tokens_in"].items(), key=lambda x: x[1], reverse=True)[:3]
            top_tokens_out = sorted(w["tokens_out"].items(), key=lambda x: x[1], reverse=True)[:3]
            tok_in_str = ", ".join([f"{s} ${v:,.0f}" for s,v in top_tokens_in]) if top_tokens_in else "-"
            tok_out_str = ", ".join([f"{s} ${v:,.0f}" for s,v in top_tokens_out]) if top_tokens_out else "-"
            
            whale_str = "+".join(w["whales"])
            shared = " 🔗" if len(w["whales"]) > 1 else ""
            
            log(f"\n  #{i}. {w['addr']}")
            log(f"      SOL: {w['sol']:.2f} | {direction} | {w['tx_count_in']+w['tx_count_out']} tx | {w['avg_tx_day']} tx/day")
            log(f"      Whale: {whale_str}{shared} | Period: {w['first'][:10]} ~ {w['last'][:10]}")
            if tok_in_str != "-":
                log(f"      Token IN:  {tok_in_str}")
            if tok_out_str != "-":
                log(f"      Token OUT: {tok_out_str}")
    else:
        log(f"\n⚠️  Tidak ada wallet personal terverifikasi.")
    
    log(f"\nReport: {out_path}")
    log(f"=== VERIFIKASI SELESAI ===")


if __name__ == "__main__":
    main()
