import urllib.request
import json
import time

tokens_seed = [
    # Symbol, Mint, Chain
    ("JUGGERNAUT", "0xd7321801caae694090694ff55a9323139f043b88", "ROBINHOOD", 8, 1850000.0, "@Aurelius0121, @unipcs, @Bane"),
    ("AI", "0x2e8c3116892607fa668478d8a7c13284ff111e18", "ROBINHOOD", 17, 29001833.0, "@DumbCrayonEater, @Salem1299534, @AvgJoesCrypto"),
    ("PONS", "0x39dbed3a2bd333467115de45665cc57f813c4571", "ROBINHOOD", 11, 27686773.0, "@unipcs, @AvgJoesCrypto, @theveeman"),
    ("STONK", "6GmAFSYs4ggBwcm2UGLx91g4YHgj5p75B2kX1RMpUNgx", "SOLANA", 6, 15181447.0, "@pointfarmcap, @The__Solstice, @theveeman"),
    ("MarsCoin", "0xfe189e97832da1573e4e4ff034f4ffc3a15c7777", "ROBINHOOD", 6, 8197500.0, "@unipcs, @frogmanhaha, @0xAvast"),
    ("CATE", "Ai66LHZG9MSvA28uUsk4V896D76gTz866z38dZ5ppump", "SOLANA", 3, 3023342.0, "@PoorGoat_, @Aurelius0121, @ether_monk"),
    ("CASHCAT", "0x020bfc65cb1dfd694cb5c6b907fe23f5b71018b4", "ROBINHOOD", 3, 2816830.0, "@frogmanhaha, @0xAvast, @change"),
    ("BONER", "0x98096d17b508f7ce69830509caeaecb030b11e18", "ROBINHOOD", 3, 1747061.0, "@Chubbi230, @CardinalSaint2, @econoar"),
    ("ZZZ", "0x7dbf38977bb505e608ea71aa2687c4a03509ee6a", "ROBINHOOD", 3, 984409.0, "@ogle, @0xAvast, @Quanterty"),
    ("PURR", "8RNUw4N655Nq5Vp44kH8FmX3H6kY2YJ1eA2kME2NZHQ", "SOLANA", 3, 577381.0, "@pointfarmcap, @motiyawerey, @DriftyBearSkis"),
    ("ZCAT", "HcRLc9VDgj32Xf9pCspQnL1y9Nf9QdD9pQiDeJR", "SOLANA", 7, 5268319.0, "@pointfarmcap, @motiyawerey, @DriftyBearSkis"),
    ("USELESS", "Dz9mQ9NzkBcCsuGPFJ3r1bS4wgqKMHBPiVuniW8Mbonk", "SOLANA", 2, 4823198.0, "@unipcs, @FullPinkYak"),
    ("MOO", "0xd9db30bb8b88dc369c71a3bcfeac09df8c791e18", "ROBINHOOD", 4, 1054093.0, "@AvgJoesCrypto, @notanicecat69, @picadura"),
]

with open("data/default_whale_gems.json", "w") as f:
    json.dump(tokens_seed, f, indent=2)

print(f"Saved {len(tokens_seed)} high-conviction whale gems seed file to data/default_whale_gems.json")
