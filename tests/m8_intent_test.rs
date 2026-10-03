use tempfile::tempdir;
use zenithsw::db::{ExecutionIntentMetadata, SmartWalletDb};
use zenithsw::models::Chain;

#[test]
fn execution_intent_state_machine_rejects_invalid_transition() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    assert!(db
        .create_execution_intent(
            "intent-1",
            Chain::Robinhood,
            "BUY",
            "0xtoken",
            "0xwallet",
            1.0
        )
        .expect("intent pertama harus tersimpan"));
    assert!(!db
        .create_execution_intent(
            "intent-1",
            Chain::Robinhood,
            "BUY",
            "0xtoken",
            "0xwallet",
            1.0
        )
        .expect("duplikat harus idempotent"));
    for state in ["QUOTED", "SIMULATED", "SIGNED"] {
        db.update_execution_intent("intent-1", state, None, None)
            .expect("state transition harus valid");
    }
    assert!(db
        .update_execution_intent("intent-1", "INTENT", None, None)
        .is_err());
    let tx_hash = format!("0x{}", "11".repeat(32));
    db.update_execution_intent("intent-1", "BROADCASTED", Some(&tx_hash), None)
        .expect("SIGNED -> BROADCASTED valid");
    db.update_execution_intent("intent-1", "CONFIRMED", None, None)
        .expect("BROADCASTED -> CONFIRMED valid");
    db.update_execution_intent("intent-1", "RECONCILED", None, None)
        .expect("CONFIRMED -> RECONCILED valid");
    assert!(db
        .update_execution_intent("intent-1", "OPEN", None, None)
        .is_err());
}

#[test]
fn execution_intent_metadata_roundtrip_and_restart() {
    let dir = tempdir().expect("tempdir harus dibuat");
    let path = dir.path().join("trading.db");
    let path = path.to_str().expect("path UTF-8").to_owned();
    {
        let db = SmartWalletDb::new(&path).expect("file DB harus dibuat");
        db.create_execution_intent("restart-1", Chain::Solana, "BUY", "mint", "wallet", 2.5)
            .expect("intent harus tersimpan");
        let expected = ExecutionIntentMetadata {
            nonce: Some(7),
            recent_blockhash: Some("blockhash".to_string()),
            last_valid_block_height: Some(99),
            calldata_hash: Some("0xhash".to_string()),
            quote_block: Some(88),
            expiry: Some(1234),
        };
        db.update_execution_intent_metadata("restart-1", &expected)
            .expect("metadata harus tersimpan");
        assert_eq!(
            db.get_execution_intent_metadata("restart-1").unwrap(),
            Some(expected)
        );
        db.update_execution_intent("restart-1", "UNKNOWN", None, None)
            .expect("INTENT -> UNKNOWN valid");
    }
    let db = SmartWalletDb::new(&path).expect("file DB harus dibuka ulang");
    assert_eq!(
        db.get_execution_intent("restart-1").unwrap().unwrap().0,
        "UNKNOWN"
    );
}

#[test]
fn execution_intent_signature_and_tx_hash_are_unique() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    assert!(db
        .create_execution_intent("a", Chain::Solana, "BUY", "mint-a", "wallet", 1.0)
        .unwrap());
    assert!(db
        .create_execution_intent("b", Chain::Solana, "BUY", "mint-b", "wallet", 1.0)
        .unwrap());
    db.update_execution_intent("a", "SIGNED", None, Some("same-signature"))
        .unwrap();
    assert!(db
        .update_execution_intent("b", "SIGNED", None, Some("same-signature"))
        .is_err());
    db.update_execution_intent("b", "BROADCASTED", Some("same-tx"), None)
        .unwrap();
    assert!(db
        .update_execution_intent("a", "BROADCASTED", Some("same-tx"), None)
        .is_err());
}

