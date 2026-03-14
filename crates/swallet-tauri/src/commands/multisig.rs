use serde::Serialize;
use swallet_core::multisig;

use crate::error::CommandResult;
use crate::state::AppState;

#[derive(Serialize)]
pub struct MultisigDetailDto {
    pub address: String,
    pub threshold: u16,
    pub members: Vec<MemberDto>,
    pub transaction_index: u64,
}

#[derive(Serialize)]
pub struct MemberDto {
    pub address: String,
}

#[derive(Serialize)]
pub struct ProposalDto {
    pub index: usize,
    pub transaction_index: u64,
    pub status: String,
    pub approved_count: usize,
    pub rejected_count: usize,
}

#[derive(Serialize)]
pub struct FeePayerDto {
    pub address: String,
    pub label: String,
    pub balance: String,
    pub wallet_index: usize,
    pub account_index: usize,
}

#[derive(Serialize)]
pub struct ChainDto {
    pub id: String,
    pub name: String,
    pub rpc_url: String,
}

#[tauri::command]
pub fn get_solana_chains(state: tauri::State<'_, AppState>) -> Vec<ChainDto> {
    let service = state.service.lock().unwrap();
    service.config.chains.solana.iter().map(|c| ChainDto {
        id: c.id.clone(), name: c.name.clone(), rpc_url: c.rpc_url.clone(),
    }).collect()
}

#[tauri::command]
pub fn get_fee_payers(state: tauri::State<'_, AppState>) -> Vec<FeePayerDto> {
    let service = state.service.lock().unwrap();
    service.build_fee_payer_list("").iter().map(|fp| FeePayerDto {
        address: fp.address.clone(),
        label: fp.label.clone(),
        balance: swallet_core::chain::format_balance(fp.balance_lamports, 9),
        wallet_index: fp.wallet_index,
        account_index: fp.account_index,
    }).collect()
}

#[tauri::command]
pub async fn import_multisig(
    state: tauri::State<'_, AppState>,
    chain_id: String,
    address: String,
) -> CommandResult<MultisigDetailDto> {
    let (rpc_url, chain_name) = {
        let service = state.service.lock().unwrap();
        let chain = service.config.chains.solana.iter().find(|c| c.id == chain_id)
            .ok_or("未找到链配置")?;
        (chain.rpc_url.clone(), chain.name.clone())
    };

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    let info = multisig::squads::fetch_multisig(&client, &rpc_url, &address).await
        .map_err(|e| format!("获取多签失败: {e}"))?;

    let dto = MultisigDetailDto {
        address: info.address.to_string(),
        threshold: info.threshold,
        members: info.members.iter().map(|m| MemberDto { address: m.address() }).collect(),
        transaction_index: info.transaction_index,
    };

    {
        let mut service = state.service.lock().unwrap();
        service.save_multisig_to_store(&info, &rpc_url, &chain_id, &chain_name);
        let _ = service.save_store();
    }

    Ok(dto)
}

#[tauri::command]
pub async fn fetch_proposals(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
) -> CommandResult<Vec<ProposalDto>> {
    let (rpc_url, ms_address) = {
        let service = state.service.lock().unwrap();
        let rpc = service.get_current_ms_rpc_url(wallet_index, 0);
        let store = service.store.as_ref().ok_or("钱包未解锁")?;
        let wallet = store.wallets.get(wallet_index).ok_or("无效的钱包")?;
        let addr = match &wallet.wallet_type {
            swallet_core::storage::data::WalletType::Multisig { multisig_address, .. } => multisig_address.clone(),
            _ => return Err("不是多签钱包".into()),
        };
        (rpc, addr)
    };

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    let info = multisig::squads::fetch_multisig(&client, &rpc_url, &ms_address).await
        .map_err(|e| format!("获取多签失败: {e}"))?;
    let proposals = multisig::squads::fetch_active_proposals(&client, &rpc_url, &info).await
        .map_err(|e| format!("获取提案失败: {e}"))?;

    Ok(proposals.iter().enumerate().map(|(i, p)| ProposalDto {
        index: i,
        transaction_index: p.transaction_index,
        status: p.status.label().to_string(),
        approved_count: p.approved.len(),
        rejected_count: p.rejected.len(),
    }).collect())
}

