#!/usr/bin/env python3
"""One-shot leaderboard wallet harvester untuk ZenithSW.

Sumber yang didukung:
- Solana Tracker PnL leaderboard
- OKX OnchainOS Smart Money leaderboard

Mode default adalah dry-run. Gunakan --execute untuk menulis wallet yang lolos
validasi format dan filter kualitas ke SQLite. Verifikasi on-chain lanjutan tetap
menjadi tanggung jawab worker/indexer ZenithSW. Kredensial dibaca dari environment
atau file .env lokal dan tidak pernah dicetak.
"""

import argparse
import base64
import hashlib
import hmac
import json
import os
import re
import sqlite3
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

DB_DEFAULT = "data/smart_wallets.db"
OKX_CHAIN_NAMES = {
    "1": "ETHEREUM",
    "501": "SOLANA",
    "42161": "ARBITRUM",
    "8453": "BASE",
}
SOL_TRACKER_URL = "https://data.solanatracker.io/v2/pnl/leaderboard/top"
OKX_HOST = os.getenv("OKX_API_HOST", "https://web3.okx.ac")
OKX_URL = OKX_HOST + "/api/v6/dex/market/leaderboard/list"
EVM_RE = re.compile(r"^0x[0-9a-fA-F]{40}$")


def load_dotenv_keys():
    """Membaca key lokal tanpa mencetak nilainya."""
    path = ".env"
    try:
        with open(path, encoding="utf-8") as fh:
            for raw in fh:
                line = raw.strip()
                if not line or line.startswith("#") or "=" not in line:
                    continue
                key, value = line.split("=", 1)
                key = key.strip()
                value = value.strip().strip('"').strip("'")
                if key not in os.environ:
                    os.environ[key] = value
    except FileNotFoundError:
        pass


def request_json(url, headers, timeout=20):
    req = urllib.request.Request(url, headers=headers, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body)
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            detail = json.loads(body)
        except json.JSONDecodeError:
            detail = {"raw": body[:300]}
        return exc.code, {"error": detail}


def number(value, default=0.0):
    try:
        if isinstance(value, str):
            value = value.replace(",", "").replace("$", "").replace("%", "")
        return float(value)
    except (TypeError, ValueError):
        return default


def normalize_solana(items):
    result = []
    for rank, item in enumerate(items, 1):
        if not isinstance(item, dict):
            continue
        address = item.get("wallet") or item.get("address") or item.get("walletAddress")
        if not isinstance(address, str) or not (32 <= len(address) <= 44):
            continue
        # Base58 alphabet check; this is syntax validation, not RPC verification.
        if not re.fullmatch(r"[1-9A-HJ-NP-Za-km-z]+", address):
            continue
        period = item.get("period") or {}
        days = period.get("days") or {}
        realized = number(period.get("realized", item.get("realizedPnl")))
        win_rate = number(days.get("winRate", item.get("winRate")))
        trades = int(number(item.get("counts", {}).get("trades", item.get("trades"))))
        result.append({
            "address": address,
            "chain": "SOLANA",
            "rank": rank,
            "name": (item.get("identity") or {}).get("name") or "Solana Tracker Trader",
            "handle": "st_" + address[:10].lower(),
            "pnl": realized,
            "win_rate": win_rate,
            "trades": trades,
            "source": "SOLANA_TRACKER_LEADERBOARD",
        })
    return result


def normalize_okx(payload, chain_index):
    data = payload.get("data", payload.get("result", [])) if isinstance(payload, dict) else []
    if isinstance(data, dict):
        data = data.get("list", data.get("rows", []))
    chain = OKX_CHAIN_NAMES.get(str(chain_index), "UNKNOWN")
    result = []
    for rank, item in enumerate(data or [], 1):
        if not isinstance(item, dict):
            continue
        address = item.get("walletAddress") or item.get("address")
        if not isinstance(address, str):
            continue
        if chain == "SOLANA":
            valid_address = bool(re.fullmatch(r"[1-9A-HJ-NP-Za-km-z]{32,44}", address))
            handle = "okx_" + address[:10].lower()
        else:
            valid_address = bool(EVM_RE.fullmatch(address))
            handle = "okx_" + chain.lower() + "_" + address[2:10].lower() if valid_address else ""
        if not valid_address:
            continue
        result.append({
            "address": address,
            "chain": chain,
            "rank": rank,
            "name": "OKX Smart Money",
            "handle": handle,
            "pnl": number(item.get("realizedPnlUsd")),
            "win_rate": number(item.get("winRatePercent")),
            "trades": int(number(item.get("txs", item.get("txCount", item.get("transactionCount"))))),
            "source": "OKX_SMART_MONEY_LEADERBOARD",
        })
    return result


def fetch_solana(limit):
    key = os.getenv("SOLANA_TRACKER_API_KEY")
    if not key:
        return [], "SOLANA_TRACKER_API_KEY tidak tersedia"
    query = urllib.parse.urlencode({
        "sort": "realized", "direction": "desc", "limit": min(limit, 100),
        "excludeArbitrage": "true", "pnlMode": "strict", "days": "90",
        "minTrades": "20", "minDays": "3",
    })
    status, payload = request_json(SOL_TRACKER_URL + "?" + query, {"x-api-key": key, "Accept": "application/json"})
    if status != 200:
        return [], f"HTTP {status}: respons API ditolak"
    items = payload.get("traders", payload.get("data", [])) if isinstance(payload, dict) else []
    return normalize_solana(items), "ok"


