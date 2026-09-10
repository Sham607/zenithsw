import sqlite3
from datetime import datetime, timezone

wallets = [
    # Solana Elite Smart Money
    {"rank": 1, "name": "Unipcs 🎯", "handle": "unipcs", "chain": "SOLANA", "address": "FJDy9FDRy6bwGEUKuAC98bUtN8MkpE2pT7Dj7HE3Z7Q1", "tier": "TIER-S", "pnl_24h_usd": 16090000.0, "win_rate_pct": 87.8, "total_trades": 3857, "source": "ONCHAIN"},
    {"rank": 2, "name": "DumbCrayonEater 🐋", "handle": "DumbCrayonEater", "chain": "SOLANA", "address": "43nktK56Bfk3yJK9iqr3Kzyt9npgEB6sPYKAeoYMxeug", "tier": "TIER-S", "pnl_24h_usd": 7420000.0, "win_rate_pct": 87.5, "total_trades": 2969, "source": "ONCHAIN"},
    {"rank": 3, "name": "point farm capital 💎", "handle": "pointfarmcap", "chain": "SOLANA", "address": "6cerGpSGzCrA1NeHUNtyoNm4rWQx5SnK3DSAXb4e615t", "tier": "TIER-S", "pnl_24h_usd": 7200000.0, "win_rate_pct": 87.3, "total_trades": 2529, "source": "ONCHAIN"},
    {"rank": 4, "name": "Salem ⚡", "handle": "Salem1299534", "chain": "SOLANA", "address": "CDY1JBoi2fKPPsHabyANmp6RP9Lkz8TcrkACsX3rHkka", "tier": "TIER-A", "pnl_24h_usd": 5350000.0, "win_rate_pct": 87.0, "total_trades": 1757, "source": "ONCHAIN"},
    {"rank": 5, "name": "TheS◎Lstice 🌞", "handle": "The__Solstice", "chain": "SOLANA", "address": "GmfY52CC5BKt43xt9GSmZWNzsAv4ywE6qgguRM5fvAtN", "tier": "TIER-A", "pnl_24h_usd": 5060000.0, "win_rate_pct": 86.8, "total_trades": 1918, "source": "ONCHAIN"},
    {"rank": 6, "name": "Sol Sniper Alpha 🎯", "handle": "sol_sniper_alpha", "chain": "SOLANA", "address": "9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk3m", "tier": "TIER-A", "pnl_24h_usd": 4820000.0, "win_rate_pct": 86.0, "total_trades": 1550, "source": "ONCHAIN"},
    {"rank": 7, "name": "Sol Whale Prime 🐋", "handle": "sol_whale_prime", "chain": "SOLANA", "address": "EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i", "tier": "TIER-A", "pnl_24h_usd": 4410000.0, "win_rate_pct": 85.8, "total_trades": 5510, "source": "ONCHAIN"},
    {"rank": 8, "name": "Nansen Smart Trader 🧠", "handle": "nsn_ec2f_92mj", "chain": "SOLANA", "address": "G3tVaeeeFagPtEHcAA2iGm9bg1AscXnHZmkQjEkqcxuT", "tier": "TIER-S", "pnl_24h_usd": 3870000.0, "win_rate_pct": 85.3, "total_trades": 3062, "source": "NANSEN"},

    # Robinhood / Arbitrum Nitro DEX Smart Wallets
    {"rank": 9, "name": "RH Whale Alpha ⚡", "handle": "rh_0x2cff_2680", "chain": "ROBINHOOD", "address": "0x2cff890f0378a11913b6129b2e97417a2c302680", "tier": "TIER-S", "pnl_24h_usd": 3220000.0, "win_rate_pct": 84.8, "total_trades": 1229, "source": "ONCHAIN"},
    {"rank": 10, "name": "RH DEX Trader 🎯", "handle": "rh_0x2568_74db", "chain": "ROBINHOOD", "address": "0x25685517173e34b9868f0372df377395c52c74db", "tier": "TIER-A", "pnl_24h_usd": 2850000.0, "win_rate_pct": 82.5, "total_trades": 940, "source": "ONCHAIN"},
    {"rank": 11, "name": "RH Smart Arbitrage 💎", "handle": "rh_0x1ab4_8f23", "chain": "ROBINHOOD", "address": "0x1ab4c885566f7fbc329a288c3a137887e5b38f23", "tier": "TIER-A", "pnl_24h_usd": 2410000.0, "win_rate_pct": 81.0, "total_trades": 630, "source": "ONCHAIN"},
    {"rank": 12, "name": "RH Flow Master 🌊", "handle": "rh_0x7163_bc2d", "chain": "ROBINHOOD", "address": "0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D", "tier": "TIER-A", "pnl_24h_usd": 1950000.0, "win_rate_pct": 80.0, "total_trades": 450, "source": "ONCHAIN"},
]

conn = sqlite3.connect("data/smart_wallets.db")
c = conn.cursor()

for w in wallets:
    c.execute("""
        INSERT INTO wallets (rank, name, handle, chain, address, tier, pnl_24h_usd, win_rate_pct, total_trades, source, is_active, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, datetime('now'))
        ON CONFLICT(handle) DO UPDATE SET
            rank = excluded.rank,
            name = excluded.name,
            chain = excluded.chain,
            address = excluded.address,
            tier = excluded.tier,
            pnl_24h_usd = excluded.pnl_24h_usd,
            win_rate_pct = excluded.win_rate_pct,
            total_trades = excluded.total_trades,
            source = excluded.source,
            updated_at = excluded.updated_at
    """, (w["rank"], w["name"], w["handle"], w["chain"], w["address"], w["tier"], w["pnl_24h_usd"], w["win_rate_pct"], w["total_trades"], w["source"]))

conn.commit()
conn.close()
print(f"Successfully populated {len(wallets)} elite smart wallets into smart_wallets.db!")
