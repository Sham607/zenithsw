import json

gems = [
    ("AI", "0x2e8cf3b2a26fa32d4b8e057f58d08c5c7d031e18", "ROBINHOOD", 17, 29001833.0, "@DumbCrayonEater, @Salem, @0xSun, @Aurelius0121"),
    ("PONS", "0x39dbed3a2bd333467115de45665cc57f813c4571", "ROBINHOOD", 11, 27686773.0, "@unipcs, @theveeman, @Aurelius0121, @L_T"),
    ("STONK", "6GmAFg6ZkEshd4v7rG4UeM1g8U4r9Fknx2Hk8JpLUNgx", "SOLANA", 6, 15181447.0, "@pointfarmcap, @The_Solstice, @0xSun"),
    ("MarsCoin", "0xfe189e97832da1573e4e4ff034f4ffc3a15c7777", "ROBINHOOD", 6, 8197500.0, "@unipcs, @0xAvast, @Aurelius0121"),
    ("USELESS", "Dz9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk", "SOLANA", 4, 4823198.0, "@unipcs, @Aurelius0121, @b3b3_2004"),
    ("CATE", "Ai66n5a5xwh86d34e68s56d2k89sd87sdpump", "SOLANA", 3, 3023342.0, "@Aurelius0121, @PoorGoat_, @100x_alts"),
    ("JUGGERNAUT", "0xd7321801caae694090694ff55a9323139f043b88", "ROBINHOOD", 8, 1850000.0, "@Aurelius0121, @unipcs, @Bane, @cryptobreakout_"),
    ("CASHCAT", "0x9c425e4c605be5332f3083dbd636c7a36cb74321", "ROBINHOOD", 3, 1420000.0, "@L_o_i_c, @b3b3_2004, @L_T"),
    ("ZZZ", "0x3f6285ad4ef7df1b0333215fe367e9976bb2d888", "ROBINHOOD", 2, 980000.0, "@DumbCrayonEater, @Salem"),
    ("BONER", "0x7890123456789012345678901234567890123456", "ROBINHOOD", 2, 650000.0, "@unipcs, @Bane"),
    ("4Stock", "0xd270f2095f9c1b3f9ff709ca0fc6f4284faaffff", "ROBINHOOD", 3, 298179.0, "@dreamloaderxo, @rasmr, @unipcs"),
    ("PURR", "0x1234567890abcdef1234567890abcdef12345678", "ROBINHOOD", 2, 250000.0, "@unipcs, @theveeman")
]

with open("data/default_whale_gems.json", "w") as f:
    json.dump(gems, f, indent=2)
print(f"Updated data/default_whale_gems.json with {len(gems)} high conviction gems")
