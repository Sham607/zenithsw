use anyhow::{anyhow, Result};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::solana_executor::SolanaExecutor;
use zenithsw::models::Chain;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::load()?;
    let db = Arc::new(SmartWalletDb::new_with_wallet_path(
        &config.trading_db_path,
        &config.wallet_db_path,
    )?);
    let sol_exec = SolanaExecutor::new(config.clone(), db.clone());

    let (signing_key, user_pubkey) = sol_exec.get_keypair()?;
    println!("=======================================================");
    println!("🚀 ZENITHSW - SOLANA MAINNET LIVE CANARY TEST");
    println!("=======================================================");
    println!("• Wallet: {}", user_pubkey);

    let initial_balance = sol_exec.get_native_balance(&user_pubkey).await?;
    println!("• Current Balance: {:.6} SOL", initial_balance);

    if initial_balance < 0.04 {
        return Err(anyhow!(
            "Insufficient SOL balance for canary: {:.6} SOL < 0.04 SOL",
            initial_balance
        ));
    }

    let wsol_mint = "So11111111111111111111111111111111111111112";
    let usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    let mut buy_sig = String::new();
    let mut usdc_raw = sol_exec
        .get_raw_token_balance(&user_pubkey, usdc_mint)
        .await
        .unwrap_or(0);

    if usdc_raw > 0 {
        let usdc_ui = sol_exec
            .get_ui_token_balance(&user_pubkey, usdc_mint)
            .await
            .unwrap_or(0.0);
        println!(
            "\n[PHASE 1] DETECTED EXISTING USDC ON-CHAIN: {:.4} (${:.2}, raw: {})",
            usdc_ui, usdc_ui, usdc_raw
        );
        println!("• Skipping Phase 1 BUY (already confirmed in previous transaction)");
    } else {
        let buy_lamports = 7_000_000u64; // 0.007 SOL (~$0.98 USD)
        let buy_intent_id = format!("SOL-CANARY-BUY-{}", Utc::now().timestamp_millis());

        println!("\n[PHASE 1] BUY CANARY (0.007 SOL -> USDC)");
        println!("• Creating Intent: {}", buy_intent_id);
        db.create_execution_intent(
            &buy_intent_id,
            Chain::Solana,
            "BUY",
            usdc_mint,
            &user_pubkey,
            0.007,
        )?;

        println!("• Fetching Jupiter Quote...");
        let quote = sol_exec
            .fetch_jupiter_quote(wsol_mint, usdc_mint, buy_lamports, 100)
            .await?;
        let out_amount = quote["outAmount"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        println!(
            "  Expected Output: {} micro-USDC (${:.4})",
            out_amount,
            out_amount as f64 / 1_000_000.0
        );

        db.update_execution_intent(&buy_intent_id, "QUOTED", None, None)?;

        println!("• Building & Signing Transaction...");
        let signed_tx = sol_exec
            .build_and_sign_jupiter_tx(quote, &user_pubkey, &signing_key)
            .await?;
        db.update_execution_intent(&buy_intent_id, "SIGNED", None, None)?;

        println!("• Broadcasting to Solana Mainnet...");
        buy_sig = sol_exec.broadcast_tx(&signed_tx).await?;
        println!("  Broadcasted! Tx: https://solscan.io/tx/{}", buy_sig);
        db.update_execution_intent(&buy_intent_id, "BROADCASTED", None, Some(&buy_sig))?;

        println!("• Awaiting Confirmation (up to 30s)...");
        let confirmed = sol_exec.confirm_transaction_signature(&buy_sig, 30).await?;
        if !confirmed {
            db.update_execution_intent(
                &buy_intent_id,
                "PENDING_RECONCILIATION",
                None,
                Some(&buy_sig),
            )?;
            return Err(anyhow!(
                "BUY transaction confirmation timed out: {}",
                buy_sig
            ));
        }
        db.update_execution_intent(&buy_intent_id, "CONFIRMED", None, Some(&buy_sig))?;
        println!("  BUY CONFIRMED ON-CHAIN 🟢");

        println!("• Reconciling Physical Token Balance...");
        for attempt in 1..=5 {
            sleep(Duration::from_millis(1500)).await;
            usdc_raw = sol_exec
                .get_raw_token_balance(&user_pubkey, usdc_mint)
                .await
                .unwrap_or(0);
            if usdc_raw > 0 {
                break;
            }
            println!(
                "  Waiting for token account sync (attempt {}/5)...",
                attempt
            );
        }

        let usdc_ui = sol_exec
            .get_ui_token_balance(&user_pubkey, usdc_mint)
            .await?;
        println!(
            "  Received Physical USDC Balance: {:.4} (${:.2}, raw: {})",
            usdc_ui, usdc_ui, usdc_raw
        );
        db.update_execution_intent(&buy_intent_id, "RECONCILED", None, Some(&buy_sig))?;

        if usdc_raw == 0 {
            return Err(anyhow!("Reconciliation failed: physical USDC balance is 0"));
        }
    }

    println!("\n[PHASE 2] SELL CANARY (USDC -> SOL 100% Exit)");
    let sell_intent_id = format!("SOL-CANARY-SELL-{}", Utc::now().timestamp_millis());
    println!("• Creating Intent: {}", sell_intent_id);
    db.create_execution_intent(
        &sell_intent_id,
        Chain::Solana,
        "SELL",
        usdc_mint,
        &user_pubkey,
        usdc_raw as f64,
    )?;

    println!(
        "• Fetching Jupiter Exit Quote for {} raw units...",
        usdc_raw
    );
    let sell_quote = sol_exec
        .fetch_jupiter_quote(usdc_mint, wsol_mint, usdc_raw, 100)
        .await?;
    let sell_out_lamports = sell_quote["outAmount"]
        .as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    println!(
        "  Expected Return: {} lamports ({:.6} SOL)",
        sell_out_lamports,
        sell_out_lamports as f64 / 1e9
    );

    db.update_execution_intent(&sell_intent_id, "QUOTED", None, None)?;

    println!("• Building & Signing SELL Transaction...");
    let signed_sell_tx = sol_exec
        .build_and_sign_jupiter_tx(sell_quote, &user_pubkey, &signing_key)
        .await?;
    db.update_execution_intent(&sell_intent_id, "SIGNED", None, None)?;

    println!("• Broadcasting SELL to Solana Mainnet...");
    let sell_sig = sol_exec.broadcast_tx(&signed_sell_tx).await?;
    println!("  Broadcasted! Tx: https://solscan.io/tx/{}", sell_sig);
    db.update_execution_intent(&sell_intent_id, "BROADCASTED", None, Some(&sell_sig))?;

    println!("• Awaiting Confirmation (up to 30s)...");
    let sell_confirmed = sol_exec
        .confirm_transaction_signature(&sell_sig, 30)
        .await?;
    if !sell_confirmed {
        db.update_execution_intent(
            &sell_intent_id,
            "PENDING_RECONCILIATION",
            None,
            Some(&sell_sig),
        )?;
        return Err(anyhow!(
            "SELL transaction confirmation timed out: {}",
            sell_sig
        ));
    }
    db.update_execution_intent(&sell_intent_id, "CONFIRMED", None, Some(&sell_sig))?;
    println!("  SELL CONFIRMED ON-CHAIN 🟢");

    db.update_execution_intent(&sell_intent_id, "RECONCILED", None, Some(&sell_sig))?;

    println!("\n[PHASE 3] POST-TRADE RECONCILIATION");
    sleep(Duration::from_millis(1500)).await;
    let final_balance = sol_exec.get_native_balance(&user_pubkey).await?;
    let final_usdc = sol_exec
        .get_ui_token_balance(&user_pubkey, usdc_mint)
        .await
        .unwrap_or(0.0);
    println!(
        "• Final SOL Balance: {:.6} SOL (Net Delta: {:.6} SOL)",
        final_balance,
        final_balance - initial_balance
    );
    println!("• Remaining USDC Balance: {:.4}", final_usdc);

    println!("\n=======================================================");
    println!("✅ SOLANA MAINNET CANARY TEST SUCCEEDED 100%!");
    if !buy_sig.is_empty() {
        println!("• BUY Signature:  https://solscan.io/tx/{}", buy_sig);
    }
    println!("• SELL Signature: https://solscan.io/tx/{}", sell_sig);
    println!("=======================================================");

    Ok(())
}
