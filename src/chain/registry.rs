use std::collections::HashMap;

use reqwest::Client;

use crate::config::AppConfig;
use crate::storage::data::{ChainType, WalletStore, WalletType};

use super::ethereum::query_evm_balance;
use super::solana::query_sol_balance;
use super::{AddressPortfolio, BalanceCache, ChainBalance};

/// 查询所有钱包地址的余额
pub async fn fetch_all_balances(
    config: &AppConfig,
    store: &WalletStore,
) -> BalanceCache {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // 收集所有需要查询的地址
    let mut eth_addresses: Vec<String> = Vec::new();
    let mut sol_addresses: Vec<String> = Vec::new();

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
        }
    }

    let mut cache: BalanceCache = HashMap::new();

    // 查询 ETH 地址在所有 EVM 链上的余额
    for address in &eth_addresses {
        let mut portfolio = AddressPortfolio {
            address: address.clone(),
            chains: Vec::new(),
        };
        for evm_config in &config.chains.ethereum {
            let balance = query_evm_balance(&client, evm_config, address)
                .await
                .unwrap_or(ChainBalance {
                    chain_id: evm_config.id.clone(),
                    chain_name: evm_config.name.clone(),
                    native_symbol: evm_config.native_symbol.clone(),
                    native_decimals: evm_config.native_decimals,
                    native_balance: 0,
                    staked_balance: 0,
                    tokens: Vec::new(),
                    rpc_failed: true,
                });
            portfolio.chains.push(balance);
        }
        cache.insert(address.clone(), portfolio);
    }

    // 查询 SOL 地址在所有 Solana 链上的余额
    for address in &sol_addresses {
        let mut portfolio = AddressPortfolio {
            address: address.clone(),
            chains: Vec::new(),
        };
        for sol_config in &config.chains.solana {
            let balance = query_sol_balance(&client, sol_config, address)
                .await
                .unwrap_or(ChainBalance {
                    chain_id: sol_config.id.clone(),
                    chain_name: sol_config.name.clone(),
                    native_symbol: sol_config.native_symbol.clone(),
                    native_decimals: sol_config.native_decimals,
                    native_balance: 0,
                    staked_balance: 0,
                    tokens: Vec::new(),
                    rpc_failed: true,
                });
            portfolio.chains.push(balance);
        }
        cache.insert(address.clone(), portfolio);
    }

    cache
}
