use serde::Serialize;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct VoteAccountDto {
    pub address: String,
    pub validator_identity: String,
    pub authorized_voter: String,
    pub authorized_withdrawer: String,
    pub commission: u8,
    pub credits: Option<String>,
}

#[derive(Serialize)]
pub struct StakeAccountDto {
    pub address: String,
    pub state: String,
    pub delegated_vote_account: Option<String>,
    pub stake_lamports: String,
    pub authorized_staker: String,
    pub authorized_withdrawer: String,
    pub lockup_timestamp: i64,
}

#[tauri::command]
pub async fn fetch_vote_account(address: String, rpc_url: String) -> CommandResult<VoteAccountDto> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    let info = swallet_core::staking::sol_staking::fetch_vote_account(&client, &rpc_url, &address).await
        .map_err(|e| format!("获取 Vote 账户失败: {e}"))?;

    let credits = info.epoch_credits.last().map(|(epoch, credits, prev)| {
        format!("Epoch {} +{} (累计 {})", epoch, credits - prev, credits)
    });

    Ok(VoteAccountDto {
        address: info.address,
        validator_identity: info.validator_identity,
        authorized_voter: info.authorized_voter,
        authorized_withdrawer: info.authorized_withdrawer,
        commission: info.commission,
        credits,
    })
}

#[tauri::command]
pub async fn fetch_stake_account(address: String, rpc_url: String) -> CommandResult<StakeAccountDto> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    let info = swallet_core::staking::sol_staking::fetch_stake_account(&client, &rpc_url, &address).await
        .map_err(|e| format!("获取 Stake 账户失败: {e}"))?;

    Ok(StakeAccountDto {
        address: info.address,
        state: info.state,
        delegated_vote_account: info.delegated_vote_account,
        stake_lamports: swallet_core::chain::format_balance(info.stake_lamports as u128, 9),
        authorized_staker: info.authorized_staker,
        authorized_withdrawer: info.authorized_withdrawer,
        lockup_timestamp: info.lockup_timestamp,
    })
}

#[tauri::command]
pub async fn create_vote_account(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    identity: String,
    withdrawer: String,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::create_vote_account(&client, &rpc_url, &pk, &fp, &identity, &withdrawer)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn create_stake_account(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    amount: String,
    lockup_days: u64,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::create_stake_account(&client, &rpc_url, &pk, &fp, &amount, lockup_days)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn stake_delegate(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    vote_account: String,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::stake_delegate(&client, &rpc_url, &pk, &fp, &vote_account)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn stake_deactivate(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::stake_deactivate(&client, &rpc_url, &pk, &fp)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn stake_withdraw(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    to_address: String,
    amount: String,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::stake_withdraw(&client, &rpc_url, &pk, &fp, &to_address, &amount)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn vote_withdraw(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    rpc_url: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    to_address: String,
    amount: String,
    password: String,
) -> CommandResult<String> {
    let (pk, fp) = get_staking_keys(&state, wallet_index, account_index, fee_payer_wi, fee_payer_ai, &password)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    swallet_core::staking::sol_staking::vote_withdraw(&client, &rpc_url, &pk, &fp, &to_address, &amount)
        .await.map_err(|e| e.into())
}

fn get_staking_keys(
    state: &tauri::State<'_, AppState>,
    wallet_index: usize,
    account_index: usize,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
    password: &str,
) -> CommandResult<(Vec<u8>, Vec<u8>)> {
    let service = state.service.lock().unwrap();
    if !service.verify_password(password.as_bytes()) { return Err("密码错误".into()); }
    let pk = service.get_sol_private_key_by_index(wallet_index, account_index).ok_or("无法获取私钥")?;
    let fp = service.get_sol_private_key_by_index(fee_payer_wi, fee_payer_ai).ok_or("无法获取 Fee Payer 私钥")?;
    Ok((pk, fp))
}
