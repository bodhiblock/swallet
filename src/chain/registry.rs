use std::collections::HashMap;

use futures::future::join_all;
use reqwest::Client;

use crate::config::AppConfig;
use crate::storage::data::{ChainType, WalletStore, WalletType};

use super::ethereum::query_evm_balance;
use super::solana::query_sol_balance;
use super::{AddressPortfolio, BalanceCache, ChainBalance};

/// 构建占位余额缓存（所有链显示 `-`，等待 RPC 查询）
pub fn build_placeholder_cache(config: &AppConfig, store: &WalletStore) -> BalanceCache {
    let (eth_addresses, sol_addresses, chain_sol_addresses) = collect_addresses(store);
    let mut cache: BalanceCache = HashMap::new();

    for address in &eth_addresses {
        let mut portfolio = AddressPortfolio {
            address: address.clone(),
            chains: Vec::new(),
        };
        for evm_config in &config.chains.ethereum {
            portfolio.chains.push(ChainBalance {
                chain_id: evm_config.id.clone(),
                chain_name: evm_config.name.clone(),
                native_symbol: evm_config.native_symbol.clone(),
                native_decimals: evm_config.native_decimals,
                native_balance: 0,
                staked_balance: 0,
                tokens: Vec::new(),
                rpc_failed: true,
            });
        }
        cache.insert(address.clone(), portfolio);
    }

    for address in &sol_addresses {
        let mut portfolio = AddressPortfolio {
            address: address.clone(),
            chains: Vec::new(),
        };
        for sol_config in &config.chains.solana {
            portfolio.chains.push(ChainBalance {
                chain_id: sol_config.id.clone(),
                chain_name: sol_config.name.clone(),
                native_symbol: sol_config.native_symbol.clone(),
                native_decimals: sol_config.native_decimals,
                native_balance: 0,
                staked_balance: 0,
                tokens: Vec::new(),
                rpc_failed: true,
            });
        }
        cache.insert(address.clone(), portfolio);
    }

    // 多签 vault 等限定链的地址，只添加对应链的占位
    for (address, chain_id) in &chain_sol_addresses {
        let mut portfolio = AddressPortfolio {
            address: address.clone(),
            chains: Vec::new(),
        };
        if let Some(sol_config) = config.chains.solana.iter().find(|c| c.id == *chain_id) {
            portfolio.chains.push(ChainBalance {
                chain_id: sol_config.id.clone(),
                chain_name: sol_config.name.clone(),
                native_symbol: sol_config.native_symbol.clone(),
                native_decimals: sol_config.native_decimals,
                native_balance: 0,
                staked_balance: 0,
                tokens: Vec::new(),
                rpc_failed: true,
            });
        }
        cache.insert(address.clone(), portfolio);
    }

    cache
}

/// 收集所有需要查询的地址
/// 返回 (eth_addresses, sol_addresses, chain_specific_sol_addresses)
/// chain_specific_sol_addresses: 多签 vault 等只需查特定链的地址
fn collect_addresses(store: &WalletStore) -> (Vec<String>, Vec<String>, Vec<(String, String)>) {
    let mut eth_addresses: Vec<String> = Vec::new();
    let mut sol_addresses: Vec<String> = Vec::new();
    let mut chain_sol_addresses: Vec<(String, String)> = Vec::new();

    for wallet in &store.wallets {
        if wallet.hidden {
            continue;
        }
        match &wallet.wallet_type {
            WalletType::Mnemonic {
                eth_accounts,
                sol_accounts,
                ..
            } => {
                for acc in eth_accounts.iter().filter(|a| !a.hidden) {
                    if !eth_addresses.contains(&acc.address) {
                        eth_addresses.push(acc.address.clone());
                    }
                }
                for acc in sol_accounts.iter().filter(|a| !a.hidden) {
                    if !sol_addresses.contains(&acc.address) {
                        sol_addresses.push(acc.address.clone());
                    }
                }
            }
            WalletType::PrivateKey {
                chain_type,
                address,
                hidden,
                ..
            } => {
                if !hidden {
                    match chain_type {
                        ChainType::Ethereum => {
                            if !eth_addresses.contains(address) {
                                eth_addresses.push(address.clone());
                            }
                        }
                        ChainType::Solana => {
                            if !sol_addresses.contains(address) {
                                sol_addresses.push(address.clone());
                            }
                        }
                    }
                }
            }
            WalletType::WatchOnly {
                chain_type,
                address,
                ..
            } => match chain_type {
                ChainType::Ethereum => {
                    if !eth_addresses.contains(address) {
                        eth_addresses.push(address.clone());
                    }
                }
                ChainType::Solana => {
                    if !sol_addresses.contains(address) {
                        sol_addresses.push(address.clone());
                    }
                }
            },
            WalletType::Multisig { vaults, chain_id, .. } => {
                for v in vaults.iter().filter(|v| !v.hidden) {
                    let pair = (v.address.clone(), chain_id.clone());
                    if !chain_sol_addresses.contains(&pair) {
                        chain_sol_addresses.push(pair);
                    }
                }
            }
        }
    }

    (eth_addresses, sol_addresses, chain_sol_addresses)
}

