use bip32::{DerivationPath, XPrv};
use k256::ecdsa::SigningKey;
use sha3::{Digest, Keccak256};

use crate::error::CryptoError;

/// 以太坊 BIP44 派生路径: m/44'/60'/0'/0/{index}
fn eth_derivation_path(index: u32) -> Result<DerivationPath, CryptoError> {
    let path = format!("m/44'/60'/0'/0/{index}");
    path.parse::<DerivationPath>()
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))
}

/// 从种子派生以太坊私钥
pub fn derive_eth_private_key(seed: &[u8; 64], index: u32) -> Result<Vec<u8>, CryptoError> {
    let path = eth_derivation_path(index)?;
    let xprv = XPrv::derive_from_path(seed, &path)
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    Ok(xprv.private_key().to_bytes().to_vec())
}

/// 从种子派生以太坊地址
pub fn derive_eth_address(seed: &[u8; 64], index: u32) -> Result<String, CryptoError> {
    let private_key_bytes = derive_eth_private_key(seed, index)?;
    private_key_to_eth_address(&private_key_bytes)
}

/// 从私钥字节计算以太坊地址
pub fn private_key_to_eth_address(private_key: &[u8]) -> Result<String, CryptoError> {
    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;

    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_encoded_point(false);
    // 去掉第一个字节 (0x04 前缀)
    let pub_key_uncompressed = &public_key_bytes.as_bytes()[1..];

    let hash = Keccak256::digest(pub_key_uncompressed);
    let address_bytes = &hash[12..]; // 取后 20 字节

    Ok(format!("0x{}", hex::encode(address_bytes)))
}

/// 从十六进制私钥字符串解析并返回地址
pub fn hex_private_key_to_address(hex_key: &str) -> Result<String, CryptoError> {
    let hex_key = hex_key.strip_prefix("0x").unwrap_or(hex_key);
    let private_key = hex::decode(hex_key)
        .map_err(|e| CryptoError::KeyDerivationFailed(format!("无效的十六进制私钥: {e}")))?;
    if private_key.len() != 32 {
        return Err(CryptoError::KeyDerivationFailed(
            "私钥长度必须为 32 字节".into(),
        ));
    }
    private_key_to_eth_address(&private_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::mnemonic;

    #[test]
    fn test_derive_eth_address() {
        let phrase = mnemonic::generate_mnemonic().unwrap();
        let seed = mnemonic::mnemonic_to_seed(&phrase, "").unwrap();
        let addr = derive_eth_address(&seed, 0).unwrap();

        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_deterministic_derivation() {
        let phrase = mnemonic::generate_mnemonic().unwrap();
        let seed = mnemonic::mnemonic_to_seed(&phrase, "").unwrap();
        let addr1 = derive_eth_address(&seed, 0).unwrap();
        let addr2 = derive_eth_address(&seed, 0).unwrap();
        assert_eq!(addr1, addr2);

        // 不同索引应产生不同地址
        let addr3 = derive_eth_address(&seed, 1).unwrap();
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_known_vector() {
        // 使用已知的测试助记词验证
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let seed = mnemonic::mnemonic_to_seed(phrase, "").unwrap();
        let addr = derive_eth_address(&seed, 0).unwrap();
        // 这个助记词的第一个 ETH 地址是已知的
        assert_eq!(addr.to_lowercase(), "0x9858effd232b4033e47d90003d41ec34ecaeda94");
    }
}
