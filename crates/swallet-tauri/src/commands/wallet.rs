use serde::Serialize;
use swallet_core::storage::data::{WalletType, ChainType};

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct WalletDto {
    pub id: String,
    pub name: String,
    pub wallet_type: String,
    pub sort_order: u32,
    pub hidden: bool,
    pub accounts: Vec<AccountDto>,
}

#[derive(Serialize)]
pub struct AccountDto {
    pub address: String,
    pub label: Option<String>,
    pub chain_type: String,
    pub hidden: bool,
}

fn wallet_to_dto(w: &swallet_core::storage::data::Wallet) -> WalletDto {
    let (wallet_type, accounts) = match &w.wallet_type {
        WalletType::Mnemonic { eth_accounts, sol_accounts, .. } => {
            let mut accs = Vec::new();
            for a in eth_accounts {
                accs.push(AccountDto {
                    address: a.address.clone(),
                    label: a.label.clone(),
                    chain_type: "ethereum".to_string(),
                    hidden: a.hidden,
                });
            }
            for a in sol_accounts {
                accs.push(AccountDto {
                    address: a.address.clone(),
                    label: a.label.clone(),
                    chain_type: "solana".to_string(),
                    hidden: a.hidden,
                });
            }
            ("mnemonic".to_string(), accs)
        }
        WalletType::PrivateKey { chain_type, address, label, hidden, .. } => {
            let ct = match chain_type {
                ChainType::Ethereum => "ethereum",
                ChainType::Solana => "solana",
            };
            ("private_key".to_string(), vec![AccountDto {
                address: address.clone(),
                label: label.clone(),
                chain_type: ct.to_string(),
                hidden: *hidden,
            }])
        }
        WalletType::WatchOnly { chain_type, address, .. } => {
            let ct = match chain_type {
                ChainType::Ethereum => "ethereum",
                ChainType::Solana => "solana",
            };
            ("watch_only".to_string(), vec![AccountDto {
                address: address.clone(),
                label: None,
                chain_type: ct.to_string(),
                hidden: false,
            }])
        }
        WalletType::Multisig { vaults, .. } => {
            let accs = vaults.iter().map(|v| AccountDto {
                address: v.address.clone(),
                label: v.label.clone(),
                chain_type: "solana".to_string(),
                hidden: v.hidden,
            }).collect();
            ("multisig".to_string(), accs)
        }
    };

    WalletDto {
        id: w.id.clone(),
        name: w.name.clone(),
        wallet_type,
        sort_order: w.sort_order,
        hidden: w.hidden,
        accounts,
    }
}

#[tauri::command]
pub fn get_wallets(state: tauri::State<'_, AppState>) -> CommandResult<Vec<WalletDto>> {
    let service = state.service.lock().unwrap();
    let store = service.store.as_ref().ok_or("钱包未解锁")?;
    Ok(store.wallets.iter().map(wallet_to_dto).collect())
}

#[tauri::command]
pub fn generate_mnemonic() -> CommandResult<String> {
    swallet_core::service::generate_mnemonic()
        .map_err(|e| crate::error::CommandError { message: e.to_string() })
}