/// 查询所有钱包地址的余额（全并发，每个查询独立 task）
pub async fn fetch_all_balances(config: &AppConfig, store: &WalletStore) -> BalanceCache {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default();

    let (eth_addresses, sol_addresses, chain_sol_addresses) = collect_addresses(store);

    // 每个 (地址, 链) 独立 spawn，真正多线程并发
    let mut handles = Vec::new();

    for address in &eth_addresses {
        for evm_config in &config.chains.ethereum {
            let client = client.clone();
            let cfg = evm_config.clone();
            let addr = address.clone();
            handles.push(tokio::spawn(async move {
                let balance = query_evm_balance(&client, &cfg, &addr)
                    .await
                    .unwrap_or(ChainBalance {
                        chain_id: cfg.id.clone(),
                        chain_name: cfg.name.clone(),
                        native_symbol: cfg.native_symbol.clone(),
                        native_decimals: cfg.native_decimals,
                        native_balance: 0,
                        staked_balance: 0,
                        tokens: Vec::new(),
                        rpc_failed: true,
                    });
                (addr, balance)
            }));
        }
    }

    for address in &sol_addresses {
        for sol_config in &config.chains.solana {
            let client = client.clone();
            let cfg = sol_config.clone();
            let addr = address.clone();
            handles.push(tokio::spawn(async move {
                let balance = query_sol_balance(&client, &cfg, &addr)
                    .await
                    .unwrap_or(ChainBalance {
                        chain_id: cfg.id.clone(),
                        chain_name: cfg.name.clone(),
                        native_symbol: cfg.native_symbol.clone(),
                        native_decimals: cfg.native_decimals,
                        native_balance: 0,
                        staked_balance: 0,
                        tokens: Vec::new(),
                        rpc_failed: true,
                    });
                (addr, balance)
            }));
        }
    }

    // 多签 vault 等限定链的地址，只查对应链
    for (address, chain_id) in &chain_sol_addresses {
        if let Some(sol_config) = config.chains.solana.iter().find(|c| c.id == *chain_id) {
            let client = client.clone();
            let cfg = sol_config.clone();
            let addr = address.clone();
            handles.push(tokio::spawn(async move {
                let balance = query_sol_balance(&client, &cfg, &addr)
                    .await
                    .unwrap_or(ChainBalance {
                        chain_id: cfg.id.clone(),
                        chain_name: cfg.name.clone(),
                        native_symbol: cfg.native_symbol.clone(),
                        native_decimals: cfg.native_decimals,
                        native_balance: 0,
                        staked_balance: 0,
                        tokens: Vec::new(),
                        rpc_failed: true,
                    });
                (addr, balance)
            }));
        }
    }

    // 等待所有 task 完成
    let results = join_all(handles).await;

    let mut cache: BalanceCache = HashMap::new();
    for (addr, balance) in results.into_iter().flatten() {
        cache
            .entry(addr.clone())
            .or_insert_with(|| AddressPortfolio {
                address: addr,
                chains: Vec::new(),
            })
            .chains
            .push(balance);
    }

    cache
}
