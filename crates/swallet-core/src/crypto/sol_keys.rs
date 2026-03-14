use hmac::{Hmac, Mac};
use sha2::Sha512;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

use crate::error::CryptoError;

type HmacSha512 = Hmac<Sha512>;

/// SLIP-0010 Ed25519 主密钥派生
fn master_key_from_seed(seed: &[u8]) -> Result<([u8; 32], [u8; 32]), CryptoError> {
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed")
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    mac.update(seed);
    let result = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    let mut chain_code = [0u8; 32];
    key.copy_from_slice(&result[..32]);
    chain_code.copy_from_slice(&result[32..]);
    Ok((key, chain_code))
}

/// SLIP-0010 Ed25519 子密钥派生（仅支持 hardened）
fn derive_child(
    key: &[u8; 32],
    chain_code: &[u8; 32],
    index: u32,
) -> Result<([u8; 32], [u8; 32]), CryptoError> {
    // Ed25519 只支持 hardened 派生
    let hardened_index = 0x80000000 | index;
    let mut mac = HmacSha512::new_from_slice(chain_code)
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    mac.update(&[0x00]); // padding
    mac.update(key);
    mac.update(&hardened_index.to_be_bytes());
    let result = mac.finalize().into_bytes();

    let mut child_key = [0u8; 32];
    let mut child_chain_code = [0u8; 32];
    child_key.copy_from_slice(&result[..32]);
    child_chain_code.copy_from_slice(&result[32..]);
    Ok((child_key, child_chain_code))
}

/// Solana BIP44 派生路径: m/44'/501'/{index}'/0'
/// 按照 Phantom/Solflare 标准派生
pub fn derive_sol_private_key(seed: &[u8; 64], index: u32) -> Result<Vec<u8>, CryptoError> {
    let (key, chain_code) = master_key_from_seed(seed)?;

    // m/44'
    let (key, chain_code) = derive_child(&key, &chain_code, 44)?;
    // m/44'/501'
    let (key, chain_code) = derive_child(&key, &chain_code, 501)?;
    // m/44'/501'/{index}'
    let (key, chain_code) = derive_child(&key, &chain_code, index)?;
    // m/44'/501'/{index}'/0'
    let (key, _chain_code) = derive_child(&key, &chain_code, 0)?;

    Ok(key.to_vec())
}

/// 从种子派生 Solana 地址（Base58 编码的公钥）
pub fn derive_sol_address(seed: &[u8; 64], index: u32) -> Result<String, CryptoError> {
    let private_key_bytes = derive_sol_private_key(seed, index)?;
    private_key_to_sol_address(&private_key_bytes)
}

/// 从私钥字节计算 Solana 地址
pub fn private_key_to_sol_address(private_key: &[u8]) -> Result<String, CryptoError> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| CryptoError::KeyDerivationFailed("私钥长度必须为 32 字节".into()))?;

    let keypair = Keypair::new_from_array(key_bytes);
    Ok(keypair.pubkey().to_string())
}

/// 从 Base58 编码的私钥解析并返回地址
/// Solana 私钥通常是 64 字节 keypair（前 32 字节是私钥，后 32 字节是公钥）
/// 或者 32 字节纯私钥
pub fn bs58_private_key_to_address(bs58_key: &str) -> Result<String, CryptoError> {
    let bytes = bs58::decode(bs58_key)
        .into_vec()
        .map_err(|e| CryptoError::KeyDerivationFailed(format!("无效的 Base58 私钥: {e}")))?;

    match bytes.len() {
        64 => {
            // Keypair 格式：前 32 字节是私钥，后 32 字节是公钥
            // 验证公钥一致性
            let key_bytes: [u8; 32] = bytes[..32]
                .try_into()
                .map_err(|_| CryptoError::KeyDerivationFailed("私钥解析失败".into()))?;
            let keypair = Keypair::new_from_array(key_bytes);
            let derived_pubkey = keypair.pubkey();
            if derived_pubkey.to_bytes() != bytes[32..] {
                return Err(CryptoError::KeyDerivationFailed(
                    "Keypair 公钥不匹配：后 32 字节与私钥派生的公钥不一致".into(),
                ));
            }
            Ok(derived_pubkey.to_string())
        }
        32 => private_key_to_sol_address(&bytes),
        _ => Err(CryptoError::KeyDerivationFailed(
            "Solana 私钥长度必须为 32 或 64 字节".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::mnemonic;

    #[test]
    fn test_derive_sol_address() {
        let phrase = mnemonic::generate_mnemonic().unwrap();
        let seed = mnemonic::mnemonic_to_seed(&phrase, "").unwrap();
        let addr = derive_sol_address(&seed, 0).unwrap();

        // Solana 地址是 Base58，通常 32-44 字符
        assert!(!addr.is_empty());
        assert!(addr.len() <= 44);
        // 验证是有效的 Base58
        assert!(bs58::decode(&addr).into_vec().is_ok());
    }

    #[test]
    fn test_deterministic_derivation() {
        let phrase = mnemonic::generate_mnemonic().unwrap();
        let seed = mnemonic::mnemonic_to_seed(&phrase, "").unwrap();
        let addr1 = derive_sol_address(&seed, 0).unwrap();
        let addr2 = derive_sol_address(&seed, 0).unwrap();
        assert_eq!(addr1, addr2);

        let addr3 = derive_sol_address(&seed, 1).unwrap();
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_known_vector() {
        // "abandon ... about" 助记词的 Solana 地址 (Phantom 路径 m/44'/501'/0'/0')
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let seed = mnemonic::mnemonic_to_seed(phrase, "").unwrap();
        let addr = derive_sol_address(&seed, 0).unwrap();
        // SLIP-0010 Ed25519 派生路径 m/44'/501'/0'/0'
        assert_eq!(addr, "HAgk14JpMQLgt6rVgv7cBQFJWFto5Dqxi472uT3DKpqk");
    }
}
