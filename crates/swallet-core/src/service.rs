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

    pub fn change_password(&mut self, old_password: &[u8], new_password: &[u8]) -> Result<(), crate::error::StorageError> {
        if !self.verify_password(old_password) {
            return Err(crate::error::StorageError::InvalidFormat("旧密码错误".into()));
        }
        if self.store.is_none() {
            return Err(crate::error::StorageError::InvalidFormat("钱包未解锁".into()));
        }
        self.password = Some(new_password.to_vec());
        self.save_store()?;
        Ok(())
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
        // 优先从 config 按 chain_id 查 RPC URL
        if let Some(ref store) = self.store
            && let Some(w) = store.wallets.get(wallet_index)
            && let WalletType::Multisig { ref chain_id, .. } = w.wallet_type
        {
            if let Some(cfg) = self.config.chains.solana.iter().find(|c| c.id == *chain_id) {
                return cfg.rpc_url.clone();
            }
        }
        // 回退到 legacy multisigs
        if let Some(ref store) = self.store {
            if let Some(ms) = store.multisigs.get(legacy_index) {
                if let Some(cfg) = self.config.chains.solana.iter().find(|c| c.id == ms.chain_id) {
                    return cfg.rpc_url.clone();
                }
            }
        }
        self.get_solana_rpc_url()
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

    // ========== 钱包创建 ==========

    /// 创建助记词钱包（加密助记词 + 派生 ETH/SOL 地址）
    pub fn create_mnemonic_wallet(&mut self, name: &str, phrase: &str) -> Result<(), String> {
        let mut seed = mnemonic::mnemonic_to_seed(phrase, "")
            .map_err(|e| format!("种子生成失败: {e}"))?;
        let eth_addr = eth_keys::derive_eth_address(&seed, 0)
            .map_err(|e| format!("ETH 地址派生失败: {e}"))?;
        let sol_addr = sol_keys::derive_sol_address(&seed, 0)
            .map_err(|e| format!("SOL 地址派生失败: {e}"))?;
        seed.clear_sensitive();

        let pw = self.password.as_ref().ok_or("密码未设置")?;
        let (salt, nonce, ct) = crate::crypto::encryption::encrypt(phrase.as_bytes(), pw)
            .map_err(|e| format!("加密失败: {e}"))?;
        let encrypted_mnemonic = format!("{}:{}:{}", hex::encode(&salt), hex::encode(&nonce), hex::encode(&ct));

        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            wallet_type: WalletType::Mnemonic {
                encrypted_mnemonic,
                eth_accounts: vec![crate::storage::data::DerivedAccount {
                    derivation_index: 0, address: eth_addr, label: None, hidden: false,
                }],
                sol_accounts: vec![crate::storage::data::DerivedAccount {
                    derivation_index: 0, address: sol_addr, label: None, hidden: false,
                }],
                next_eth_index: 1,
                next_sol_index: 1,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.add_wallet(wallet)
    }

    /// 导入私钥钱包
    pub fn import_private_key_wallet(&mut self, name: &str, pk: &str, chain_type: ChainType) -> Result<(), String> {
        let address = match &chain_type {
            ChainType::Ethereum => eth_keys::hex_private_key_to_address(pk)
                .map_err(|e| format!("无效的 ETH 私钥: {e}"))?,
            ChainType::Solana => sol_keys::bs58_private_key_to_address(pk)
                .map_err(|e| format!("无效的 SOL 私钥: {e}"))?,
        };
        let pw = self.password.as_ref().ok_or("密码未设置")?;
        let (salt, nonce, ct) = crate::crypto::encryption::encrypt(pk.as_bytes(), pw)
            .map_err(|e| format!("加密失败: {e}"))?;
        let encrypted_pk = format!("{}:{}:{}", hex::encode(&salt), hex::encode(&nonce), hex::encode(&ct));

        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            wallet_type: WalletType::PrivateKey {
                chain_type, encrypted_private_key: encrypted_pk, address, label: None, hidden: false,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.add_wallet(wallet)
    }

    /// 导入观察钱包
    pub fn import_watch_wallet(&mut self, name: &str, address: &str, chain_type: ChainType) -> Result<(), String> {
        // 地址格式验证
        match &chain_type {
            ChainType::Ethereum => {
                if !address.starts_with("0x") || address.len() != 42
                    || !address[2..].chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err("无效的 ETH 地址".to_string());
                }
            }
            ChainType::Solana => {
                if bs58::decode(address).into_vec().is_err() {
                    return Err("无效的 SOL 地址".to_string());
                }
            }
        }
        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            wallet_type: WalletType::WatchOnly {
                chain_type, address: address.to_string(), label: None,
                source: crate::storage::data::WatchOnlySource::Manual,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };
        self.add_wallet(wallet)
    }

    /// 添加派生地址
    pub fn add_derived_address(&mut self, wallet_index: usize, chain_type: ChainType) -> Result<String, String> {
        let (encrypted_mnemonic, eth_idx, sol_idx) = {
            let store = self.store.as_ref().ok_or("钱包未初始化")?;
            let wallet = store.wallets.get(wallet_index).ok_or("无效的钱包索引")?;
            match &wallet.wallet_type {
                WalletType::Mnemonic { encrypted_mnemonic, next_eth_index, next_sol_index, .. } => {
                    (encrypted_mnemonic.clone(), *next_eth_index, *next_sol_index)
                }
                _ => return Err("不是助记词钱包".to_string()),
            }
        };

        let phrase = self.decrypt_inner_secret(&encrypted_mnemonic).ok_or("解密助记词失败")?;
        let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").map_err(|e| format!("种子生成失败: {e}"))?;

        let new_address = match chain_type {
            ChainType::Ethereum => {
                let addr = eth_keys::derive_eth_address(&seed, eth_idx).map_err(|e| format!("派生失败: {e}"))?;
                seed.clear_sensitive();
                if let Some(ref mut store) = self.store
                    && let Some(wallet) = store.wallets.get_mut(wallet_index)
                    && let WalletType::Mnemonic { ref mut eth_accounts, ref mut next_eth_index, .. } = wallet.wallet_type
                {
                    eth_accounts.push(crate::storage::data::DerivedAccount {
                        derivation_index: eth_idx, address: addr.clone(), label: None, hidden: false,
                    });
                    *next_eth_index = eth_idx + 1;
                }
                addr
            }
            ChainType::Solana => {
                let addr = sol_keys::derive_sol_address(&seed, sol_idx).map_err(|e| format!("派生失败: {e}"))?;
                seed.clear_sensitive();
                if let Some(ref mut store) = self.store
                    && let Some(wallet) = store.wallets.get_mut(wallet_index)
                    && let WalletType::Mnemonic { ref mut sol_accounts, ref mut next_sol_index, .. } = wallet.wallet_type
                {
                    sol_accounts.push(crate::storage::data::DerivedAccount {
                        derivation_index: sol_idx, address: addr.clone(), label: None, hidden: false,
                    });
                    *next_sol_index = sol_idx + 1;
                }
                addr
            }
        };
        self.save_store().map_err(|e| format!("保存失败: {e}"))?;
        Ok(new_address)
    }

    // ========== 钱包编辑 ==========

    pub fn edit_wallet_name(&mut self, wallet_index: usize, name: &str) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let wallet = store.wallets.get_mut(wallet_index).ok_or("无效的钱包索引")?;
        wallet.name = name.to_string();
        self.save_store().map_err(|e| format!("保存失败: {e}"))
    }

    pub fn edit_address_label(&mut self, wallet_index: usize, chain_type: &str, account_index: usize, label: &str) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let wallet = store.wallets.get_mut(wallet_index).ok_or("无效的钱包索引")?;
        match &mut wallet.wallet_type {
            WalletType::Mnemonic { eth_accounts, sol_accounts, .. } => {
                let label_val = if label.is_empty() { None } else { Some(label.to_string()) };
                if chain_type == "ethereum" {
                    if let Some(acc) = eth_accounts.get_mut(account_index) { acc.label = label_val; }
                } else if let Some(acc) = sol_accounts.get_mut(account_index) { acc.label = label_val; }
            }
            WalletType::PrivateKey { label: existing_label, .. } => {
                *existing_label = if label.is_empty() { None } else { Some(label.to_string()) };
            }
            WalletType::WatchOnly { label: existing_label, .. } => {
                *existing_label = if label.is_empty() { None } else { Some(label.to_string()) };
            }
            WalletType::Multisig { vaults, .. } => {
                if let Some(v) = vaults.get_mut(account_index) {
                    v.label = if label.is_empty() { None } else { Some(label.to_string()) };
                }
            }
        }
        self.save_store().map_err(|e| format!("保存失败: {e}"))
    }

    pub fn hide_wallet(&mut self, wallet_index: usize) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let wallet = store.wallets.get_mut(wallet_index).ok_or("无效的钱包索引")?;
        wallet.hidden = true;
        self.save_store().map_err(|e| format!("保存失败: {e}"))
    }

    pub fn show_wallet(&mut self, wallet_index: usize) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let wallet = store.wallets.get_mut(wallet_index).ok_or("无效的钱包索引")?;
        wallet.hidden = false;
        self.save_store().map_err(|e| format!("保存失败: {e}"))
    }

    pub fn hide_address(&mut self, wallet_index: usize, chain_type: &str, account_index: usize) -> Result<(), String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let wallet = store.wallets.get_mut(wallet_index).ok_or("无效的钱包索引")?;
        match &mut wallet.wallet_type {
            WalletType::Mnemonic { eth_accounts, sol_accounts, .. } => {
                if chain_type == "ethereum" {
                    if let Some(acc) = eth_accounts.get_mut(account_index) { acc.hidden = true; }
                } else if let Some(acc) = sol_accounts.get_mut(account_index) { acc.hidden = true; }
            }
            WalletType::Multisig { vaults, .. } => {
                if let Some(v) = vaults.get_mut(account_index) { v.hidden = true; }
            }
            _ => {}
        }
        self.save_store().map_err(|e| format!("保存失败: {e}"))
    }

    pub fn restore_hidden_wallets(&mut self) -> Result<usize, String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let mut count = 0;
        for w in &mut store.wallets {
            if w.hidden { w.hidden = false; count += 1; }
        }
        self.save_store().map_err(|e| format!("保存失败: {e}"))?;
        Ok(count)
    }

    pub fn restore_hidden_addresses(&mut self) -> Result<usize, String> {
        let store = self.store.as_mut().ok_or("钱包未初始化")?;
        let mut count = 0;
        for w in &mut store.wallets {
            match &mut w.wallet_type {
                WalletType::Mnemonic { eth_accounts, sol_accounts, .. } => {
                    for a in eth_accounts.iter_mut().chain(sol_accounts.iter_mut()) {
                        if a.hidden { a.hidden = false; count += 1; }
                    }
                }
                WalletType::Multisig { vaults, .. } => {
                    for v in vaults { if v.hidden { v.hidden = false; count += 1; } }
                }
                _ => {}
            }
        }
        self.save_store().map_err(|e| format!("保存失败: {e}"))?;
        Ok(count)
    }

    /// 保存多签到 store
    pub fn save_multisig_to_store(
        &mut self,
        info: &multisig::MultisigInfo,
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

// ========== 异步业务函数 ==========

/// 执行转账（ETH/ERC20/SOL/SPL）
pub async fn execute_transfer(
    private_key: Vec<u8>,
    asset: crate::transfer::TransferableAsset,
    to_address: String,
    amount_raw: u128,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    match (&asset.chain_type, &asset.asset_kind) {
        (ChainType::Ethereum, crate::transfer::AssetKind::Native) => {
            let chain_id = asset.evm_chain_id.ok_or("缺少 chain_id")?;
            crate::transfer::eth_transfer::send_eth_native(
                &client, &asset.rpc_url, chain_id, &private_key, &to_address, amount_raw,
            ).await
        }
        (ChainType::Ethereum, crate::transfer::AssetKind::Erc20 { contract_address }) => {
            let chain_id = asset.evm_chain_id.ok_or("缺少 chain_id")?;
            crate::transfer::eth_transfer::send_erc20(
                &client, &asset.rpc_url, chain_id, &private_key, contract_address, &to_address, amount_raw,
            ).await
        }
        (ChainType::Solana, crate::transfer::AssetKind::Native) => {
            let amount_u64: u64 = amount_raw.try_into().map_err(|_| "SOL 转账数量超出范围".to_string())?;
            crate::transfer::sol_transfer::send_sol_native(
                &client, &asset.rpc_url, &private_key, &to_address, amount_u64,
            ).await
        }
        (ChainType::Solana, crate::transfer::AssetKind::SplToken { mint_address, is_token_2022 }) => {
            let amount_u64: u64 = amount_raw.try_into().map_err(|_| "SPL 转账数量超出范围".to_string())?;
            let token_program = if *is_token_2022 {
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
            } else {
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            };
            crate::transfer::sol_transfer::send_spl_token(
                &client, &asset.rpc_url, &private_key, mint_address, &to_address, amount_u64, token_program,
            ).await
        }
        _ => Err("不支持的转账类型".into()),
    }
}

/// 执行创建提案
#[allow(clippy::too_many_arguments)]
pub async fn execute_create_proposal(
    rpc_url: &str,
    private_key: &[u8],
    fee_payer_key: &[u8],
    multisig_address: &str,
    proposal_type_idx: usize,
    to_address: &str,
    amount_str: &str,
    upgrade_program: &str,
    upgrade_buffer: &str,
    preset_program_idx: usize,
    preset_instruction_idx: usize,
    preset_args: &[String],
    chain_id: &str,
    vault_index: u8,
    vs_op: Option<&crate::multisig::MsVoteStakeOp>,
    vs_target: &str,
    vs_param: &str,
    vs_amount: &str,
) -> Result<String, String> {
    use std::str::FromStr;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let multisig_pubkey = solana_sdk::pubkey::Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    let (vault_pda, _) = crate::multisig::derive_vault_pda(&multisig_pubkey, vault_index);

    let proposal_types = crate::multisig::ProposalType::for_chain(chain_id);
    let proposal_type = proposal_types.get(proposal_type_idx).ok_or("无效的提案类型")?;

    let inner_instructions = match proposal_type {
        crate::multisig::ProposalType::SolTransfer => {
            let to_pubkey: [u8; 32] = bs58::decode(to_address)
                .into_vec().map_err(|e| format!("无效的目标地址: {e}"))?
                .try_into().map_err(|_| "目标地址长度无效".to_string())?;
            let amount_raw = crate::transfer::parse_amount(amount_str, 9)?;
            let lamports: u64 = amount_raw.try_into().map_err(|_| "SOL 数量超出范围".to_string())?;
            vec![crate::multisig::proposals::build_sol_transfer_instruction(
                &vault_pda.to_bytes(), &to_pubkey, lamports,
            )]
        }
        crate::multisig::ProposalType::TokenTransfer => {
            return Err("Token 转账提案暂未实现，请使用 SOL 转账".into());
        }
        crate::multisig::ProposalType::ProgramCall => {
            let programs = crate::multisig::presets::programs_for_chain(chain_id);
            let program = programs.get(preset_program_idx).ok_or("无效的预制程序")?;
            let instruction = program.instructions.get(preset_instruction_idx).ok_or("无效的预制指令")?;
            (instruction.build)(&vault_pda.to_bytes(), &program.program_id, preset_args)?
        }
        crate::multisig::ProposalType::ProgramUpgrade => {
            let program_bytes: [u8; 32] = bs58::decode(upgrade_program)
                .into_vec().map_err(|e| format!("无效的程序地址: {e}"))?
                .try_into().map_err(|_| "程序地址长度无效".to_string())?;
            let buffer_bytes: [u8; 32] = bs58::decode(upgrade_buffer)
                .into_vec().map_err(|e| format!("无效的 Buffer 地址: {e}"))?
                .try_into().map_err(|_| "Buffer 地址长度无效".to_string())?;
            verify_upgrade_authority(&client, rpc_url, &program_bytes, &vault_pda).await?;
            verify_buffer_exists(&client, rpc_url, upgrade_buffer).await?;

            // 检查是否需要扩容 ProgramData（ExtendProgram 不能通过 Squads CPI 执行，需直接交易）
            let extend_bytes = check_program_extend_needed(&client, rpc_url, &program_bytes, upgrade_buffer).await?;
            if extend_bytes > 0 {
                eprintln!("[upgrade] 需要先扩容 ProgramData {} 字节，由 fee payer 直接执行", extend_bytes);
                extend_program_direct(&client, rpc_url, fee_payer_key, &program_bytes, extend_bytes).await?;
                eprintln!("[upgrade] 扩容完成，继续创建升级提案");
            }

            crate::multisig::proposals::build_program_upgrade_instructions(
                &program_bytes, &buffer_bytes, &vault_pda.to_bytes(), &vault_pda.to_bytes(),
            )
        }
        crate::multisig::ProposalType::VoteManage | crate::multisig::ProposalType::StakeManage => {
            let op = vs_op.ok_or("未选择操作类型")?;
            let target_bytes: [u8; 32] = crate::multisig::proposals::decode_bs58_pubkey(vs_target)
                .ok_or_else(|| format!("无效的目标地址: {vs_target}"))?;
            let vault_bytes = vault_pda.to_bytes();

            use crate::multisig::MsVoteStakeOp;
            match op {
                MsVoteStakeOp::VoteAuthorizeVoter => {
                    let new_auth = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的新权限地址")?;
                    vec![crate::multisig::proposals::build_vote_authorize_instruction(&target_bytes, &vault_bytes, &new_auth, 0)]
                }
                MsVoteStakeOp::VoteAuthorizeWithdrawer => {
                    let new_auth = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的新权限地址")?;
                    vec![crate::multisig::proposals::build_vote_authorize_instruction(&target_bytes, &vault_bytes, &new_auth, 1)]
                }
                MsVoteStakeOp::VoteWithdraw => {
                    let to_bytes = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的提取目标地址")?;
                    let lamports: u64 = crate::transfer::parse_amount(vs_amount, 9)?.try_into().map_err(|_| "SOL 数量超出范围".to_string())?;
                    vec![crate::multisig::proposals::build_vote_withdraw_instruction(&target_bytes, &to_bytes, &vault_bytes, lamports)]
                }
                MsVoteStakeOp::StakeAuthorizeStaker => {
                    let new_auth = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的新权限地址")?;
                    vec![crate::multisig::proposals::build_stake_authorize_instruction(&target_bytes, &vault_bytes, &new_auth, 0)]
                }
                MsVoteStakeOp::StakeAuthorizeWithdrawer => {
                    let new_auth = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的新权限地址")?;
                    vec![crate::multisig::proposals::build_stake_authorize_instruction(&target_bytes, &vault_bytes, &new_auth, 1)]
                }
                MsVoteStakeOp::StakeDelegate => {
                    let vote_account = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的 Vote 账户地址")?;
                    vec![crate::multisig::proposals::build_stake_delegate_instruction(&target_bytes, &vote_account, &vault_bytes)]
                }
                MsVoteStakeOp::StakeDeactivate => {
                    vec![crate::multisig::proposals::build_stake_deactivate_instruction(&target_bytes, &vault_bytes)]
                }
                MsVoteStakeOp::StakeWithdraw => {
                    let to_bytes = crate::multisig::proposals::decode_bs58_pubkey(vs_param).ok_or("无效的提取目标地址")?;
                    let lamports: u64 = crate::transfer::parse_amount(vs_amount, 9)?.try_into().map_err(|_| "SOL 数量超出范围".to_string())?;
                    vec![crate::multisig::proposals::build_stake_withdraw_instruction(&target_bytes, &to_bytes, &vault_bytes, lamports)]
                }
            }
        }
    };

    crate::multisig::squads::create_proposal_and_approve(
        &client, rpc_url, private_key, fee_payer_key, multisig_address, vault_index, inner_instructions,
    ).await
}

/// 检查程序的 upgrade authority 是否为指定的 vault PDA
pub async fn verify_upgrade_authority(
    client: &reqwest::Client,
    rpc_url: &str,
    program_bytes: &[u8; 32],
    vault_pda: &solana_sdk::pubkey::Pubkey,
) -> Result<(), String> {
    use solana_sdk::pubkey::Pubkey;
    use std::str::FromStr;

    let program_pk = Pubkey::new_from_array(*program_bytes);
    let bpf_loader_id = Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111")
        .map_err(|e| format!("BPF Loader 地址解析失败: {e}"))?;
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_loader_id);

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [programdata_pda.to_string(), {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;

    let value = resp.get("result").and_then(|r| r.get("value")).ok_or("无法获取 ProgramData 账户")?;
    if value.is_null() { return Err("ProgramData 账户不存在，请确认程序地址正确".into()); }

    let data_arr = value.get("data").and_then(|d| d.as_array()).ok_or("ProgramData 缺少 data 字段")?;
    let base64_str = data_arr.first().and_then(|v| v.as_str()).ok_or("ProgramData 数据格式无效")?;
    let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_str)
        .map_err(|e| format!("ProgramData base64 解码失败: {e}"))?;

    if data.len() < 45 { return Err("ProgramData 账户数据过短".into()); }
    let variant = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if variant != 3 { return Err(format!("不是有效的 ProgramData 账户 (variant={})", variant)); }
    if data[12] == 0 { return Err("程序不可升级（upgrade authority 已撤销）".into()); }

    let authority = Pubkey::try_from(&data[13..45]).map_err(|_| "解析 upgrade authority 失败")?;
    if authority != *vault_pda {
        return Err(format!("upgrade authority 不匹配\n当前 vault: {}\n链上 authority: {}", vault_pda, authority));
    }
    Ok(())
}

/// 检查 buffer 账户是否存在
pub async fn verify_buffer_exists(
    client: &reqwest::Client,
    rpc_url: &str,
    buffer_address: &str,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [buffer_address, {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;
    let value = resp.get("result").and_then(|r| r.get("value"));
    if value.is_none() || value.unwrap().is_null() {
        return Err(format!("Buffer 账户 {} 不存在，请先执行 solana program write-buffer", buffer_address));
    }
    Ok(())
}

/// 检查 ProgramData 是否需要扩容，返回需要扩展的字节数（0 表示无需扩容）
///
/// Buffer 账户布局: variant(4) + Option<authority>(1+32) + program_data
/// ProgramData 账户布局: variant(4) + slot(8) + Option<authority>(1+32) + program_data
/// ProgramData 头部比 Buffer 多 8 字节（slot），所以实际可用空间 = data_len - 45
/// Buffer 中程序大小 = data_len - 37
pub async fn check_program_extend_needed(
    client: &reqwest::Client,
    rpc_url: &str,
    program_bytes: &[u8; 32],
    buffer_address: &str,
) -> Result<u32, String> {
    use solana_sdk::pubkey::Pubkey;

    let program_pk = Pubkey::new_from_array(*program_bytes);
    let bpf_loader_id: Pubkey = "BPFLoaderUpgradeab1e11111111111111111111111".parse().unwrap();
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_loader_id);

    // 获取 ProgramData 账户大小
    let pd_size = get_account_data_len(client, rpc_url, &programdata_pda.to_string()).await
        .map_err(|e| format!("获取 ProgramData 大小失败: {e}"))?;

    // 获取 Buffer 账户大小
    let buf_size = get_account_data_len(client, rpc_url, buffer_address).await
        .map_err(|e| format!("获取 Buffer 大小失败: {e}"))?;

    // Buffer 中程序二进制大小 = buf_size - 37 (4 variant + 1 option flag + 32 authority)
    // ProgramData 中可用空间 = pd_size - 45 (4 variant + 8 slot + 1 option flag + 32 authority)
    let buffer_program_len = buf_size.saturating_sub(37);
    let programdata_capacity = pd_size.saturating_sub(45);

    if buffer_program_len > programdata_capacity {
        let extend = (buffer_program_len - programdata_capacity) as u32;
        eprintln!("[extend] ProgramData 需要扩容 {} 字节 (buffer程序={}，当前容量={})",
            extend, buffer_program_len, programdata_capacity);
        Ok(extend)
    } else {
        Ok(0)
    }
}

/// 获取账户数据长度（使用 dataSlice 避免下载大账户全部数据）
async fn get_account_data_len(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<usize, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64", "dataSlice": {"offset": 0, "length": 0}, "commitment": "confirmed"}],
        "id": 1
    });
    let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;
    let value = resp.get("result").and_then(|r| r.get("value"))
        .ok_or_else(|| format!("账户 {} 不存在", address))?;
    if value.is_null() {
        return Err(format!("账户 {} 不存在", address));
    }
    value.get("space").and_then(|s| s.as_u64())
        .map(|s| s as usize)
        .ok_or_else(|| format!("账户 {} 缺少 space 字段", address))
}

