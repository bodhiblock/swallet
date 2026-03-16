use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use reqwest::Client;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::str::FromStr;

use super::{
    derive_multisig_pda, derive_program_config_pda,
    derive_proposal_pda, derive_transaction_pda, derive_vault_pda,
    MultisigInfo, MultisigMember, ProposalInfo, ProposalStatus,
};
use crate::squads_multisig_program::{accounts as sdk_accounts, client, types};
use crate::transfer::sol_transfer::{self, AccountMeta, Instruction};

// ========== SDK → 本地指令转换 ==========

/// 用 SDK 的 accounts + args 构建本地 Instruction
fn sdk_instruction(
    accounts: impl ToAccountMetas,
    data: impl InstructionData,
) -> Instruction {
    let program_id = crate::squads_multisig_program::ID;
    let metas = accounts.to_account_metas(None);
    Instruction {
        program_id: program_id.to_bytes(),
        accounts: metas
            .into_iter()
            .map(|m| AccountMeta {
                pubkey: m.pubkey.to_bytes(),
                is_signer: m.is_signer,
                is_writable: m.is_writable,
            })
            .collect(),
        data: data.data(),
    }
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
            Ok(mut proposal) => {
                // 尝试获取 VaultTransaction 解析摘要
                let (tx_pda, _) = derive_transaction_pda(&multisig.address, idx);
                if let Ok(tx_data) = fetch_account_data(client, rpc_url, &tx_pda.to_string()).await {
                    proposal.summary = decode_vault_tx_summary(&tx_data).ok();
                }
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

/// 解析 Multisig 账户数据（使用 SDK 反序列化）
fn parse_multisig_account(data: &[u8], address: Pubkey) -> Result<MultisigInfo, String> {
    let ms = sdk_accounts::Multisig::try_deserialize(&mut &data[..])
        .map_err(|e| format!("Multisig 反序列化失败: {e}"))?;

    let members = ms
        .members
        .iter()
        .map(|m| MultisigMember {
            key: m.key,
            permissions: m.permissions.mask,
        })
        .collect();

    Ok(MultisigInfo {
        address,
        create_key: ms.create_key,
        config_authority: ms.config_authority,
        threshold: ms.threshold,
        time_lock: ms.time_lock,
        transaction_index: ms.transaction_index,
        stale_transaction_index: ms.stale_transaction_index,
        rent_collector: ms.rent_collector,
        bump: ms.bump,
        members,
    })
}

/// 解析 Proposal 账户数据（使用 SDK 反序列化）
fn parse_proposal_account(data: &[u8], address: Pubkey) -> Result<ProposalInfo, String> {
    let p = sdk_accounts::Proposal::try_deserialize(&mut &data[..])
        .map_err(|e| format!("Proposal 反序列化失败: {e}"))?;

    let status = match p.status {
        types::ProposalStatus::Draft { .. } => ProposalStatus::Draft,
        types::ProposalStatus::Active { .. } => ProposalStatus::Active,
        types::ProposalStatus::Rejected { .. } => ProposalStatus::Rejected,
        types::ProposalStatus::Approved { .. } => ProposalStatus::Approved,
        types::ProposalStatus::Executing => ProposalStatus::Executing,
        types::ProposalStatus::Executed { .. } => ProposalStatus::Executed,
        types::ProposalStatus::Cancelled { .. } => ProposalStatus::Cancelled,
    };

    Ok(ProposalInfo {
        address,
        multisig: p.multisig,
        transaction_index: p.transaction_index,
        status,
        approved: p.approved,
        rejected: p.rejected,
        cancelled: p.cancelled,
        bump: p.bump,
        summary: None,
    })
}

// ========== 交易构建 ==========

/// 创建 VaultTransaction + Proposal + Approve（一笔 Solana 交易包含 3 个指令）
pub async fn create_proposal_and_approve(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    fee_payer_key: &[u8],
    multisig_address: &str,
    vault_index: u8,
    inner_instructions: Vec<proposals::VaultInstruction>,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let creator_pubkey = keypair.pubkey();
    let fp_bytes: [u8; 32] = fee_payer_key
        .try_into()
        .map_err(|_| "Fee Payer 私钥长度必须为 32 字节".to_string())?;
    let fee_payer = Keypair::new_from_array(fp_bytes);

    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    // 获取当前 multisig 的 transaction_index
    let multisig_info = fetch_multisig(client, rpc_url, multisig_address).await?;
    let new_tx_index = multisig_info.transaction_index + 1;

    // 推导 PDA
    let (transaction_pda, _) = derive_transaction_pda(&multisig_pubkey, new_tx_index);
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, new_tx_index);

    let system_program = Pubkey::default();

    // 序列化内部交易消息
    let vault_message_bytes = proposals::serialize_vault_transaction_message(
        vault_index,
        &inner_instructions,
    );

    // 指令 1: vault_transaction_create
    let vault_tx_create_ix = sdk_instruction(
        client::accounts::VaultTransactionCreate {
            multisig: multisig_pubkey,
            transaction: transaction_pda,
            creator: creator_pubkey,
            rent_payer: creator_pubkey,
            system_program,
        },
        client::args::VaultTransactionCreate {
            args: types::VaultTransactionCreateArgs {
                vault_index,
                ephemeral_signers: 0,
                transaction_message: vault_message_bytes,
                memo: None,
            },
        },
    );

    // 指令 2: proposal_create
    let proposal_create_ix = sdk_instruction(
        client::accounts::ProposalCreate {
            multisig: multisig_pubkey,
            proposal: proposal_pda,
            creator: creator_pubkey,
            rent_payer: creator_pubkey,
            system_program,
        },
        client::args::ProposalCreate {
            args: types::ProposalCreateArgs {
                transaction_index: new_tx_index,
                draft: false,
            },
        },
    );

    // 指令 3: proposal_approve
    let proposal_approve_ix = sdk_instruction(
        client::accounts::ProposalApprove {
            multisig: multisig_pubkey,
            member: creator_pubkey,
            proposal: proposal_pda,
        },
        client::args::ProposalApprove {
            args: types::ProposalVoteArgs { memo: None },
        },
    );

    // 构建并签名 Solana 交易
    let instructions = [vault_tx_create_ix, proposal_create_ix, proposal_approve_ix];
    sign_and_send_with_fee_payer(client, rpc_url, &keypair, &fee_payer, &instructions).await
}

/// 审批提案
pub async fn approve_proposal(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    fee_payer_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
) -> Result<String, String> {
    let (keypair, multisig_pubkey, proposal_pda) =
        prepare_vote(private_key, multisig_address, transaction_index)?;
    let fee_payer = prepare_fee_payer(fee_payer_key)?;

    let ix = sdk_instruction(
        client::accounts::ProposalApprove {
            multisig: multisig_pubkey,
            member: keypair.pubkey(),
            proposal: proposal_pda,
        },
        client::args::ProposalApprove {
            args: types::ProposalVoteArgs { memo: None },
        },
    );

    sign_and_send_with_fee_payer(client, rpc_url, &keypair, &fee_payer, &[ix]).await
}

/// 拒绝提案
pub async fn reject_proposal(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    fee_payer_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
) -> Result<String, String> {
    let (keypair, multisig_pubkey, proposal_pda) =
        prepare_vote(private_key, multisig_address, transaction_index)?;
    let fee_payer = prepare_fee_payer(fee_payer_key)?;

    let ix = sdk_instruction(
        client::accounts::ProposalReject {
            multisig: multisig_pubkey,
            member: keypair.pubkey(),
            proposal: proposal_pda,
        },
        client::args::ProposalReject {
            args: types::ProposalVoteArgs { memo: None },
        },
    );

    sign_and_send_with_fee_payer(client, rpc_url, &keypair, &fee_payer, &[ix]).await
}

/// 准备投票所需的公共参数
fn prepare_vote(
    private_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
) -> Result<(Keypair, Pubkey, Pubkey), String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, transaction_index);
    Ok((keypair, multisig_pubkey, proposal_pda))
}

