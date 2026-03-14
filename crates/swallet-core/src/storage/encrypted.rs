use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::crypto::encryption;
use crate::error::StorageError;
use crate::storage::data::WalletStore;

/// 文件魔数
const MAGIC: &[u8; 4] = b"SWLT";
/// 当前数据格式版本
const FORMAT_VERSION: u32 = 1;

const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;
/// 文件头长度: magic(4) + version(4) + salt(32) + nonce(12) = 52
const HEADER_LEN: usize = 4 + 4 + SALT_LEN + NONCE_LEN;

/// 获取默认数据文件路径
pub fn default_data_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let config_dir = PathBuf::from(home).join(".config").join("swallet");
    config_dir.join("data.dat")
}

/// 检查数据文件是否存在
pub fn data_file_exists(path: &Path) -> bool {
    path.exists()
}

/// 将 WalletStore 加密后保存到文件
pub fn save(store: &WalletStore, password: &[u8], path: &Path) -> Result<(), StorageError> {
    let json = serde_json::to_vec(store)?;
    let (salt, nonce, ciphertext) =
        encryption::encrypt(&json, password).map_err(StorageError::Crypto)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // 原子写入：先写临时文件，再 rename
    let tmp_path = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(MAGIC)?;
        file.write_all(&FORMAT_VERSION.to_le_bytes())?;
        file.write_all(&salt)?;
        file.write_all(&nonce)?;
        file.write_all(&ciphertext)?;
        file.sync_all()?;
    }
    fs::rename(&tmp_path, path)?;

    Ok(())
}

/// 从加密文件加载 WalletStore
pub fn load(password: &[u8], path: &Path) -> Result<WalletStore, StorageError> {
    if !path.exists() {
        return Err(StorageError::DataFileNotFound);
    }

    let data = fs::read(path)?;
    if data.len() < HEADER_LEN {
        return Err(StorageError::InvalidFormat("文件太短".into()));
    }

    // 校验魔数
    if &data[0..4] != MAGIC {
        return Err(StorageError::InvalidFormat("文件魔数不匹配".into()));
    }

    // 校验版本
    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    if version != FORMAT_VERSION {
        return Err(StorageError::UnsupportedVersion(version));
    }

    let salt = &data[8..8 + SALT_LEN];
    let nonce = &data[8 + SALT_LEN..8 + SALT_LEN + NONCE_LEN];
    let ciphertext = &data[HEADER_LEN..];

    let plaintext =
        encryption::decrypt(ciphertext, password, salt, nonce).map_err(StorageError::Crypto)?;

    let store: WalletStore = serde_json::from_slice(&plaintext)?;
    Ok(store)
}
