use anyhow::{bail, Result};
use k256::ecdsa::{Signature, SigningKey};
use sha3::{Digest, Keccak256};

pub const ROBINHOOD_CHAIN_ID: u64 = 4663;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Eip1559Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub value: u128,
    pub data: Vec<u8>,
}

impl Eip1559Transaction {
    pub fn validate(&self) -> Result<()> {
        if self.chain_id != ROBINHOOD_CHAIN_ID { bail!("unexpected Robinhood chain id"); }
        if self.max_priority_fee_per_gas > self.max_fee_per_gas {
            bail!("priority fee exceeds max fee");
        }
        if self.gas_limit == 0 { bail!("gas limit must be non-zero"); }
        Ok(())
    }

    pub fn signing_payload(&self) -> Result<Vec<u8>> {
        self.validate()?;
        Ok(typed(rlp_list(&unsigned_fields(self))))
    }

    pub fn signing_hash(&self) -> Result<[u8; 32]> {
        Ok(Keccak256::digest(self.signing_payload()?).into())
    }

    pub fn sign(&self, private_key: &[u8; 32]) -> Result<Vec<u8>> {
        self.validate()?;
        let key = SigningKey::from_bytes(private_key.into())?;
        let mut digest = Keccak256::new();
        digest.update(self.signing_payload()?);
        let (signature, recovery_id) = key.sign_digest_recoverable(digest)?;
        let (r, s) = signature_words(&signature);
        let mut fields = unsigned_fields(self);
        fields.push(rlp_bytes(&[recovery_id.to_byte()]));
        fields.push(rlp_bytes(&r));
        fields.push(rlp_bytes(&s));
        Ok(typed(rlp_list(&fields)))
    }
}

fn unsigned_fields(tx: &Eip1559Transaction) -> Vec<Vec<u8>> {
    vec![
        rlp_bytes(&uint(tx.chain_id)),
        rlp_bytes(&uint(tx.nonce)),
        rlp_bytes(&uint(tx.max_priority_fee_per_gas)),
        rlp_bytes(&uint(tx.max_fee_per_gas)),
        rlp_bytes(&uint(tx.gas_limit)),
        rlp_bytes(&tx.to),
        rlp_bytes(&uint(tx.value)),
        rlp_bytes(&tx.data),
        rlp_list(&[]),
    ]
}

fn signature_words(signature: &Signature) -> (Vec<u8>, Vec<u8>) {
    let raw = signature.to_bytes();
    (raw[..32].to_vec(), raw[32..].to_vec())
}

fn typed(mut list: Vec<u8>) -> Vec<u8> {
    let mut out = vec![0x02];
    out.append(&mut list);
    out
}

fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload: Vec<u8> = items.iter().flatten().copied().collect();
    let mut out = rlp_prefix(0xc0, payload.len());
    out.extend(payload);
    out
}

fn rlp_bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 { return value.to_vec(); }
    let mut out = rlp_prefix(0x80, value.len());
    out.extend(value);
    out
}

fn rlp_prefix(short_prefix: u8, length: usize) -> Vec<u8> {
    if length < 56 { return vec![short_prefix + length as u8]; }
    let raw = uint(length as u128);
    let mut out = vec![short_prefix + 55 + raw.len() as u8];
    out.extend(raw);
    out
}

fn uint(value: impl Into<u128>) -> Vec<u8> {
    let mut bytes = value.into().to_be_bytes().to_vec();
    while bytes.first() == Some(&0) { bytes.remove(0); }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tx() -> Eip1559Transaction {
        Eip1559Transaction { chain_id: ROBINHOOD_CHAIN_ID, nonce: 0,
            max_priority_fee_per_gas: 1, max_fee_per_gas: 2, gas_limit: 21_000,
            to: [0u8; 20], value: 0, data: Vec::new() }
    }

    #[test]
    fn unsigned_robinhood_vector_is_deterministic() {
        assert_eq!(hex::encode(tx().signing_payload().unwrap()),
            "02e18212378001028252089400000000000000000000000000000000000000008080c0");
    }

    #[test]
    fn rejects_wrong_chain_and_fee_order() {
        let mut value = tx();
        value.chain_id = 1;
        assert!(value.validate().is_err());
        value.chain_id = ROBINHOOD_CHAIN_ID;
        value.max_priority_fee_per_gas = 3;
        assert!(value.validate().is_err());
    }
}
