#!/usr/bin/env python3
"""Decode receipt-backed Robinhood swap candidates; read-only, no DB writes."""
import concurrent.futures,json,os,sqlite3,time
from pathlib import Path
from urllib.request import Request,urlopen
ROOT=Path(__file__).resolve().parents[1]; IN=ROOT/'data/audit/robinhood_wallet_onchain_audit.json'; OUT=ROOT/'data/audit/robinhood_verified_swaps.json'

def env():
 for l in (ROOT/'.env').read_text(errors='ignore').splitlines():
  if '=' in l and not l.lstrip().startswith('#'):
   k,v=l.split('=',1);os.environ.setdefault(k.strip(),v.strip().strip('"'))
env(); RPC=os.environ['ROBINHOOD_RPC_URL']
V4='0x40e9cecb9f5f1f1c5b9c97dec2917b7ee92e57ba5563708daca94dd84ad7112f'; V3='0xd78ad95fa46c994b6551d0da85fc275fe6131b0a'; TRANS='0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55aebc6f6f8'
def batch(calls):
 b=json.dumps(calls).encode();r=__import__('urllib.request').request.urlopen(Request(RPC,data=b,headers={'Content-Type':'application/json'}),timeout=60);return json.loads(r.read())
def main():
 d=json.loads(IN.read_text()); hashes={}
 for x in d['results']:
  w=x['wallet']
  if not(w['is_active'] and w['tier'] in ('TIER-S','TIER-A')):continue
  for t in x.get('route_candidates',[]): hashes[t['hash']]={'wallet':w,'candidate':t}
 items=list(hashes.items()); results=[]
 def one(chunk):
  calls=[{'jsonrpc':'2.0','id':i,'method':'eth_getTransactionReceipt','params':[h]} for i,(h,_) in enumerate(chunk)]
  try:return batch(calls)
  except Exception as e:return [{'id':-1,'error':str(e)}]
 chunks=[items[i:i+50] for i in range(0,len(items),50)]
 with concurrent.futures.ThreadPoolExecutor(max_workers=8) as ex:
  for n,(chunk,res) in enumerate(zip(chunks,ex.map(one,chunks)),1):
   byid={x.get('id'):x.get('result') for x in res if isinstance(x,dict)}
   for i,(h,meta) in enumerate(chunk):
    rec=byid.get(i)
    if not isinstance(rec,dict) or rec.get('status')!='0x1':continue
    logs=rec.get('logs',[])
    topics=[str(l.get('topics',[None])[0]).lower() for l in logs if l.get('topics')]
    swaps=[l for l in logs if l.get('topics') and str(l['topics'][0]).lower() in (V4.lower(),V3.lower())]
    if not swaps:continue
    results.append({'hash':h,'wallet':meta['wallet'],'candidate':meta['candidate'],'block':rec.get('blockNumber'),'swap_log_count':len(swaps),'transfer_log_count':sum(t==TRANS for t in topics),'router_to':rec.get('to')})
   if n%20==0:print('batches',n,'/',len(chunks),'verified',len(results),flush=True)
 OUT.write_text(json.dumps({'read_only':True,'wallet_scope':'active TIER-S/A','candidate_hashes':len(items),'verified_swaps':len(results),'results':results},indent=2));print('DONE',len(results))
if __name__=='__main__':main()
