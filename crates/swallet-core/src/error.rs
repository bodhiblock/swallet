use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum SwalletError {
    #[error("加密错误: {0}")]
    Crypto(#[from] CryptoError),

    #[error("存储错误: {0}")]
    Storage(#[from] StorageError),

    #[error("配置错误: {0}")]
    Config(#[from] ConfigError),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("密码错误")]
    WrongPassword,

    #[error("加密失败: {0}")]
    EncryptionFailed(String),

    #[error("解密失败: {0}")]
    DecryptionFailed(String),

    #[error("密钥派生失败: {0}")]
    KeyDerivationFailed(String),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("数据文件不存在")]
    DataFileNotFound,

    #[error("数据文件格式无效: {0}")]
    InvalidFormat(String),

    #[error("数据版本不支持: {0}")]
    UnsupportedVersion(u32),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("序列化错误: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("加密错误: {0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("配置文件解析失败: {0}")]
    ParseFailed(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

/// 统一结果类型（预留供后续使用）
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, SwalletError>;
