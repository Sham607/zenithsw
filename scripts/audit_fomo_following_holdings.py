#!/usr/bin/env python3
"""Audit read-only FOMO following dan feed swaps.

Token dibaca dari prompt tersembunyi atau environment FOMO_BEARER.
Tidak menulis token, tidak mengubah database, dan berhenti pada 401/403/429.
"""
import getpass
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone

BASE = "https://prod-api.fomo.family/v2"
FOLLOWING_USER_ID = "a649f1f6-e5ee-5732-8e93-b0d0a1522560"
OUT = os.path.join(os.path.dirname(os.path.dirname(__file__)), "data", "fomo_following_holdings_audit.json")
DELAY_SECONDS = 5.0


def bearer_token():
    token = os.environ.get("FOMO_BEARER", "").strip()
    if not token:
        token = getpass.getpass("FOMO Bearer token (hidden): ").strip()
    if token.lower().startswith("bearer "):
        token = token[7:].strip()
    if not token:
        raise SystemExit("Token kosong; audit dibatalkan.")
    return token


def fetch(url, token):
    headers = {
        "Authorization": "Bearer " + token,
        "Accept": "application/json, text/plain, */*",
        "User-Agent": "Mozilla/5.0 (audit-read-only)",
        "Origin": "https://fomo.family",
        "Referer": "https://fomo.family/",
    }
    request = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status, json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        if exc.code in (401, 403, 429):
            raise RuntimeError("STOP_HTTP_" + str(exc.code)) from exc
        raise


def extract_users(payload):
    response = payload.get("responseObject", {}) if isinstance(payload, dict) else {}
    for key in ("users", "following", "items", "data"):
        value = response.get(key) if isinstance(response, dict) else None
        if isinstance(value, list):
            return value
    if isinstance(response, list):
        return response
    return []


def main():
    token = bearer_token()
    result = {
        "audited_at": datetime.now(timezone.utc).isoformat(),
        "source": "FOMO following + validated swaps endpoint",
        "database_mutated": False,
        "rate_limit_safe": True,
        "users": [],
    }
    following_url = f"{BASE}/users/{FOLLOWING_USER_ID}/followingPaginate"
    print("[1/2] Mengambil daftar following satu kali...")
    try:
        status, payload = fetch(following_url, token)
    except RuntimeError as exc:
        print(f"Audit dihentikan: {exc}")
        return 2
    users = extract_users(payload)
    print(f"Following diterima: {len(users)}")
    print("[2/2] Mengambil swaps dengan jeda konservatif...")

    for number, user in enumerate(users, 1):
        user_id = user.get("id") or user.get("userId")
        handle = user.get("userHandle") or user.get("handle") or user.get("username")
        item = {
            "id": user_id,
            "handle": handle,
            "profile_solana_address": user.get("address"),
            "profile_evm_address": user.get("evmAddress"),
            "profile_metrics": {k: user.get(k) for k in ("numTrades", "swapCount", "totalVolume", "pnl24h", "isRestricted")},
            "swaps_status": "not_requested",
            "swaps": [],
        }
        if not user_id:
            item["swaps_status"] = "missing_user_id"
        else:
            try:
                _, swaps_payload = fetch(f"{BASE}/users/{user_id}/swaps", token)
                response = swaps_payload.get("responseObject", {})
                item["swaps"] = response.get("swaps", []) if isinstance(response, dict) else []
                item["swaps_status"] = "ok"
                print(f"  {number}/{len(users)} @{handle}: {len(item['swaps'])} swaps")
            except RuntimeError as exc:
                item["swaps_status"] = str(exc)
                result["users"].append(item)
                print(f"  Audit dihentikan pada @{handle}: {exc}")
                break
            except Exception:
                item["swaps_status"] = "request_error"
                print(f"  {number}/{len(users)} @{handle}: request error")
        result["users"].append(item)
        if number < len(users):
            time.sleep(DELAY_SECONDS)

    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as output:
        json.dump(result, output, indent=2, ensure_ascii=False)
    print(f"Laporan disimpan: {OUT}")
    print(f"Selesai: {len(result['users'])}/{len(users)} akun")
    print("Database tidak diubah.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
