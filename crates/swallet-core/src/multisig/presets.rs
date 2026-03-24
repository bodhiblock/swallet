//! 预制程序指令注册表 —— 多签提案可直接调用的管理员函数
//!
//! 使用 `declare_program!` 生成的 CPI 类型（`client::accounts` + `client::args`）
//! 自动处理 discriminator 和 Borsh 序列化，无需手动编码。
//!
//! 设计为可扩展：新增 IDL 只需添加一个 `xxx_program()` 函数并注册到 `all_programs()`。

use anchor_lang::{InstructionData, ToAccountMetas};
use super::proposals::{VaultAccountMeta, VaultInstruction};
use solana_sdk::pubkey::Pubkey;

// ==================== 数据结构 ====================

/// 预制参数类型
#[derive(Debug, Clone)]
pub enum ArgType {
    /// Solana 公钥 (bs58 输入)
    Pubkey,
    /// u64 数值
    U64,
    /// u32 数值
    U32,
    /// i64 数值
    I64,
}

/// 预制参数定义
#[derive(Debug, Clone)]
pub struct PresetArg {
    #[allow(dead_code)]
    pub name: &'static str,
    pub label: &'static str,
    pub arg_type: ArgType,
}

/// 指令构建函数类型
pub type BuildFn = fn(&[u8; 32], &[u8; 32], &[String]) -> Result<Vec<VaultInstruction>, String>;

/// 预制指令定义
pub struct PresetInstruction {
    #[allow(dead_code)]
    pub name: &'static str,
    pub label: &'static str,
    pub args: Vec<PresetArg>,
    /// 构建 vault 指令:
    /// (vault_pubkey, program_id_bytes, arg_values) -> Vec<VaultInstruction>
    pub build: BuildFn,
}

/// 预制程序
pub struct PresetProgram {
    pub name: &'static str,
    pub program_id: [u8; 32],
    /// 所属链 ID（与 config 中的 SolanaChainConfig.id 对应）
    pub chain_id: &'static str,
    pub instructions: Vec<PresetInstruction>,
}

/// 获取所有预制程序
pub fn all_programs() -> Vec<PresetProgram> {
    vec![
        quest_program(),
        agent_registry_program(),
        skills_hub_program(),
        zk_program(),
    ]
}

/// 获取指定链的预制程序
pub fn programs_for_chain(chain_id: &str) -> Vec<PresetProgram> {
    all_programs()
        .into_iter()
        .filter(|p| p.chain_id == chain_id)
        .collect()
}

// ==================== 辅助函数 ====================

/// 解析 bs58 地址为 Pubkey
fn parse_pubkey(s: &str) -> Result<Pubkey, String> {
    s.parse::<Pubkey>().map_err(|e| format!("无效的地址: {e}"))
}

fn parse_u64(s: &str) -> Result<u64, String> {
    s.parse::<u64>().map_err(|_| format!("无效的数值: {s}"))
}

fn parse_u32(s: &str) -> Result<u32, String> {
    s.parse::<u32>().map_err(|_| format!("无效的数值: {s}"))
}

fn parse_i64(s: &str) -> Result<i64, String> {
    s.parse::<i64>().map_err(|_| format!("无效的数值: {s}"))
}

fn pk(bytes: &[u8; 32]) -> Pubkey {
    Pubkey::new_from_array(*bytes)
}

fn derive_pda(seeds: &[&[u8]], program_id: &Pubkey) -> Pubkey {
    let (pda, _) = Pubkey::find_program_address(seeds, program_id);
    pda
}

/// 将 SDK 的 ToAccountMetas + InstructionData 转换为 VaultInstruction
fn to_vault_ix(
    pid: &[u8; 32],
    accounts: impl ToAccountMetas,
    data: impl InstructionData,
) -> VaultInstruction {
    let metas = accounts.to_account_metas(None);
    VaultInstruction {
        program_id: *pid,
        accounts: metas
            .into_iter()
            .map(|m| VaultAccountMeta {
                pubkey: m.pubkey.to_bytes(),
                is_signer: m.is_signer,
                is_writable: m.is_writable,
            })
            .collect(),
        data: data.data(),
    }
}

// ==================== Nara Quest ====================

