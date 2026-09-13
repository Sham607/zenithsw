#!/usr/bin/env python3
"""Fingerprint audit read-only dari laporan FOMO dan Solana RPC.
Tidak mengubah database dan tidak menulis credential."""
import json, os, urllib.request
from collections import defaultdict

ROOT=os.path.dirname(os.path.dirname(__file__))
AUDIT=os.path.join(ROOT,'data','fomo_following_holdings_audit.json')
OUT=os.path.join(ROOT,'data','fomo_fingerprint_audit.json')
RPC=os.environ.get('SOLANA_RPC_URL','https://api.mainnet-beta.solana.com')

def rpc(method, params):
    body=json.dumps({'jsonrpc':'2.0','id':1,'method':method,'params':params}).encode()
    req=urllib.request.Request(RPC,data=body,headers={'Content-Type':'application/json','User-Agent':'zenithsw-readonly-audit'})
    with urllib.request.urlopen(req,timeout=25) as r: return json.loads(r.read())

def main():
    data=json.load(open(AUDIT,encoding='utf-8'))
    report={'source':AUDIT,'database_mutated':False,'rpc':RPC,'users':[]}
    for user in data.get('users',[]):
        swaps=user.get('swaps',[])
        addresses=sorted({s.get('address') for s in swaps if s.get('address') and int(s.get('networkId') or 0)==1399811149})
        item={'handle':user.get('handle'),'profile_address':user.get('profile_solana_address'),'swap_addresses':addresses,'candidates':[]}
        for address in addresses:
            candidate={'address':address,'rpc_status':'error','token_accounts':[]}
            try:
                res=rpc('getTokenAccountsByOwner',[address,{'programId':'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'},{'encoding':'jsonParsed'}])
                value=res.get('result',{}).get('value',[])
                for row in value:
                    info=row.get('account',{}).get('data',{}).get('parsed',{}).get('info',{})
                    ta=info.get('tokenAmount',{})
                    amount=ta.get('uiAmount')
                    if amount and amount > 0:
                        candidate['token_accounts'].append({'token_account':row.get('pubkey'),'mint':info.get('mint'),'amount':amount,'raw_amount':ta.get('amount'),'decimals':ta.get('decimals')})
                candidate['rpc_status']='ok'
            except Exception as exc:
                candidate['error']=type(exc).__name__
            item['candidates'].append(candidate)
        report['users'].append(item)
    with open(OUT,'w',encoding='utf-8') as f: json.dump(report,f,indent=2)
    total=sum(len(c['token_accounts']) for u in report['users'] for c in u['candidates'])
    ok=sum(c['rpc_status']=='ok' for u in report['users'] for c in u['candidates'])
    print(f'users={len(report["users"])} candidates={ok} token_accounts={total}')
    print(f'output={OUT}')
    print('database_mutated=False')
if __name__=='__main__': main()
