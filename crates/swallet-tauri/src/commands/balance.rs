use serde::Serialize;
use swallet_core::chain;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct BalanceDto {
    pub address: String,
    pub account_owner: Option<String>,
    pub account_owner_chain_id: Option<String>,
    pub chains: Vec<ChainBalanceDto>,
}

#[derive(Serialize)]
pub struct ChainBalanceDto {
    pub chain_id: String,
    pub chain_name: String,
    pub native_symbol: String,
    pub native_balance: String,
    pub native_balance_raw: String,
    pub staked_balance: String,
    pub tokens: Vec<TokenBalanceDto>,
    pub rpc_failed: bool,
}

#[derive(Serialize)]
pub struct TokenBalanceDto {
    pub symbol: String,
    pub balance: String,
    pub balance_raw: String,
    pub decimals: u8,
}

fn cache_to_dtos(cache: &chain::BalanceCache) -> Vec<BalanceDto> {
    cache.iter().map(|(address, portfolio)| {
        BalanceDto {
            address: address.clone(),
            account_owner: portfolio.account_owner.clone(),
            account_owner_chain_id: portfolio.account_owner_chain_id.clone(),
            chains: portfolio.chains.iter().map(|cb| {
                ChainBalanceDto {
                    chain_id: cb.chain_id.clone(),
                    chain_name: cb.chain_name.clone(),
                    native_symbol: cb.native_symbol.clone(),
                    native_balance: chain::format_balance(cb.native_balance, cb.native_decimals),
                    native_balance_raw: cb.native_balance.to_string(),
                    staked_balance: chain::format_balance(cb.staked_balance, cb.native_decimals),
                    tokens: cb.tokens.iter().map(|t| {
                        TokenBalanceDto {
                            symbol: t.symbol.clone(),
                            balance: chain::format_balance(t.balance, t.decimals),
                            balance_raw: t.balance.to_string(),
                            decimals: t.decimals,
                        }
                    }).collect(),
                    rpc_failed: cb.rpc_failed,
                }
            }).collect(),
        }
    }).collect()
}

#[tauri::command]
pub fn get_rpc_url_for_address(state: tauri::State<'_, AppState>, address: String) -> String {
    let service = state.service.lock().unwrap();
    service.get_rpc_url_for_address(&address)
}

#[tauri::command]
pub fn get_cached_balances(state: tauri::State<'_, AppState>) -> CommandResult<Vec<BalanceDto>> {
    let service = state.service.lock().unwrap();
    Ok(cache_to_dtos(&service.balance_cache))
}

#[tauri::command]
pub async fn refresh_balances(state: tauri::State<'_, AppState>) -> CommandResult<Vec<BalanceDto>> {
    // 锁内取数据
    let (config, store) = {
        let service = state.service.lock().unwrap();
        let store = service.store.as_ref().ok_or("钱包未解锁")?.clone();
        (service.config.clone(), store)
    };

    // 锁外执行异步 RPC
    let cache = chain::registry::fetch_all_balances(&config, &store).await;

    // 锁内更新缓存
    {
        let mut service = state.service.lock().unwrap();
        service.balance_cache = cache.clone();
    }

    Ok(cache_to_dtos(&cache))
}