#[test]
fn execution_intent_timeout_reconciliation_follows_receipt_evidence() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    db.create_execution_intent(
        "timeout-1",
        Chain::Robinhood,
        "BUY",
        "0xtoken",
        "0xwallet",
        1.0,
    )
    .expect("intent harus tersimpan");

    // Broadcast path: stepwise states, then BROADCASTED with tx hash evidence.
    let tx_hash = format!("0x{}", "ab".repeat(32));
    db.update_execution_intent("timeout-1", "QUOTED", None, None)
        .expect("INTENT -> QUOTED harus valid");
    db.update_execution_intent("timeout-1", "SIMULATED", None, None)
        .expect("QUOTED -> SIMULATED harus valid");
    db.update_execution_intent("timeout-1", "SIGNED", None, None)
        .expect("SIMULATED -> SIGNED harus valid");
    db.update_execution_intent("timeout-1", "BROADCASTED", Some(&tx_hash), None)
        .expect("SIGNED -> BROADCASTED dengan bukti hash harus valid");

    // Timeout policy lives in the engine's receipt-validated reconciliation
    // (validate_receipt_transaction_hash); the state machine (db.rs) reserves
    // BROADCASTED -> CONFIRMED for that receipt path and offers
    // PENDING_RECONCILIATION as the safe demotion while unresolved. The
    // terminal terminal jump PENDING_RECONCILIATION -> RECONCILED is allowed
    // only because CONFIRMED semantics are enforced by the receipt validator,
    // never by silent promotion.
    db.update_execution_intent("timeout-1", "PENDING_RECONCILIATION", None, None)
        .expect("BROADCASTED -> PENDING_RECONCILIATION harus valid saat timeout");
    db.update_execution_intent("timeout-1", "CONFIRMED", None, None)
        .expect("PENDING_RECONCILIATION -> CONFIRMED valid dengan bukti receipt");
    db.update_execution_intent("timeout-1", "RECONCILED", None, None)
        .expect("CONFIRMED -> RECONCILED valid setelah balance reconciliation");
    // Terminal: RECONCILED cannot restart or be re-confirmed (no virtual fills).
    assert!(db
        .update_execution_intent("timeout-1", "CONFIRMED", None, None)
        .is_err());
}

#[test]
fn execution_intent_stepwise_timeout_paths_are_enforced() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");

    // Timeout with no broadcast evidence: QUOTED -> UNKNOWN is legal (ambiguity),
    // but never directly to CONFIRMED/RECONCILED.
    db.create_execution_intent("t-quoted", Chain::Robinhood, "BUY", "0xt", "0xw", 1.0)
        .unwrap();
    db.update_execution_intent("t-quoted", "QUOTED", None, None)
        .unwrap();
    db.update_execution_intent("t-quoted", "UNKNOWN", None, None)
        .expect("QUOTED -> UNKNOWN harus valid");
    assert!(db
        .update_execution_intent("t-quoted", "CONFIRMED", None, None)
        .is_err());

    // Timeout after broadcast with hash: BROADCASTED -> PENDING_RECONCILIATION,
    // then only receipt evidence may promote or demote the state.
    db.create_execution_intent("t-broadcast", Chain::Robinhood, "BUY", "0xt", "0xw", 1.0)
        .unwrap();
    let tx_hash = format!("0x{}", "cd".repeat(32));
    db.update_execution_intent("t-broadcast", "QUOTED", None, None)
        .unwrap();
    db.update_execution_intent("t-broadcast", "SIMULATED", None, None)
        .unwrap();
    db.update_execution_intent("t-broadcast", "SIGNED", None, None)
        .unwrap();
    db.update_execution_intent("t-broadcast", "BROADCASTED", Some(&tx_hash), None)
        .unwrap();
    db.update_execution_intent("t-broadcast", "PENDING_RECONCILIATION", None, None)
        .expect("BROADCASTED -> PENDING_RECONCILIATION harus valid");
    // PENDING_RECONCILIATION cannot silently become BROADCASTED again (no rebroadcast).
    assert!(db
        .update_execution_intent("t-broadcast", "BROADCASTED", None, None)
        .is_err());
    db.update_execution_intent("t-broadcast", "CONFIRMED", None, None)
        .expect("PENDING_RECONCILIATION -> CONFIRMED valid saat receipt sukses");
    db.update_execution_intent("t-broadcast", "RECONCILED", None, None)
        .expect("CONFIRMED -> RECONCILED valid setelah balance reconciliation");
}

#[test]
fn execution_intent_replacement_signature_is_rejected_after_restart() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    db.create_execution_intent("replace-1", Chain::Solana, "BUY", "mint", "wallet", 1.0)
        .unwrap();
    let original_sig = "sig-original";
    db.update_execution_intent("replace-1", "QUOTED", None, None)
        .unwrap();
    db.update_execution_intent("replace-1", "SIMULATED", None, None)
        .unwrap();
    db.update_execution_intent("replace-1", "SIGNED", None, Some(original_sig))
        .unwrap();
    db.update_execution_intent("replace-1", "BROADCASTED", None, None)
        .unwrap();

    // Restart recovery has only ambiguous evidence: it may go UNKNOWN, but a
    // different signature (a replacement transaction) must never be attached
    // to the same intent.
    db.update_execution_intent("replace-1", "UNKNOWN", None, None)
        .expect("BROADCASTED -> UNKNOWN valid untuk evidence ambigu");
    assert!(db
        .update_execution_intent("replace-1", "SIGNED", None, Some("sig-replacement"))
        .is_err());
}

