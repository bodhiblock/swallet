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
    native_symbol: &str,
    tokens: &[crate::config::SplTokenConfig],
) -> Result<Vec<ProposalInfo>, String> {
    // 从最新的交易开始向前查找，最多查找 20 个
    let start = multisig.transaction_index;
    let end = if start > 20 { start - 20 } else { 1 };
    let indices: Vec<u64> = (end..=start).rev().collect();
    let count = indices.len();

    // 一次性批量获取所有 proposal 和 vault transaction 账户
    // 先放 proposals 再放 vault txs，便于切片分离
    let mut addresses: Vec<String> = Vec::with_capacity(count * 2);
    let mut proposal_pdas: Vec<Pubkey> = Vec::with_capacity(count);
    for &idx in &indices {
        let (pda, _) = derive_proposal_pda(&multisig.address, idx);
        proposal_pdas.push(pda);
        addresses.push(pda.to_string());
    }
    for &idx in &indices {
        let (pda, _) = derive_transaction_pda(&multisig.address, idx);
        addresses.push(pda.to_string());
    }

    let raw = fetch_multiple_accounts(client, rpc_url, &addresses).await?;
    let (proposal_data, tx_data) = raw.split_at(count);

    let mut proposals = Vec::with_capacity(count);
    for i in 0..count {
        let Some(p_bytes) = &proposal_data[i] else { continue };
        let Ok(mut proposal) = parse_proposal_account(p_bytes, proposal_pdas[i]) else { continue };
        if let Some(tx_bytes) = &tx_data[i] {
            match sdk_accounts::VaultTransaction::try_deserialize(&mut &tx_bytes[..]) {
                Ok(tx) if tx.multisig == proposal.multisig && tx.index == proposal.transaction_index => {
                    proposal.vault_index = Some(tx.vault_index);
                    let (vault, _) = derive_vault_pda(&tx.multisig, tx.vault_index);
                    let (summary, error) = review_transaction(client, rpc_url, tx_bytes, native_symbol, tokens).await;
                    proposal.summary = Some(format!("Vault #{}: {vault}\n{summary}", tx.vault_index));
                    proposal.review_error = error;
                }
                _ => proposal.review_error = Some("交易账户无效或与提案不匹配".into()),
            }
        } else {
            proposal.review_error = Some("未找到提案的交易账户".into());
        }
        proposals.push(proposal);
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
        vault_index: None,
        review_error: None,
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
    _vault_index: u8,
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

    // 从链上获取 VaultTransaction，SDK 用标准 Borsh 反序列化（与链上存储格式一致）
    let tx_data = fetch_account_data(client, rpc_url, &transaction_pda.to_string()).await?;
    let transaction = sdk_accounts::VaultTransaction::try_deserialize(&mut &tx_data[..])
        .map_err(|e| format!("VaultTransaction 反序列化失败: {e}"))?;
    if transaction.multisig != multisig_pubkey || transaction.index != transaction_index {
        return Err("交易不属于当前多签/提案".into());
    }
    let (vault_pda, _) = derive_vault_pda(&multisig_pubkey, transaction.vault_index);
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
    let tx = sdk_accounts::VaultTransaction::try_deserialize(&mut &data[..])
        .map_err(|e| format!("VaultTransaction 反序列化失败: {e}"))?;
    let msg = tx.message;
    if !msg.address_table_lookups.is_empty() {
        return Err("暂不支持含地址查找表的提案，无法完整核对账户".into());
    }
    let count = msg.account_keys.len();
    if msg.num_signers as usize > count || msg.num_writable_signers > msg.num_signers
        || msg.num_writable_non_signers as usize > count - msg.num_signers as usize {
        return Err("交易账户权限头无效".into());
    }
    for ix in &msg.instructions {
        if ix.program_id_index as usize >= count || ix.account_indexes.iter().any(|&i| i as usize >= count) {
            return Err("交易指令账户索引无效".into());
        }
    }
    Ok(ParsedMessage {
        num_signers: msg.num_signers,
        num_writable_signers: msg.num_writable_signers,
        num_writable_non_signers: msg.num_writable_non_signers,
        account_keys: msg.account_keys,
        instructions: msg.instructions.into_iter().map(|ix| ParsedInstruction {
            program_id_index: ix.program_id_index, account_indices: ix.account_indexes, data: ix.data,
        }).collect(),
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
pub fn decode_vault_tx_summary(data: &[u8], native_symbol: &str) -> Result<String, String> {
    let msg = parse_vault_tx_message_borsh(data)?;
    if msg.instructions.is_empty() {
        return Ok("空交易".into());
    }

    let summaries: Vec<String> = msg.instructions.iter().map(|ix| {
        let program_id = msg.account_keys.get(ix.program_id_index as usize)
            .copied()
            .unwrap_or_default();
        decode_instruction_summary(&program_id, &ix.data, &ix.account_indices, &msg.account_keys, native_symbol)
    }).collect();

    Ok(summaries.join(" + "))
}

/// 从真实指令和链上账户补齐 SPL 审批信息；失败保留原始参数，不伪造金额。
async fn review_transaction(client: &Client, rpc: &str, data: &[u8], native_symbol: &str,
    tokens: &[crate::config::SplTokenConfig]) -> (String, Option<String>)
{
    use std::collections::HashMap;
    use super::token;
    let msg = match parse_vault_tx_message_borsh(data) {
        Ok(msg) => msg,
        Err(e) => return (format!("无法解析交易: {e}"), Some(e)),
    };
    let has_tokens = msg.instructions.iter().any(|ix| matches!(msg.account_keys[ix.program_id_index as usize].to_string().as_str(), token::TOKEN | token::TOKEN_2022 | token::ATA));
    let mut accounts = HashMap::new();
    let mut error = None;
    if has_tokens {
        let addresses: Vec<String> = msg.account_keys.iter().map(ToString::to_string).collect();
        match token::fetch_accounts(client, rpc, &addresses).await {
            Ok(values) => {
                accounts.extend(addresses.into_iter().zip(values));
                let mut mints: Vec<String> = accounts.values().filter_map(|v| v["data"]["parsed"]["info"]["mint"].as_str())
                    .filter(|m| !accounts.contains_key(*m)).map(str::to_string).collect();
                mints.sort(); mints.dedup();
                match token::fetch_accounts(client, rpc, &mints).await {
                    Ok(values) => accounts.extend(mints.into_iter().zip(values)),
                    Err(e) => error = Some(format!("查询 Mint 失败: {e}")),
                }
            }
            Err(e) => error = Some(format!("查询代币账户失败: {e}")),
        }
    }
    let account_error = error.clone();
    let mut created = HashMap::new();
    let mut summaries = Vec::new();
    for (i, ix) in msg.instructions.iter().enumerate() {
        let pid = msg.account_keys[ix.program_id_index as usize].to_string();
        let keys: Vec<String> = ix.account_indices.iter().map(|&i| msg.account_keys[i as usize].to_string()).collect();
        let result = match pid.as_str() {
            token::TOKEN | token::TOKEN_2022 => match &account_error {
                Some(e) => Err(e.clone()),
                None => token::describe_transfer(&pid, &ix.data, &keys, &accounts, &created, tokens),
            },
            token::ATA => token::describe_ata(&ix.data, &keys).map(|entry| {
                created.insert(keys[1].clone(), entry);
                format!("{}收款 ATA\n付款方: {}\n目标代币账户: {}\n收款钱包: {}\nMint: {}\nToken Program: {}",
                    if ix.data == [1] { "按需创建" } else { "创建" }, keys[0], keys[1], keys[2], keys[3], keys[5])
            }),
            _ => Ok(decode_instruction_summary(&msg.account_keys[ix.program_id_index as usize], &ix.data,
                &ix.account_indices, &msg.account_keys, native_symbol)),
        };
        let text = match result {
            Ok(text) => text,
            Err(e) => {
                error.get_or_insert(e.clone());
                let amount = if matches!(ix.data.first(), Some(3 | 12)) && ix.data.len() >= 9 {
                    format!("\n原始数量: {}", u64::from_le_bytes(ix.data[1..9].try_into().unwrap()))
                } else { String::new() };
                format!("无法完整解析: {e}\nProgram: {pid}\n账户（按指令顺序）:\n{}\n指令数据: {}{amount}", keys.join("\n"), hex::encode(&ix.data))
            }
        };
        summaries.push(format!("指令 {}: {text}", i + 1));
    }
    (summaries.join("\n\n"), error)
}

/// 解码 Anchor Borsh 序列化的参数
fn decode_anchor_args(data: &[u8], args: &[super::presets::PresetArg]) -> String {
    use super::presets::ArgType;
    let mut parts = Vec::new();
    let mut off = 0;
    for arg in args {
        match arg.arg_type {
            ArgType::Pubkey => {
                if off + 32 > data.len() { break; }
                let pk = Pubkey::try_from(&data[off..off + 32]).ok();
                let s = pk.map(|p| { let s = p.to_string(); format!("{}..{}", &s[..4], &s[s.len()-4..]) }).unwrap_or_default();
                parts.push(format!("{}={}", arg.name, s));
                off += 32;
            }
            ArgType::U64 => {
                if off + 8 > data.len() { break; }
                let v = u64::from_le_bytes(data[off..off + 8].try_into().unwrap_or_default());
                parts.push(format!("{}={}", arg.name, v));
                off += 8;
            }
            ArgType::U32 => {
                if off + 4 > data.len() { break; }
                let v = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default());
                parts.push(format!("{}={}", arg.name, v));
                off += 4;
            }
            ArgType::I64 => {
                if off + 8 > data.len() { break; }
                let v = i64::from_le_bytes(data[off..off + 8].try_into().unwrap_or_default());
                parts.push(format!("{}={}", arg.name, v));
                off += 8;
            }
            ArgType::I32 => {
                if off + 4 > data.len() { break; }
                let v = i32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default());
                parts.push(format!("{}={}", arg.name, v));
                off += 4;
            }
            ArgType::String => {
                // Borsh string: u32 len prefix + utf8 bytes
                if off + 4 > data.len() { break; }
                let len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default()) as usize;
                off += 4;
                if off + len > data.len() { break; }
                let s = std::str::from_utf8(&data[off..off + len]).unwrap_or("<invalid>");
                parts.push(format!("{}=\"{}\"", arg.name, s));
                off += len;
            }
            ArgType::HyperlaneDomain => {
                // 串行 u32, 按已知 domain 值反查 label
                if off + 4 > data.len() { break; }
                let v = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default());
                let label = super::presets::HYPERLANE_DOMAINS
                    .iter()
                    .find(|(_, val)| *val == v)
                    .map(|(l, _)| *l)
                    .unwrap_or("?");
                parts.push(format!("{}={}({})", arg.name, label, v));
                off += 4;
            }
            ArgType::EvmAddressList => {
                // Borsh Vec<H160>: u32 LE len + 20*N bytes
                if off + 4 > data.len() { break; }
                let len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default()) as usize;
                off += 4;
                if off + 20 * len > data.len() { break; }
                let addrs: Vec<String> = (0..len)
                    .map(|i| format!("0x{}", hex::encode(&data[off + i * 20..off + (i + 1) * 20])))
                    .collect();
                parts.push(format!("{}=[{}]", arg.name, addrs.join(",")));
                off += 20 * len;
            }
        }
    }
    parts.join(", ")
}

