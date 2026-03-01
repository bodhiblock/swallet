use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::str::FromStr;

use super::{
    derive_multisig_pda, derive_program_config_pda,
    derive_proposal_pda, derive_transaction_pda, derive_vault_pda,
    MultisigInfo, MultisigMember, ProposalInfo, ProposalStatus,
};
use crate::transfer::sol_transfer::{self, AccountMeta, Instruction};

// ========== Anchor 判别码（私有辅助） ==========

fn anchor_instruction_discriminator(name: &str) -> [u8; 8] {
    let input = format!("global:{name}");
    let hash = Sha256::digest(input.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

fn anchor_account_discriminator(name: &str) -> [u8; 8] {
    let input = format!("account:{name}");
    let hash = Sha256::digest(input.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

// ========== 账户数据解析 ==========

/// 从 RPC 获取并解析 Multisig 账户
pub async fn fetch_multisig(
    client: &Client,
    rpc_url: &str,
    multisig_address: &str,
) -> Result<MultisigInfo, String> {
    let data = fetch_account_data(client, rpc_url, multisig_address).await?;
    let address = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;
    parse_multisig_account(&data, address)
}

/// 从 RPC 获取并解析 Proposal 账户
pub async fn fetch_proposal(
    client: &Client,
    rpc_url: &str,
    proposal_address: &str,
) -> Result<ProposalInfo, String> {
    let data = fetch_account_data(client, rpc_url, proposal_address).await?;
    let address = Pubkey::from_str(proposal_address)
        .map_err(|e| format!("无效的提案地址: {e}"))?;
    parse_proposal_account(&data, address)
}

/// 获取多签的活跃提案列表
pub async fn fetch_active_proposals(
    client: &Client,
    rpc_url: &str,
    multisig: &MultisigInfo,
) -> Result<Vec<ProposalInfo>, String> {
    let mut proposals = Vec::new();

    // 从最新的交易开始向前查找，最多查找 20 个
    let start = multisig.transaction_index;
    let end = if start > 20 { start - 20 } else { 1 };

    for idx in (end..=start).rev() {
        let (proposal_pda, _) = derive_proposal_pda(&multisig.address, idx);
        let proposal_addr = proposal_pda.to_string();

        match fetch_proposal(client, rpc_url, &proposal_addr).await {
            Ok(proposal) => {
                proposals.push(proposal);
            }
            Err(_) => {
                // 提案不存在或解析失败，跳过
                continue;
            }
        }
    }

    Ok(proposals)
}

/// 解析 Multisig 账户数据
fn parse_multisig_account(data: &[u8], address: Pubkey) -> Result<MultisigInfo, String> {
    let expected_disc = anchor_account_discriminator("Multisig");
    if data.len() < 8 {
        return Err("数据太短".into());
    }
    if data[..8] != expected_disc {
        return Err("账户判别码不匹配，不是 Squads Multisig 账户".into());
    }

    let mut offset = 8;

    let create_key = read_pubkey(data, &mut offset)?;
    let config_authority = read_pubkey(data, &mut offset)?;
    let threshold = read_u16(data, &mut offset)?;
    let time_lock = read_u32(data, &mut offset)?;
    let transaction_index = read_u64(data, &mut offset)?;
    let stale_transaction_index = read_u64(data, &mut offset)?;

    // rent_collector: Option<Pubkey>
    let rent_collector = read_option_pubkey(data, &mut offset)?;

    let bump = read_u8(data, &mut offset)?;

    // members: Vec<Member>
    let members_len = read_u32(data, &mut offset)? as usize;
    let mut members = Vec::with_capacity(members_len);
    for _ in 0..members_len {
        let key = read_pubkey(data, &mut offset)?;
        let permissions = read_u8(data, &mut offset)?;
        members.push(MultisigMember { key, permissions });
    }

    Ok(MultisigInfo {
        address,
        create_key,
        config_authority,
        threshold,
        time_lock,
        transaction_index,
        stale_transaction_index,
        rent_collector,
        bump,
        members,
    })
}

/// 解析 Proposal 账户数据
fn parse_proposal_account(data: &[u8], address: Pubkey) -> Result<ProposalInfo, String> {
    let expected_disc = anchor_account_discriminator("Proposal");
    if data.len() < 8 {
        return Err("数据太短".into());
    }
    if data[..8] != expected_disc {
        return Err("账户判别码不匹配，不是提案账户".into());
    }

    let mut offset = 8;

    let multisig = read_pubkey(data, &mut offset)?;
    let transaction_index = read_u64(data, &mut offset)?;

    // status: ProposalStatus (Borsh enum)
    let status_tag = read_u8(data, &mut offset)?;
    let status = match status_tag {
        0 => ProposalStatus::Draft,
        1 => {
            // Active { timestamp: i64 }
            let _timestamp = read_i64(data, &mut offset)?;
            ProposalStatus::Active
        }
        2 => {
            let _timestamp = read_i64(data, &mut offset)?;
            ProposalStatus::Rejected
        }
        3 => {
            let _timestamp = read_i64(data, &mut offset)?;
            ProposalStatus::Approved
        }
        4 => ProposalStatus::Executing,
        5 => {
            let _timestamp = read_i64(data, &mut offset)?;
            ProposalStatus::Executed
        }
        6 => {
            let _timestamp = read_i64(data, &mut offset)?;
            ProposalStatus::Cancelled
        }
        _ => return Err(format!("未知的提案状态: {status_tag}")),
    };

    // approved: Vec<Pubkey>
    let approved = read_pubkey_vec(data, &mut offset)?;
    // rejected: Vec<Pubkey>
    let rejected = read_pubkey_vec(data, &mut offset)?;
    // cancelled: Vec<Pubkey>
    let cancelled = read_pubkey_vec(data, &mut offset)?;

    let bump = read_u8(data, &mut offset)?;

    Ok(ProposalInfo {
        address,
        multisig,
        transaction_index,
        status,
        approved,
        rejected,
        cancelled,
        bump,
    })
}

// ========== 交易构建 ==========

/// 创建 VaultTransaction + Proposal + Approve（一笔 Solana 交易包含 3 个指令）
pub async fn create_proposal_and_approve(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    vault_index: u8,
    inner_instructions: Vec<proposals::VaultInstruction>,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let creator_pubkey = keypair.pubkey();

    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    // 获取当前 multisig 的 transaction_index
    let multisig_info = fetch_multisig(client, rpc_url, multisig_address).await?;
    let new_tx_index = multisig_info.transaction_index + 1;

    let program_id = crate::squads_multisig_program::ID;

    // 推导 PDA
    let (transaction_pda, _) = derive_transaction_pda(&multisig_pubkey, new_tx_index);
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, new_tx_index);

    let rent_sysvar = Pubkey::from_str("SysvarRent111111111111111111111111111111111").unwrap();
    let system_program = Pubkey::default(); // 11111111111111111111111111111111

    // ===== 指令 1: vault_transaction_create =====
    let vault_tx_create_disc = anchor_instruction_discriminator("vault_transaction_create");

    // 序列化内部交易消息
    let vault_message_bytes = proposals::serialize_vault_transaction_message(
        vault_index,
        &inner_instructions,
    );

    let mut vault_tx_data = Vec::new();
    vault_tx_data.extend_from_slice(&vault_tx_create_disc);
    vault_tx_data.push(vault_index); // vault_index: u8
    // ephemeral_signers: u8 = 0
    vault_tx_data.push(0);
    // transaction_message: Vec<u8> (Borsh: 4-byte length + data)
    vault_tx_data.extend_from_slice(&(vault_message_bytes.len() as u32).to_le_bytes());
    vault_tx_data.extend_from_slice(&vault_message_bytes);
    // memo: Option<String> = None
    vault_tx_data.push(0);

    let vault_tx_create_ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: multisig_pubkey.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: transaction_pda.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: creator_pubkey.to_bytes(), is_signer: true, is_writable: true },
            AccountMeta { pubkey: rent_sysvar.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: system_program.to_bytes(), is_signer: false, is_writable: false },
        ],
        data: vault_tx_data,
    };

    // ===== 指令 2: proposal_create =====
    let proposal_create_disc = anchor_instruction_discriminator("proposal_create");

    let mut proposal_data = Vec::new();
    proposal_data.extend_from_slice(&proposal_create_disc);
    // transaction_index: u64
    proposal_data.extend_from_slice(&new_tx_index.to_le_bytes());
    // draft: bool = false (直接进入 Active)
    proposal_data.push(0);

    let proposal_create_ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: multisig_pubkey.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: creator_pubkey.to_bytes(), is_signer: true, is_writable: true },
            AccountMeta { pubkey: rent_sysvar.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: system_program.to_bytes(), is_signer: false, is_writable: false },
        ],
        data: proposal_data,
    };

    // ===== 指令 3: proposal_approve =====
    let proposal_approve_disc = anchor_instruction_discriminator("proposal_approve");

    let mut approve_data = Vec::new();
    approve_data.extend_from_slice(&proposal_approve_disc);
    // memo: Option<String> = None
    approve_data.push(0);

    let proposal_approve_ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: multisig_pubkey.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: creator_pubkey.to_bytes(), is_signer: true, is_writable: false },
            AccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
        ],
        data: approve_data,
    };

    // 构建并签名 Solana 交易
    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;

    let message_bytes = sol_transfer::build_and_serialize_message(
        &creator_pubkey.to_bytes(),
        &recent_blockhash,
        &[vault_tx_create_ix, proposal_create_ix, proposal_approve_ix],
    );

    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = sol_transfer::build_transaction(&[sig_bytes], &message_bytes);

    sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await
}