#[tauri::command]
pub async fn create_sol_transfer_proposal(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    vault_index: u8,
    to_address: String,
    amount: String,
    password: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<String> {
    let (private_key, fee_payer_key, rpc_url, ms_address) = {
        let service = state.service.lock().unwrap();
        if !service.verify_password(password.as_bytes()) { return Err("密码错误".into()); }
        let (ms_addr, member_addresses) = get_ms_info(&service, wallet_index)?;
        let mut pk = None;
        for addr in &member_addresses {
            if let Some(key) = service.get_sol_private_key(addr) { pk = Some(key); break; }
        }
        let pk = pk.ok_or("未找到签名私钥")?;
        let fp = service.get_sol_private_key_by_index(fee_payer_wi, fee_payer_ai).ok_or("无法获取 Fee Payer 私钥")?;
        let rpc = service.get_current_ms_rpc_url(wallet_index, 0);
        (pk, fp, rpc, ms_addr)
    };

    swallet_core::service::execute_create_proposal(
        &rpc_url, &private_key, &fee_payer_key, &ms_address,
        0, // SolTransfer index
        &to_address, &amount, "", "", 0, 0, &[], "", vault_index, None, "", "", "",
    ).await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn approve_proposal(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    tx_index: u64,
    password: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<String> {
    let (private_key, fee_payer_key, rpc_url, ms_address) = get_vote_params(&state, wallet_index, &password, fee_payer_wi, fee_payer_ai)?;

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    multisig::squads::approve_proposal(&client, &rpc_url, &private_key, &fee_payer_key, &ms_address, tx_index)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn reject_proposal(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    tx_index: u64,
    password: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<String> {
    let (private_key, fee_payer_key, rpc_url, ms_address) = get_vote_params(&state, wallet_index, &password, fee_payer_wi, fee_payer_ai)?;

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    multisig::squads::reject_proposal(&client, &rpc_url, &private_key, &fee_payer_key, &ms_address, tx_index)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn execute_proposal(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    tx_index: u64,
    vault_index: u8,
    password: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<String> {
    let (private_key, fee_payer_key, rpc_url, ms_address) = get_vote_params(&state, wallet_index, &password, fee_payer_wi, fee_payer_ai)?;

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
    multisig::squads::execute_vault_transaction(&client, &rpc_url, &private_key, &fee_payer_key, &ms_address, tx_index, vault_index)
        .await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn create_proposal(
    state: tauri::State<'_, AppState>,
    wallet_index: usize,
    vault_index: u8,
    proposal_type_idx: usize,
    to_address: String,
    amount: String,
    upgrade_program: String,
    upgrade_buffer: String,
    vs_op_idx: usize,
    vs_target: String,
    vs_param: String,
    vs_amount: String,
    chain_id: String,
    password: String,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<String> {
    let (private_key, fee_payer_key, rpc_url, ms_address) = {
        let service = state.service.lock().unwrap();
        if !service.verify_password(password.as_bytes()) { return Err("密码错误".into()); }
        let (ms_addr, member_addresses) = get_ms_info(&service, wallet_index)?;
        let mut pk = None;
        for addr in &member_addresses {
            if let Some(key) = service.get_sol_private_key(addr) { pk = Some(key); break; }
        }
        let pk = pk.ok_or("未找到签名私钥")?;
        let fp = service.get_sol_private_key_by_index(fee_payer_wi, fee_payer_ai).ok_or("无法获取 Fee Payer 私钥")?;
        let rpc = service.get_current_ms_rpc_url(wallet_index, 0);
        (pk, fp, rpc, ms_addr)
    };

    // Map vs_op_idx to MsVoteStakeOp
    let vs_ops = [
        multisig::MsVoteStakeOp::VoteAuthorizeVoter,
        multisig::MsVoteStakeOp::VoteAuthorizeWithdrawer,
        multisig::MsVoteStakeOp::VoteWithdraw,
        multisig::MsVoteStakeOp::StakeAuthorizeStaker,
        multisig::MsVoteStakeOp::StakeAuthorizeWithdrawer,
        multisig::MsVoteStakeOp::StakeDelegate,
        multisig::MsVoteStakeOp::StakeDeactivate,
        multisig::MsVoteStakeOp::StakeWithdraw,
    ];
    let vs_op = vs_ops.get(vs_op_idx);

    swallet_core::service::execute_create_proposal(
        &rpc_url, &private_key, &fee_payer_key, &ms_address,
        proposal_type_idx, &to_address, &amount, &upgrade_program, &upgrade_buffer,
        0, 0, &[], &chain_id, vault_index, vs_op, &vs_target, &vs_param, &vs_amount,
    ).await.map_err(|e| e.into())
}

#[tauri::command]
pub async fn create_multisig(
    state: tauri::State<'_, AppState>,
    chain_id: String,
    creator_address: String,
    members: Vec<String>,
    threshold: u16,
    password: String,
    seed: Option<String>,
) -> CommandResult<String> {
    let (private_key, rpc_url, chain_name) = {
        let service = state.service.lock().unwrap();
        if !service.verify_password(password.as_bytes()) { return Err("密码错误".into()); }
        let pk = service.get_sol_private_key(&creator_address).ok_or("无法获取创建者私钥")?;
        let chain = service.config.chains.solana.iter().find(|c| c.id == chain_id).ok_or("未找到链配置")?;
        (pk, chain.rpc_url.clone(), chain.name.clone())
    };

    let member_pubkeys: Vec<solana_sdk::pubkey::Pubkey> = members.iter()
        .map(|a| a.parse().map_err(|e| format!("无效的成员地址 {a}: {e}")))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| crate::error::CommandError { message: e })?;

    let seed_key = match seed {
        Some(s) if !s.is_empty() => {
            let bytes = bs58::decode(&s).into_vec().map_err(|_| "种子私钥无效")?;
            if bytes.len() != 32 { return Err("种子私钥长度必须为32字节".into()); }
            Some(bytes)
        }
        _ => None,
    };

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;

    let result = multisig::squads::create_multisig_v2(
        &client, &rpc_url, &private_key, &member_pubkeys, threshold, seed_key.as_deref(),
    ).await.map_err(|e| crate::error::CommandError { message: e })?;

    // result format: "multisig_pda|tx_sig"
    let mut parts = result.splitn(2, '|');
    let ms_address = parts.next().unwrap_or(&result).to_string();
    let _tx_sig = parts.next().unwrap_or("").to_string();

    // Auto import
    {
        let client2 = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build()
            .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
        if let Ok(info) = multisig::squads::fetch_multisig(&client2, &rpc_url, &ms_address).await {
            let mut service = state.service.lock().unwrap();
            service.save_multisig_to_store(&info, &rpc_url, &chain_id, &chain_name);
        }
    }

    Ok(ms_address)
}

fn get_vote_params(
    state: &tauri::State<'_, AppState>,
    wallet_index: usize,
    password: &str,
    fee_payer_wi: usize,
    fee_payer_ai: usize,
) -> CommandResult<(Vec<u8>, Vec<u8>, String, String)> {
    let service = state.service.lock().unwrap();
    if !service.verify_password(password.as_bytes()) { return Err("密码错误".into()); }

    let (ms_addr, member_addresses) = get_ms_info(&service, wallet_index)?;

    // 遍历成员地址找到本地的签名私钥
    let mut pk = None;
    for addr in &member_addresses {
        if let Some(key) = service.get_sol_private_key(addr) {
            pk = Some(key);
            break;
        }
    }
    let pk = pk.ok_or("未找到匹配的签名私钥")?;
    let fp = service.get_sol_private_key_by_index(fee_payer_wi, fee_payer_ai).ok_or("无法获取 Fee Payer 私钥")?;
    let rpc = service.get_current_ms_rpc_url(wallet_index, 0);

    Ok((pk, fp, rpc, ms_addr))
}

fn get_ms_info(service: &swallet_core::service::WalletService, wallet_index: usize) -> CommandResult<(String, Vec<String>)> {
    let store = service.store.as_ref().ok_or("钱包未解锁")?;
    let wallet = store.wallets.get(wallet_index).ok_or("无效的钱包")?;
    match &wallet.wallet_type {
        swallet_core::storage::data::WalletType::Multisig { multisig_address, member_addresses, .. } => {
            Ok((multisig_address.clone(), member_addresses.clone()))
        }
        _ => Err("不是多签钱包".into()),
    }
}
