const { Connection, Keypair, PublicKey, Transaction, sendAndConfirmTransaction } = require("@solana/web3.js");
const { createCloseAccountInstruction, TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID } = require("@solana/spl-token");
const bs58Raw = require("bs58");
const bs58 = bs58Raw.default || bs58Raw;
const fs = require("fs");
const path = require("path");

// Load .env manually
const envPath = path.join(__dirname, "../.env");
const envContent = fs.readFileSync(envPath, "utf8");
let solanaPrivateKey = null;
for (const line of envContent.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("SOLANA_PRIVATE_KEY=")) {
        solanaPrivateKey = trimmed.split("=")[1].replace(/["']/g, "").trim();
    }
}

if (!solanaPrivateKey) {
    console.error("SOLANA_PRIVATE_KEY not found in .env");
    process.exit(1);
}

const secretKey = bs58.decode(solanaPrivateKey);
const keypair = Keypair.fromSecretKey(secretKey);
const ownerPubkey = keypair.publicKey;

console.log(`Wallet Public Key: ${ownerPubkey.toBase58()}`);

const connection = new Connection("https://api.mainnet-beta.solana.com", "confirmed");

async function run() {
    const balanceBefore = await connection.getBalance(ownerPubkey);
    console.log(`Initial SOL Balance: ${(balanceBefore / 1e9).toFixed(6)} SOL`);

    // Fetch SPL Token and Token-2022 accounts
    const programs = [
        { name: "SPL Token", id: TOKEN_PROGRAM_ID },
        { name: "Token-2022", id: TOKEN_2022_PROGRAM_ID }
    ];

    const toClose = [];
    const activePositions = [];

    for (const prog of programs) {
        const resp = await connection.getParsedTokenAccountsByOwner(ownerPubkey, { programId: prog.id });
        for (const item of resp.value) {
            const accPubkey = item.pubkey;
            const parsed = item.account.data.parsed.info;
            const mint = parsed.mint;
            const amount = parsed.tokenAmount.amount; // raw string
            const uiAmount = parsed.tokenAmount.uiAmount;

            if (amount === "0" || uiAmount === 0 || !amount) {
                toClose.push({
                    pubkey: accPubkey,
                    mint: mint,
                    programId: prog.id,
                    programName: prog.name
                });
            } else {
                activePositions.push({
                    pubkey: accPubkey.toBase58(),
                    mint: mint,
                    uiAmount: uiAmount,
                    programName: prog.name
                });
            }
        }
    }

    console.log(`\n=== ACTIVE POSITIONS (SAFEGUARD - WILL NOT BE TOUCHED): ${activePositions.length} ===`);
    for (const pos of activePositions) {
        console.log(`[PROTECTED] Mint: ${pos.mint} | Balance: ${pos.uiAmount} (${pos.programName})`);
    }

    console.log(`\n=== EMPTY ACCOUNTS TO CLOSE (RENT RECLAIM): ${toClose.length} ===`);
    for (const c of toClose) {
        console.log(`[TARGET] Account: ${c.pubkey.toBase58()} | Mint: ${c.mint} (${c.programName})`);
    }

    if (toClose.length === 0) {
        console.log("No empty token accounts found to close.");
        return;
    }

    const isDryRun = process.argv.includes("--dry-run");
    if (isDryRun) {
        console.log(`\n[DRY-RUN] Would close ${toClose.length} accounts and reclaim ~${(toClose.length * 0.002039).toFixed(6)} SOL.`);
        return;
    }

    // Execute closing in batches of 5 instructions per transaction
    const BATCH_SIZE = 5;
    for (let i = 0; i < toClose.length; i += BATCH_SIZE) {
        const batch = toClose.slice(i, i + BATCH_SIZE);
        const tx = new Transaction();

        for (const item of batch) {
            const ix = createCloseAccountInstruction(
                item.pubkey,      // account to close
                ownerPubkey,      // destination for rent lamports
                ownerPubkey,      // owner of account
                [],               // multiSigners
                item.programId    // token program ID
            );
            tx.add(ix);
        }

        console.log(`\nSubmitting transaction to close batch ${Math.floor(i / BATCH_SIZE) + 1} (${batch.length} accounts)...`);
        const sig = await sendAndConfirmTransaction(connection, tx, [keypair], {
            commitment: "confirmed"
        });
        console.log(`Success! Signature: https://solscan.io/tx/${sig}`);
    }

    const balanceAfter = await connection.getBalance(ownerPubkey);
    const reclaimed = (balanceAfter - balanceBefore) / 1e9;
    console.log(`\n=== RECLAIM COMPLETED ===`);
    console.log(`Final SOL Balance: ${(balanceAfter / 1e9).toFixed(6)} SOL`);
    console.log(`Net SOL Reclaimed: +${reclaimed.toFixed(6)} SOL`);
}

run().catch(err => {
    console.error("Execution error:", err);
    process.exit(1);
});