/// 审批提案
pub async fn approve_proposal(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
) -> Result<String, String> {
    send_vote_instruction(client, rpc_url, private_key, multisig_address, transaction_index, "proposal_approve").await
}

/// 拒绝提案
pub async fn reject_proposal(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
) -> Result<String, String> {
    send_vote_instruction(client, rpc_url, private_key, multisig_address, transaction_index, "proposal_reject").await
}

/// 发送投票指令（approve 或 reject）
async fn send_vote_instruction(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
    instruction_name: &str,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let member_pubkey = keypair.pubkey();

    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    let program_id = crate::squads_multisig_program::ID;
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, transaction_index);

    let disc = anchor_instruction_discriminator(instruction_name);
    let mut data = Vec::new();
    data.extend_from_slice(&disc);
    // memo: Option<String> = None
    data.push(0);

    let ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: multisig_pubkey.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: member_pubkey.to_bytes(), is_signer: true, is_writable: false },
            AccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
        ],
        data,
    };

    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = sol_transfer::build_and_serialize_message(&member_pubkey.to_bytes(), &recent_blockhash, &[ix]);
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = sol_transfer::build_transaction(&[sig_bytes], &message_bytes);

    sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await
}

/// 执行已通过的 Vault Transaction
pub async fn execute_vault_transaction(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
    vault_index: u8,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let executor_pubkey = keypair.pubkey();

    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    let program_id = crate::squads_multisig_program::ID;
    let (transaction_pda, _) = derive_transaction_pda(&multisig_pubkey, transaction_index);
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, transaction_index);
    let (vault_pda, _) = derive_vault_pda(&multisig_pubkey, vault_index);

    let disc = anchor_instruction_discriminator("vault_transaction_execute");
    let data = disc.to_vec();

    let ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: multisig_pubkey.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: executor_pubkey.to_bytes(), is_signer: true, is_writable: true },
            AccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: transaction_pda.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: vault_pda.to_bytes(), is_signer: false, is_writable: true },
        ],
        data,
    };

    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = sol_transfer::build_and_serialize_message(&executor_pubkey.to_bytes(), &recent_blockhash, &[ix]);
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = sol_transfer::build_transaction(&[sig_bytes], &message_bytes);

    sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await
}

