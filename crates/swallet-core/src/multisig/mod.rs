pub mod presets;
pub mod proposals;
pub mod squads;

pub use solana_sdk::pubkey::Pubkey;

/// PDA seed 前缀
const SEED_PREFIX: &[u8] = b"multisig";
const SEED_VAULT: &[u8] = b"vault";
const SEED_PROPOSAL: &[u8] = b"proposal";
const SEED_TRANSACTION: &[u8] = b"transaction";

/// 多签信息（从链上解析）
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MultisigInfo {
    pub address: Pubkey,
    pub create_key: Pubkey,
    pub config_authority: Pubkey,
    pub threshold: u16,
    pub time_lock: u32,
    pub transaction_index: u64,
    pub stale_transaction_index: u64,
    pub rent_collector: Option<Pubkey>,
    pub bump: u8,
    pub members: Vec<MultisigMember>,
}

/// 多签成员
#[derive(Debug, Clone)]
pub struct MultisigMember {
    pub key: Pubkey,
    pub permissions: u8,
}

impl MultisigMember {
    pub fn can_initiate(&self) -> bool {
        self.permissions & 1 != 0
    }
    pub fn can_vote(&self) -> bool {
        self.permissions & 2 != 0
    }
    pub fn can_execute(&self) -> bool {
        self.permissions & 4 != 0
    }

    pub fn address(&self) -> String {
        self.key.to_string()
    }

    pub fn permission_label(&self) -> String {
        let mut parts = Vec::new();
        if self.can_initiate() {
            parts.push("发起");
        }
        if self.can_vote() {
            parts.push("投票");
        }
        if self.can_execute() {
            parts.push("执行");
        }
        if parts.is_empty() {
            "无权限".to_string()
        } else {
            parts.join("+")
        }
    }
}

/// 提案状态
#[derive(Debug, Clone, PartialEq)]
pub enum ProposalStatus {
    Draft,
    Active,
    Rejected,
    Approved,
    Executing,
    Executed,
    Cancelled,
}

impl ProposalStatus {
    pub fn label(&self) -> &str {
        match self {
            Self::Draft => "草案",
            Self::Active => "投票中",
            Self::Rejected => "已拒绝",
            Self::Approved => "已通过",
            Self::Executing => "执行中",
            Self::Executed => "已执行",
            Self::Cancelled => "已取消",
        }
    }
}

/// 提案信息（从链上解析）
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProposalInfo {
    pub address: Pubkey,
    pub multisig: Pubkey,
    pub transaction_index: u64,
    pub status: ProposalStatus,
    pub approved: Vec<Pubkey>,
    pub rejected: Vec<Pubkey>,
    pub cancelled: Vec<Pubkey>,
    pub bump: u8,
}

/// 提案类型（用户选择要创建的提案）
#[derive(Debug, Clone, PartialEq)]
pub enum ProposalType {
    /// SOL 转账（从 vault 转出 SOL）
    SolTransfer,
    /// SPL Token 转账（从 vault 转出代币）
    TokenTransfer,
    /// BPF Loader Upgradeable 程序升级
    ProgramUpgrade,
    /// 调用预制程序指令
    ProgramCall,
    /// Vote 账户管理（vault 作为 voter/withdrawer 权限）
    VoteManage,
    /// Stake 账户管理（vault 作为 staker/withdrawer 权限）
    StakeManage,
}

impl ProposalType {
    pub fn label(&self) -> &str {
        match self {
            Self::SolTransfer => "原生币转账",
            Self::TokenTransfer => "代币转账",
            Self::ProgramUpgrade => "升级程序",
            Self::ProgramCall => "调用程序",
            Self::VoteManage => "Vote 账户管理",
            Self::StakeManage => "Stake 账户管理",
        }
    }

    /// 根据链过滤可用提案类型（无预制程序时隐藏 ProgramCall）
    pub fn for_chain(chain_id: &str) -> Vec<Self> {
        let mut types = vec![
            Self::SolTransfer,
            Self::TokenTransfer,
            Self::ProgramUpgrade,
            Self::VoteManage,
            Self::StakeManage,
        ];
        if !presets::programs_for_chain(chain_id).is_empty() {
            types.push(Self::ProgramCall);
        }
        types
    }
}

/// Vote/Stake 多签提案操作类型
#[derive(Debug, Clone, PartialEq)]
pub enum MsVoteStakeOp {
    VoteAuthorizeVoter,
    VoteAuthorizeWithdrawer,
    VoteWithdraw,
    StakeAuthorizeStaker,
    StakeAuthorizeWithdrawer,
    StakeDelegate,
    StakeDeactivate,
    StakeWithdraw,
}

