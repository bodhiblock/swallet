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

#[tauri::command]
pub fn add_mnemonic_wallet(state: tauri::State<'_, AppState>, name: String, phrase: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.create_mnemonic_wallet(&name, &phrase).map_err(|e| e.into())
}

#[tauri::command]
pub fn add_private_key_wallet(state: tauri::State<'_, AppState>, name: String, private_key: String, chain_type: String) -> CommandResult<()> {
    let ct = parse_chain_type(&chain_type)?;
    let mut service = state.service.lock().unwrap();
    service.import_private_key_wallet(&name, &private_key, ct).map_err(|e| e.into())
}

#[tauri::command]
pub fn add_watch_wallet(state: tauri::State<'_, AppState>, name: String, address: String, chain_type: String) -> CommandResult<()> {
    let ct = parse_chain_type(&chain_type)?;
    let mut service = state.service.lock().unwrap();
    service.import_watch_wallet(&name, &address, ct).map_err(|e| e.into())
}

#[tauri::command]
pub fn add_derived_address(state: tauri::State<'_, AppState>, wallet_index: usize, chain_type: String) -> CommandResult<String> {
    let ct = parse_chain_type(&chain_type)?;
    let mut service = state.service.lock().unwrap();
    service.add_derived_address(wallet_index, ct).map_err(|e| e.into())
}

#[tauri::command]
pub fn edit_wallet_name(state: tauri::State<'_, AppState>, wallet_index: usize, name: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.edit_wallet_name(wallet_index, &name).map_err(|e| e.into())
}

#[tauri::command]
pub fn edit_address_label(state: tauri::State<'_, AppState>, wallet_index: usize, chain_type: String, account_index: usize, label: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.edit_address_label(wallet_index, &chain_type, account_index, &label).map_err(|e| e.into())
}

#[tauri::command]
pub fn move_wallet(state: tauri::State<'_, AppState>, wallet_index: usize, up: bool) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.move_wallet(wallet_index, up).map_err(|e| e.into())
}

#[tauri::command]
pub fn hide_wallet(state: tauri::State<'_, AppState>, wallet_index: usize) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.hide_wallet(wallet_index).map_err(|e| e.into())
}

#[tauri::command]
pub fn hide_address(state: tauri::State<'_, AppState>, wallet_index: usize, chain_type: String, account_index: usize) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    service.hide_address(wallet_index, &chain_type, account_index).map_err(|e| e.into())
}

#[tauri::command]
pub fn delete_wallet(state: tauri::State<'_, AppState>, wallet_index: usize, password: String) -> CommandResult<()> {
    let mut service = state.service.lock().unwrap();
    if !service.verify_password(password.as_bytes()) {
        return Err("密码错误".into());
    }
    service.delete_wallet(wallet_index).map_err(|e| e.into())
}

#[tauri::command]
pub fn restore_hidden_wallets(state: tauri::State<'_, AppState>) -> CommandResult<usize> {
    let mut service = state.service.lock().unwrap();
    service.restore_hidden_wallets().map_err(|e| e.into())
}

#[tauri::command]
pub fn restore_hidden_addresses(state: tauri::State<'_, AppState>) -> CommandResult<usize> {
    let mut service = state.service.lock().unwrap();
    service.restore_hidden_addresses().map_err(|e| e.into())
}

fn parse_chain_type(s: &str) -> CommandResult<ChainType> {
    match s {
        "ethereum" => Ok(ChainType::Ethereum),
        "solana" => Ok(ChainType::Solana),
        _ => Err(format!("无效的链类型: {s}").into()),
    }
}
