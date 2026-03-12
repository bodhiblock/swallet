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
    Multisig {
        /// 多签 PDA 地址
        multisig_address: String,
        /// RPC URL
        rpc_url: String,
        /// 链 ID
        chain_id: String,
        /// 链名称
        chain_name: String,
        /// 当前阈值
        threshold: u16,
        /// 成员地址列表
        member_addresses: Vec<String>,
        /// vault 列表
        vaults: Vec<VaultAccount>,
    },
}

/// 多签 Vault 账户
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VaultAccount {
    pub vault_index: u8,
    pub address: String,
    pub label: Option<String>,
    pub hidden: bool,
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

    /// 构建地址→备注映射（用于 UI 显示时标注自己的地址）
    pub fn address_labels(&self) -> std::collections::HashMap<String, String> {
        let mut map = std::collections::HashMap::new();
        for wallet in &self.wallets {
            if wallet.hidden {
                continue;
            }
            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    for acc in eth_accounts.iter().chain(sol_accounts.iter()) {
                        if !acc.hidden {
                            let label = acc
                                .label
                                .as_deref()
                                .unwrap_or(&wallet.name);
                            map.insert(acc.address.clone(), label.to_string());
                        }
                    }
                }
                WalletType::PrivateKey {
                    address,
                    label,
                    hidden,
                    ..
                } => {
                    if !hidden {
                        let l = label.as_deref().unwrap_or(&wallet.name);
                        map.insert(address.clone(), l.to_string());
                    }
                }
                WalletType::WatchOnly {
                    address, label, ..
                } => {
                    let l = label.as_deref().unwrap_or(&wallet.name);
                    map.insert(address.clone(), l.to_string());
                }
                WalletType::Multisig { vaults, .. } => {
                    for v in vaults.iter().filter(|v| !v.hidden) {
                        let l = v.label.as_deref().unwrap_or(&wallet.name);
                        map.insert(v.address.clone(), l.to_string());
                    }
                }
            }
        }
        map
    }

    /// 数据迁移：将旧的 multisigs 列表迁移为 WalletType::Multisig 钱包
    pub fn migrate(&mut self) {
        if self.multisigs.is_empty() {
            return;
        }

        for ms in std::mem::take(&mut self.multisigs) {
            // 检查是否已存在同地址的多签钱包
            let already_exists = self.wallets.iter().any(|w| {
                matches!(
                    &w.wallet_type,
                    WalletType::Multisig { multisig_address, .. } if multisig_address == &ms.address
                )
            });
            if already_exists {
                continue;
            }

            // 从对应的 SquadsVault 观察钱包中提取 label
            let vault_label = self.wallets.iter().find_map(|w| {
                if let WalletType::WatchOnly {
                    address,
                    label,
                    source: WatchOnlySource::SquadsVault { multisig_id },
                    ..
                } = &w.wallet_type
                {
                    if multisig_id == &ms.address {
                        return label.clone().or_else(|| Some(format!("Vault #{}", 0)));
                    }
                    let _ = address;
                }
                None
            });

            // 删除对应的 SquadsVault 观察钱包
            self.wallets.retain(|w| {
                !matches!(
                    &w.wallet_type,
                    WalletType::WatchOnly {
                        source: WatchOnlySource::SquadsVault { multisig_id },
                        ..
                    } if multisig_id == &ms.address
                )
            });

            let sort_order = self.wallets.len() as u32;
            self.wallets.push(Wallet {
                id: ms.id,
                name: ms.name,
                wallet_type: WalletType::Multisig {
                    multisig_address: ms.address,
                    rpc_url: ms.rpc_url,
                    chain_id: ms.chain_id,
                    chain_name: ms.chain_name,
                    threshold: ms.threshold,
                    member_addresses: ms.member_addresses,
                    vaults: vec![VaultAccount {
                        vault_index: 0,
                        address: ms.vault_address,
                        label: vault_label,
                        hidden: false,
                    }],
                },
                sort_order,
                hidden: ms.hidden,
                created_at: ms.created_at,
            });
        }
    }
}

impl Default for WalletStore {
    fn default() -> Self {
        Self::new()
    }
}