fn decode_instruction_summary(
    program_id: &Pubkey,
    data: &[u8],
    account_indices: &[u8],
    account_keys: &[Pubkey],
    native_symbol: &str,
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

    if matches!(pid.as_str(), super::token::TOKEN | super::token::TOKEN_2022) {
        let checked = data.first() == Some(&12);
        if matches!(data.first(), Some(3 | 12)) && data.len() == if checked { 10 } else { 9 } {
            let amount = u64::from_le_bytes(data[1..9].try_into().unwrap());
            let mint = if checked { get_account(1).map(ToString::to_string).unwrap_or_default() } else { "需查询来源账户".into() };
            let accounts = account_indices.iter().filter_map(|&i| account_keys.get(i as usize)).map(ToString::to_string).collect::<Vec<_>>().join("\n");
            return format!("SPL 转账（未链上核对）\nMint: {mint}\nToken Program: {pid}\n原始数量: {amount}\n账户（按指令顺序）:\n{accounts}");
        }
    }

    // System Program (全 0 = 11111111111111111111111111111111)
    if *program_id == Pubkey::default() {
        if data.len() >= 12 && data[..4] == [2, 0, 0, 0] {
            let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
            let from = get_account(0).map(|p| p.to_string()).unwrap_or_default();
            let to = get_account(1).map(|p| p.to_string()).unwrap_or_default();
            return format!("{sym} 转账\n源: {from}\n目标: {to}\n金额: {} {sym}", format_sol(lamports), sym = native_symbol);
        }
        return "System 调用".into();
    }

    // Vote Program
    if pid == "Vote111111111111111111111111111111111111111" {
        if data.len() >= 4 {
            let ix_type = u32::from_le_bytes(data[..4].try_into().unwrap_or_default());
            let vote_account = get_account(0).map(|p| p.to_string()).unwrap_or_default();
            match ix_type {
                1 => { // Authorize
                    if data.len() >= 40 {
                        let new_auth = Pubkey::try_from(&data[4..36]).ok().map(|p| p.to_string()).unwrap_or_default();
                        let auth_type = u32::from_le_bytes(data[36..40].try_into().unwrap_or_default());
                        let role = if auth_type == 0 { "Voter" } else { "Withdrawer" };
                        return format!("Vote 授权 ({role})\nVote: {vote_account}\n新{role}: {new_auth}");
                    }
                }
                3 | 7 => { // Withdraw (3=实际指令索引, 7=备用)
                    if data.len() >= 12 {
                        let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
                        let to = get_account(1).map(|p| p.to_string()).unwrap_or_default();
                        return format!("Vote 提取 (Withdraw)\nVote: {vote_account}\n目标: {to}\n金额: {} {}", format_sol(lamports), native_symbol);
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
            let stake_account = get_account(0).map(|p| p.to_string()).unwrap_or_default();
            match ix_type {
                1 => { // Authorize
                    if data.len() >= 40 {
                        let new_auth = Pubkey::try_from(&data[4..36]).ok().map(|p| p.to_string()).unwrap_or_default();
                        let auth_type = u32::from_le_bytes(data[36..40].try_into().unwrap_or_default());
                        let role = if auth_type == 0 { "Staker" } else { "Withdrawer" };
                        return format!("Stake 授权 ({role})\nStake: {stake_account}\n新{role}: {new_auth}");
                    }
                }
                2 => { // DelegateStake
                    let vote = get_account(1).map(|p| p.to_string()).unwrap_or_default();
                    return format!("Stake 委托 (Delegate)\nStake: {stake_account}\nVote: {vote}");
                }
                4 => { // Withdraw
                    if data.len() >= 12 {
                        let lamports = u64::from_le_bytes(data[4..12].try_into().unwrap_or_default());
                        let to = get_account(1).map(|p| p.to_string()).unwrap_or_default();
                        return format!("Stake 提取 (Withdraw)\nStake: {stake_account}\n目标: {to}\n金额: {} {}", format_sol(lamports), native_symbol);
                    }
                }
                5 => {
                    return format!("Stake 取消质押 (Deactivate)\nStake: {stake_account}");
                }
                _ => {}
            }
        }
        return "Stake 调用".into();
    }

    // BPF Loader Upgradeable
    if pid == "BPFLoaderUpgradeab1e11111111111111111111111" {
        if data.len() >= 4 {
            let bpf_type = u32::from_le_bytes(data[..4].try_into().unwrap_or_default());
            match bpf_type {
                3 => {
                    let prog = get_account(1).map(short).unwrap_or_default();
                    return format!("升级程序 (upgrade) {prog}");
                }
                4 => { return "设置 Authority (set_authority)".into(); }
                5 => { return "关闭 Buffer (close)".into(); }
                _ => {}
            }
        }
        return "BPF Loader 调用".into();
    }

    // 预制程序匹配（Anchor discriminator = sha256("global:<name>")[..8]）
    for preset in super::presets::all_programs() {
        if preset.program_id == program_id.to_bytes() {
            if data.len() >= 8 {
                for ix in &preset.instructions {
                    use sha2::{Sha256, Digest};
                    let mut hasher = Sha256::new();
                    hasher.update(format!("global:{}", ix.name).as_bytes());
                    let hash = hasher.finalize();
                    if data[..8] == hash[..8] {
                        let args_str = decode_anchor_args(&data[8..], &ix.args);
                        if args_str.is_empty() {
                            return format!("{}: {} ({})", preset.name, ix.label, ix.name);
                        }
                        return format!("{}: {} ({})\n    {}", preset.name, ix.label, ix.name, args_str);
                    }
                }
            }
            return format!("调用 {}", preset.name);
        }
    }

    // 未知程序
    format!("调用程序 {program_id}\n账户（按指令顺序）:\n{}\n指令数据: {}",
        account_indices.iter().filter_map(|&i| account_keys.get(i as usize)).map(ToString::to_string).collect::<Vec<_>>().join("\n"), hex::encode(data))
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

/// 批量获取账户数据，返回与输入相同顺序的 Vec<Option<Vec<u8>>>
/// 账户不存在返回 None。自动分批（每批最多 100 个）。
async fn fetch_multiple_accounts(
    client: &Client,
    rpc_url: &str,
    addresses: &[String],
) -> Result<Vec<Option<Vec<u8>>>, String> {
    use base64::Engine;
    let mut results: Vec<Option<Vec<u8>>> = Vec::with_capacity(addresses.len());

    for chunk in addresses.chunks(100) {
        let body = json!({
            "jsonrpc": "2.0",
            "method": "getMultipleAccounts",
            "params": [chunk, {"encoding": "base64", "commitment": "confirmed"}],
            "id": 1
        });

        let resp = sol_transfer::rpc_call(client, rpc_url, &body).await?;

        // 检查 JSON-RPC error
        if let Some(err) = resp.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
            return Err(format!("getMultipleAccounts RPC error: {msg}"));
        }

        let values = resp
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_array())
            .ok_or("getMultipleAccounts 响应格式错误")?;

        for val in values {
            if val.is_null() {
                results.push(None);
                continue;
            }
            let data = val
                .get("data")
                .and_then(|d| d.as_array())
                .and_then(|a| a.first())
                .and_then(|s| s.as_str())
                .and_then(|b64| base64::engine::general_purpose::STANDARD.decode(b64).ok());
            results.push(data);
        }
    }

    Ok(results)
}

use super::proposals;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::token::{self, tests as fixture};
    use anchor_lang::AccountSerialize;

    fn transaction(instructions: &[proposals::VaultInstruction], vault: Pubkey) -> Vec<u8> {
        let mut keys = vec![vault];
        for ix in instructions {
            for key in ix.accounts.iter().map(|a| Pubkey::new_from_array(a.pubkey)).chain([Pubkey::new_from_array(ix.program_id)]) {
                if !keys.contains(&key) { keys.push(key); }
            }
        }
        let instructions = instructions.iter().map(|ix| types::MultisigCompiledInstruction {
            program_id_index: keys.iter().position(|k| k.to_bytes() == ix.program_id).unwrap() as u8,
            account_indexes: ix.accounts.iter().map(|a| keys.iter().position(|k| k.to_bytes() == a.pubkey).unwrap() as u8).collect(),
            data: ix.data.clone(),
        }).collect();
        let tx = sdk_accounts::VaultTransaction {
            multisig: Pubkey::new_unique(), creator: Pubkey::new_unique(), index: 7,
            bump: 0, vault_index: 3, vault_bump: 0, ephemeral_signer_bumps: vec![],
            message: types::VaultTransactionMessage {
                num_signers: 1, num_writable_signers: 1, num_writable_non_signers: 2,
                account_keys: keys, instructions, address_table_lookups: vec![],
            },
        };
        let mut bytes = Vec::new();
        tx.try_serialize(&mut bytes).unwrap();
        bytes
    }

    #[tokio::test]
    async fn spl_proposal_review_round_trip() {
        for pid in [token::TOKEN, token::TOKEN_2022] {
            for checked in [false, true] {
                let vault = Pubkey::new_unique();
                let mint = Pubkey::new_unique();
                let to = Pubkey::new_unique();
                let source = crate::chain::solana::derive_ata_address(&vault.to_string(), &mint.to_string(), pid == token::TOKEN_2022).unwrap().parse::<Pubkey>().unwrap();
                let dest = crate::chain::solana::derive_ata_address(&to.to_string(), &mint.to_string(), pid == token::TOKEN_2022).unwrap().parse::<Pubkey>().unwrap();
                let mut instructions = proposals::build_spl_transfer_instructions(&vault.to_bytes(), &to.to_bytes(), &mint.to_bytes(), 1_230_000, 6,
                    &pid.parse::<Pubkey>().unwrap().to_bytes(), &source.to_bytes(), &dest.to_bytes(), true);
                if !checked {
                    instructions[1].accounts.remove(1);
                    instructions[1].data = spl_token::instruction::TokenInstruction::Transfer { amount: 1_230_000 }.pack();
                }
                let bytes = transaction(&instructions, vault);
                assert!(decode_vault_tx_summary(&bytes, "SOL").unwrap().contains("1230000"));
                let msg = parse_vault_tx_message_borsh(&bytes).unwrap();
                let values: Vec<_> = msg.account_keys.iter().map(|key| {
                    if key == &source { fixture::account(pid, &mint.to_string(), &vault.to_string(), 9_000_000) }
                    else if key == &mint { fixture::mint(pid, 6) }
                    else { serde_json::Value::Null }
                }).collect();
                let tokens = [crate::config::SplTokenConfig { mint_address: mint.to_string(), symbol: "USDC".into(), decimals: 6, is_token_2022: pid == token::TOKEN_2022 }];
                let (url, server) = fixture::rpc(vec![json!({"value": values})]);
                let (summary, error) = review_transaction(&Client::new(), &url, &bytes, "SOL", &tokens).await;
                server.join().unwrap();
                assert_eq!(error, None, "{summary}");
                for value in ["1.23 USDC", "按需创建", &source.to_string(), &dest.to_string(), &to.to_string(), &mint.to_string(), pid] {
                    assert!(summary.contains(value), "missing {value}: {summary}");
                }
                if !checked {
                    // Old Transfer without ATA creation carries neither mint nor decimals.
                    let old_bytes = transaction(&instructions[1..], vault);
                    let old_msg = parse_vault_tx_message_borsh(&old_bytes).unwrap();
                    let values: Vec<_> = old_msg.account_keys.iter().map(|key| {
                        if key == &source { fixture::account(pid, &mint.to_string(), &vault.to_string(), 9_000_000) }
                        else if key == &dest { fixture::account(pid, &mint.to_string(), &to.to_string(), 0) }
                        else { serde_json::Value::Null }
                    }).collect();
                    let (url, server) = fixture::rpc(vec![json!({"value":values}), json!({"value":[fixture::mint(pid, 6)]})]);
                    let (summary, error) = review_transaction(&Client::new(), &url, &old_bytes, "SOL", &[]).await;
                    let requests = server.join().unwrap();
                    assert_eq!(requests[1]["params"][0], json!([mint.to_string()]));
                    assert_eq!(error, None);
                    assert!(summary.contains("1.23 未知代币"));
                    assert!(summary.contains(&to.to_string()));
                }
                // RPC failures preserve raw values and prevent approval instead of inventing decimals.
                let (url, server) = fixture::rpc(vec![json!({"value": []})]);
                let (summary, error) = review_transaction(&Client::new(), &url, &bytes, "SOL", &tokens).await;
                server.join().unwrap();
                assert!(error.is_some());
                assert!(summary.contains("原始数量: 1230000"));
                assert!(summary.contains(&dest.to_string()));
                assert!(!summary.contains("1.23 USDC"));
                // Every truncated account is rejected, rather than partially decoding it.
                for len in 0..bytes.len() { assert!(parse_vault_tx_message_borsh(&bytes[..len]).is_err(), "length {len}"); }
                let mut bad = sdk_accounts::VaultTransaction::try_deserialize(&mut &bytes[..]).unwrap();
                bad.message.instructions[0].account_indexes[0] = 255;
                let mut bytes = Vec::new(); bad.try_serialize(&mut bytes).unwrap();
                assert!(parse_vault_tx_message_borsh(&bytes).is_err());
            }
        }
    }
}

#[cfg(test)]
#[path = "spl_local_test.rs"]
mod spl_local_test;
