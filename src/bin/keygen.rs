use ed25519_dalek::SigningKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::SecretKey;
use rand::rngs::OsRng;
use sha3::{Digest, Keccak256};
use std::io::Write;

fn main() {
    println!("===========================================================");
    println!("🔐 ZENITHSW - CRYPTOGRAPHIC BURNER KEYPAIR GENERATOR");
    println!("===========================================================\n");

    // 1. Solana SVM Keypair (Ed25519)
    let sol_signing_key = SigningKey::generate(&mut OsRng);
    let sol_pubkey = sol_signing_key.verifying_key();
    let sol_pubkey_b58 = bs58::encode(sol_pubkey.as_bytes()).into_string();

    // Standard Solana 64-byte secret key (32 bytes seed + 32 bytes pubkey)
    let mut sol_keypair_bytes = Vec::with_capacity(64);
    sol_keypair_bytes.extend_from_slice(sol_signing_key.as_bytes());
    sol_keypair_bytes.extend_from_slice(sol_pubkey.as_bytes());
    let sol_privkey_b58 = bs58::encode(&sol_keypair_bytes).into_string();

    println!("☀️ [SOLANA SVM BURNER WALLET]");
    println!(
        "   • Public Address (Fund & Watch-Only): {}",
        sol_pubkey_b58
    );
    println!("   • Private Key Format: Base58 (64-byte Ed25519)");
    println!();

    // 2. Robinhood / Arbitrum EVM Keypair (Secp256k1 + Keccak256)
    let evm_secret = SecretKey::random(&mut OsRng);
    let evm_priv_hex = format!("0x{}", hex::encode(evm_secret.to_bytes()));
    let evm_pub_point = evm_secret
        .public_key()
        .to_projective()
        .to_encoded_point(false);
    let evm_pub_bytes = &evm_pub_point.as_bytes()[1..]; // Strip 0x04 uncompressed prefix (64 bytes)

    let mut hasher = Keccak256::new();
    hasher.update(evm_pub_bytes);
    let hash = hasher.finalize();
    let evm_addr_raw = &hash[12..32]; // Last 20 bytes
    let evm_address = format!("0x{}", hex::encode(evm_addr_raw));

    println!("🔷 [ROBINHOOD / ARBITRUM EVM BURNER WALLET]");
    println!("   • Public Address (Fund & Watch-Only): {}", evm_address);
    println!("   • Private Key Format: Hex 64-char (Secp256k1)");
    println!();

    // 3. Save directly to credentials directory & .env template safely
    let creds_json = serde_json::json!({
        "solana": {
            "public_key": sol_pubkey_b58,
            "private_key": sol_privkey_b58
        },
        "robinhood_evm": {
            "public_key": evm_address,
            "private_key": evm_priv_hex
        },
        "created_at": chrono::Utc::now().to_rfc3339()
    });

    let _ = std::fs::create_dir_all("data/credentials");
    let cred_path = "data/credentials/burner_keypairs.json";
    if let Ok(mut f) = std::fs::File::create(cred_path) {
        let _ = f.write_all(
            serde_json::to_string_pretty(&creds_json)
                .unwrap()
                .as_bytes(),
        );
        println!("💾 Burner credentials securely written to: {}", cred_path);
    }

    // 4. Update .env file automatically
    if let Ok(env_content) = std::fs::read_to_string(".env") {
        let mut new_lines = Vec::new();
        let mut has_sol_pk = false;
        let mut has_evm_pk = false;

        for line in env_content.lines() {
            if line.starts_with("SOLANA_PRIVATE_KEY=") {
                new_lines.push(format!("SOLANA_PRIVATE_KEY={}", sol_privkey_b58));
                has_sol_pk = true;
            } else if line.starts_with("EVM_PRIVATE_KEY=")
                || line.starts_with("ROBINHOOD_PRIVATE_KEY=")
            {
                new_lines.push(format!("EVM_PRIVATE_KEY={}", evm_priv_hex));
                has_evm_pk = true;
            } else {
                new_lines.push(line.to_string());
            }
        }

        if !has_sol_pk {
            new_lines.push(format!("SOLANA_PRIVATE_KEY={}", sol_privkey_b58));
        }
        if !has_evm_pk {
            new_lines.push(format!("EVM_PRIVATE_KEY={}", evm_priv_hex));
        }

        let _ = std::fs::write(".env", new_lines.join("\n") + "\n");
        println!("⚙️  Configured SOLANA_PRIVATE_KEY & EVM_PRIVATE_KEY directly into .env");
    }

    println!("===========================================================");
    println!("✅ SETUP BURNER KEYPAIR COMPLETE!");
    println!("===========================================================");
}
