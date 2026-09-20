use anyhow::{bail, Result};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvmAddressKind {
    ExternallyOwnedAccount,
    SmartContract,
}

/// Mengklasifikasikan hasil `eth_getCode` tanpa menganggap response malformed sebagai EOA.
pub fn classify_eth_get_code(response: &Value) -> Result<EvmAddressKind> {
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("invalid JSON-RPC version");
    }
    if response.get("error").is_some() {
        bail!("eth_getCode returned an error");
    }
    let code = response
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("eth_getCode result missing or invalid"))?;
    let normalized = code.trim().to_ascii_lowercase();
    if normalized == "0x" || normalized.is_empty() {
        return Ok(EvmAddressKind::ExternallyOwnedAccount);
    }
    if !normalized.starts_with("0x")
        || normalized.len() % 2 != 0
        || !normalized[2..].chars().all(|c| c.is_ascii_hexdigit())
    {
        bail!("eth_getCode returned malformed bytecode");
    }
    Ok(EvmAddressKind::SmartContract)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_code_is_eoa() {
        assert_eq!(
            classify_eth_get_code(&json!({"jsonrpc":"2.0","result":"0x"})).unwrap(),
            EvmAddressKind::ExternallyOwnedAccount
        );
    }

    #[test]
    fn bytecode_is_contract() {
        assert_eq!(
            classify_eth_get_code(&json!({"jsonrpc":"2.0","result":"0x6000"})).unwrap(),
            EvmAddressKind::SmartContract
        );
    }

    #[test]
    fn malformed_or_error_is_rejected() {
        assert!(classify_eth_get_code(&json!({"jsonrpc":"2.0","result":"0x6"})).is_err());
        assert!(classify_eth_get_code(&json!({"jsonrpc":"2.0","error":{"code":-1}})).is_err());
    }
}
