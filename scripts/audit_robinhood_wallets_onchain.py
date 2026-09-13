#!/usr/bin/env python3
"""Read-only Robinhood wallet forensic audit via Alchemy Transfers + receipts.

Never writes to wallet_registry.db. Produces audit artifacts only.
"""
import concurrent.futures, csv, json, os, sqlite3, sys, time
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "data/wallet_registry.db"
OUT = ROOT / "data/audit"
def load_env_file():
    env_path = ROOT / ".env"
    if not env_path.exists():
        return
    for line in env_path.read_text(errors="ignore").splitlines():
        if not line.strip() or line.lstrip().startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip().strip('"').strip("'"))

load_env_file()
RPC = os.environ.get("ROBINHOOD_RPC_URL", "")
LATEST = None
ROUTERS = {
    "0x8876789976decbfcbbbe364623c63652db8c0904": "UNIVERSAL_ROUTER",
    "0xcaf681a66d020601342297493863e78c959e5cb2": "SWAP_ROUTER_02",
    "0xe33e9e479df8802cb0866d5d05258bec4cf62948": "PONS_V2_LAUNCH_ROUTER",
    "0xc7819b64a1daecd7ec19856d026cb14efbd89046": "PONS_GRADUATION",
    "0x8366a39cc670b4001a1121b8f6a443a643e40951": "V4_POOL_MANAGER",
}

def rpc(method, params):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode()
    req = Request(RPC, data=body, headers={"Content-Type":"application/json"})
    with urlopen(req, timeout=30) as r:
        data = json.loads(r.read())
    if data.get("error"):
        raise RuntimeError(data["error"])
    return data.get("result")

def transfers(address, direction, max_pages=10):
    out=[]; page=None; truncated=False
    for _ in range(max_pages):
        q={"category":["external","erc20","erc721","erc1155"],"withMetadata":True,
           "maxCount":"0x3e8","order":"desc"}
        q["fromAddress" if direction=="out" else "toAddress"] = address
        if page: q["pageKey"] = page
        data=rpc("alchemy_getAssetTransfers",[q]) or {}
        batch=data.get("transfers",[]); out.extend(batch)
        page=data.get("pageKey")
        if not page or not batch: break
    else: truncated=True
    return out,truncated

def load_wallets():
    c=sqlite3.connect(f"file:{DB}?mode=ro",uri=True)
    c.row_factory=sqlite3.Row
    rows=c.execute("select id,handle,address,chain,tier,source,is_active,execution_track from wallets where lower(chain)='robinhood'").fetchall()
    c.close(); return [dict(x) for x in rows]

def audit_one(w):
    address=w["address"]
    if not isinstance(address,str) or len(address)!=42: return {"wallet":w,"error":"invalid_address"}
    try:
        code=rpc("eth_getCode",[address,"latest"])
        is_contract=code not in (None,"0x","0x0")
    except Exception as e: return {"wallet":w,"error":"code_query:"+str(e)[:120]}
    try:
        inc,tr_in=transfers(address,"in"); out,tr_out=transfers(address,"out")
        tx={}
        for x in inc+out:
            h=x.get("hash")
            if h: tx.setdefault(h,{"hash":h,"block":x.get("blockNum"),"timestamp":x.get("metadata",{}).get("blockTimestamp"),"transfers":[]})["transfers"].append(x)
        router_txs=[]; token_in=0; token_out=0
        for h,v in tx.items():
            tos={str(x.get("to","")).lower() for x in v["transfers"]}
            frs={str(x.get("from","")).lower() for x in v["transfers"]}
            venue=next((ROUTERS[a] for a in ROUTERS if a in tos or a in frs),None)
            if venue: router_txs.append({"hash":h,"venue":venue,"block":v["block"],"timestamp":v["timestamp"],"transfers":v["transfers"]})
            token_in += sum(1 for x in v["transfers"] if x.get("category")=="erc20" and str(x.get("to","")).lower()==address.lower())
            token_out += sum(1 for x in v["transfers"] if x.get("category")=="erc20" and str(x.get("from","")).lower()==address.lower())
        return {"wallet":w,"is_contract":is_contract,"incoming_count":len(inc),"outgoing_count":len(out),"unique_tx_count":len(tx),"verified_route_candidate_count":len(router_txs),"token_in_events":token_in,"token_out_events":token_out,"truncated":tr_in or tr_out,"route_candidates":router_txs}
    except Exception as e: return {"wallet":w,"is_contract":is_contract,"error":"transfer_query:"+str(e)[:160]}

def main():
    global RPC
    if not RPC: raise SystemExit("ROBINHOOD_RPC_URL missing")
    OUT.mkdir(parents=True,exist_ok=True)
    wallets=load_wallets(); latest=rpc("eth_blockNumber",[])
    started=datetime.now(timezone.utc).isoformat()
    results=[]
    with concurrent.futures.ThreadPoolExecutor(max_workers=12) as ex:
        futs=[ex.submit(audit_one,w) for w in wallets]
        for i,f in enumerate(concurrent.futures.as_completed(futs),1):
            try: results.append(f.result())
            except Exception as e: results.append({"error":str(e)})
            if i%50==0: print(f"audited {i}/{len(wallets)}",flush=True)
    results.sort(key=lambda x:x.get("wallet",{}).get("id",0))
    manifest={"timestamp":started,"chain_id":4663,"latest_block":latest,"registry":str(DB),"wallet_count":len(wallets),"read_only":True,"results":len(results)}
    (OUT/"robinhood_wallet_onchain_audit.json").write_text(json.dumps({"manifest":manifest,"results":results},indent=2))
    with (OUT/"robinhood_wallet_onchain_summary.csv").open("w",newline="") as fh:
        fields=["id","handle","address","tier","source","is_active","is_contract","unique_tx_count","verified_route_candidate_count","token_in_events","token_out_events","truncated","error"]
        wr=csv.DictWriter(fh,fieldnames=fields); wr.writeheader()
        for r in results:
            w=r.get("wallet",{}); wr.writerow({k:r.get(k,w.get(k,"")) for k in fields})
    print(json.dumps({"audit_file":str(OUT/"robinhood_wallet_onchain_audit.json"),"summary_file":str(OUT/"robinhood_wallet_onchain_summary.csv"),"wallets":len(wallets),"results":len(results)}))
if __name__=="__main__": main()
