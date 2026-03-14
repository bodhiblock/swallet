use serde::Serialize;
use swallet_core::storage::data::ChainType;
use swallet_core::transfer;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct AssetDto {
    pub index: usize,
    pub chain_name: String,
    pub symbol: String,
    pub decimals: u8,
    pub balance: String,
    pub balance_raw: String,
    pub asset_type: String, // "native" | "erc20" | "spl"
}

#[tauri::command]
pub fn build_transfer_assets(
    state: tauri::State<'_, AppState>,
    address: String,
    chain_type: String,
) -> CommandResult<Vec<AssetDto>> {
    let service = state.service.lock().unwrap();
    let assets = match chain_type.as_str() {
        "ethereum" => transfer::build_eth_assets(&service.config, &address, &service.balance_cache),
        "solana" => transfer::build_sol_assets(&service.config, &address, &service.balance_cache),
        _ => return Err("无效的链类型".into()),
    };
    Ok(assets.iter().enumerate().map(|(i, a)| {
        let balance = a.balance.map(|b| swallet_core::chain::format_balance(b, a.decimals)).unwrap_or_else(|| "-".to_string());
        let balance_raw = a.balance.map(|b| b.to_string()).unwrap_or_else(|| "0".to_string());
        let asset_type = match &a.asset_kind {
            transfer::AssetKind::Native => "native",
            transfer::AssetKind::Erc20 { .. } => "erc20",
            transfer::AssetKind::SplToken { .. } => "spl",
        };
        AssetDto { index: i, chain_name: a.chain_name.clone(), symbol: a.symbol.clone(), decimals: a.decimals, balance, balance_raw, asset_type: asset_type.to_string() }
    }).collect())
}

#[tauri::command]
pub async fn execute_transfer(
    state: tauri::State<'_, AppState>,
    password: String,
    wallet_index: usize,
    account_index: usize,
    chain_type: String,
    asset_index: usize,
    to_address: String,
    amount: String,
) -> CommandResult<String> {
    // 锁内：验证密码、获取私钥、构建资产
    let (private_key, asset, amount_raw) = {
        let service = state.service.lock().unwrap();
        if !service.verify_password(password.as_bytes()) {
            return Err("密码错误".into());
        }
        let ct = match chain_type.as_str() {
            "ethereum" => ChainType::Ethereum,
            "solana" => ChainType::Solana,
            _ => return Err("无效的链类型".into()),
        };
        let pk = service.get_transfer_private_key(wallet_index, Some(account_index), &ct)
            .ok_or("无法获取私钥")?;

        // 构建资产列表并选择
        let address = match ct {
            ChainType::Ethereum => {
                let store = service.store.as_ref().ok_or("钱包未解锁")?;
                let wallet = store.wallets.get(wallet_index).ok_or("无效的钱包")?;
                match &wallet.wallet_type {
                    swallet_core::storage::data::WalletType::Mnemonic { eth_accounts, .. } => {
                        eth_accounts.get(account_index).map(|a| a.address.clone()).ok_or("无效的地址")?
                    }
                    swallet_core::storage::data::WalletType::PrivateKey { address, .. } => address.clone(),
                    _ => return Err("不支持的钱包类型".into()),
                }
            }
            ChainType::Solana => {
                service.get_sol_address(wallet_index, account_index).ok_or("无效的地址")?
            }
        };
        let assets = match ct {
            ChainType::Ethereum => transfer::build_eth_assets(&service.config, &address, &service.balance_cache),
            ChainType::Solana => transfer::build_sol_assets(&service.config, &address, &service.balance_cache),
        };
        let asset = assets.into_iter().nth(asset_index).ok_or("无效的资产索引")?;
        let raw = transfer::parse_amount(&amount, asset.decimals)?;
        (pk, asset, raw)
    };

    // 锁外：执行转账
    swallet_core::service::execute_transfer(private_key, asset, to_address, amount_raw)
        .await
        .map_err(|e| e.into())
}
