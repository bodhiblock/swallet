pub mod defaults;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::ConfigError;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppConfig {
    pub version: u32,
    pub chains: ChainsConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChainsConfig {
    pub ethereum: Vec<EvmChainConfig>,
    pub solana: Vec<SolanaChainConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EvmChainConfig {
    pub id: String,
    pub name: String,
    pub rpc_url: String,
    pub chain_id: u64,
    pub native_symbol: String,
    pub native_decimals: u8,
    pub explorer_url: Option<String>,
    pub tokens: Vec<Erc20TokenConfig>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Erc20TokenConfig {
    pub contract_address: String,
    pub symbol: String,
    pub decimals: u8,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SolanaChainConfig {
    pub id: String,
    pub name: String,
    pub rpc_url: String,
    pub native_symbol: String,
    pub native_decimals: u8,
    pub explorer_url: Option<String>,
    pub tokens: Vec<SplTokenConfig>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SplTokenConfig {
    pub mint_address: String,
    pub symbol: String,
    pub decimals: u8,
    #[serde(default)]
    pub is_token_2022: bool,
}

impl AppConfig {
    /// 获取配置文件路径
    pub fn config_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let config_dir = PathBuf::from(home).join(".config").join("swallet");
        config_dir.join("config.toml")
    }

    /// 加载配置，如果不存在则创建默认配置
    /// 可通过 `override_path` 指定自定义配置文件路径
    pub fn load_or_create(override_path: Option<&Path>) -> Result<Self, ConfigError> {
        let path = override_path
            .map(|p| p.to_path_buf())
            .unwrap_or_else(Self::config_path);

        if path.exists() {
            let content = std::fs::read_to_string(&path).map_err(ConfigError::Io)?;
            toml::from_str(&content)
                .map_err(|e| ConfigError::ParseFailed(e.to_string()))
        } else {
            let config = defaults::default_config();
            // 确保目录存在
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
            }
            let content = toml::to_string_pretty(&config)
                .map_err(|e| ConfigError::ParseFailed(e.to_string()))?;
            std::fs::write(&path, content).map_err(ConfigError::Io)?;
            Ok(config)
        }
    }
}