def fetch_okx(limit, chain_index="501"):
    key = os.getenv("OKX_API_KEY")
    secret = os.getenv("OKX_SECRET_KEY")
    passphrase = os.getenv("OKX_PASSPHRASE")
    if not key or not secret or not passphrase:
        return [], "OKX_API_KEY/OKX_SECRET_KEY/OKX_PASSPHRASE belum lengkap"

    # OKX V5 signature: Base64(HMAC-SHA256(timestamp + method + request_path + body)).
    params = urllib.parse.urlencode({
        "chainIndex": str(chain_index), "timeFrame": "3", "sortBy": "1",
        "walletType": "3", "minRealizedPnlUsd": "10000", "minTxs": "20",
        "limit": min(limit, 20),
    })
    request_path = "/api/v6/dex/market/leaderboard/list?" + params
    timestamp = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
    prehash = timestamp + "GET" + request_path
    signature = base64.b64encode(hmac.new(secret.encode(), prehash.encode(), hashlib.sha256).digest()).decode()
    status, payload = request_json(OKX_HOST + request_path, {
        "OK-ACCESS-KEY": key,
        "OK-ACCESS-SIGN": signature,
        "OK-ACCESS-TIMESTAMP": timestamp,
        "OK-ACCESS-PASSPHRASE": passphrase,
        "Accept": "application/json",
        "User-Agent": "ZenithSW-leaderboard-harvester/1.0",
    })
    if status != 200:
        return [], f"HTTP {status}: OKX signature/request ditolak"
    return normalize_okx(payload, chain_index), "ok"


def quality_filter(wallets):
    accepted = []
    seen = set()
    for wallet in wallets:
        identity = (wallet["chain"], wallet["address"].lower())
        if identity in seen:
            continue
        seen.add(identity)
        if wallet["pnl"] < 10000 or wallet["trades"] < 20:
            continue
        if wallet["win_rate"] and wallet["win_rate"] < 55:
            continue
        accepted.append(wallet)
    return accepted


def ingest(wallets, db_path):
    now = datetime.now(timezone.utc).isoformat()
    conn = sqlite3.connect(db_path)
    try:
        for w in wallets:
            conn.execute("""
                INSERT INTO wallets
                (rank, name, handle, chain, address, tier, pnl_24h_usd,
                 win_rate_pct, total_trades, source, reputation_score,
                 is_active, last_seen_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
                ON CONFLICT(handle) DO UPDATE SET
                  rank=excluded.rank, name=excluded.name, chain=excluded.chain,
                  address=excluded.address, tier=excluded.tier,
                  pnl_24h_usd=excluded.pnl_24h_usd,
                  win_rate_pct=excluded.win_rate_pct,
                  total_trades=excluded.total_trades, source=excluded.source,
                  reputation_score=excluded.reputation_score,
                  is_active=1, last_seen_at=excluded.last_seen_at,
                  updated_at=excluded.updated_at
            """, (
                w["rank"], w["name"], w["handle"], w["chain"], w["address"],
                "TierS" if w["pnl"] >= 50000 else "TierA",
                w["pnl"], w["win_rate"], w["trades"], w["source"],
                min(99.0, max(80.0, 70.0 + min(w["pnl"] / 10000.0, 29.0))), now, now,
            ))
        conn.commit()
    finally:
        conn.close()


def main():
    parser = argparse.ArgumentParser(description="Harvest leaderboard wallet addresses")
    parser.add_argument("--execute", action="store_true", help="Tulis wallet lolos filter ke SQLite")
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--db", default=DB_DEFAULT)
    args = parser.parse_args()
    load_dotenv_keys()
    sol, sol_status = fetch_solana(args.limit)
    okx = []
    okx_statuses = []
    for chain_index in ("501", "1", "42161", "8453"):
        batch, status = fetch_okx(args.limit, chain_index)
        okx.extend(batch)
        okx_statuses.append(f"{OKX_CHAIN_NAMES[chain_index]}={status}/{len(batch)}")
    accepted = quality_filter(sol + okx)
    print(f"[SOLANA TRACKER] {sol_status}; candidate_valid={len(sol)}")
    print(f"[OKX] {'; '.join(okx_statuses)}; candidate_valid={len(okx)}")
    print(f"[FILTER] accepted={len(accepted)}; mode={'EXECUTE' if args.execute else 'DRY-RUN'}")
    if args.execute and accepted:
        ingest(accepted, args.db)
        print(f"[INGEST] inserted_or_updated={len(accepted)} database={args.db}")
    elif args.execute:
        print("[INGEST] tidak ada wallet yang aman untuk diinsert")
    for w in accepted[:10]:
        print(f"  {w['chain']} rank={w['rank']} address={w['address']} pnl=${w['pnl']:.2f} source={w['source']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
