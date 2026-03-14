use std::path::{Path, PathBuf};

use crate::chain::BalanceCache;
use crate::config::AppConfig;
use crate::crypto::{eth_keys, mnemonic, sol_keys, SecureClear};
use crate::multisig;
use crate::storage::data::{
    ChainType, VaultAccount, Wallet, WalletStore, WalletType,
};
use crate::storage::encrypted;

/// 核心钱包服务 - 纯业务逻辑，无 UI 依赖
pub struct WalletService {
    pub config: AppConfig,
    pub store: Option<WalletStore>,
    pub balance_cache: BalanceCache,
    password: Option<Vec<u8>>,
    data_path: PathBuf,
}

impl WalletService {
    // ========== 生命周期 ==========

    pub fn new(config: AppConfig, data_path: Option<PathBuf>) -> Self {
        let data_path = data_path.unwrap_or_else(encrypted::default_data_file_path);
        Self {
            config,
            store: None,
            balance_cache: BalanceCache::new(),
            password: None,
            data_path,
        }
    }

    pub fn has_data_file(&self) -> bool {
        self.data_path.exists()
    }

    pub fn data_path(&self) -> &Path {
        &self.data_path
    }

    pub fn password(&self) -> Option<&[u8]> {
        self.password.as_deref()
    }

    pub fn set_password(&mut self, pw: Vec<u8>) {
        self.password = Some(pw);
    }

    // ========== 解锁/初始化 ==========

    pub fn create_new_store(&mut self, password: &[u8]) -> Result<(), crate::error::StorageError> {
        let store = WalletStore::new();
        encrypted::save(&store, password, &self.data_path)?;
        self.store = Some(store);
        self.password = Some(password.to_vec());
        Ok(())
    }

    pub fn unlock(&mut self, password: &[u8]) -> Result<(), crate::error::StorageError> {
        let store = encrypted::load(password, &self.data_path)?;
        self.store = Some(store);
        self.password = Some(password.to_vec());
        Ok(())
    }

    pub fn verify_password(&self, input: &[u8]) -> bool {
        self.password.as_deref() == Some(input)
    }

    // ========== 存储 ==========

    pub fn save_store(&self) -> Result<(), crate::error::StorageError> {
        if let (Some(store), Some(pw)) = (&self.store, &self.password) {
            encrypted::save(store, pw, &self.data_path)?;
        }
        Ok(())
    }

    // ========== 密钥管理 ==========

    /// 解密内层加密的秘密（助记词/私钥）
    pub fn decrypt_inner_secret(&self, encrypted: &str) -> Option<String> {
        let pw = self.password.as_ref()?;
        let parts: Vec<&str> = encrypted.split(':').collect();
        if parts.len() != 3 {
            return None;
        }
        let salt = hex::decode(parts[0]).ok()?;
        let nonce = hex::decode(parts[1]).ok()?;
        let ciphertext = hex::decode(parts[2]).ok()?;
        let plaintext =
            crate::crypto::encryption::decrypt(&ciphertext, pw, &salt, &nonce).ok()?;
        String::from_utf8(plaintext).ok()
    }

    /// 通过地址查找 SOL 私钥（遍历所有钱包）
    pub fn get_sol_private_key(&self, address: &str) -> Option<Vec<u8>> {
        let source = {
            let store = self.store.as_ref()?;
            let mut found = None;
            for wallet in &store.wallets {
                match &wallet.wallet_type {
                    WalletType::Mnemonic { encrypted_mnemonic, sol_accounts, .. } => {
                        for acc in sol_accounts {
                            if acc.address == address {
                                found = Some(SolKeySource::Mnemonic {
                                    encrypted: encrypted_mnemonic.clone(),
                                    derivation_index: acc.derivation_index,
                                });
                                break;
                            }
                        }
                    }
                    WalletType::PrivateKey {
                        chain_type: ChainType::Solana,
                        encrypted_private_key,
                        address: pk_address,
                        ..
                    } => {
                        if pk_address == address {
                            found = Some(SolKeySource::PrivateKey {
                                encrypted: encrypted_private_key.clone(),
                            });
                        }
                    }
                    _ => {}
                }
                if found.is_some() { break; }
            }
            found?
        };

        self.derive_sol_key(source)
    }