/// 直接执行 ExtendProgram 交易（不经过多签，由 fee payer 付费）
pub async fn extend_program_direct(
    client: &reqwest::Client,
    rpc_url: &str,
    payer_key: &[u8],
    program_bytes: &[u8; 32],
    additional_len: u32,
) -> Result<(), String> {
    use solana_sdk::pubkey::Pubkey;

    let payer_bytes: [u8; 32] = payer_key.try_into()
        .map_err(|_| "payer 私钥长度无效".to_string())?;
    let payer_kp = solana_sdk::signer::keypair::Keypair::new_from_array(payer_bytes);
    use solana_sdk::signer::Signer;
    let payer_pk = payer_kp.pubkey();

    let bpf_loader: Pubkey = "BPFLoaderUpgradeab1e11111111111111111111111".parse().unwrap();
    let program_pk = Pubkey::new_from_array(*program_bytes);
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_loader);

    // ExtendProgram: discriminator=6(u32 LE) + additional_len(u32 LE)
    let mut ix_data = vec![6u8, 0, 0, 0];
    ix_data.extend_from_slice(&additional_len.to_le_bytes());

    use crate::transfer::sol_transfer::{Instruction, AccountMeta};
    let ix = Instruction {
        program_id: bpf_loader.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: programdata_pda.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: program_pk.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: solana_sdk::system_program::ID.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: payer_pk.to_bytes(), is_signer: true, is_writable: true },
        ],
        data: ix_data,
    };

    let recent_blockhash = crate::transfer::sol_transfer::get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = crate::transfer::sol_transfer::build_and_serialize_message(
        &payer_pk.to_bytes(), &recent_blockhash, &[ix],
    );
    let sig = payer_kp.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = crate::transfer::sol_transfer::build_transaction(&[sig_bytes], &message_bytes);
    let tx_sig = crate::transfer::sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await?;
    eprintln!("[extend] ExtendProgram tx: {}", tx_sig);

    // 等待确认
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    Ok(())
}