#[test]
fn execution_intent_solana_sell_lifecycle_and_timeout_reconciliation() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    let intent_id = "SOL-SELL-pos1-123456789";

    // 1. Create SELL intent
    assert!(db
        .create_execution_intent(
            intent_id,
            Chain::Solana,
            "SELL",
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
            "bLiHABX6WnneYX94mqR7eUh9VsMvZB23772eWAtvvdk",
            1500000.0,
        )
        .expect("SELL intent harus tersimpan"));

    // 2. Normal forward flow
    db.update_execution_intent(intent_id, "QUOTED", None, None)
        .expect("INTENT -> QUOTED valid");
    db.update_execution_intent(intent_id, "SIGNED", None, None)
        .expect("QUOTED -> SIGNED valid");

    let sig = "5Ver7TJ9AKSL...mock...sell...signature";
    db.update_execution_intent(intent_id, "BROADCASTED", None, Some(sig))
        .expect("SIGNED -> BROADCASTED valid");

    // Must show up in pending intents
    let pending = db.get_pending_execution_intents().expect("query pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0, intent_id);
    assert_eq!(pending[0].2, "SELL");
    assert_eq!(pending[0].5, "BROADCASTED");
    assert_eq!(pending[0].7.as_deref(), Some(sig));

    // 3. Timeout demotion path
    db.update_execution_intent(intent_id, "PENDING_RECONCILIATION", None, Some(sig))
        .expect("BROADCASTED -> PENDING_RECONCILIATION valid saat RPC timeout");

    // 4. On-chain confirmation & reconciliation
    db.update_execution_intent(intent_id, "CONFIRMED", None, Some(sig))
        .expect("PENDING_RECONCILIATION -> CONFIRMED valid");
    db.update_execution_intent(intent_id, "RECONCILED", None, Some(sig))
        .expect("CONFIRMED -> RECONCILED valid");

    // Terminal: no longer pending
    let pending_after = db.get_pending_execution_intents().expect("query pending");
    assert!(
        pending_after.is_empty(),
        "RECONCILED intent tidak boleh pending lagi"
    );
}

#[test]
fn exclusive_intent_blocks_second_active_intent_per_mint() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    assert!(db
        .create_execution_intent_exclusive("buy-1", Chain::Solana, "BUY", "mint-x", "w", 0.08)
        .unwrap());
    // Intent BUY kedua untuk mint sama ditolak selama intent pertama aktif.
    assert!(!db
        .create_execution_intent_exclusive("buy-2", Chain::Solana, "BUY", "mint-x", "w", 0.08)
        .unwrap());
    // SELL untuk mint sama juga ditolak (anti double-sell / tumpang tindih).
    assert!(!db
        .create_execution_intent_exclusive("sell-1", Chain::Solana, "SELL", "mint-x", "w", 1.0)
        .unwrap());
    // Mint berbeda tetap boleh.
    assert!(db
        .create_execution_intent_exclusive("buy-3", Chain::Solana, "BUY", "mint-y", "w", 0.08)
        .unwrap());
    // PENDING_RECONCILIATION masih memblokir.
    db.update_execution_intent("buy-1", "SIGNED", None, Some("sig-1"))
        .unwrap();
    db.update_execution_intent("buy-1", "PENDING_RECONCILIATION", None, None)
        .unwrap();
    assert!(!db
        .create_execution_intent_exclusive("buy-4", Chain::Solana, "BUY", "mint-x", "w", 0.08)
        .unwrap());
    // Setelah terminal, mint terbuka lagi.
    db.update_execution_intent("buy-1", "FAILED", None, None)
        .unwrap();
    assert!(db
        .create_execution_intent_exclusive("buy-5", Chain::Solana, "BUY", "mint-x", "w", 0.08)
        .unwrap());
}

#[test]
fn stale_pending_intents_expose_signature_and_expiry() {
    let db = SmartWalletDb::new_in_memory().expect("in-memory DB harus dibuat");
    db.create_execution_intent("p-1", Chain::Solana, "BUY", "mint", "w", 1.0)
        .unwrap();
    db.update_execution_intent_metadata(
        "p-1",
        &ExecutionIntentMetadata {
            nonce: None,
            recent_blockhash: Some("bh".to_string()),
            last_valid_block_height: Some(430_000_000),
            calldata_hash: None,
            quote_block: None,
            expiry: None,
        },
    )
    .unwrap();
    db.update_execution_intent("p-1", "SIGNED", None, Some("sig-p1"))
        .unwrap();
    db.create_execution_intent("r-1", Chain::Robinhood, "BUY", "0xt", "0xw", 1.0)
        .unwrap();
    let pending = db.get_stale_pending_intents(Chain::Solana, 0).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].signature.as_deref(), Some("sig-p1"));
    assert_eq!(pending[0].last_valid_block_height, Some(430_000_000));
    // Intent yang baru di-update tidak dianggap stale untuk ambang 600 detik.
    assert!(db
        .get_stale_pending_intents(Chain::Solana, 600)
        .unwrap()
        .is_empty());
    // SIGNED -> PENDING_RECONCILIATION valid (broadcast tak terbukti).
    db.update_execution_intent("p-1", "PENDING_RECONCILIATION", None, None)
        .unwrap();
}
