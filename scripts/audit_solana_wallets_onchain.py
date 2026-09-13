#!/usr/bin/env python3
"""Read-only Solana wallet forensic discovery via Alchemy JSON-RPC."""
import concurrent.futures, json, os, sqlite3
from datetime import datetime, timezone
from pathlib import Path
from urllib.request import Request, urlopen

ROOT=Path(__file__).resolve().parents[1]; DB=ROOT/'data/wallet_registry.db'; OUT=ROOT/'data/audit'

def load_env():
 for l in (ROOT/'.env').read_text(errors='ignore').splitlines():
  if '=' in l and not l.lstrip().startswith('#'):
   k,v=l.split('=',1); os.environ.setdefault(k.strip(),v.strip().strip('"').strip("'"))
load_env(); RPC=os.environ.get('SOLANA_RPC_URL','')
if not RPC: raise SystemExit('SOLANA_RPC_URL missing')

def rpc(method,params):
 body=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode()
 req=Request(RPC,data=body,headers={'Content-Type':'application/json'})
 with urlopen(req,timeout=35) as r: data=json.loads(r.read())
 if data.get('error'): raise RuntimeError(data['error'])
 return data.get('result')

def wallet_rows():
 c=sqlite3.connect(f'file:{DB}?mode=ro',uri=True); c.row_factory=sqlite3.Row
 x=c.execute("select id,handle,address,tier,source,is_active,execution_track from wallets where lower(chain)='solana'").fetchall(); c.close(); return [dict(r) for r in x]

def one(w):
 a=w['address']; result={'wallet':w}
 try:
  sigs=[]; before=None; truncated=False
  for _ in range(20):
   q={'limit':1000}
   if before:q['before']=before
   batch=rpc('getSignaturesForAddress',[a,q]) or []
   sigs.extend(batch)
   if len(batch)<1000:break
   before=batch[-1]['signature']
  else:truncated=True
  good=[x for x in sigs if not x.get('err')]
  result.update({'signature_count':len(sigs),'successful_signature_count':len(good),'oldest_slot':sigs[-1].get('slot') if sigs else None,'newest_slot':sigs[0].get('slot') if sigs else None,'truncated':truncated})
 except Exception as e: result['error']=str(e)[:180]
 return result

def main():
 OUT.mkdir(exist_ok=True); ws=wallet_rows(); results=[]
 with concurrent.futures.ThreadPoolExecutor(max_workers=8) as ex:
  for i,r in enumerate(ex.map(one,ws),1):
   results.append(r)
   if i%25==0:print(f'audited {i}/{len(ws)}',flush=True)
 results.sort(key=lambda x:x.get('wallet',{}).get('id',0))
 manifest={'timestamp':datetime.now(timezone.utc).isoformat(),'chain':'SOLANA','rpc_read_only':True,'wallet_count':len(ws),'results':len(results)}
 (OUT/'solana_wallet_onchain_audit.json').write_text(json.dumps({'manifest':manifest,'results':results},indent=2))
 import csv
 with (OUT/'solana_wallet_onchain_summary.csv').open('w',newline='') as f:
  fields=['id','handle','address','tier','source','is_active','signature_count','successful_signature_count','oldest_slot','newest_slot','truncated','error']; wr=csv.DictWriter(f,fieldnames=fields);wr.writeheader()
  for r in results:
   w=r.get('wallet',{}); wr.writerow({k:r.get(k,w.get(k,'')) for k in fields})
 print(json.dumps({'audit_file':str(OUT/'solana_wallet_onchain_audit.json'),'summary_file':str(OUT/'solana_wallet_onchain_summary.csv'),'wallets':len(ws)}))
if __name__=='__main__':main()