use crate::nara_quest::client as quest_client;

fn quest_program() -> PresetProgram {
    let program_id = crate::nara_quest::ID.to_bytes();
    PresetProgram {
        name: "Nara Quest",
        program_id,
        chain_id: "nara-mainnet",
        instructions: vec![
            PresetInstruction {
                name: "initialize",
                label: "初始化",
                args: vec![],
                build: build_quest_initialize,
            },
            PresetInstruction {
                name: "create_question",
                label: "创建问题",
                args: vec![
                    PresetArg { name: "answer_hash", label: "答案哈希 (bs58)", arg_type: ArgType::Pubkey },
                    PresetArg { name: "deadline", label: "截止时间 (Unix时间戳)", arg_type: ArgType::I64 },
                    PresetArg { name: "difficulty", label: "难度", arg_type: ArgType::U32 },
                ],
                build: build_quest_create_question,
            },
            PresetInstruction {
                name: "set_reward_config",
                label: "设置奖励配置",
                args: vec![
                    PresetArg { name: "min_reward_count", label: "最小奖励人数", arg_type: ArgType::U32 },
                    PresetArg { name: "max_reward_count", label: "最大奖励人数", arg_type: ArgType::U32 },
                ],
                build: build_quest_set_reward_config,
            },
            PresetInstruction {
                name: "set_reward_per_share",
                label: "设置每份奖励",
                args: vec![
                    PresetArg { name: "reward_per_share", label: "每份奖励 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "extra_reward", label: "额外奖励 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_quest_set_reward_per_share,
            },
            PresetInstruction {
                name: "set_stake_config",
                label: "设置质押配置",
                args: vec![
                    PresetArg { name: "bps_high", label: "质押倍率上限 (bps)", arg_type: ArgType::U64 },
                    PresetArg { name: "bps_low", label: "质押倍率下限 (bps)", arg_type: ArgType::U64 },
                    PresetArg { name: "decay_ms", label: "衰减时间 (毫秒)", arg_type: ArgType::I64 },
                ],
                build: build_quest_set_stake_config,
            },
            PresetInstruction {
                name: "set_quest_authority",
                label: "设置出题权限",
                args: vec![
                    PresetArg { name: "new_quest_authority", label: "新出题地址", arg_type: ArgType::Pubkey },
                ],
                build: build_quest_set_quest_authority,
            },
            PresetInstruction {
                name: "set_quest_interval",
                label: "设置出题间隔",
                args: vec![
                    PresetArg { name: "min_quest_interval", label: "最小间隔 (秒)", arg_type: ArgType::I64 },
                ],
                build: build_quest_set_quest_interval,
            },
            PresetInstruction {
                name: "transfer_authority",
                label: "转移管理权",
                args: vec![
                    PresetArg { name: "new_authority", label: "新管理员地址", arg_type: ArgType::Pubkey },
                ],
                build: build_quest_transfer_authority,
            },
        ],
    }
}

fn build_quest_initialize(vault: &[u8; 32], pid: &[u8; 32], _args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    let program_id = pk(pid);
    let vault_pk = pk(vault);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::Initialize {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            pool: derive_pda(&[b"quest_pool"], &program_id),
            treasury: derive_pda(&[b"quest_treasury"], &program_id),
            authority: vault_pk,
            system_program: solana_sdk::system_program::ID,
        },
        quest_client::args::Initialize,
    )])
}

fn build_quest_create_question(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    let vault_pk = pk(vault);
    let answer_hash: [u8; 32] = parse_pubkey(&args[0])?.to_bytes();
    let deadline = parse_i64(&args[1])?;
    let difficulty = parse_u32(&args[2])?;

    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::CreateQuestion {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            pool: derive_pda(&[b"quest_pool"], &program_id),
            vault: derive_pda(&[b"quest_vault"], &program_id),
            treasury: derive_pda(&[b"quest_treasury"], &program_id),
            caller: vault_pk,
            system_program: solana_sdk::system_program::ID,
        },
        quest_client::args::CreateQuestion {
            question: String::new(),
            answer_hash,
            deadline,
            difficulty,
        },
    )])
}