/// 创建 Squads v4 多签 (MultisigCreateV2)
///
/// 返回多签 PDA 地址字符串
pub async fn create_multisig_v2(
    client: &Client,
    rpc_url: &str,
    creator_private_key: &[u8],
    member_pubkeys: &[Pubkey],
    threshold: u16,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = creator_private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let creator_keypair = Keypair::new_from_array(key_bytes);
    let creator_pubkey = creator_keypair.pubkey();

    // 生成随机 create_key（防前抢交易）
    let create_key_keypair = Keypair::new();
    let create_key_pubkey = create_key_keypair.pubkey();

    // 推导 PDA
    let (multisig_pda, _) = derive_multisig_pda(&create_key_pubkey);
    let (program_config_pda, _) = derive_program_config_pda();

    // 获取 ProgramConfig 以得到 treasury 地址
    let config_data = fetch_account_data(client, rpc_url, &program_config_pda.to_string()).await?;
    let treasury = parse_program_config_treasury(&config_data)?;

    let program_id = crate::squads_multisig_program::ID;
    let system_program = Pubkey::default();

    // 构建成员列表（所有成员都有 Initiate+Vote+Execute 权限 = 7）
    let all_permissions: u8 = 7; // 1(Initiate) | 2(Vote) | 4(Execute)

    // 序列化 MultisigCreateArgsV2 指令数据
    let disc = anchor_instruction_discriminator("multisig_create_v2");
    let mut data = Vec::new();
    data.extend_from_slice(&disc);
    // configAuthority: Option<Pubkey> = None（自治多签）
    data.push(0);
    // threshold: u16
    data.extend_from_slice(&threshold.to_le_bytes());
    // members: Vec<Member> (Borsh: 4-byte len + N * (32 + 1))
    data.extend_from_slice(&(member_pubkeys.len() as u32).to_le_bytes());
    for member_key in member_pubkeys {
        data.extend_from_slice(member_key.as_ref());
        data.push(all_permissions); // permissions.mask
    }
    // timeLock: u32 = 0
    data.extend_from_slice(&0u32.to_le_bytes());
    // rentCollector: Option<Pubkey> = None
    data.push(0);
    // memo: Option<String> = None
    data.push(0);

    let ix = Instruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            AccountMeta { pubkey: program_config_pda.to_bytes(), is_signer: false, is_writable: false },
            AccountMeta { pubkey: treasury.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: true },
            AccountMeta { pubkey: create_key_pubkey.to_bytes(), is_signer: true, is_writable: false },
            AccountMeta { pubkey: creator_pubkey.to_bytes(), is_signer: true, is_writable: true },
            AccountMeta { pubkey: system_program.to_bytes(), is_signer: false, is_writable: false },
        ],
        data,
    };

    // 构建交易（双签名：creator + create_key）
    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;

    let message_bytes = sol_transfer::build_and_serialize_message(
        &creator_pubkey.to_bytes(),
        &recent_blockhash,
        &[ix],
    );

    // creator 签名（payer，排在第一位）
    let creator_sig = creator_keypair.sign_message(&message_bytes);
    let mut creator_sig_bytes = [0u8; 64];
    creator_sig_bytes.copy_from_slice(creator_sig.as_ref());

    // create_key 签名
    let create_key_sig = create_key_keypair.sign_message(&message_bytes);
    let mut create_key_sig_bytes = [0u8; 64];
    create_key_sig_bytes.copy_from_slice(create_key_sig.as_ref());

    let tx_bytes = sol_transfer::build_transaction(
        &[creator_sig_bytes, create_key_sig_bytes],
        &message_bytes,
    );

    let tx_sig = sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await?;

    // 返回多签 PDA 地址
    Ok(format!("{}|{}", multisig_pda, tx_sig))
}

