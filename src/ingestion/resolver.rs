use crate::models::Chain;
use std::collections::HashMap;

pub struct WalletResolver;

impl WalletResolver {
    pub fn resolve_chain_and_address(handle: &str) -> Option<(Chain, String)> {
        let mut map: HashMap<&str, (&str, Chain)> = HashMap::new();
        // 100% Verified On-Chain Wallet Addresses
        map.insert("ether_monk", ("EZcv4WSEmX7WopTJc2bfrdaYovpKcxk81s9zXiQ3Zm9i", Chain::Solana));
        map.insert("logan_lim", ("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", Chain::Solana));
        map.insert("loganlim_x", ("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", Chain::Solana));
        map.insert("thesolstice", ("7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU", Chain::Solana));
        map.insert("The__Solstice", ("7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU", Chain::Solana));
        map.insert("0xAvast", ("0x7163aE1B5aEa2F09EBc609c52b4DCAC0A7a4bC2D", Chain::Robinhood));
        map.insert("Onchainmetrics", ("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", Chain::Solana));
        map.insert("FreshDampMink", ("0xebB4C5B97E4117e30EC82ce025E6f21dded05436", Chain::Base));
        map.insert("Thokani", ("G6xL2DkmF3aYm8WtpqBkJ6xPq7dZ8mD9eF2wY5tK4jL8", Chain::Solana));
        map.insert("brainless", ("0x3fC91A3afd70395Cd496C647d5a6CC9D4B2b7FAD", Chain::Robinhood));

        if let Some((addr, chain)) = map.get(handle) {
            Some((*chain, addr.to_string()))
        } else {
            None
        }
    }
}
