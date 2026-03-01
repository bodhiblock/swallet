use serde::{Deserialize, Serialize};

/// 整个钱包存储的顶层结构 — 序列化为 JSON 后加密存储
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WalletStore {
    pub version: u32,
    pub wallets: Vec<Wallet>,
    #[serde(default)]
    pub multisigs: Vec<MultisigAccount>,
}

/// 多签账户（本地存储）
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultisigAccount {
    pub id: String,
    pub name: String,
    /// 多签 PDA 地址
    pub address: String,
    /// 默认 vault 地址（vault_index=0）
    pub vault_address: String,
    /// 关联的 Solana RPC URL
    pub rpc_url: String,
    /// 链 ID（如 "solana-mainnet", "nara-mainnet"）
    #[serde(default)]
    pub chain_id: String,
    /// 链显示名称（如 "Solana", "Nara"）
    #[serde(default)]
    pub chain_name: String,
    /// 当前阈值
    pub threshold: u16,
    /// 成员地址列表
    pub member_addresses: Vec<String>,
    /// 是否隐藏
    pub hidden: bool,
    pub created_at: i64,
}

/// 钱包
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Wallet {
    pub id: String,
    pub name: String,
    pub wallet_type: WalletType,
    pub sort_order: u32,
    pub hidden: bool,
    pub created_at: i64,
}

/// 钱包类型
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum WalletType {
    Mnemonic {
        /// 助记词（在 JSON 中再次加密，双层保护）
        encrypted_mnemonic: String,
        eth_accounts: Vec<DerivedAccount>,
        sol_accounts: Vec<DerivedAccount>,
        next_eth_index: u32,
        next_sol_index: u32,
    },
    PrivateKey {
        chain_type: ChainType,
        encrypted_private_key: String,
        address: String,
        label: Option<String>,
        hidden: bool,
    },
    WatchOnly {
        chain_type: ChainType,
        address: String,
        label: Option<String>,
        source: WatchOnlySource,
    },
}

/// 派生账户
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DerivedAccount {
    pub derivation_index: u32,
    pub address: String,
    pub label: Option<String>,
    pub hidden: bool,
}

/// 链类型
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ChainType {
    Ethereum,
    Solana,
}

/// 观察钱包来源
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum WatchOnlySource {
    Manual,
    SquadsVault { multisig_id: String },
}

impl WalletStore {
    pub fn new() -> Self {
        Self {
            version: 1,
            wallets: Vec::new(),
            multisigs: Vec::new(),
        }
    }
}

impl Default for WalletStore {
    fn default() -> Self {
        Self::new()
    }
}