fn prepare_fee_payer(fee_payer_key: &[u8]) -> Result<Keypair, String> {
    let fp_bytes: [u8; 32] = fee_payer_key
        .try_into()
        .map_err(|_| "Fee Payer 私钥长度必须为 32 字节".to_string())?;
    Ok(Keypair::new_from_array(fp_bytes))
}

/// 签名并发送单签名者交易
async fn sign_and_send(
    client: &Client,
    rpc_url: &str,
    keypair: &Keypair,
    instructions: &[Instruction],
) -> Result<String, String> {
    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = sol_transfer::build_and_serialize_message(
        &keypair.pubkey().to_bytes(),
        &recent_blockhash,
        instructions,
    );
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = sol_transfer::build_transaction(&[sig_bytes], &message_bytes);
    sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await
}

async fn sign_and_send_with_fee_payer(
    client: &Client,
    rpc_url: &str,
    signer: &Keypair,
    fee_payer: &Keypair,
    instructions: &[Instruction],
) -> Result<String, String> {
    if signer.pubkey() == fee_payer.pubkey() {
        return sign_and_send(client, rpc_url, signer, instructions).await;
    }
    let recent_blockhash = sol_transfer::get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = sol_transfer::build_and_serialize_message(
        &fee_payer.pubkey().to_bytes(),
        &recent_blockhash,
        instructions,
    );
    let sig1 = fee_payer.sign_message(&message_bytes);
    let sig2 = signer.sign_message(&message_bytes);
    let mut sig1_bytes = [0u8; 64];
    let mut sig2_bytes = [0u8; 64];
    sig1_bytes.copy_from_slice(sig1.as_ref());
    sig2_bytes.copy_from_slice(sig2.as_ref());
    let tx_bytes = sol_transfer::build_transaction(&[sig1_bytes, sig2_bytes], &message_bytes);
    sol_transfer::send_transaction(client, rpc_url, &tx_bytes).await
}

