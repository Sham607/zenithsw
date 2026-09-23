#!/usr/bin/env python3
"""
GENERIC WHALE TRACER — Forensik feeder/satellite wallet untuk arbitrary Solana address.
Adaptasi dari unipcs_deep_trace_30d.py.
Fitur: checkpoint resume, pacing adaptif anti-429, event log JSONL, laporan JSON.
RPC: publik Solana mainnet ONLY (bukan Helius).
"""
import urllib.request, urllib.error, json, time, os, sys, signal
from datetime import datetime, timezone

RPC = "https://api.mainnet-beta.solana.com"
DATA_DIR = "/home/node/Developments/zenithsw/data/whale_traces"

# Cutoff: 30 hari kebelakang dari sekarang
CUTOFF_TS = time.time() - (30 * 86400)

IGNORE_MINTS = {
    "So11111111111111111111111111111111111111112",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
    "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo",
}

price_cache = {}

def log(msg):
    print(f"[{datetime.now(timezone.utc).strftime('%H:%M:%S')}] {msg}", flush=True)

def state_file(label):
    return os.path.join(DATA_DIR, f"{label}_state.json")

def events_file(label):
    return os.path.join(DATA_DIR, f"{label}_events.jsonl")

def report_file(label):
    return os.path.join(DATA_DIR, f"{label}_report.json")

def load_state(label):
    sf = state_file(label)
    if os.path.exists(sf):
        with open(sf) as f:
            return json.load(f)
    return {"oldest_sig": None, "processed": 0, "feeders": {}, "outbound": {}, "done": False}

def save_state(st, label):
    sf = state_file(label)
    tmp = sf + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f)
    os.replace(tmp, sf)

def http_json(body, max_retries=8):
    delay = 3.0
    for _ in range(max_retries):
        try:
            req = urllib.request.Request(RPC, data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=30) as r:
                return json.loads(r.read().decode())
        except urllib.error.HTTPError as e:
            if e.code == 429:
                log(f"  429 backoff {delay:.0f}s")
                time.sleep(delay)
                delay = min(delay * 2, 120)
            else:
                log(f"  HTTP {e.code}, retry")
                time.sleep(2)
        except Exception as e:
            log(f"  net err: {str(e)[:60]}, retry")
            time.sleep(2)
    return None

def get_dex_price(mint):
    if mint in price_cache:
        return price_cache[mint]
    try:
        url = f"https://api.dexscreener.com/latest/dex/tokens/{mint}"
        req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urllib.request.urlopen(req, timeout=8) as r:
            d = json.loads(r.read().decode())
            pairs = d.get("pairs") or []
            if pairs:
                p = float(pairs[0].get("priceUsd") or 0.0)
                sym = pairs[0].get("baseToken", {}).get("symbol", "UNKNOWN")
                liq = float((pairs[0].get("liquidity") or {}).get("usd") or 0.0)
                price_cache[mint] = (p, sym, liq)
                return price_cache[mint]
    except Exception:
        pass
    price_cache[mint] = (0.0, "UNKNOWN", 0.0)
    return price_cache[mint]

def append_events(events, label):
    ef = events_file(label)
    with open(ef, "a") as f:
        for e in events:
            f.write(json.dumps(e) + "\n")

def resolve_account_keys(tx):
    """Resolve full account list including loadedAddresses for v1 tx."""
    msg = tx.get("transaction", {}).get("message", {})
    keys = [a["pubkey"] if isinstance(a, dict) else a for a in msg.get("accountKeys", [])]
    loaded = tx.get("meta", {}).get("loadedAddresses") or {}
    for addr in loaded.get("writable", []):
        keys.append(addr)
    for addr in loaded.get("readonly", []):
        keys.append(addr)
    return keys

