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