/// 执行已通过的 Vault Transaction
pub async fn execute_vault_transaction(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    fee_payer_key: &[u8],
    multisig_address: &str,
    transaction_index: u64,
    vault_index: u8,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let executor_pubkey = keypair.pubkey();
    let fee_payer = prepare_fee_payer(fee_payer_key)?;

    let multisig_pubkey = Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    let (transaction_pda, _) = derive_transaction_pda(&multisig_pubkey, transaction_index);
    let (proposal_pda, _) = derive_proposal_pda(&multisig_pubkey, transaction_index);
    let (vault_pda, _) = derive_vault_pda(&multisig_pubkey, vault_index);

    // 从链上获取 VaultTransaction，SDK 用标准 Borsh 反序列化（与链上存储格式一致）
    let tx_data = fetch_account_data(client, rpc_url, &transaction_pda.to_string()).await?;
    let msg = parse_vault_tx_message_borsh(&tx_data)?;

    let ix = sdk_instruction(
        client::accounts::VaultTransactionExecute {
            multisig: multisig_pubkey,
            proposal: proposal_pda,
            transaction: transaction_pda,
            member: executor_pubkey,
        },
        client::args::VaultTransactionExecute,
    );

    // remaining accounts: 按 account_keys 顺序传入
    // signer keys 替换为实际的 vault PDA / ephemeral signer PDAs
    let mut ix_with_remaining = ix;

    for (i, key) in msg.account_keys.iter().enumerate() {
        let is_signer_key = i < msg.num_signers as usize;
        let is_writable = if i < msg.num_signers as usize {
            i < msg.num_writable_signers as usize
        } else {
            (i - msg.num_signers as usize) < msg.num_writable_non_signers as usize
        };

        // signer keys[0] = vault PDA，其余为 ephemeral signer PDAs
        let actual_key = if is_signer_key {
            if i == 0 {
                vault_pda
            } else {
                // ephemeral signer PDA（当前不支持，用原 key）
                *key
            }
        } else {
            *key
        };

        ix_with_remaining.accounts.push(AccountMeta {
            pubkey: actual_key.to_bytes(),
            is_signer: false,
            is_writable,
        });
    }

    sign_and_send_with_fee_payer(client, rpc_url, &keypair, &fee_payer, &[ix_with_remaining]).await
}

