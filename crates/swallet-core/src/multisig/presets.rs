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
    /// i32 数值
    I32,
    /// 字符串
    String,
}

/// 预制参数定义
#[derive(Debug, Clone)]
pub struct PresetArg {
    #[allow(dead_code)]
    pub name: &'static str,
    pub label: &'static str,
    pub arg_type: ArgType,
    /// 对应的链上 config 字段名，用于显示当前值提示
    pub config_field: Option<&'static str>,
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

fn parse_i32(s: &str) -> Result<i32, String> {
    s.parse::<i32>().map_err(|_| format!("无效的数值: {s}"))
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
                    PresetArg { name: "answer_hash", label: "答案哈希 (bs58)", arg_type: ArgType::Pubkey, config_field: None },
                    PresetArg { name: "deadline", label: "截止时间 (Unix时间戳)", arg_type: ArgType::I64, config_field: None },
                    PresetArg { name: "difficulty", label: "难度", arg_type: ArgType::U32, config_field: None },
                ],
                build: build_quest_create_question,
            },
            PresetInstruction {
                name: "set_reward_config",
                label: "设置奖励配置",
                args: vec![
                    PresetArg { name: "min_reward_count", label: "最小奖励人数", arg_type: ArgType::U32, config_field: Some("min_reward_count") },
                    PresetArg { name: "max_reward_count", label: "最大奖励人数", arg_type: ArgType::U32, config_field: Some("max_reward_count") },
                ],
                build: build_quest_set_reward_config,
            },
            PresetInstruction {
                name: "set_reward_per_share",
                label: "设置每份奖励",
                args: vec![
                    PresetArg { name: "reward_per_share", label: "每份奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("reward_per_share") },
                    PresetArg { name: "extra_reward", label: "额外奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("extra_reward") },
                ],
                build: build_quest_set_reward_per_share,
            },
            PresetInstruction {
                name: "set_stake_config",
                label: "设置质押配置",
                args: vec![
                    PresetArg { name: "bps_high", label: "质押倍率上限 (bps)", arg_type: ArgType::U64, config_field: Some("stake_bps_high") },
                    PresetArg { name: "bps_low", label: "质押倍率下限 (bps)", arg_type: ArgType::U64, config_field: Some("stake_bps_low") },
                    PresetArg { name: "decay_ms", label: "衰减时间 (毫秒)", arg_type: ArgType::I64, config_field: Some("decay_ms") },
                ],
                build: build_quest_set_stake_config,
            },
            PresetInstruction {
                name: "set_quest_authority",
                label: "设置出题权限",
                args: vec![
                    PresetArg { name: "new_quest_authority", label: "新出题地址", arg_type: ArgType::Pubkey, config_field: Some("quest_authority") },
                ],
                build: build_quest_set_quest_authority,
            },
            PresetInstruction {
                name: "set_quest_interval",
                label: "设置出题间隔",
                args: vec![
                    PresetArg { name: "min_quest_interval", label: "最小间隔 (秒)", arg_type: ArgType::I64, config_field: Some("min_quest_interval") },
                ],
                build: build_quest_set_quest_interval,
            },
            PresetInstruction {
                name: "transfer_authority",
                label: "转移管理权",
                args: vec![
                    PresetArg { name: "new_authority", label: "新管理员地址", arg_type: ArgType::Pubkey, config_field: Some("authority") },
                ],
                build: build_quest_transfer_authority,
            },
            PresetInstruction {
                name: "set_stake_authority",
                label: "设置质押权限",
                args: vec![
                    PresetArg { name: "new_stake_authority", label: "新质押权限地址", arg_type: ArgType::Pubkey, config_field: Some("stake_authority") },
                ],
                build: build_quest_set_stake_authority,
            },
            PresetInstruction {
                name: "expand_config",
                label: "扩展配置账户",
                args: vec![
                    PresetArg { name: "additional_size", label: "扩展大小 (bytes)", arg_type: ArgType::U32, config_field: None },
                ],
                build: build_quest_expand_config,
            },
            PresetInstruction {
                name: "set_airdrop_config",
                label: "设置空投配置",
                args: vec![
                    PresetArg { name: "airdrop_amount", label: "空投数量 (lamports)", arg_type: ArgType::U64, config_field: Some("airdrop_amount") },
                    PresetArg { name: "max_airdrop_count", label: "最大空投次数", arg_type: ArgType::U32, config_field: Some("max_airdrop_count") },
                ],
                build: build_quest_set_airdrop_config,
            },
            PresetInstruction {
                name: "adjust_free_stake",
                label: "调整免费质押",
                args: vec![
                    PresetArg { name: "user", label: "用户地址", arg_type: ArgType::Pubkey, config_field: None },
                    PresetArg { name: "delta", label: "调整量 (正增负减)", arg_type: ArgType::I32, config_field: None },
                    PresetArg { name: "reason", label: "原因", arg_type: ArgType::String, config_field: None },
                ],
                build: build_quest_adjust_free_stake,
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

fn build_quest_set_stake_authority(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetStakeAuthority {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetStakeAuthority {
            new_stake_authority: parse_pubkey(&args[0])?,
        },
    )])
}

fn build_quest_expand_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::ExpandConfig {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
            system_program: solana_sdk::system_program::ID,
        },
        quest_client::args::ExpandConfig {
            additional_size: parse_u32(&args[0])?,
        },
    )])
}

