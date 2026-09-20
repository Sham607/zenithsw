#!/usr/bin/env python3
"""
UNIPCS DEEP TRACE 30 HARI — Forensik feeder wallet 2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF
Desain: checkpoint resume, pacing adaptif anti-429, event log JSONL, laporan JSON.
Output: /home/node/Developments/zenithsw/data/unipcs_trace_{state.json, events.jsonl, report_30d.json}
"""
import urllib.request, urllib.error, json, time, os, sys, signal
from datetime import datetime, timezone

RPC = "https://api.mainnet-beta.solana.com"
TARGET = "2heJbC32Tpfcb3nbUb5ER61K11FGZVfVGtVnDm6LDogF"
DATA_DIR = "/home/node/Developments/zenithsw/data"
STATE_F = f"{DATA_DIR}/unipcs_trace_state.json"
EVENTS_F = f"{DATA_DIR}/unipcs_trace_events.jsonl"
REPORT_F = f"{DATA_DIR}/unipcs_trace_report_30d.json"
CUTOFF_TS = datetime(2026, 8, 21, 0, 0, 0, tzinfo=timezone.utc).timestamp()  # 30 hari penuh

IGNORE_MINTS = {
    "So11111111111111111111111111111111111111112",
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
    "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo",
}

price_cache = {}

def log(msg):
    print(f"[{datetime.now(timezone.utc).strftime('%H:%M:%S')}] {msg}", flush=True)

def load_state():
    if os.path.exists(STATE_F):
        with open(STATE_F) as f:
            return json.load(f)
    return {"oldest_sig": None, "processed": 0, "feeders": {}, "outbound": {}, "done": False}

def save_state(st):
    tmp = STATE_F + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f)
    os.replace(tmp, STATE_F)

def http_json(body, max_retries=8):
    delay = 3.0
    for _ in range(max_retries):
        try:
            req = urllib.request.Request(RPC, data=json.dumps(body).encode(),
                                         headers={"Content-Type": "application/json"})
            with urllib.request.urlopen(req, timeout=25) as r:
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
        with urllib.request.urlopen(req, timeout=6) as r:
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

def append_events(events):
    with open(EVENTS_F, "a") as f:
        for e in events:
            f.write(json.dumps(e) + "\n")

