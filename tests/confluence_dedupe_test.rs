use tempfile::tempdir;
use zenithsw::db::SmartWalletDb;
use zenithsw::models::{ActivityType, Chain, WalletActivity};

/// Confluence harus menghitung wallet FISIK (address), bukan label handle.
/// Bug produksi 20 Sep 2026: jalur WS menulis address sebagai handle sehingga
/// satu wallet terhitung dua whale dan sinyal palsu lolos gate.
#[test]
fn confluence_dedupes_same_address_with_different_handles() {
    let dir = tempdir().unwrap();
    let db = SmartWalletDb::new(dir.path().join("confluence.db").to_str().unwrap())
        .expect("DB harus dibuat");

    let mint = "WXMRyRZhsa19ety5erZhHg4N3xj3EVN92u94422teJp";
    let addr = "GpMZbSM2GgvTKHJirzeGfMFoaZ8UR2X7F4v8vHTvxFbL";

    let mk = |handle: &str, sig: &str| WalletActivity {
        id: None,
        wallet_handle: handle.to_string(),
        wallet_address: addr.to_string(),
        chain: Chain::Solana,
        activity_type: ActivityType::Buy,
        token_symbol: "XMR".to_string(),
        token_mint: mint.to_string(),
        amount_usd: 100.0,
        token_amount: 0.1,
        execution_price_usd: 530.0,
        whale_balance_before: 1.0,
        whale_balance_after: 1.1,
        sell_ratio_pct: 0.0,
        tx_signature: sig.to_string(),
        timestamp: chrono::Utc::now(),
        source_reference: Some("test".to_string()),
        parser_version: Some("test".to_string()),
        finality: Some("confirmed".to_string()),
    };

    db.insert_activity(&mk("bunker_mmbot_01", "sig-A")).unwrap();
    // Jalur WS lama: address mentah dipakai sebagai handle.
    db.insert_activity(&mk(addr, "sig-B")).unwrap();

    // Dua baris, satu wallet fisik => confluence WAJIB 1.
    assert_eq!(
        db.count_recent_buyers_for_mint(mint, 300).unwrap(),
        1,
        "confluence harus dedupe per address, bukan per handle"
    );
}