/// 从 VaultTransaction 原始账户数据中解析消息（标准 Borsh 格式）
///
/// VaultTransaction 布局:
///   discriminator(8) + multisig(32) + creator(32) + index(8) +
///   bump(1) + vault_index(1) + vault_bump(1) +
///   ephemeral_signer_bumps(Vec<u8>, Borsh u32前缀) +
///   message(标准 Borsh: u32前缀)
///
/// 注意: 输入格式是 beet (u8/u16)，但程序在链上以标准 Borsh (u32) 存储
fn parse_vault_tx_message_borsh(data: &[u8]) -> Result<ParsedMessage, String> {
    if data.len() < 87 {
        return Err("VaultTransaction 数据太短".into());
    }

    let mut off = 8 + 32 + 32 + 8 + 1 + 1 + 1; // = 83, skip to esb

    // ephemeral_signer_bumps: Vec<u8> (标准 Borsh u32 前缀)
    if off + 4 > data.len() {
        return Err("数据不足: esb length".into());
    }
    let esb_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4 + esb_len;

    // message 开始（标准 Borsh 格式: u32 前缀）
    if off + 3 > data.len() {
        return Err("数据不足: message header".into());
    }
    let num_signers = data[off]; off += 1;
    let num_writable_signers = data[off]; off += 1;
    let num_writable_non_signers = data[off]; off += 1;

    // account_keys: 标准 Borsh u32 长度前缀 + N * 32 bytes
    if off + 4 > data.len() {
        return Err("数据不足: account_keys length".into());
    }
    let ak_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4;

    let mut account_keys = Vec::with_capacity(ak_len);
    for _ in 0..ak_len {
        if off + 32 > data.len() {
            return Err("数据不足: account_key".into());
        }
        let key = Pubkey::try_from(&data[off..off + 32])
            .map_err(|_| "无效的 Pubkey")?;
        off += 32;
        account_keys.push(key);
    }

    // instructions: Borsh 格式 u32 长度前缀
    if off + 4 > data.len() {
        return Ok(ParsedMessage {
            num_signers, num_writable_signers, num_writable_non_signers,
            account_keys, instructions: vec![],
        });
    }
    let ix_count = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4;

    let mut instructions = Vec::with_capacity(ix_count);
    for _ in 0..ix_count {
        if off >= data.len() { break; }
        let program_id_index = data[off]; off += 1;

        // account_indices: Borsh u32 长度前缀
        if off + 4 > data.len() { break; }
        let ai_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + ai_len > data.len() { break; }
        let account_indices = data[off..off + ai_len].to_vec();
        off += ai_len;

        // data: Borsh u32 长度前缀
        if off + 4 > data.len() { break; }
        let d_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if off + d_len > data.len() { break; }
        let ix_data = data[off..off + d_len].to_vec();
        off += d_len;

        instructions.push(ParsedInstruction { program_id_index, account_indices, data: ix_data });
    }

    Ok(ParsedMessage {
        num_signers,
        num_writable_signers,
        num_writable_non_signers,
        account_keys,
        instructions,
    })
}

/// 解析后的指令
struct ParsedInstruction {
    program_id_index: u8,
    account_indices: Vec<u8>,
    data: Vec<u8>,
}

/// 解析后的 VaultTransaction 消息
struct ParsedMessage {
    num_signers: u8,
    num_writable_signers: u8,
    num_writable_non_signers: u8,
    account_keys: Vec<Pubkey>,
    instructions: Vec<ParsedInstruction>,
}

/// 解码 VaultTransaction 原始数据为人类可读摘要
pub fn decode_vault_tx_summary(data: &[u8]) -> Result<String, String> {
    let msg = parse_vault_tx_message_borsh(data)?;
    if msg.instructions.is_empty() {
        return Ok("空交易".into());
    }

    let summaries: Vec<String> = msg.instructions.iter().map(|ix| {
        let program_id = msg.account_keys.get(ix.program_id_index as usize)
            .copied()
            .unwrap_or_default();
        decode_instruction_summary(&program_id, &ix.data, &ix.account_indices, &msg.account_keys)
    }).collect();

    Ok(summaries.join(" + "))
}