fn build_quest_set_reward_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetRewardConfig {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetRewardConfig {
            min_reward_count: parse_u32(&args[0])?,
            max_reward_count: parse_u32(&args[1])?,
        },
    )])
}

fn build_quest_set_reward_per_share(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetRewardPerShare {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetRewardPerShare {
            reward_per_share: parse_u64(&args[0])?,
            extra_reward: parse_u64(&args[1])?,
        },
    )])
}

fn build_quest_set_stake_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetStakeConfig {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetStakeConfig {
            bps_high: parse_u64(&args[0])?,
            bps_low: parse_u64(&args[1])?,
            decay_ms: parse_i64(&args[2])?,
        },
    )])
}

fn build_quest_set_quest_authority(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetQuestAuthority {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetQuestAuthority {
            new_quest_authority: parse_pubkey(&args[0])?,
        },
    )])
}

fn build_quest_set_quest_interval(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetQuestInterval {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetQuestInterval {
            min_quest_interval: parse_i64(&args[0])?,
        },
    )])
}

fn build_quest_transfer_authority(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::TransferAuthority {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::TransferAuthority {
            new_authority: parse_pubkey(&args[0])?,
        },
    )])
}

// ==================== Nara Agent Registry ====================

use crate::nara_agent_registry::client as agent_client;

fn agent_registry_program() -> PresetProgram {
    let program_id = crate::nara_agent_registry::ID.to_bytes();
    PresetProgram {
        name: "Nara Agent Registry",
        program_id,
        chain_id: "nara-mainnet",
        instructions: vec![
            PresetInstruction {
                name: "init_config",
                label: "初始化配置",
                args: vec![],
                build: build_agent_init_config,
            },
            PresetInstruction {
                name: "update_admin",
                label: "更新管理员",
                args: vec![
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey },
                ],
                build: build_agent_update_admin,
            },
            PresetInstruction {
                name: "update_register_fee",
                label: "更新注册费",
                args: vec![
                    PresetArg { name: "new_fee", label: "新注册费 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_agent_update_register_fee,
            },
            PresetInstruction {
                name: "update_activity_config",
                label: "更新活动配置",
                args: vec![
                    PresetArg { name: "activity_reward", label: "活动奖励 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "referral_activity_reward", label: "推荐活动奖励 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_agent_update_activity_config,
            },
            PresetInstruction {
                name: "update_points_config",
                label: "更新积分配置",
                args: vec![
                    PresetArg { name: "points_self", label: "自身积分", arg_type: ArgType::U64 },
                    PresetArg { name: "points_referral", label: "推荐积分", arg_type: ArgType::U64 },
                ],
                build: build_agent_update_points_config,
            },
            PresetInstruction {
                name: "update_referral_config",
                label: "更新推荐配置",
                args: vec![
                    PresetArg { name: "referral_register_fee", label: "推荐注册费 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "referral_fee_share", label: "推荐费分成 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "referral_register_points", label: "推荐注册积分", arg_type: ArgType::U64 },
                ],
                build: build_agent_update_referral_config,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_agent_withdraw_fees,
            },
            PresetInstruction {
                name: "expand_config",
                label: "扩展配置账户",
                args: vec![
                    PresetArg { name: "extend_size", label: "扩展大小 (bytes)", arg_type: ArgType::U64 },
                ],
                build: build_agent_expand_config,
            },
            PresetInstruction {
                name: "update_twitter_verification_config",
                label: "更新推特验证配置",
                args: vec![
                    PresetArg { name: "fee", label: "验证费 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "reward", label: "验证奖励 (lamports)", arg_type: ArgType::U64 },
                    PresetArg { name: "points", label: "验证积分", arg_type: ArgType::U64 },
                ],
                build: build_agent_update_twitter_verification_config,
            },
            PresetInstruction {
                name: "update_twitter_verifier",
                label: "更新推特验证者",
                args: vec![
                    PresetArg { name: "new_verifier", label: "新验证者地址", arg_type: ArgType::Pubkey },
                ],
                build: build_agent_update_twitter_verifier,
            },
            PresetInstruction {
                name: "withdraw_twitter_verify_fees",
                label: "提取推特验证手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_agent_withdraw_twitter_verify_fees,
            },
        ],
    }
}

fn build_agent_init_config(vault: &[u8; 32], pid: &[u8; 32], _args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    let program_id = pk(pid);
    let vault_pk = pk(vault);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::InitConfig {
            admin: vault_pk,
            config: derive_pda(&[b"config"], &program_id),
            fee_vault: derive_pda(&[b"fee_vault"], &program_id),
            point_mint: derive_pda(&[b"point_mint"], &program_id),
            referee_mint: derive_pda(&[b"referee_mint"], &program_id),
            referee_activity_mint: derive_pda(&[b"referee_activity_mint"], &program_id),
            mint_authority: derive_pda(&[b"mint_authority"], &program_id),
            token_program: Pubkey::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
            system_program: solana_sdk::system_program::ID,
        },
        agent_client::args::InitConfig,
    )])
}

fn build_agent_update_admin(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateAdmin {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateAdmin {
            new_admin: parse_pubkey(&args[0])?,
        },
    )])
}