    /// 通过 wallet_index + account_index 获取 SOL 私钥
    pub fn get_sol_private_key_by_index(&self, wallet_index: usize, account_index: usize) -> Option<Vec<u8>> {
        enum KeySource {
            Mnemonic { encrypted: String, derivation_index: u32 },
            PrivateKey { encrypted: String },
        }
        let source = {
            let store = self.store.as_ref()?;
            let wallet = store.wallets.get(wallet_index)?;
            match &wallet.wallet_type {
                WalletType::Mnemonic { encrypted_mnemonic, sol_accounts, .. } => {
                    let acc = sol_accounts.get(account_index)?;
                    KeySource::Mnemonic {
                        encrypted: encrypted_mnemonic.clone(),
                        derivation_index: acc.derivation_index,
                    }
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    encrypted_private_key,
                    ..
                } => KeySource::PrivateKey { encrypted: encrypted_private_key.clone() },
                _ => return None,
            }
        };
        match source {
            KeySource::Mnemonic { encrypted, derivation_index } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.clear_sensitive();
                let result = sol_keys::derive_sol_private_key(&seed, derivation_index).ok();
                seed.clear_sensitive();
                result
            }
            KeySource::PrivateKey { encrypted } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                pk_str.clear_sensitive();
                let result = match bytes.len() {
                    64 => Some(bytes[..32].to_vec()),
                    32 => Some(bytes.clone()),
                    _ => None,
                };
                bytes.clear_sensitive();
                result
            }
        }
    }

    /// 获取多签签名用的私钥（遍历钱包找成员地址）
    pub fn get_multisig_signer_key(&self, ms_info: &multisig::MultisigInfo) -> Option<Vec<u8>> {
        let member_addrs: Vec<String> = ms_info.members.iter().map(|m| m.address()).collect();

        let matched_address = {
            let store = self.store.as_ref()?;
            let mut found = None;
            for wallet in &store.wallets {
                match &wallet.wallet_type {
                    WalletType::Mnemonic { sol_accounts, .. } => {
                        for acc in sol_accounts {
                            if member_addrs.contains(&acc.address) {
                                found = Some(acc.address.clone());
                                break;
                            }
                        }
                    }
                    WalletType::PrivateKey {
                        chain_type: ChainType::Solana,
                        address,
                        ..
                    } => {
                        if member_addrs.contains(address) {
                            found = Some(address.clone());
                        }
                    }
                    _ => {}
                }
                if found.is_some() { break; }
            }
            found?
        };

        self.get_sol_private_key(&matched_address)
    }

    /// 获取转账私钥（ETH 或 SOL）
    pub fn get_transfer_private_key(
        &self,
        wallet_index: usize,
        account_index: Option<usize>,
        chain_type: &ChainType,
    ) -> Option<Vec<u8>> {
        enum TransferKeySource {
            Mnemonic { encrypted: String, chain: ChainType, derivation_index: u32 },
            PrivateKey { encrypted: String, chain: ChainType },
        }
        let source = {
            let store = self.store.as_ref()?;
            let wallet = store.wallets.get(wallet_index)?;
            match &wallet.wallet_type {
                WalletType::Mnemonic { encrypted_mnemonic, eth_accounts, sol_accounts, .. } => {
                    let (accs, chain) = match chain_type {
                        ChainType::Ethereum => (eth_accounts.as_slice(), ChainType::Ethereum),
                        ChainType::Solana => (sol_accounts.as_slice(), ChainType::Solana),
                    };
                    let acc = accs.get(account_index.unwrap_or(0))?;
                    TransferKeySource::Mnemonic {
                        encrypted: encrypted_mnemonic.clone(),
                        chain,
                        derivation_index: acc.derivation_index,
                    }
                }
                WalletType::PrivateKey { encrypted_private_key, chain_type: ct, .. } => {
                    TransferKeySource::PrivateKey {
                        encrypted: encrypted_private_key.clone(),
                        chain: ct.clone(),
                    }
                }
                _ => return None,
            }
        };

        match source {
            TransferKeySource::Mnemonic { encrypted, chain, derivation_index } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                match chain {
                    ChainType::Ethereum => {
                        let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                        phrase.clear_sensitive();
                        let result = eth_keys::derive_eth_private_key(&seed, derivation_index).ok();
                        seed.clear_sensitive();
                        result
                    }
                    ChainType::Solana => {
                        let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                        phrase.clear_sensitive();
                        let result = sol_keys::derive_sol_private_key(&seed, derivation_index).ok();
                        seed.clear_sensitive();
                        result
                    }
                }
            }
            TransferKeySource::PrivateKey { encrypted, chain } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let result = match chain {
                    ChainType::Ethereum => {
                        let clean = pk_str.strip_prefix("0x").unwrap_or(&pk_str);
                        hex::decode(clean).ok()
                    }
                    ChainType::Solana => {
                        let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                        let r = match bytes.len() {
                            64 => Some(bytes[..32].to_vec()),
                            32 => Some(bytes.clone()),
                            _ => None,
                        };
                        bytes.clear_sensitive();
                        r
                    }
                };
                pk_str.clear_sensitive();
                result
            }
        }
    }

    // ========== 钱包查询 ==========

    /// 获取 SOL 地址
    pub fn get_sol_address(&self, wallet_index: usize, account_index: usize) -> Option<String> {
        let store = self.store.as_ref()?;
        let wallet = store.wallets.get(wallet_index)?;
        match &wallet.wallet_type {
            WalletType::Mnemonic { sol_accounts, .. } => {
                sol_accounts.get(account_index).map(|a| a.address.clone())
            }
            WalletType::PrivateKey {
                chain_type: ChainType::Solana,
                address,
                ..
            } => Some(address.clone()),
            _ => None,
        }
    }

    /// 收集本地所有 SOL 地址
    pub fn collect_local_sol_addresses(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        let store = match &self.store {
            Some(s) => s,
            None => return result,
        };

        for wallet in &store.wallets {
            if wallet.hidden { continue; }
            match &wallet.wallet_type {
                WalletType::Mnemonic { sol_accounts, .. } => {
                    for acc in sol_accounts {
                        if !acc.hidden {
                            let label = acc.label.clone().unwrap_or_else(|| wallet.name.clone());
                            result.push((acc.address.clone(), label));
                        }
                    }
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    address,
                    ..
                } => {
                    result.push((address.clone(), wallet.name.clone()));
                }
                _ => {}
            }
        }
        result
    }

    /// 构建有余额的 SOL 地址列表（用于 fee payer 选择）
    pub fn build_fee_payer_list(&self, exclude_address: &str) -> Vec<FeePayer> {
        let mut list = Vec::new();
        let store = match self.store.as_ref() {
            Some(s) => s,
            None => return list,
        };
        for (wi, wallet) in store.wallets.iter().enumerate() {
            if wallet.hidden { continue; }
            let sol_addrs: Vec<(String, String, usize)> = match &wallet.wallet_type {
                WalletType::Mnemonic { sol_accounts, .. } => {
                    sol_accounts.iter().enumerate()
                        .filter(|(_, acc)| !acc.hidden)
                        .map(|(ai, acc)| {
                            let label = acc.label.as_deref().unwrap_or(&wallet.name).to_string();
                            (acc.address.clone(), label, ai)
                        })
                        .collect()
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    address, label, hidden, ..
                } => {
                    if *hidden { vec![] } else {
                        let lbl = label.as_deref().unwrap_or(&wallet.name).to_string();
                        vec![(address.clone(), lbl, 0)]
                    }
                }
                _ => vec![],
            };
            for (addr, label, ai) in sol_addrs {
                if addr == exclude_address { continue; }
                // 排除 Vote/Stake 账户
                let is_special = self.balance_cache.get(&addr)
                    .and_then(|p| p.account_owner.as_deref())
                    .is_some_and(|o| o == crate::chain::solana::VOTE_PROGRAM || o == crate::chain::solana::STAKE_PROGRAM);
                if is_special { continue; }
                let has_balance = self.balance_cache.get(&addr)
                    .is_some_and(|p| p.chains.iter().any(|c| c.native_balance > 0));
                if has_balance {
                    let balance_lamports = self.balance_cache.get(&addr)
                        .and_then(|p| p.chains.iter().find(|c| c.native_balance > 0))
                        .map(|c| c.native_balance)
                        .unwrap_or(0);
                    list.push(FeePayer { address: addr, label, balance_lamports, wallet_index: wi, account_index: ai });
                }
            }
        }
        list
    }

    // ========== RPC URL 辅助 ==========

    pub fn get_solana_rpc_url(&self) -> String {
        self.config.chains.solana.first()
            .map(|c| c.rpc_url.clone())
            .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string())
    }

    pub fn get_sol_rpc_url(&self) -> String {
        self.get_solana_rpc_url()
    }

    pub fn get_rpc_url_for_address(&self, address: &str) -> String {
        if let Some(portfolio) = self.balance_cache.get(address)
            && let Some(chain_id) = &portfolio.account_owner_chain_id
            && let Some(cfg) = self.config.chains.solana.iter().find(|c| &c.id == chain_id)
        {
            return cfg.rpc_url.clone();
        }
        self.get_sol_rpc_url()
    }

    pub fn get_native_symbol_for_address(&self, address: &str) -> String {
        if let Some(portfolio) = self.balance_cache.get(address)
            && let Some(chain_id) = &portfolio.account_owner_chain_id
            && let Some(chain_bal) = portfolio.chains.iter().find(|c| c.chain_id == *chain_id)
        {
            return chain_bal.native_symbol.clone();
        }
        self.config.chains.solana.first()
            .map(|c| c.native_symbol.clone())
            .unwrap_or_else(|| "SOL".to_string())
    }

    pub fn get_current_ms_rpc_url(&self, wallet_index: usize, legacy_index: usize) -> String {
        if let Some(ref store) = self.store
            && let Some(w) = store.wallets.get(wallet_index)
            && let WalletType::Multisig { ref rpc_url, .. } = w.wallet_type
        {
            return rpc_url.clone();
        }
        self.store.as_ref()
            .and_then(|s| s.multisigs.get(legacy_index))
            .map(|m| m.rpc_url.clone())
            .unwrap_or_else(|| self.get_solana_rpc_url())
    }

    // ========== 钱包管理 ==========

    pub fn next_sort_order(&self) -> u32 {
        self.store.as_ref()
            .map(|s| s.wallets.len() as u32)
            .unwrap_or(0)
    }

    /// 添加钱包到 store 并保存
    pub fn add_wallet(&mut self, wallet: Wallet) -> Result<(), String> {
        if let Some(ref mut store) = self.store {
            store.wallets.push(wallet);
            self.save_store().map_err(|e| format!("保存失败: {e}"))?;
            Ok(())
        } else {
            Err("钱包未初始化".to_string())
        }
    }

    pub fn delete_wallet(&mut self, wallet_index: usize) -> Result<(), String> {
        if let Some(ref mut store) = self.store {
            if wallet_index < store.wallets.len() {
                store.wallets.remove(wallet_index);
                self.save_store().map_err(|e| format!("保存失败: {e}"))?;
                Ok(())
            } else {
                Err("无效的钱包索引".to_string())
            }
        } else {
            Err("钱包未初始化".to_string())
        }
    }

    pub fn move_wallet(&mut self, wallet_index: usize, up: bool) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let len = store.wallets.len();
        if (up && wallet_index == 0) || (!up && wallet_index + 1 >= len) {
            return Ok(());
        }
        let other = if up { wallet_index - 1 } else { wallet_index + 1 };
        store.wallets.swap(wallet_index, other);
        // 更新 sort_order
        for (i, w) in store.wallets.iter_mut().enumerate() {
            w.sort_order = i as u32;
        }
        self.save_store().map_err(|e| format!("保存失败: {e}"))?;
        Ok(())
    }

    /// 保存多签到 store
    pub fn save_multisig_to_store(
        &mut self,
        info: &multisig::MultisigInfo,
        rpc_url: &str,
        chain_id: &str,
        chain_name: &str,
    ) -> Option<usize> {
        let info_address_str = info.address.to_string();

        // 检查是否已存在
        if let Some(ref store) = self.store {
            let exists = store.wallets.iter().any(|w| {
                matches!(&w.wallet_type, WalletType::Multisig { multisig_address, .. } if multisig_address == &info_address_str)
            });
            if exists { return None; }
        }

        let (vault_pda, _) = multisig::derive_vault_pda(&info.address, 0);

        if let Some(ref mut store) = self.store {
            let wallet_index = store.wallets.len();
            store.wallets.push(Wallet {
                id: uuid::Uuid::new_v4().to_string(),
                name: format!("Multisig {}", &info_address_str[..8]),
                wallet_type: WalletType::Multisig {
                    multisig_address: info_address_str,
                    rpc_url: rpc_url.to_string(),
                    chain_id: chain_id.to_string(),
                    chain_name: chain_name.to_string(),
                    threshold: info.threshold,
                    member_addresses: info.members.iter().map(|m| m.address()).collect(),
                    vaults: vec![VaultAccount {
                        vault_index: 0,
                        address: vault_pda.to_string(),
                        label: None,
                        hidden: false,
                    }],
                },
                sort_order: wallet_index as u32,
                hidden: false,
                created_at: chrono::Utc::now().timestamp(),
            });
            let _ = self.save_store();
            Some(wallet_index)
        } else {
            None
        }
    }

    pub fn add_vault_to_multisig(&mut self, wallet_index: usize) {
        if let Some(ref mut store) = self.store
            && let Some(w) = store.wallets.get_mut(wallet_index)
            && let WalletType::Multisig { ref multisig_address, ref mut vaults, .. } = w.wallet_type
        {
            let next_index = vaults.iter().map(|v| v.vault_index).max().unwrap_or(0) + 1;
            let ms_pubkey = multisig_address.parse::<multisig::Pubkey>().expect("invalid multisig address");
            let (vault_pda, _) = multisig::derive_vault_pda(&ms_pubkey, next_index);
            vaults.push(VaultAccount {
                vault_index: next_index,
                address: vault_pda.to_string(),
                label: None,
                hidden: false,
            });
            let _ = self.save_store();
        }
    }

    // ========== 内部辅助 ==========

    fn derive_sol_key(&self, source: SolKeySource) -> Option<Vec<u8>> {
        match source {
            SolKeySource::Mnemonic { encrypted, derivation_index } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.clear_sensitive();
                let result = sol_keys::derive_sol_private_key(&seed, derivation_index).ok();
                seed.clear_sensitive();
                result
            }
            SolKeySource::PrivateKey { encrypted } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                pk_str.clear_sensitive();
                let result = match bytes.len() {
                    64 => Some(bytes[..32].to_vec()),
                    32 => Some(bytes.clone()),
                    _ => None,
                };
                bytes.clear_sensitive();
                result
            }
        }
    }
}

enum SolKeySource {
    Mnemonic { encrypted: String, derivation_index: u32 },
    PrivateKey { encrypted: String },
}

/// Fee Payer 信息
pub struct FeePayer {
    pub address: String,
    pub label: String,
    pub balance_lamports: u128,
    pub wallet_index: usize,
    pub account_index: usize,
}

/// 生成助记词
pub fn generate_mnemonic() -> Result<String, crate::error::CryptoError> {
    mnemonic::generate_mnemonic()
}
