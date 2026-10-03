use anyhow::{anyhow, Result};
use std::sync::Arc;
use zenithsw::config::Config;
use zenithsw::db::SmartWalletDb;
use zenithsw::execution::solana_executor::SolanaExecutor;

const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// 0.007 SOL ukuran canary BUY.
const CANARY_BUY_LAMPORTS: u64 = 7_000_000;
/// Cadangan gas minimum di luar ukuran canary.
const CANARY_GAS_BUFFER_LAMPORTS: u64 = 40_000_000;
const CANARY_SLIPPAGE_BPS: u16 = 100;

/// Canary live BUY (SOL→USDC) lalu SELL (USDC→SOL) melalui pipeline swap
/// yang sama dengan engine. Gagal keras bila salah satu fase tidak terbukti on-chain.
#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::load()?;
    let db = Arc::new(SmartWalletDb::new_with_wallet_path(
        &config.trading_db_path,
        &config.wallet_db_path,
    )?);
    let sol_exec = SolanaExecutor::new(config.clone(), db.clone());
    let (_, user_pubkey) = sol_exec.get_keypair()?;

    println!("=======================================================");
    println!("🚀 ZENITHSW - SOLANA MAINNET LIVE CANARY (pipeline engine)");
    println!("=======================================================");
    println!("• Wallet: {user_pubkey}");
    println!("• Trading DB: {}", config.trading_db_path);

    let initial_lamports = sol_exec.get_native_balance_lamports(&user_pubkey).await?;
    let initial_usdc_raw = sol_exec
        .get_raw_token_balance(&user_pubkey, USDC_MINT)
        .await?;
    println!(
        "• Saldo awal: {} lamports ({:.6} SOL), USDC raw {}",
        initial_lamports,
        initial_lamports as f64 / 1e9,
        initial_usdc_raw
    );
    if initial_lamports < CANARY_BUY_LAMPORTS + CANARY_GAS_BUFFER_LAMPORTS {
        return Err(anyhow!(
            "saldo tidak cukup untuk canary: {initial_lamports} lamports"
        ));
    }

    let args: Vec<String> = std::env::args().collect();
    let recover_sell_only = args.iter().any(|arg| arg == "--sell-only");
    let requested_sell_raw = args
        .windows(2)
        .find(|pair| pair[0] == "--amount-raw")
        .map(|pair| pair[1].parse::<u64>())
        .transpose()
        .map_err(|error| anyhow!("--amount-raw invalid: {error}"))?;

    let (bought_raw, buy_lamports_delta) = if recover_sell_only {
        let amount = requested_sell_raw.unwrap_or(initial_usdc_raw);
        if amount == 0 || amount > initial_usdc_raw {
            return Err(anyhow!(
                "jumlah SELL invalid: {amount}, saldo USDC raw {initial_usdc_raw}"
            ));
        }
        println!("\n[PHASE 1] DILEWATI — recovery SELL {amount} raw USDC");
        (amount, 0)
    } else {
        println!("\n[PHASE 1] BUY 0.007 SOL -> USDC");
        let (buy_sig, buy_fill) = sol_exec
            .execute_canary_swap(
                "BUY",
                WSOL_MINT,
                USDC_MINT,
                USDC_MINT,
                CANARY_BUY_LAMPORTS,
                CANARY_SLIPPAGE_BPS,
            )
            .await?;
        println!("• Tx: https://solscan.io/tx/{buy_sig}");
        println!(
            "• Fill meta: token_raw_delta={} lamports_delta={} fee={} decimals={:?}",
            buy_fill.token_raw_delta,
            buy_fill.owner_lamports_delta,
            buy_fill.fee_lamports,
            buy_fill.token_decimals
        );
        let amount = u64::try_from(buy_fill.token_raw_delta)
            .ok()
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow!("BUY fill tidak menghasilkan token positif"))?;
        (amount, buy_fill.owner_lamports_delta)
    };

    println!("\n[PHASE 2] SELL {bought_raw} raw USDC -> SOL");
    let (sell_sig, sell_fill) = sol_exec
        .execute_canary_swap(
            "SELL",
            USDC_MINT,
            WSOL_MINT,
            USDC_MINT,
            bought_raw,
            CANARY_SLIPPAGE_BPS,
        )
        .await?;
    println!("• Tx: https://solscan.io/tx/{sell_sig}");
    println!(
        "• Fill meta: token_raw_delta={} lamports_delta={} fee={}",
        sell_fill.token_raw_delta, sell_fill.owner_lamports_delta, sell_fill.fee_lamports
    );
    if sell_fill.token_raw_delta != -i128::from(bought_raw) {
        return Err(anyhow!(
            "SELL fill mismatch: token_raw_delta {} != -{}",
            sell_fill.token_raw_delta,
            bought_raw
        ));
    }

    println!("\n[PHASE 3] REKONSILIASI");
    let round_trip = buy_lamports_delta + sell_fill.owner_lamports_delta;
    println!(
        "• Biaya round-trip dari meta tx: {} lamports ({:.6} SOL)",
        round_trip,
        round_trip as f64 / 1e9
    );
    let final_lamports = sol_exec.get_native_balance_lamports(&user_pubkey).await?;
    let final_usdc_raw = sol_exec
        .get_raw_token_balance(&user_pubkey, USDC_MINT)
        .await?;
    println!(
        "• Saldo akhir: {} lamports (delta RPC {}), USDC raw {} (awal {})",
        final_lamports,
        i128::from(final_lamports) - i128::from(initial_lamports),
        final_usdc_raw,
        initial_usdc_raw
    );

    println!("\n=======================================================");
    println!("✅ CANARY SUKSES: SELL landing, fill terbaca dari meta transaksi");
    println!("• SELL: https://solscan.io/tx/{sell_sig}");
    println!("=======================================================");
    Ok(())
}