/// 验证预制程序的 config PDA 中的 authority 是否为当前 vault
pub async fn verify_program_authority(
    client: &reqwest::Client,
    rpc_url: &str,
    program_id: &[u8; 32],
    vault_pda: &solana_sdk::pubkey::Pubkey,
) -> Result<(), String> {
    use solana_sdk::pubkey::Pubkey;

    let pid = Pubkey::new_from_array(*program_id);
    let config_pda = Pubkey::find_program_address(&[b"config"], &pid).0;
    let quest_config_pda = Pubkey::find_program_address(&[b"quest_config"], &pid).0;

    for config_addr in &[config_pda, quest_config_pda] {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getAccountInfo",
            "params": [config_addr.to_string(), {"encoding": "base64", "commitment": "confirmed"}],
            "id": 1
        });
        let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;
        let value = resp.get("result").and_then(|r| r.get("value"));
        if value.is_none() || value.unwrap().is_null() { continue; }

        let data_b64 = value.unwrap().get("data")
            .and_then(|d| d.as_array()).and_then(|arr| arr.first())
            .and_then(|v| v.as_str()).ok_or("无法解析 config 账户数据")?;

        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD.decode(data_b64)
            .map_err(|e| format!("base64 解码失败: {e}"))?;

        if data.len() < 40 { return Err("config 账户数据太短".to_string()); }
        let authority_bytes: [u8; 32] = data[8..40].try_into().map_err(|_| "无法读取 authority 字段")?;
        let authority = Pubkey::new_from_array(authority_bytes);

        if authority != *vault_pda {
            return Err(format!("authority 不匹配\n当前 vault: {}\n链上 authority: {}", vault_pda, authority));
        }
        return Ok(());
    }
    Err("未找到 config 账户，该程序可能尚未初始化".to_string())
}