/// 从 ProgramConfig 账户数据中解析 treasury 地址
fn parse_program_config_treasury(data: &[u8]) -> Result<Pubkey, String> {
    let expected_disc = anchor_account_discriminator("ProgramConfig");
    if data.len() < 8 {
        return Err("ProgramConfig 数据太短".into());
    }
    if data[..8] != expected_disc {
        return Err("不是 ProgramConfig 账户".into());
    }

    let mut offset = 8;
    // authority: Pubkey (32 bytes)
    let _authority = read_pubkey(data, &mut offset)?;
    // multisig_creation_fee: u64 (8 bytes)
    let _fee = read_u64(data, &mut offset)?;
    // treasury: Pubkey (32 bytes)
    let treasury = read_pubkey(data, &mut offset)?;

    Ok(treasury)
}

// ========== RPC 辅助 ==========

/// 获取账户数据（base64 解码）
async fn fetch_account_data(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<Vec<u8>, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64"}],
        "id": 1
    });

    let resp = sol_transfer::rpc_call(client, rpc_url, &body).await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .ok_or("账户不存在")?;

    if value.is_null() {
        return Err("账户不存在".into());
    }

    let data_arr = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("缺少 data 字段")?;

    let base64_str = data_arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or("无效的数据格式")?;

    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(base64_str)
        .map_err(|e| format!("Base64 解码失败: {e}"))
}

// ========== 数据读取辅助 ==========

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, String> {
    if *offset + 1 > data.len() {
        return Err("数据不足".into());
    }
    let val = data[*offset];
    *offset += 1;
    Ok(val)
}

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, String> {
    if *offset + 2 > data.len() {
        return Err("数据不足".into());
    }
    let val = u16::from_le_bytes(data[*offset..*offset + 2].try_into().unwrap());
    *offset += 2;
    Ok(val)
}

fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, String> {
    if *offset + 4 > data.len() {
        return Err("数据不足".into());
    }
    let val = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    Ok(val)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, String> {
    if *offset + 8 > data.len() {
        return Err("数据不足".into());
    }
    let val = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(val)
}

fn read_i64(data: &[u8], offset: &mut usize) -> Result<i64, String> {
    if *offset + 8 > data.len() {
        return Err("数据不足".into());
    }
    let val = i64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(val)
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Result<Pubkey, String> {
    if *offset + 32 > data.len() {
        return Err("数据不足".into());
    }
    let bytes: [u8; 32] = data[*offset..*offset + 32].try_into().unwrap();
    *offset += 32;
    Ok(Pubkey::new_from_array(bytes))
}

fn read_option_pubkey(data: &[u8], offset: &mut usize) -> Result<Option<Pubkey>, String> {
    let tag = read_u8(data, offset)?;
    if tag == 0 {
        Ok(None)
    } else {
        let key = read_pubkey(data, offset)?;
        Ok(Some(key))
    }
}

fn read_pubkey_vec(data: &[u8], offset: &mut usize) -> Result<Vec<Pubkey>, String> {
    let len = read_u32(data, offset)? as usize;
    let mut keys = Vec::with_capacity(len);
    for _ in 0..len {
        keys.push(read_pubkey(data, offset)?);
    }
    Ok(keys)
}

use super::proposals;