fn decode_instruction_summary(
    program_id: &Pubkey,
    data: &[u8],
    account_indices: &[u8],
    account_keys: &[Pubkey],
) -> String {
    let pid = program_id.to_string();
    let short = |pk: &Pubkey| { let s = pk.to_string(); format!("{}..{}", &s[..4], &s[s.len()-4..]) };
    let format_sol = |lamports: u64| {
        let whole = lamports / 1_000_000_000;
        let frac = lamports % 1_000_000_000;
        if frac == 0 { format!("{whole}") } else { format!("{whole}.{frac:09}").trim_end_matches('0').to_string() }
    };
    let get_account = |idx: usize| -> Option<&Pubkey> {
        account_indices.get(idx).and_then(|&i| account_keys.get(i as usize))
    };

    // System Program (全 0 = 11111111111111111111111111111111)
    if *program_id == Pubkey::default() {
        if data.len() >= 12 && data[..4] == [2, 0, 0, 0] {
            let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
            let to = get_account(1).map(short).unwrap_or_default();
            return format!("SOL 转账 → {to} {} SOL", format_sol(lamports));
        }
        return "System 调用".into();
    }

    // Vote Program
    if pid == "Vote111111111111111111111111111111111111111" {
        if data.len() >= 4 {
            let ix_type = u32::from_le_bytes(data[..4].try_into().unwrap_or_default());
            match ix_type {
                1 => { // Authorize
                    if data.len() >= 40 {
                        let new_auth = Pubkey::try_from(&data[4..36]).ok().map(|p| short(&p)).unwrap_or_default();
                        let auth_type = u32::from_le_bytes(data[36..40].try_into().unwrap_or_default());
                        let role = if auth_type == 0 { "Voter" } else { "Withdrawer" };
                        return format!("Vote 授权 {role} → {new_auth}");
                    }
                }
                7 => { // Withdraw
                    if data.len() >= 12 {
                        let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
                        let to = get_account(1).map(short).unwrap_or_default();
                        return format!("Vote 提取 {} SOL → {to}", format_sol(lamports));
                    }
                }
                _ => {}
            }
        }
        return "Vote 调用".into();
    }

    // Stake Program
    if pid == "Stake11111111111111111111111111111111111111" {
        if data.len() >= 4 {
            let ix_type = u32::from_le_bytes(data[..4].try_into().unwrap_or_default());
            match ix_type {
                1 => { // Authorize
                    if data.len() >= 40 {
                        let new_auth = Pubkey::try_from(&data[4..36]).ok().map(|p| short(&p)).unwrap_or_default();
                        let auth_type = u32::from_le_bytes(data[36..40].try_into().unwrap_or_default());
                        let role = if auth_type == 0 { "Staker" } else { "Withdrawer" };
                        return format!("Stake 授权 {role} → {new_auth}");
                    }
                }
                2 => { // DelegateStake
                    let vote = get_account(1).map(short).unwrap_or_default();
                    return format!("Stake 委托 → {vote}");
                }
                4 => { // Withdraw
                    if data.len() >= 12 {
                        let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
                        let to = get_account(1).map(short).unwrap_or_default();
                        return format!("Stake 提取 {} SOL → {to}", format_sol(lamports));
                    }
                }
                5 => { return "Stake 取消质押".into(); }
                _ => {}
            }
        }
        return "Stake 调用".into();
    }

    // BPF Loader Upgradeable
    if pid == "BPFLoaderUpgradeab1e11111111111111111111111" {
        if data.len() >= 4 && data[..4] == [3, 0, 0, 0] {
            let prog = get_account(1).map(short).unwrap_or_default();
            return format!("升级程序 {prog}");
        }
        return "BPF Loader 调用".into();
    }

    // 预制程序匹配
    for preset in super::presets::all_programs() {
        if preset.program_id == program_id.to_bytes() {
            // 尝试匹配 Anchor discriminator (前8字节)
            if data.len() >= 8 {
                for ix in &preset.instructions {
                    // 无法直接获取 discriminator，只显示程序名
                    let _ = ix;
                }
            }
            return format!("调用 {}", preset.name);
        }
    }

    // 未知程序
    format!("调用程序 {}", short(program_id))
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
    seed_private_key: Option<&[u8]>,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = creator_private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let creator_keypair = Keypair::new_from_array(key_bytes);
    let creator_pubkey = creator_keypair.pubkey();

    // 使用种子或生成随机 create_key
    let create_key_keypair = if let Some(seed_key) = seed_private_key {
        let seed_bytes: [u8; 32] = seed_key
            .try_into()
            .map_err(|_| "种子私钥长度必须为 32 字节".to_string())?;
        Keypair::new_from_array(seed_bytes)
    } else {
        Keypair::new()
    };
    let create_key_pubkey = create_key_keypair.pubkey();

    // 推导 PDA
    let (multisig_pda, _) = derive_multisig_pda(&create_key_pubkey);
    let (program_config_pda, _) = derive_program_config_pda();

    // 获取 ProgramConfig 以得到 treasury 地址
    let config_data = fetch_account_data(client, rpc_url, &program_config_pda.to_string()).await?;
    let treasury = parse_program_config_treasury(&config_data)?;

    let system_program = Pubkey::default();

    // 构建成员列表（所有成员都有 Initiate+Vote+Execute 权限 = 7）
    let members: Vec<types::Member> = member_pubkeys
        .iter()
        .map(|key| types::Member {
            key: *key,
            permissions: types::Permissions { mask: 7 },
        })
        .collect();

    let ix = sdk_instruction(
        client::accounts::MultisigCreateV2 {
            program_config: program_config_pda,
            treasury,
            multisig: multisig_pda,
            create_key: create_key_pubkey,
            creator: creator_pubkey,
            system_program,
        },
        client::args::MultisigCreateV2 {
            args: types::MultisigCreateArgsV2 {
                config_authority: None,
                threshold,
                members,
                time_lock: 0,
                rent_collector: None,
                memo: None,
            },
        },
    );

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

/// 从 ProgramConfig 账户数据中解析 treasury 地址（使用 SDK 反序列化）
fn parse_program_config_treasury(data: &[u8]) -> Result<Pubkey, String> {
    let config = sdk_accounts::ProgramConfig::try_deserialize(&mut &data[..])
        .map_err(|e| format!("ProgramConfig 反序列化失败: {e}"))?;
    Ok(config.treasury)
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
        "params": [address, {"encoding": "base64", "commitment": "confirmed"}],
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

use super::proposals;