def trace_wallet(target, label):
    """Trace satu wallet. Resume-safe."""
    os.makedirs(DATA_DIR, exist_ok=True)
    st = load_state(label)
    log(f"=== TRACE {label} ({target[:12]}...) ===")
    log(f"Resume dari: oldest_sig={str(st['oldest_sig'])[:20] if st['oldest_sig'] else 'AWAL'} | processed={st['processed']}")

    if st.get("done"):
        log("State menandakan sudah selesai. Hapus state untuk scan ulang.")
        return st

    events_buf = []

    while not st["done"]:
        params = {"limit": 500}
        if st["oldest_sig"]:
            params["before"] = st["oldest_sig"]
        res = http_json({"jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
                         "params": [target, params]})
        sigs = (res or {}).get("result") or []

        if not sigs:
            # Verifikasi: histori habis vs RPC gagal
            probe = http_json({"jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
                               "params": [target, {"limit": 10}]})
            probe_ok = isinstance(probe, dict) and "result" in probe
            if not probe_ok:
                log("RPC masih gagal total — JANGAN tandai selesai. Tunggu 60s lalu ulang.")
                time.sleep(60)
                continue
            log("Terkonfirmasi histori habis (bukan error RPC). SELESAI.")
            st["done"] = True
            save_state(st, label)
            break

        st["oldest_sig"] = sigs[-1]["signature"]
        batch_start_sig = st["oldest_sig"]
        processed_before = st["processed"]
        oldest_ts = sigs[-1].get("blockTime")

        # Cek cutoff
        if oldest_ts and oldest_ts <= CUTOFF_TS:
            # Filter sigs yang masih dalam window
            sigs = [s for s in sigs if (s.get("blockTime") or 0) >= CUTOFF_TS]
            st["done"] = True  # Akan selesai setelah batch ini

        batch_failed = False
        for i in range(0, len(sigs), 3):
            chunk = sigs[i:i+3]
            # maxSupportedTransactionVersion: 1 (support v1 tx)
            payload = [{"jsonrpc": "2.0", "id": idx, "method": "getTransaction",
                        "params": [s["signature"],
                                   {"maxSupportedTransactionVersion": 1, "encoding": "jsonParsed"}]}
                       for idx, s in enumerate(chunk)]
            resp = http_json(payload)
            if not isinstance(resp, list):
                log(f"Chunk {i//3} gagal total setelah retry — rollback batch.")
                batch_failed = True
                break

            for item in resp:
                idx = item.get("id")
                if idx is None or idx >= len(chunk):
                    continue
                s_meta = chunk[idx]
                bt = s_meta.get("blockTime")
                if not bt or s_meta.get("err"):
                    continue
                tx = item.get("result")
                if not tx or not tx.get("meta") or tx["meta"].get("err"):
                    continue
                meta = tx["meta"]
                dt = datetime.fromtimestamp(bt, timezone.utc).strftime("%Y-%m-%d %H:%M:%S")

                # Resolve semua account keys (termasuk loadedAddresses untuk v1 tx)
                all_keys = resolve_account_keys(tx)

                # Pre/post token balance — resolve owner via accountIndex + all_keys jika 'owner' field kosong
                def resolve_owner(b):
                    owner = b.get("owner")
                    if owner:
                        return owner
                    ai = b.get("accountIndex")
                    if ai is not None and ai < len(all_keys):
                        return all_keys[ai]
                    return None

                pre_map = {}
                for b in meta.get("preTokenBalances", []):
                    owner = resolve_owner(b)
                    if owner == target:
                        mint = b.get("mint")
                        amt = float(b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                        pre_map[mint] = amt

                post_map = {}
                for b in meta.get("postTokenBalances", []):
                    owner = resolve_owner(b)
                    if owner == target:
                        mint = b.get("mint")
                        amt = float(b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                        post_map[mint] = amt

                signers = []
                for a in tx["transaction"]["message"].get("accountKeys", []):
                    if isinstance(a, dict) and a.get("signer"):
                        signers.append(a["pubkey"])
                target_signed = target in signers

                for mint, post_amt in post_map.items():
                    if mint in IGNORE_MINTS:
                        continue
                    pre_amt = pre_map.get(mint, 0.0)
                    delta = post_amt - pre_amt
                    if abs(delta) <= 0.00001:
                        continue

                    # Identifikasi counterparties
                    counterparties = set()
                    if delta > 0:  # inbound
                        for pb in meta.get("preTokenBalances", []):
                            if pb.get("mint") == mint:
                                pb_owner = resolve_owner(pb)
                                if pb_owner and pb_owner != target:
                                    p_pre = float(pb.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                    for post_b in meta.get("postTokenBalances", []):
                                        if post_b.get("accountIndex") == pb.get("accountIndex"):
                                            p_post = float(post_b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                            if p_pre - p_post > 0.00001:
                                                counterparties.add(pb_owner)
                    else:  # outbound
                        for post_b in meta.get("postTokenBalances", []):
                            if post_b.get("mint") == mint:
                                pb_owner = resolve_owner(post_b)
                                if pb_owner and pb_owner != target:
                                    p_post = float(post_b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                    for pb in meta.get("preTokenBalances", []):
                                        if pb.get("accountIndex") == post_b.get("accountIndex"):
                                            p_pre = float(pb.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                            if p_post - p_pre > 0.00001:
                                                counterparties.add(pb_owner)

                    counterparties.discard(target)
                    for sn in signers:
                        if sn != target:
                            counterparties.add(sn)
                    if not counterparties:
                        continue

                    price, sym, liq = get_dex_price(mint)
                    val = delta * price
                    event = {"time": dt, "sig": s_meta["signature"], "mint": mint, "sym": sym,
                             "delta": delta, "usd": val, "liq": liq,
                             "target_signed": target_signed, "counterparties": sorted(counterparties)}
                    events_buf.append(event)

                    bucket = st["feeders"] if delta > 0 else st["outbound"]
                    for cp in counterparties:
                        f = bucket.setdefault(cp, {"tokens": {}, "count": 0, "first": dt,
                                                   "last": dt, "total_usd": 0.0,
                                                   "sigs": [], "target_signed": 0})
                        f["count"] += 1
                        f["first"] = min(f["first"], dt)
                        f["last"] = max(f["last"], dt)
                        f["total_usd"] += val
                        if target_signed:
                            f["target_signed"] += 1
                        if len(f["sigs"]) < 3 and s_meta["signature"] not in f["sigs"]:
                            f["sigs"].append(s_meta["signature"])
                        tk = f["tokens"].setdefault(sym, {"amount": 0.0, "usd": 0.0})
                        tk["amount"] += delta
                        tk["usd"] += val

            st["processed"] += len(chunk)
            time.sleep(1.2)

        if batch_failed:
            st["oldest_sig"] = batch_start_sig
            st["processed"] = processed_before
            events_buf = []
            save_state(st, label)
            log("Cooldown 90s setelah kegagalan batch (throttle berat).")
            time.sleep(90)
            continue

        # Flush events per batch
        if events_buf:
            append_events(events_buf, label)
            events_buf = []

        oldest_dt = datetime.fromtimestamp(oldest_ts, timezone.utc) if oldest_ts else None
        log(f"Progress: {st['processed']} tx | oldest={oldest_dt.strftime('%Y-%m-%d') if oldest_dt else '?'} | feeders={len(st['feeders'])} | outbound={len(st['outbound'])}")

        if not st["done"]:
            save_state(st, label)
        time.sleep(1.0)

    if events_buf:
        append_events(events_buf, label)

    # Laporan akhir
    feeders_clean = {}
    for addr, info in st["feeders"].items():
        if info["count"] >= 2 or abs(info["total_usd"]) >= 100.0:
            feeders_clean[addr] = info

    ranked = sorted(feeders_clean.items(), key=lambda x: abs(x[1]["total_usd"]), reverse=True)
    cutoff_dt = datetime.fromtimestamp(CUTOFF_TS, timezone.utc).strftime("%Y-%m-%d")
    report = {
        "generated": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC"),
        "target": target,
        "label": label,
        "window": f"{cutoff_dt} s/d sekarang (30 hari atau history tersedia)",
        "processed_tx": st["processed"],
        "verified_feeders": ranked,
        "outbound_destinations": sorted(st["outbound"].items(),
                                        key=lambda x: abs(x[1]["total_usd"]), reverse=True)[:30],
        "note": "feeder_clean: count>=2 ATAU total_usd>=100. Sisanya dianggap noise spam/airdrop.",
    }
    rf = report_file(label)
    with open(rf, "w") as f:
        json.dump(report, f, indent=2)
    log(f"SELESAI {label}. {st['processed']} tx diproses. Feeders bersih: {len(feeders_clean)}. Report: {rf}")
    save_state(st, label)
    return st


# ---- MAIN: Jalankan 2 whale secara sekuensial ----
TARGETS = [
    ("F5hkYsi8JxjyA2JHN5CA7MbnnhWubkXB2ZQB7Gkaxqs6", "theveeman"),
    ("EC2f5DnHzuNRit1ExqghSifDbp1wgrzktsRRZCtU92MJ", "0xdetweiler"),
]

if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda s, f: sys.exit(0))
    for target, label in TARGETS:
        try:
            trace_wallet(target, label)
        except KeyboardInterrupt:
            log(f"Interrupted saat tracing {label}. State tersimpan.")
            sys.exit(1)
        except Exception as e:
            log(f"ERROR tracing {label}: {e}")
            import traceback
            traceback.print_exc()
        # Jeda antar wallet biar RPC publik gak marah
        log("Jeda 30s sebelum whale berikutnya...")
        time.sleep(30)
    log("=== SEMUA TRACE SELESAI ===")