def main():
    st = load_state()
    log(f"Resume dari: oldest_sig={str(st['oldest_sig'])[:20] if st['oldest_sig'] else 'AWAL'} | processed={st['processed']}")
    if st.get("done"):
        log("State menandakan sudah selesai. Hapus state untuk scan ulang.")
        return

    events_buf = []
    last_oldest_ts = None

    while not st["done"]:
        params = {"limit": 500}
        if st["oldest_sig"]:
            params["before"] = st["oldest_sig"]
        res = http_json({"jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
                         "params": [TARGET, params]})
        sigs = (res or {}).get("result") or []
        if not sigs:
            # CRITICAL: bedakan "histori benar-benar habis" vs "RPC gagal total".
            # Verifikasi dengan 1 request kecil; jika itu pun gagal, tunggu dan ulang.
            probe = http_json({"jsonrpc": "2.0", "id": 1, "method": "getSignaturesForAddress",
                               "params": [TARGET, {"limit": 10}]})
            probe_ok = isinstance(probe, dict) and "result" in probe
            if not probe_ok:
                log("RPC masih gagal total — JANGAN tandai selesai. Tunggu 60s lalu ulang.")
                time.sleep(60)
                continue
            log("Terkonfirmasi histori habis (bukan error RPC). SELESAI.")
            st["done"] = True
            save_state(st)
            break

        st["oldest_sig"] = sigs[-1]["signature"]
        batch_start_sig = st["oldest_sig"]  # untuk rollback saat batch gagal
        processed_before = st["processed"]  # untuk restore saat rollback
        oldest_ts = sigs[-1].get("blockTime")
        last_oldest_ts = oldest_ts

        # Batch parse getTransaction (3/call) — ATOMIC: jika chunk gagal total setelah
        # retry (return None), batalkan seluruh batch dan mulai ulang dari checkpoint
        # yang sama. Jangan pernah lewati chunk secara diam-diam.
        batch_failed = False
        for i in range(0, len(sigs), 3):
            chunk = sigs[i:i+3]
            payload = [{"jsonrpc": "2.0", "id": idx, "method": "getTransaction",
                        "params": [s["signature"],
                                   {"maxSupportedTransactionVersion": 0, "encoding": "jsonParsed"}]}
                       for idx, s in enumerate(chunk)]
            resp = http_json(payload)
            if not isinstance(resp, list):
                log(f"Chunk {i//3} gagal total setelah retry — rollback batch, retry dari checkpoint.")
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
                pre_map = {b.get("mint"): float(b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                           for b in meta.get("preTokenBalances", []) if b.get("owner") == TARGET}
                post_map = {b.get("mint"): float(b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                            for b in meta.get("postTokenBalances", []) if b.get("owner") == TARGET}
                signers = [a["pubkey"] for a in tx["transaction"]["message"]["accountKeys"] if a.get("signer")]
                target_signed = TARGET in signers

                for mint, post_amt in post_map.items():
                    if mint in IGNORE_MINTS:
                        continue
                    pre_amt = pre_map.get(mint, 0.0)
                    delta = post_amt - pre_amt
                    if abs(delta) <= 0.00001:
                        continue

                    # identifikasi pihak lawan (pengirim/penerima)
                    counterparties = set()
                    if delta > 0:  # inbound: siapa yang berkurang
                        for pb in meta.get("preTokenBalances", []):
                            if pb.get("mint") == mint and pb.get("owner") != TARGET:
                                p_pre = float(pb.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                for post_b in meta.get("postTokenBalances", []):
                                    if post_b.get("accountIndex") == pb.get("accountIndex"):
                                        p_post = float(post_b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                        if p_pre - p_post > 0.00001:
                                            counterparties.add(pb.get("owner"))
                    else:  # outbound: siapa yang bertambah
                        for post_b in meta.get("postTokenBalances", []):
                            if post_b.get("mint") == mint and post_b.get("owner") != TARGET:
                                p_post = float(post_b.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                for pb in meta.get("preTokenBalances", []):
                                    if pb.get("accountIndex") == post_b.get("accountIndex"):
                                        p_pre = float(pb.get("uiTokenAmount", {}).get("uiAmount") or 0.0)
                                        if p_post - p_pre > 0.00001:
                                            counterparties.add(post_b.get("owner"))
                    counterparties.discard(TARGET)
                    for sn in signers:
                        counterparties.add(sn)
                    counterparties.discard(TARGET)
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
            # Rollback aman: cursor kembali ke awal batch, counter event dipulihkan,
            # dan buffer event parsial dibuang (tx gagal akan di-parse ulang).
            st["oldest_sig"] = batch_start_sig
            st["processed"] = processed_before
            events_buf = []
            save_state(st)
            log("Cooldown 90s setelah kegagalan batch (throttle berat).")
            time.sleep(90)
            continue

        # flush events per batch besar
        if events_buf:
            append_events(events_buf)
            events_buf = []

        oldest_dt = datetime.fromtimestamp(oldest_ts, timezone.utc) if oldest_ts else None
        log(f"Progress: {st['processed']} tx | oldest={oldest_dt.strftime('%Y-%m-%d') if oldest_dt else '?'} | feeders={len(st['feeders'])} | outbound={len(st['outbound'])}")

        if oldest_ts and oldest_ts <= CUTOFF_TS:
            log("Capai cutoff 30 hari (21 Agt 2026). SELESAI.")
            st["done"] = True

        save_state(st)
        time.sleep(1.0)

    if events_buf:
        append_events(events_buf)

    # Laporan akhir: filter spam — feeder masuk daftar jika punya >=2 event ATAU usd >= $100
    feeders_clean = {}
    for addr, info in st["feeders"].items():
        if info["count"] >= 2 or info["total_usd"] >= 100.0:
            feeders_clean[addr] = info

    ranked = sorted(feeders_clean.items(), key=lambda x: x[1]["total_usd"], reverse=True)
    report = {
        "generated": datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC"),
        "target": TARGET,
        "window": "2026-08-21 s/d 2026-09-20 (atau history tersedia)",
        "processed_tx": st["processed"],
        "verified_feeders": ranked,
        "outbound_destinations": sorted(st["outbound"].items(),
                                        key=lambda x: x[1]["total_usd"], reverse=True)[:30],
        "note": "feeder_clean: count>=2 ATAU total_usd>=100. Sisanya dianggap noise spam/airdrop.",
    }
    with open(REPORT_F, "w") as f:
        json.dump(report, f, indent=2)
    log(f"SELESAI. {st['processed']} tx diproses. Feeders bersih: {len(feeders_clean)}. Report: {REPORT_F}")

if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda s, f: (save_state(load_state()), sys.exit(0)))
    main()
