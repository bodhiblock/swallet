pub mod sol_staking;

/// Vote 账户信息
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct VoteAccountInfo {
    pub address: String,
    pub validator_identity: String,
    pub authorized_voter: String,
    pub authorized_withdrawer: String,
    pub commission: u8,
    pub epoch_credits: Vec<(u64, u64, u64)>,
    pub last_timestamp_slot: u64,
}

/// Stake 账户信息
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StakeAccountInfo {
    pub address: String,
    pub state: String,
    pub delegated_vote_account: Option<String>,
    pub stake_lamports: u64,
    pub authorized_staker: String,
    pub authorized_withdrawer: String,
    pub activation_epoch: Option<u64>,
    pub deactivation_epoch: Option<u64>,
    pub lockup_timestamp: i64,
    pub lockup_epoch: u64,
    pub lockup_custodian: String,
}