fn build_agent_update_register_fee(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateRegisterFee {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateRegisterFee {
            new_fee: parse_u64(&args[0])?,
        },
    )])
}

fn build_agent_update_activity_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateActivityConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateActivityConfig {
            activity_reward: parse_u64(&args[0])?,
            referral_activity_reward: parse_u64(&args[1])?,
        },
    )])
}

fn build_agent_update_points_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdatePointsConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdatePointsConfig {
            points_self: parse_u64(&args[0])?,
            points_referral: parse_u64(&args[1])?,
        },
    )])
}

fn build_agent_update_referral_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateReferralConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateReferralConfig {
            referral_register_fee: parse_u64(&args[0])?,
            referral_fee_share: parse_u64(&args[1])?,
            referral_register_points: parse_u64(&args[2])?,
        },
    )])
}

fn build_agent_withdraw_fees(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::WithdrawFees {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            fee_vault: derive_pda(&[b"fee_vault"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        agent_client::args::WithdrawFees {
            amount: parse_u64(&args[0])?,
        },
    )])
}

fn build_agent_expand_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::ExpandConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        agent_client::args::ExpandConfig {
            extend_size: parse_u64(&args[0])?,
        },
    )])
}

fn build_agent_update_twitter_verification_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateTwitterVerificationConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateTwitterVerificationConfig {
            fee: parse_u64(&args[0])?,
            reward: parse_u64(&args[1])?,
            points: parse_u64(&args[2])?,
        },
    )])
}

fn build_agent_update_twitter_verifier(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateTwitterVerifier {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateTwitterVerifier {
            new_verifier: parse_pubkey(&args[0])?,
        },
    )])
}

fn build_agent_withdraw_twitter_verify_fees(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::WithdrawTwitterVerifyFees {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            twitter_verify_vault: derive_pda(&[b"twitter_verify_vault"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        agent_client::args::WithdrawTwitterVerifyFees {
            amount: parse_u64(&args[0])?,
        },
    )])
}

// ==================== Nara Skills Hub ====================

use crate::nara_skills_hub::client as skills_client;

fn skills_hub_program() -> PresetProgram {
    let program_id = crate::nara_skills_hub::ID.to_bytes();
    PresetProgram {
        name: "Nara Skills Hub",
        program_id,
        chain_id: "nara-mainnet",
        instructions: vec![
            PresetInstruction {
                name: "init_config",
                label: "初始化配置",
                args: vec![],
                build: build_skills_init_config,
            },
            PresetInstruction {
                name: "update_admin",
                label: "更新管理员",
                args: vec![
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey },
                ],
                build: build_skills_update_admin,
            },
            PresetInstruction {
                name: "update_register_fee",
                label: "更新注册费",
                args: vec![
                    PresetArg { name: "new_fee", label: "新注册费 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_skills_update_register_fee,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_skills_withdraw_fees,
            },
        ],
    }
}

fn build_skills_init_config(vault: &[u8; 32], pid: &[u8; 32], _args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        skills_client::accounts::InitConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        skills_client::args::InitConfig,
    )])
}

fn build_skills_update_admin(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        skills_client::accounts::UpdateAdmin {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        skills_client::args::UpdateAdmin {
            new_admin: parse_pubkey(&args[0])?,
        },
    )])
}

