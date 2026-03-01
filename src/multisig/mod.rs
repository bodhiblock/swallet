pub mod proposals;
pub mod squads;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Squads v4 程序 ID
pub const SQUADS_V4_PROGRAM_ID: &str = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";

/// PDA seed 前缀
const SEED_PREFIX: &[u8] = b"multisig";
const SEED_MULTISIG: &[u8] = b"multisig";
const SEED_VAULT: &[u8] = b"vault";
const SEED_PROPOSAL: &[u8] = b"proposal";
const SEED_TRANSACTION: &[u8] = b"transaction";

/// 多签信息（从链上解析）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigInfo {
    pub address: String,
    pub create_key: [u8; 32],
    pub config_authority: [u8; 32],
    pub threshold: u16,
    pub time_lock: u32,
    pub transaction_index: u64,
    pub stale_transaction_index: u64,
    pub rent_collector: Option<[u8; 32]>,
    pub bump: u8,
    pub members: Vec<MultisigMember>,
}

/// 多签成员
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigMember {
    pub key: [u8; 32],
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
        bs58::encode(&self.key).into_string()
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalInfo {
    pub address: String,
    pub multisig: [u8; 32],
    pub transaction_index: u64,
    pub status: ProposalStatus,
    pub approved: Vec<[u8; 32]>,
    pub rejected: Vec<[u8; 32]>,
    pub cancelled: Vec<[u8; 32]>,
    pub bump: u8,
}

/// 提案类型（用户选择要创建的提案）
#[derive(Debug, Clone, PartialEq)]
pub enum ProposalType {
    /// SOL 转账（从 vault 转出 SOL）
    SolTransfer,
    /// SPL Token 转账（从 vault 转出代币）
    TokenTransfer,
}

impl ProposalType {
    pub fn label(&self) -> &str {
        match self {
            Self::SolTransfer => "SOL 转账",
            Self::TokenTransfer => "代币转账",
        }
    }

    pub fn all() -> Vec<Self> {
        vec![Self::SolTransfer, Self::TokenTransfer]
    }
}

// ========== PDA 推导 ==========

/// 获取 Squads 程序 ID 的 pubkey 字节
pub fn squads_program_id() -> [u8; 32] {
    bs58::decode(SQUADS_V4_PROGRAM_ID)
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap()
}

/// 推导 Multisig PDA
/// Seeds: ["multisig", "multisig", create_key]
#[allow(dead_code)]
pub fn derive_multisig_pda(create_key: &[u8; 32]) -> Result<([u8; 32], u8), String> {
    let program_id = squads_program_id();
    find_program_address(
        &[SEED_PREFIX, SEED_MULTISIG, create_key],
        &program_id,
    )
}

/// 推导 Vault PDA
/// Seeds: ["multisig", multisig_pda, "vault", vault_index_u8]
pub fn derive_vault_pda(multisig_pda: &[u8; 32], vault_index: u8) -> Result<([u8; 32], u8), String> {
    let program_id = squads_program_id();
    find_program_address(
        &[SEED_PREFIX, multisig_pda, SEED_VAULT, &[vault_index]],
        &program_id,
    )
}

/// 推导 Proposal PDA
/// Seeds: ["multisig", multisig_pda, "transaction", tx_index_le, "proposal"]
pub fn derive_proposal_pda(
    multisig_pda: &[u8; 32],
    transaction_index: u64,
) -> Result<([u8; 32], u8), String> {
    let program_id = squads_program_id();
    let tx_index_bytes = transaction_index.to_le_bytes();
    find_program_address(
        &[SEED_PREFIX, multisig_pda, SEED_TRANSACTION, &tx_index_bytes, SEED_PROPOSAL],
        &program_id,
    )
}

/// 推导 Transaction PDA
/// Seeds: ["multisig", multisig_pda, "transaction", tx_index_le]
pub fn derive_transaction_pda(
    multisig_pda: &[u8; 32],
    transaction_index: u64,
) -> Result<([u8; 32], u8), String> {
    let program_id = squads_program_id();
    let tx_index_bytes = transaction_index.to_le_bytes();
    find_program_address(
        &[SEED_PREFIX, multisig_pda, SEED_TRANSACTION, &tx_index_bytes],
        &program_id,
    )
}

/// 通用 PDA 推导（find_program_address）
fn find_program_address(seeds: &[&[u8]], program_id: &[u8; 32]) -> Result<([u8; 32], u8), String> {
    for nonce in (0..=255u8).rev() {
        let mut hasher = Sha256::new();
        for seed in seeds {
            hasher.update(seed);
        }
        hasher.update([nonce]);
        hasher.update(program_id);
        hasher.update(b"ProgramDerivedAddress");
        let hash = hasher.finalize();
        let bytes: [u8; 32] = hash.into();

        // PDA 必须不在 ed25519 曲线上
        if ed25519_dalek::VerifyingKey::from_bytes(&bytes).is_err() {
            return Ok((bytes, nonce));
        }
    }
    Err("无法推导 PDA".into())
}

/// 计算 Anchor 指令判别码: SHA-256("global:<name>")[..8]
pub fn anchor_instruction_discriminator(name: &str) -> [u8; 8] {
    let input = format!("global:{name}");
    let hash = Sha256::digest(input.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

/// 计算 Anchor 账户判别码: SHA-256("account:<Name>")[..8]
pub fn anchor_account_discriminator(name: &str) -> [u8; 8] {
    let input = format!("account:{name}");
    let hash = Sha256::digest(input.as_bytes());
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_pda_derivation() {
        // Verify PDA derivation doesn't panic and returns valid results
        let fake_create_key = [1u8; 32];
        let (multisig_pda, _bump) = derive_multisig_pda(&fake_create_key).unwrap();

        let (vault_pda, _bump) = derive_vault_pda(&multisig_pda, 0).unwrap();
        assert_ne!(vault_pda, [0u8; 32]);

        let (proposal_pda, _bump) = derive_proposal_pda(&multisig_pda, 1).unwrap();
        assert_ne!(proposal_pda, [0u8; 32]);

        let (tx_pda, _bump) = derive_transaction_pda(&multisig_pda, 1).unwrap();
        assert_ne!(tx_pda, [0u8; 32]);
    }

    #[test]
    fn test_anchor_discriminators() {
        let disc = anchor_instruction_discriminator("multisig_create_v2");
        assert_eq!(disc.len(), 8);
        // Discriminators should not be all zeros
        assert!(disc.iter().any(|&b| b != 0));

        let acc_disc = anchor_account_discriminator("Multisig");
        assert!(acc_disc.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_member_permissions() {
        let member = MultisigMember {
            key: [0u8; 32],
            permissions: 7, // All permissions
        };
        assert!(member.can_initiate());
        assert!(member.can_vote());
        assert!(member.can_execute());
        assert_eq!(member.permission_label(), "发起+投票+执行");

        let voter = MultisigMember {
            key: [0u8; 32],
            permissions: 2,
        };
        assert!(!voter.can_initiate());
        assert!(voter.can_vote());
        assert!(!voter.can_execute());
    }
}
