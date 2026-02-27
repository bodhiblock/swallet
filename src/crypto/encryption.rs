use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::Argon2;
use rand::RngCore;
use zeroize::Zeroize;

use crate::error::CryptoError;

/// Argon2id 参数
const ARGON2_M_COST: u32 = 65536; // 64 MB
const ARGON2_T_COST: u32 = 3;     // 3 次迭代
const ARGON2_P_COST: u32 = 4;     // 4 并行度

const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// 从密码派生 AES-256 密钥
pub fn derive_key(password: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let params = argon2::Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(KEY_LEN))
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password, salt, &mut key)
        .map_err(|e| CryptoError::KeyDerivationFailed(e.to_string()))?;
    Ok(key)
}

/// 加密数据，返回 (salt, nonce, ciphertext)
pub fn encrypt(plaintext: &[u8], password: &[u8]) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), CryptoError> {
    let mut salt = vec![0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);

    let mut nonce_bytes = vec![0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let mut key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;
    key.zeroize();

    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| CryptoError::EncryptionFailed(e.to_string()))?;

    Ok((salt, nonce_bytes, ciphertext))
}

/// 用已知 salt 和 nonce 解密数据
pub fn decrypt(
    ciphertext: &[u8],
    password: &[u8],
    salt: &[u8],
    nonce_bytes: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut key = derive_key(password, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CryptoError::DecryptionFailed(e.to_string()))?;
    key.zeroize();

    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::WrongPassword)
}

/// 验证密码是否正确（尝试解密）
pub fn verify_password(
    ciphertext: &[u8],
    password: &[u8],
    salt: &[u8],
    nonce_bytes: &[u8],
) -> bool {
    decrypt(ciphertext, password, salt, nonce_bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let plaintext = b"hello swallet";
        let password = b"test_password_123";

        let (salt, nonce, ciphertext) = encrypt(plaintext, password).unwrap();
        let decrypted = decrypt(&ciphertext, password, &salt, &nonce).unwrap();

        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_password() {
        let plaintext = b"hello swallet";
        let password = b"correct_password";
        let wrong = b"wrong_password";

        let (salt, nonce, ciphertext) = encrypt(plaintext, password).unwrap();
        let result = decrypt(&ciphertext, wrong, &salt, &nonce);

        assert!(result.is_err());
    }
}