impl MsVoteStakeOp {
    pub fn label(&self) -> &str {
        match self {
            Self::VoteAuthorizeVoter => "修改 Voter 权限",
            Self::VoteAuthorizeWithdrawer => "修改 Withdrawer 权限",
            Self::VoteWithdraw => "提取 (Withdraw)",
            Self::StakeAuthorizeStaker => "修改 Staker 权限",
            Self::StakeAuthorizeWithdrawer => "修改 Withdrawer 权限",
            Self::StakeDelegate => "委托 (Delegate)",
            Self::StakeDeactivate => "取消质押 (Deactivate)",
            Self::StakeWithdraw => "提取 (Withdraw)",
        }
    }

    pub fn vote_ops() -> Vec<Self> {
        vec![Self::VoteAuthorizeVoter, Self::VoteAuthorizeWithdrawer, Self::VoteWithdraw]
    }

    pub fn stake_ops() -> Vec<Self> {
        vec![
            Self::StakeAuthorizeStaker,
            Self::StakeAuthorizeWithdrawer,
            Self::StakeDelegate,
            Self::StakeDeactivate,
            Self::StakeWithdraw,
        ]
    }

    /// 是否需要输入第二个参数（new authority / vote account / to address）
    pub fn needs_param(&self) -> bool {
        !matches!(self, Self::StakeDeactivate)
    }

    /// 第二个参数的提示文字
    pub fn param_label(&self) -> &str {
        match self {
            Self::VoteAuthorizeVoter => "新 Voter 地址",
            Self::VoteAuthorizeWithdrawer => "新 Withdrawer 地址",
            Self::VoteWithdraw => "提取到地址",
            Self::StakeAuthorizeStaker => "新 Staker 地址",
            Self::StakeAuthorizeWithdrawer => "新 Withdrawer 地址",
            Self::StakeDelegate => "Vote 账户地址",
            Self::StakeWithdraw => "提取到地址",
            Self::StakeDeactivate => "",
        }
    }

    /// 是否需要输入金额
    pub fn needs_amount(&self) -> bool {
        matches!(self, Self::VoteWithdraw | Self::StakeWithdraw)
    }
}

// ========== PDA 推导 ==========

/// 推导 ProgramConfig PDA
pub fn derive_program_config_pda() -> (Pubkey, u8) {
    let program_id = crate::squads_multisig_program::ID;
    Pubkey::find_program_address(
        &[SEED_PREFIX, b"program_config"],
        &program_id,
    )
}

/// 推导 Multisig PDA
pub fn derive_multisig_pda(create_key: &Pubkey) -> (Pubkey, u8) {
    let program_id = crate::squads_multisig_program::ID;
    Pubkey::find_program_address(
        &[SEED_PREFIX, SEED_PREFIX, create_key.as_ref()],
        &program_id,
    )
}

/// 推导 Vault PDA
pub fn derive_vault_pda(multisig_pda: &Pubkey, vault_index: u8) -> (Pubkey, u8) {
    let program_id = crate::squads_multisig_program::ID;
    Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_VAULT, &[vault_index]],
        &program_id,
    )
}

/// 推导 Proposal PDA
pub fn derive_proposal_pda(
    multisig_pda: &Pubkey,
    transaction_index: u64,
) -> (Pubkey, u8) {
    let program_id = crate::squads_multisig_program::ID;
    let tx_index_bytes = transaction_index.to_le_bytes();
    Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_TRANSACTION, &tx_index_bytes, SEED_PROPOSAL],
        &program_id,
    )
}

/// 推导 Transaction PDA
pub fn derive_transaction_pda(
    multisig_pda: &Pubkey,
    transaction_index: u64,
) -> (Pubkey, u8) {
    let program_id = crate::squads_multisig_program::ID;
    let tx_index_bytes = transaction_index.to_le_bytes();
    Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_TRANSACTION, &tx_index_bytes],
        &program_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_pda_derivation() {
        let fake_create_key = Pubkey::new_from_array([1u8; 32]);
        let (multisig_pda, _bump) = derive_multisig_pda(&fake_create_key);

        let (vault_pda, _bump) = derive_vault_pda(&multisig_pda, 0);
        assert_ne!(vault_pda, Pubkey::default());

        let (proposal_pda, _bump) = derive_proposal_pda(&multisig_pda, 1);
        assert_ne!(proposal_pda, Pubkey::default());

        let (tx_pda, _bump) = derive_transaction_pda(&multisig_pda, 1);
        assert_ne!(tx_pda, Pubkey::default());
    }

    #[test]
    fn test_member_permissions() {
        let member = MultisigMember {
            key: Pubkey::default(),
            permissions: 7, // All permissions
        };
        assert!(member.can_initiate());
        assert!(member.can_vote());
        assert!(member.can_execute());
        assert_eq!(member.permission_label(), "发起+投票+执行");

        let voter = MultisigMember {
            key: Pubkey::default(),
            permissions: 2,
        };
        assert!(!voter.can_initiate());
        assert!(voter.can_vote());
        assert!(!voter.can_execute());
    }
}
