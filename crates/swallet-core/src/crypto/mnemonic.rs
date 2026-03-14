use bip39::Mnemonic;

use crate::error::CryptoError;

/// 生成新的 12 词助记词
pub fn generate_mnemonic() -> Result<String, CryptoError> {
    let mnemonic = Mnemonic::generate(12)
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    Ok(mnemonic.to_string())
}

/// 验证助记词是否有效
pub fn validate_mnemonic(phrase: &str) -> Result<Mnemonic, CryptoError> {
    phrase
        .parse::<Mnemonic>()
        .map_err(|e| CryptoError::KeyDerivationFailed(format!("无效的助记词: {e}")))
}

/// 从助记词生成种子（64 字节），可选密码
pub fn mnemonic_to_seed(phrase: &str, passphrase: &str) -> Result<[u8; 64], CryptoError> {
    let mnemonic = validate_mnemonic(phrase)?;
    let seed = mnemonic.to_seed(passphrase);
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_mnemonic() {
        let phrase = generate_mnemonic().unwrap();
        let words: Vec<&str> = phrase.split_whitespace().collect();
        assert_eq!(words.len(), 12);
    }

    #[test]
    fn test_validate_mnemonic() {
        let phrase = generate_mnemonic().unwrap();
        assert!(validate_mnemonic(&phrase).is_ok());
        assert!(validate_mnemonic("invalid mnemonic phrase").is_err());
    }

    #[test]
    fn test_mnemonic_to_seed_deterministic() {
        let phrase = generate_mnemonic().unwrap();
        let seed1 = mnemonic_to_seed(&phrase, "").unwrap();
        let seed2 = mnemonic_to_seed(&phrase, "").unwrap();
        assert_eq!(seed1, seed2);
    }
}
