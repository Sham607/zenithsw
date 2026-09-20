use crate::db::WalletScanAddress;
use crate::models::Chain;
use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanManifestEntry {
    pub chain: Chain,
    pub address: String,
    pub source_db: String,
    pub role: String,
    pub tier: String,
    pub execution_track: String,
    pub is_candidate: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanManifest {
    pub scan_id: String,
    pub observed_at: DateTime<Utc>,
    pub entries: Vec<ScanManifestEntry>,
}

impl ScanManifest {
    pub fn build(addresses: &[WalletScanAddress], observed_at: DateTime<Utc>) -> Result<Self> {
        if addresses.is_empty() {
            bail!("scan manifest tidak boleh kosong");
        }
        let entries: Vec<_> = addresses
            .iter()
            .map(|address| ScanManifestEntry {
                chain: address.chain,
                address: address.address.trim().to_string(),
                source_db: address.source_db.to_string(),
                role: if address.is_candidate {
                    "MONITOR_ONLY".to_string()
                } else {
                    "QUALIFIED_OBSERVER".to_string()
                },
                tier: address.tier.clone(),
                execution_track: address.execution_track.clone(),
                is_candidate: address.is_candidate,
                is_active: address.is_active,
            })
            .collect();
        if entries.iter().any(|entry| entry.address.is_empty()) {
            bail!("scan manifest memiliki address kosong");
        }
        let canonical = serde_json::to_vec(&entries)?;
        let mut hasher = Sha3_256::new();
        hasher.update(canonical);
        hasher.update(observed_at.to_rfc3339().as_bytes());
        let scan_id = format!("scan_{}", hex::encode(hasher.finalize()));
        Ok(Self {
            scan_id,
            observed_at,
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(is_candidate: bool) -> WalletScanAddress {
        WalletScanAddress {
            chain: Chain::Solana,
            address: "Wallet1111111111111111111111111111111111111".into(),
            source_db: if is_candidate {
                "candidate_wallet_registry.db"
            } else {
                "wallet_registry.db"
            },
            is_candidate,
            is_active: !is_candidate,
            tier: if is_candidate { "TIER-B" } else { "TIER-S" }.into(),
            execution_track: "MONITOR_ONLY".into(),
        }
    }

    #[test]
    fn manifest_is_deterministic_for_same_input() {
        let now = Utc::now();
        let first = ScanManifest::build(&[address(false)], now).unwrap();
        let second = ScanManifest::build(&[address(false)], now).unwrap();
        assert_eq!(first, second);
        assert!(first.scan_id.starts_with("scan_"));
    }

    #[test]
    fn candidate_is_monitor_only_and_registry_is_not_mutated() {
        let manifest = ScanManifest::build(&[address(true)], Utc::now()).unwrap();
        assert_eq!(manifest.entries[0].role, "MONITOR_ONLY");
        assert!(manifest.entries[0].is_candidate);
    }

    #[test]
    fn empty_manifest_is_rejected() {
        assert!(ScanManifest::build(&[], Utc::now()).is_err());
    }
}