fn build_skills_update_register_fee(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        skills_client::accounts::UpdateRegisterFee {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        skills_client::args::UpdateRegisterFee {
            new_fee: parse_u64(&args[0])?,
        },
    )])
}

fn build_skills_withdraw_fees(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        skills_client::accounts::WithdrawFees {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            vault: derive_pda(&[b"fee_vault"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        skills_client::args::WithdrawFees {
            amount: parse_u64(&args[0])?,
        },
    )])
}

// ==================== Nara ZK ====================

use crate::nara_zk::client as zk_client;

fn zk_program() -> PresetProgram {
    let program_id = crate::nara_zk::ID.to_bytes();
    PresetProgram {
        name: "Nara ZK Identity",
        program_id,
        chain_id: "nara-mainnet",
        instructions: vec![
            PresetInstruction {
                name: "initialize_config",
                label: "初始化配置",
                args: vec![
                    PresetArg { name: "fee_amount", label: "手续费 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_zk_initialize_config,
            },
            PresetInstruction {
                name: "update_config",
                label: "更新配置",
                args: vec![
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey },
                    PresetArg { name: "new_fee_amount", label: "新手续费 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_zk_update_config,
            },
            PresetInstruction {
                name: "initialize",
                label: "初始化 Merkle Tree",
                args: vec![
                    PresetArg { name: "denomination", label: "面额 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_zk_initialize,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64 },
                ],
                build: build_zk_withdraw_fees,
            },
        ],
    }
}

fn build_zk_initialize_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        zk_client::accounts::InitializeConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            fee_vault: derive_pda(&[b"fee_vault"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        zk_client::args::InitializeConfig {
            fee_amount: parse_u64(&args[0])?,
        },
    )])
}

fn build_zk_update_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        zk_client::accounts::UpdateConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        zk_client::args::UpdateConfig {
            new_admin: parse_pubkey(&args[0])?,
            new_fee_amount: parse_u64(&args[1])?,
        },
    )])
}

fn build_zk_initialize(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    let denomination = parse_u64(&args[0])?;
    let denom_bytes = denomination.to_le_bytes();

    Ok(vec![to_vault_ix(
        pid,
        zk_client::accounts::Initialize {
            payer: pk(vault),
            merkle_tree: derive_pda(&[b"tree", &denom_bytes], &program_id),
            pool: derive_pda(&[b"pool", &denom_bytes], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        zk_client::args::Initialize {
            denomination,
        },
    )])
}

fn build_zk_withdraw_fees(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        zk_client::accounts::WithdrawFees {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
            fee_vault: derive_pda(&[b"fee_vault"], &program_id),
            system_program: solana_sdk::system_program::ID,
        },
        zk_client::args::WithdrawFees {
            amount: parse_u64(&args[0])?,
        },
    )])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_programs_not_empty() {
        let programs = all_programs();
        assert_eq!(programs.len(), 4);
        for p in &programs {
            assert!(!p.instructions.is_empty(), "{} should have instructions", p.name);
        }
    }

    #[test]
    fn test_build_quest_set_reward_config() {
        let vault = [1u8; 32];
        let pid = crate::nara_quest::ID.to_bytes();
        let args = vec!["5".to_string(), "10".to_string()];
        let ixs = build_quest_set_reward_config(&vault, &pid, &args).unwrap();
        assert_eq!(ixs.len(), 1);
        assert_eq!(ixs[0].program_id, pid);
        assert_eq!(ixs[0].accounts.len(), 2);
        // discriminator (8) + u32 (4) + u32 (4) = 16
        assert_eq!(ixs[0].data.len(), 16);
    }

    #[test]
    fn test_build_skills_update_admin() {
        let vault = [1u8; 32];
        let pid = crate::nara_skills_hub::ID.to_bytes();
        let new_admin = bs58::encode([2u8; 32]).into_string();
        let args = vec![new_admin];
        let ixs = build_skills_update_admin(&vault, &pid, &args).unwrap();
        assert_eq!(ixs.len(), 1);
        // discriminator (8) + pubkey (32) = 40
        assert_eq!(ixs[0].data.len(), 40);
    }
}