fn build_quest_set_airdrop_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::SetAirdropConfig {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            authority: pk(vault),
        },
        quest_client::args::SetAirdropConfig {
            airdrop_amount: parse_u64(&args[0])?,
            max_airdrop_count: parse_u32(&args[1])?,
        },
    )])
}

fn build_quest_adjust_free_stake(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    let user = parse_pubkey(&args[0])?;
    let (stake_record, _) = Pubkey::find_program_address(
        &[b"stake_record", user.as_ref()],
        &program_id,
    );
    Ok(vec![to_vault_ix(
        pid,
        quest_client::accounts::AdjustFreeStake {
            game_config: derive_pda(&[b"quest_config"], &program_id),
            stake_record,
            user,
            caller: pk(vault),
            system_program: solana_sdk::system_program::ID,
        },
        quest_client::args::AdjustFreeStake {
            delta: parse_i32(&args[1])?,
            reason: args[2].clone(),
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
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey, config_field: Some("admin") },
                ],
                build: build_agent_update_admin,
            },
            PresetInstruction {
                name: "update_register_fee",
                label: "更新注册费",
                args: vec![
                    PresetArg { name: "fee", label: "注册费 (lamports)", arg_type: ArgType::U64, config_field: Some("register_fee") },
                    PresetArg { name: "fee_7", label: "7位注册费 (lamports)", arg_type: ArgType::U64, config_field: Some("register_fee_7") },
                    PresetArg { name: "fee_6", label: "6位注册费 (lamports)", arg_type: ArgType::U64, config_field: Some("register_fee_6") },
                    PresetArg { name: "fee_5", label: "5位注册费 (lamports)", arg_type: ArgType::U64, config_field: Some("register_fee_5") },
                ],
                build: build_agent_update_register_fee,
            },
            PresetInstruction {
                name: "update_activity_config",
                label: "更新活动配置",
                args: vec![
                    PresetArg { name: "activity_reward", label: "活动奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("activity_reward") },
                    PresetArg { name: "referral_activity_reward", label: "推荐活动奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("referral_activity_reward") },
                ],
                build: build_agent_update_activity_config,
            },
            PresetInstruction {
                name: "update_points_config",
                label: "更新积分配置",
                args: vec![
                    PresetArg { name: "points_self", label: "自身积分", arg_type: ArgType::U64, config_field: Some("points_self") },
                    PresetArg { name: "points_referral", label: "推荐积分", arg_type: ArgType::U64, config_field: Some("points_referral") },
                ],
                build: build_agent_update_points_config,
            },
            PresetInstruction {
                name: "update_referral_config",
                label: "更新推荐配置",
                args: vec![
                    PresetArg { name: "referral_discount_bps", label: "推荐折扣 (bps)", arg_type: ArgType::U64, config_field: Some("referral_discount_bps") },
                    PresetArg { name: "referral_share_bps", label: "推荐分成 (bps)", arg_type: ArgType::U64, config_field: Some("referral_share_bps") },
                    PresetArg { name: "referral_register_points", label: "推荐注册积分", arg_type: ArgType::U64, config_field: Some("referral_register_points") },
                ],
                build: build_agent_update_referral_config,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64, config_field: None },
                ],
                build: build_agent_withdraw_fees,
            },
            PresetInstruction {
                name: "expand_config",
                label: "扩展配置账户",
                args: vec![
                    PresetArg { name: "extend_size", label: "扩展大小 (bytes)", arg_type: ArgType::U64, config_field: None },
                ],
                build: build_agent_expand_config,
            },
            PresetInstruction {
                name: "update_twitter_verification_config",
                label: "更新推特验证配置",
                args: vec![
                    PresetArg { name: "fee", label: "验证费 (lamports)", arg_type: ArgType::U64, config_field: Some("twitter_verification_fee") },
                    PresetArg { name: "reward", label: "验证奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("twitter_verification_reward") },
                    PresetArg { name: "points", label: "验证积分", arg_type: ArgType::U64, config_field: Some("twitter_verification_points") },
                ],
                build: build_agent_update_twitter_verification_config,
            },
            PresetInstruction {
                name: "update_twitter_verifier",
                label: "更新推特验证者",
                args: vec![
                    PresetArg { name: "new_verifier", label: "新验证者地址", arg_type: ArgType::Pubkey, config_field: Some("twitter_verifier") },
                ],
                build: build_agent_update_twitter_verifier,
            },
            PresetInstruction {
                name: "withdraw_twitter_verify_fees",
                label: "提取推特验证手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64, config_field: None },
                ],
                build: build_agent_withdraw_twitter_verify_fees,
            },
            PresetInstruction {
                name: "update_tweet_verify_config",
                label: "更新推文验证配置",
                args: vec![
                    PresetArg { name: "reward", label: "验证奖励 (lamports)", arg_type: ArgType::U64, config_field: Some("tweet_verify_reward") },
                    PresetArg { name: "points", label: "验证积分", arg_type: ArgType::U64, config_field: Some("tweet_verify_points") },
                ],
                build: build_agent_update_tweet_verify_config,
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
    if args.len() < 4 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateRegisterFee {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateRegisterFee {
            fee: parse_u64(&args[0])?,
            fee_7: parse_u64(&args[1])?,
            fee_6: parse_u64(&args[2])?,
            fee_5: parse_u64(&args[3])?,
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
            referral_discount_bps: parse_u64(&args[0])?,
            referral_share_bps: parse_u64(&args[1])?,
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

fn build_agent_update_tweet_verify_config(vault: &[u8; 32], pid: &[u8; 32], args: &[String]) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 { return Err("参数不足".into()); }
    let program_id = pk(pid);
    Ok(vec![to_vault_ix(
        pid,
        agent_client::accounts::UpdateTweetVerifyConfig {
            admin: pk(vault),
            config: derive_pda(&[b"config"], &program_id),
        },
        agent_client::args::UpdateTweetVerifyConfig {
            reward: parse_u64(&args[0])?,
            points: parse_u64(&args[1])?,
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
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey, config_field: Some("admin") },
                ],
                build: build_skills_update_admin,
            },
            PresetInstruction {
                name: "update_register_fee",
                label: "更新注册费",
                args: vec![
                    PresetArg { name: "new_fee", label: "新注册费 (lamports)", arg_type: ArgType::U64, config_field: Some("register_fee") },
                ],
                build: build_skills_update_register_fee,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64, config_field: None },
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
                    PresetArg { name: "fee_amount", label: "手续费 (lamports)", arg_type: ArgType::U64, config_field: None },
                ],
                build: build_zk_initialize_config,
            },
            PresetInstruction {
                name: "update_config",
                label: "更新配置",
                args: vec![
                    PresetArg { name: "new_admin", label: "新管理员地址", arg_type: ArgType::Pubkey, config_field: Some("admin") },
                    PresetArg { name: "new_fee_amount", label: "新手续费 (lamports)", arg_type: ArgType::U64, config_field: Some("fee_amount") },
                ],
                build: build_zk_update_config,
            },
            PresetInstruction {
                name: "initialize",
                label: "初始化 Merkle Tree",
                args: vec![
                    PresetArg { name: "denomination", label: "面额 (lamports)", arg_type: ArgType::U64, config_field: None },
                ],
                build: build_zk_initialize,
            },
            PresetInstruction {
                name: "withdraw_fees",
                label: "提取手续费",
                args: vec![
                    PresetArg { name: "amount", label: "提取数量 (lamports)", arg_type: ArgType::U64, config_field: None },
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

// ==================== 链上 Config 值解析 ====================

use std::collections::HashMap;

/// 从原始账户数据中解析 config 字段值
/// 返回 field_name -> display_value 的映射
pub fn parse_config_values(program_id: &[u8; 32], data: &[u8]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let quest_id = crate::nara_quest::ID.to_bytes();
    let agent_id = crate::nara_agent_registry::ID.to_bytes();
    let skills_id = crate::nara_skills_hub::ID.to_bytes();
    let zk_id = crate::nara_zk::ID.to_bytes();

    if program_id == &quest_id {
        // GameConfig (Borsh, packed after 8-byte discriminator)
        // authority(32) min_reward_count(4) max_reward_count(4)
        // stake_bps_high(8) stake_bps_low(8) decay_ms(8)
        // treasury(32) quest_authority(32) min_quest_interval(8)
        // reward_per_share(8) extra_reward(8)
        read_pubkey(data, 8, "authority", &mut map);
        read_u32(data, 40, "min_reward_count", &mut map);
        read_u32(data, 44, "max_reward_count", &mut map);
        read_u64(data, 48, "stake_bps_high", &mut map);
        read_u64(data, 56, "stake_bps_low", &mut map);
        read_i64(data, 64, "decay_ms", &mut map);
        read_pubkey(data, 72, "treasury", &mut map);
        read_pubkey(data, 104, "quest_authority", &mut map);
        read_i64(data, 136, "min_quest_interval", &mut map);
        read_u64(data, 144, "reward_per_share", &mut map);
        read_u64(data, 152, "extra_reward", &mut map);
        read_pubkey(data, 160, "stake_authority", &mut map);
        read_u64(data, 192, "airdrop_amount", &mut map);
        read_u32(data, 200, "max_airdrop_count", &mut map);
    } else if program_id == &agent_id {
        // ProgramConfig (bytemuck repr(C), after 8-byte discriminator)
        read_pubkey(data, 8, "admin", &mut map);
        read_pubkey(data, 40, "fee_vault", &mut map);
        read_pubkey(data, 72, "point_mint", &mut map);
        read_pubkey(data, 104, "referee_mint", &mut map);
        read_pubkey(data, 136, "referee_activity_mint", &mut map);
        read_u64(data, 168, "register_fee", &mut map);
        read_u64(data, 176, "points_self", &mut map);
        read_u64(data, 184, "points_referral", &mut map);
        read_u64(data, 192, "referral_discount_bps", &mut map);
        read_u64(data, 200, "referral_share_bps", &mut map);
        read_u64(data, 208, "referral_register_points", &mut map);
        read_u64(data, 216, "activity_reward", &mut map);
        read_u64(data, 224, "referral_activity_reward", &mut map);
        read_pubkey(data, 232, "twitter_verifier", &mut map);
        read_u64(data, 264, "twitter_verification_fee", &mut map);
        read_u64(data, 272, "twitter_verification_reward", &mut map);
        read_u64(data, 280, "twitter_verification_points", &mut map);
        read_u64(data, 288, "tweet_verify_reward", &mut map);
        read_u64(data, 296, "tweet_verify_points", &mut map);
        read_u64(data, 304, "register_fee_7", &mut map);
        read_u64(data, 312, "register_fee_6", &mut map);
        read_u64(data, 320, "register_fee_5", &mut map);
    } else if program_id == &skills_id {
        // ProgramConfig (bytemuck repr(C))
        read_pubkey(data, 8, "admin", &mut map);
        read_u64(data, 40, "register_fee", &mut map);
        read_pubkey(data, 48, "fee_vault", &mut map);
    } else if program_id == &zk_id {
        // ConfigAccount (bytemuck repr(C))
        read_pubkey(data, 8, "admin", &mut map);
        read_pubkey(data, 40, "fee_vault", &mut map);
        read_u64(data, 72, "fee_amount", &mut map);
    }

    map
}

fn read_pubkey(data: &[u8], offset: usize, name: &str, map: &mut HashMap<String, String>) {
    if data.len() >= offset + 32 {
        let bytes: [u8; 32] = data[offset..offset + 32].try_into().unwrap();
        map.insert(name.to_string(), bs58::encode(bytes).into_string());
    }
}

fn read_u64_val(data: &[u8], offset: usize) -> Option<u64> {
    if data.len() >= offset + 8 {
        Some(u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()))
    } else {
        None
    }
}

fn read_u64(data: &[u8], offset: usize, name: &str, map: &mut HashMap<String, String>) {
    if let Some(v) = read_u64_val(data, offset) {
        map.insert(name.to_string(), v.to_string());
    }
}

fn read_i64(data: &[u8], offset: usize, name: &str, map: &mut HashMap<String, String>) {
    if data.len() >= offset + 8 {
        let v = i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
        map.insert(name.to_string(), v.to_string());
    }
}

fn read_u32(data: &[u8], offset: usize, name: &str, map: &mut HashMap<String, String>) {
    if data.len() >= offset + 4 {
        let v = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
        map.insert(name.to_string(), v.to_string());
    }
}

/// 获取程序 config PDA 的种子
fn config_seeds(program_id: &[u8; 32]) -> &'static [u8] {
    let quest_id = crate::nara_quest::ID.to_bytes();
    if program_id == &quest_id {
        b"quest_config"
    } else {
        b"config"
    }
}

/// 从链上获取程序 config 的当前值
pub async fn fetch_program_config_values(
    client: &reqwest::Client,
    rpc_url: &str,
    program_id: &[u8; 32],
) -> Result<HashMap<String, String>, String> {
    let pid = Pubkey::new_from_array(*program_id);
    let seeds = config_seeds(program_id);
    let (config_pda, _) = Pubkey::find_program_address(&[seeds], &pid);

    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "getAccountInfo",
        "params": [config_pda.to_string(), {"encoding": "base64", "commitment": "confirmed"}],
    });

    let resp: serde_json::Value = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("RPC 请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))?;

    let data_arr = resp["result"]["value"]["data"]
        .as_array()
        .ok_or("无法获取 config 账户数据")?;
    let b64 = data_arr[0].as_str().ok_or("无效的 base64 数据")?;
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 解码失败: {e}"))?;

    Ok(parse_config_values(program_id, &raw))
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
