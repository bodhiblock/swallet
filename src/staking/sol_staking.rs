use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

use crate::chain::solana::rpc_call;
use crate::transfer::sol_transfer::{
    build_and_serialize_message, build_transaction, get_latest_blockhash, send_transaction,
    AccountMeta, Instruction,
};

use super::{StakeAccountInfo, VoteAccountInfo};

const SYSTEM_PROGRAM: [u8; 32] = [0u8; 32];

const VOTE_PROGRAM_STR: &str = "Vote111111111111111111111111111111111111111";
const STAKE_PROGRAM_STR: &str = "Stake11111111111111111111111111111111111111";
const SYSVAR_CLOCK: &str = "SysvarC1ock11111111111111111111111111111111";
const SYSVAR_RENT: &str = "SysvarRent111111111111111111111111111111111";
const SYSVAR_STAKE_HISTORY: &str = "SysvarStakeHistory1111111111111111111111111";
const STAKE_CONFIG: &str = "StakeConfig11111111111111111111111111111111";

fn decode_pubkey(s: &str) -> Result<[u8; 32], String> {
    let bytes = bs58::decode(s)
        .into_vec()
        .map_err(|e| format!("无效的 Base58 地址 {s}: {e}"))?;
    bytes
        .try_into()
        .map_err(|_| format!("地址长度无效: {s}"))
}

fn keypair_from_private_key(private_key: &[u8]) -> Result<Keypair, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度无效".to_string())?;
    Ok(Keypair::new_from_array(key_bytes))
}

/// 获取 getMinimumBalanceForRentExemption
async fn get_rent_exemption(client: &Client, rpc_url: &str, space: u64) -> Result<u64, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getMinimumBalanceForRentExemption",
        "params": [space],
        "id": 1
    });
    let resp = rpc_call(client, rpc_url, &body).await?;
    resp.get("result")
        .and_then(|v| v.as_u64())
        .ok_or("获取 rent 失败".to_string())
}

/// 签名并发送交易（单签名者）
async fn sign_and_send(
    client: &Client,
    rpc_url: &str,
    keypair: &Keypair,
    instructions: &[Instruction],
) -> Result<String, String> {
    let from_pubkey = keypair.pubkey().to_bytes();
    let recent_blockhash = get_latest_blockhash(client, rpc_url).await?;
    let message_bytes = build_and_serialize_message(&from_pubkey, &recent_blockhash, instructions);
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = build_transaction(&[sig_bytes], &message_bytes);
    send_transaction(client, rpc_url, &tx_bytes).await
}

/// 签名并发送交易（双签名者：fee_payer + second）
async fn sign_and_send_two(
    client: &Client,
    rpc_url: &str,
    fee_payer: &Keypair,
    second_signer: &Keypair,
    instructions: &[Instruction],
) -> Result<String, String> {
    let payer_pubkey = fee_payer.pubkey().to_bytes();
    let recent_blockhash = get_latest_blockhash(client, rpc_url).await?;
    let message_bytes =
        build_and_serialize_message(&payer_pubkey, &recent_blockhash, instructions);

    let sig1 = fee_payer.sign_message(&message_bytes);
    let sig2 = second_signer.sign_message(&message_bytes);
    let mut sig1_bytes = [0u8; 64];
    let mut sig2_bytes = [0u8; 64];
    sig1_bytes.copy_from_slice(sig1.as_ref());
    sig2_bytes.copy_from_slice(sig2.as_ref());

    let tx_bytes = build_transaction(&[sig1_bytes, sig2_bytes], &message_bytes);
    send_transaction(client, rpc_url, &tx_bytes).await
}

/// 签名并发送交易（三签名者：fee_payer + account + identity）
async fn sign_and_send_three(
    client: &Client,
    rpc_url: &str,
    fee_payer: &Keypair,
    second: &Keypair,
    third: &Keypair,
    instructions: &[Instruction],
) -> Result<String, String> {
    let payer_pubkey = fee_payer.pubkey().to_bytes();
    let recent_blockhash = get_latest_blockhash(client, rpc_url).await?;
    let message_bytes =
        build_and_serialize_message(&payer_pubkey, &recent_blockhash, instructions);

    let sig1 = fee_payer.sign_message(&message_bytes);
    let sig2 = second.sign_message(&message_bytes);
    let sig3 = third.sign_message(&message_bytes);
    let mut sig1_bytes = [0u8; 64];
    let mut sig2_bytes = [0u8; 64];
    let mut sig3_bytes = [0u8; 64];
    sig1_bytes.copy_from_slice(sig1.as_ref());
    sig2_bytes.copy_from_slice(sig2.as_ref());
    sig3_bytes.copy_from_slice(sig3.as_ref());

    let tx_bytes = build_transaction(&[sig1_bytes, sig2_bytes, sig3_bytes], &message_bytes);
    send_transaction(client, rpc_url, &tx_bytes).await
}

// ========== 创建 Vote 账户 ==========

/// 创建 Vote Account
/// - account_key: 新 Vote 账户的 32 字节私钥（空地址）
/// - fee_payer_key: Fee Payer 的 32 字节私钥（有余额的地址）
/// - identity_bs58: Validator Identity 的 bs58 私钥
/// - withdrawer: Withdrawer 地址（bs58）
pub async fn create_vote_account(
    client: &Client,
    rpc_url: &str,
    account_key: &[u8],
    fee_payer_key: &[u8],
    identity_bs58: &str,
    withdrawer: &str,
) -> Result<String, String> {
    let account_keypair = keypair_from_private_key(account_key)?;
    let fee_payer_keypair = keypair_from_private_key(fee_payer_key)?;
    let vote_pubkey = account_keypair.pubkey().to_bytes();
    let fee_payer_pubkey = fee_payer_keypair.pubkey().to_bytes();

    // 解析 identity 私钥
    let identity_bytes = bs58::decode(identity_bs58)
        .into_vec()
        .map_err(|e| format!("无效的 Identity 私钥: {e}"))?;
    let identity_key: [u8; 32] = match identity_bytes.len() {
        64 => identity_bytes[..32]
            .try_into()
            .map_err(|_| "Identity 私钥无效".to_string())?,
        32 => identity_bytes
            .try_into()
            .map_err(|_| "Identity 私钥无效".to_string())?,
        _ => return Err("Identity 私钥长度无效（需要 32 或 64 字节）".to_string()),
    };
    let identity_keypair = Keypair::new_from_array(identity_key);
    let identity_pubkey = identity_keypair.pubkey().to_bytes();

    let withdrawer_pubkey = decode_pubkey(withdrawer)?;
    let vote_program = decode_pubkey(VOTE_PROGRAM_STR)?;

    // Vote Account space = 3762
    let rent = get_rent_exemption(client, rpc_url, 3762).await?;

    // 指令1: System CreateAccount (fee_payer 出钱，vote_pubkey 为新账户)
    let create_account_ix = build_create_account_ix(
        &fee_payer_pubkey,
        &vote_pubkey,
        rent,
        3762,
        &vote_program,
    );

    // 指令2: Vote InitializeAccount (index 0)
    // data: [0,0,0,0] + identity(32) + voter(32) + withdrawer(32) + commission(1)
    let mut init_data = vec![0u8, 0, 0, 0]; // instruction index 0
    init_data.extend_from_slice(&identity_pubkey);
    init_data.extend_from_slice(&identity_pubkey); // voter = identity
    init_data.extend_from_slice(&withdrawer_pubkey);
    init_data.push(100); // commission = 100%

    let rent_sysvar = decode_pubkey(SYSVAR_RENT)?;
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;

    let init_vote_ix = Instruction {
        program_id: vote_program,
        accounts: vec![
            AccountMeta {
                pubkey: vote_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: identity_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ],
        data: init_data,
    };

    // 三个签名者: fee_payer + vote account + identity
    sign_and_send_three(
        client,
        rpc_url,
        &fee_payer_keypair,
        &account_keypair,
        &identity_keypair,
        &[create_account_ix, init_vote_ix],
    )
    .await
}

// ========== 创建 Stake 账户 ==========

/// 创建 Stake Account
/// - account_key: 新 Stake 账户的 32 字节私钥（空地址）
/// - fee_payer_key: Fee Payer 的 32 字节私钥（有余额的地址）
/// - amount_sol: 质押数量（SOL，字符串格式）
pub async fn create_stake_account(
    client: &Client,
    rpc_url: &str,
    account_key: &[u8],
    fee_payer_key: &[u8],
    amount_sol: &str,
    lockup_days: u64,
) -> Result<String, String> {
    let account_keypair = keypair_from_private_key(account_key)?;
    let fee_payer_keypair = keypair_from_private_key(fee_payer_key)?;
    let stake_pubkey = account_keypair.pubkey().to_bytes();
    let fee_payer_pubkey = fee_payer_keypair.pubkey().to_bytes();

    let amount_lamports = parse_sol_amount(amount_sol)?;
    let stake_program = decode_pubkey(STAKE_PROGRAM_STR)?;

    // Stake Account space = 200
    let rent = get_rent_exemption(client, rpc_url, 200).await?;
    let total_lamports = rent + amount_lamports;

    // 指令1: System CreateAccount (fee_payer 出钱，stake_pubkey 为新账户)
    let create_account_ix = build_create_account_ix(
        &fee_payer_pubkey,
        &stake_pubkey,
        total_lamports,
        200,
        &stake_program,
    );

    // 指令2: Stake Initialize (index 0)
    // data: [0,0,0,0] + Authorized{staker(32)+withdrawer(32)} + Lockup{timestamp(8)+epoch(8)+custodian(32)}
    let mut init_data = vec![0u8, 0, 0, 0]; // instruction index 0 = Initialize
    init_data.extend_from_slice(&stake_pubkey); // staker = stake account
    init_data.extend_from_slice(&stake_pubkey); // withdrawer = stake account
    // Lockup
    let lockup_timestamp = if lockup_days > 0 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now + (lockup_days as i64) * 86400
    } else {
        0i64
    };
    init_data.extend_from_slice(&lockup_timestamp.to_le_bytes()); // lockup.unix_timestamp
    init_data.extend_from_slice(&0u64.to_le_bytes()); // lockup.epoch = 0
    init_data.extend_from_slice(&[0u8; 32]); // lockup.custodian = default

    let rent_sysvar = decode_pubkey(SYSVAR_RENT)?;

    let init_stake_ix = Instruction {
        program_id: stake_program,
        accounts: vec![
            AccountMeta {
                pubkey: stake_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: rent_sysvar,
                is_signer: false,
                is_writable: false,
            },
        ],
        data: init_data,
    };

    // 两个签名者: fee_payer + stake account
    sign_and_send_two(
        client,
        rpc_url,
        &fee_payer_keypair,
        &account_keypair,
        &[create_account_ix, init_stake_ix],
    )
    .await
}

// ========== Vote Authorize ==========

/// 修改 Vote 账户权限
/// - authorize_type: 0=Voter, 1=Withdrawer
pub async fn vote_authorize(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    new_authority: &str,
    authorize_type: u32,
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let vote_pubkey = keypair.pubkey().to_bytes();
    let new_authority_pubkey = decode_pubkey(new_authority)?;
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;
    let vote_program = decode_pubkey(VOTE_PROGRAM_STR)?;

    // Vote Authorize (index 1)
    // data: [1,0,0,0] + new_authority(32) + authorize_type(4)
    let mut data = vec![1u8, 0, 0, 0];
    data.extend_from_slice(&new_authority_pubkey);
    data.extend_from_slice(&authorize_type.to_le_bytes());

    let ix = Instruction {
        program_id: vote_program,
        accounts: vec![
            AccountMeta {
                pubkey: vote_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: vote_pubkey, // current authority = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== Vote Withdraw ==========

/// 从 Vote 账户提取
pub async fn vote_withdraw(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    to_address: &str,
    amount_sol: &str,
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let vote_pubkey = keypair.pubkey().to_bytes();
    let to_pubkey = decode_pubkey(to_address)?;
    let vote_program = decode_pubkey(VOTE_PROGRAM_STR)?;

    let amount_lamports = parse_sol_amount(amount_sol)?;

    // Vote Withdraw (index 3)
    // data: [3,0,0,0] + lamports(8)
    let mut data = vec![3u8, 0, 0, 0];
    data.extend_from_slice(&amount_lamports.to_le_bytes());

    let ix = Instruction {
        program_id: vote_program,
        accounts: vec![
            AccountMeta {
                pubkey: vote_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: to_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_pubkey, // authorized withdrawer = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== Stake Authorize ==========

/// 修改 Stake 账户权限
/// - authorize_type: 0=Staker, 1=Withdrawer
pub async fn stake_authorize(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    new_authority: &str,
    authorize_type: u32,
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let stake_pubkey = keypair.pubkey().to_bytes();
    let new_authority_pubkey = decode_pubkey(new_authority)?;
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;
    let stake_program = decode_pubkey(STAKE_PROGRAM_STR)?;

    // Stake Authorize (index 1)
    // data: [1,0,0,0] + new_authority(32) + stake_authorize_type(4)
    let mut data = vec![1u8, 0, 0, 0];
    data.extend_from_slice(&new_authority_pubkey);
    data.extend_from_slice(&authorize_type.to_le_bytes());

    let ix = Instruction {
        program_id: stake_program,
        accounts: vec![
            AccountMeta {
                pubkey: stake_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_pubkey, // current authority = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== Stake Delegate ==========

/// 委托质押到 Vote Account
pub async fn stake_delegate(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    vote_account: &str,
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let stake_pubkey = keypair.pubkey().to_bytes();
    let vote_pubkey = decode_pubkey(vote_account)?;
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;
    let stake_history = decode_pubkey(SYSVAR_STAKE_HISTORY)?;
    let stake_config = decode_pubkey(STAKE_CONFIG)?;
    let stake_program = decode_pubkey(STAKE_PROGRAM_STR)?;

    // Stake DelegateStake (index 2)
    let data = vec![2u8, 0, 0, 0];

    let ix = Instruction {
        program_id: stake_program,
        accounts: vec![
            AccountMeta {
                pubkey: stake_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: vote_pubkey,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_config,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_pubkey, // staker authority = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== Stake Deactivate ==========

/// 取消质押
pub async fn stake_deactivate(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let stake_pubkey = keypair.pubkey().to_bytes();
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;
    let stake_program = decode_pubkey(STAKE_PROGRAM_STR)?;

    // Stake Deactivate (index 5)
    let data = vec![5u8, 0, 0, 0];

    let ix = Instruction {
        program_id: stake_program,
        accounts: vec![
            AccountMeta {
                pubkey: stake_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_pubkey, // staker authority = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== Stake Withdraw ==========

/// 提取质押
/// - to_address: 提取目标地址
/// - amount_sol: 提取数量（SOL）
pub async fn stake_withdraw(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    to_address: &str,
    amount_sol: &str,
) -> Result<String, String> {
    let keypair = keypair_from_private_key(private_key)?;
    let stake_pubkey = keypair.pubkey().to_bytes();
    let to_pubkey = decode_pubkey(to_address)?;
    let clock_sysvar = decode_pubkey(SYSVAR_CLOCK)?;
    let stake_history = decode_pubkey(SYSVAR_STAKE_HISTORY)?;
    let stake_program = decode_pubkey(STAKE_PROGRAM_STR)?;

    let amount_lamports = parse_sol_amount(amount_sol)?;

    // Stake Withdraw (index 4)
    // data: [4,0,0,0] + lamports(8)
    let mut data = vec![4u8, 0, 0, 0];
    data.extend_from_slice(&amount_lamports.to_le_bytes());

    let ix = Instruction {
        program_id: stake_program,
        accounts: vec![
            AccountMeta {
                pubkey: stake_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: to_pubkey,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_history,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: stake_pubkey, // withdrawer authority = self
                is_signer: true,
                is_writable: false,
            },
        ],
        data,
    };

    sign_and_send(client, rpc_url, &keypair, &[ix]).await
}

// ========== 辅助函数 ==========

/// System Program CreateAccount 指令
fn build_create_account_ix(
    from: &[u8; 32],
    new_account: &[u8; 32],
    lamports: u64,
    space: u64,
    owner: &[u8; 32],
) -> Instruction {
    // System CreateAccount: index 0
    // data: [0,0,0,0] + lamports(8) + space(8) + owner(32)
    let mut data = vec![0u8, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());
    data.extend_from_slice(&space.to_le_bytes());
    data.extend_from_slice(owner);

    Instruction {
        program_id: SYSTEM_PROGRAM,
        accounts: vec![
            AccountMeta {
                pubkey: *from,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: *new_account,
                is_signer: true,
                is_writable: true,
            },
        ],
        data,
    }
}

/// 解析 SOL 数量字符串为 lamports
fn parse_sol_amount(amount_str: &str) -> Result<u64, String> {
    let amount: f64 = amount_str
        .parse()
        .map_err(|_| format!("无效的数量: {amount_str}"))?;
    if amount <= 0.0 {
        return Err("数量必须大于 0".to_string());
    }
    Ok((amount * 1_000_000_000.0) as u64)
}

// ========== 查询函数 ==========

/// 使用 getAccountInfo + jsonParsed 查询 Vote 账户详情
pub async fn fetch_vote_account(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<VoteAccountInfo, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "jsonParsed", "commitment": "confirmed"}],
        "id": 1
    });

    let resp: Value = rpc_call(client, rpc_url, &body).await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"));

    // value 为 null 说明账户不存在
    let value = match value {
        Some(v) if !v.is_null() => v,
        _ => return Err(format!("账户不存在或无法访问 (rpc: {rpc_url})")),
    };

    let info = value
        .get("data")
        .and_then(|d| d.get("parsed"))
        .and_then(|p| p.get("info"))
        .ok_or_else(|| {
            let owner = value.get("owner").and_then(|o| o.as_str()).unwrap_or("unknown");
            format!("无法解析 Vote 账户数据 (owner: {owner}, rpc: {rpc_url})")
        })?;

    let authorized = info.get("authorizedVoters").and_then(|v| v.as_array());
    let voter = authorized
        .and_then(|arr| arr.last())
        .and_then(|v| v.get("authorizedVoter"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let withdrawer = info
        .get("authorizedWithdrawer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let identity = info
        .get("nodePubkey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let commission = info
        .get("commission")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u8;

    let epoch_credits = info
        .get("epochCredits")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| {
                    let e = entry.as_array()?;
                    Some((
                        e.first()?.as_u64()?,
                        e.get(1)?.as_u64()?,
                        e.get(2)?.as_u64()?,
                    ))
                })
                .collect()
        })
        .unwrap_or_default();

    let last_timestamp_slot = info
        .get("lastTimestamp")
        .and_then(|v| v.get("slot"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    Ok(VoteAccountInfo {
        address: address.to_string(),
        validator_identity: identity,
        authorized_voter: voter,
        authorized_withdrawer: withdrawer,
        commission,
        epoch_credits,
        last_timestamp_slot,
    })
}

/// 使用 getAccountInfo + jsonParsed 查询 Stake 账户详情
pub async fn fetch_stake_account(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<StakeAccountInfo, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "jsonParsed", "commitment": "confirmed"}],
        "id": 1
    });

    let resp: Value = rpc_call(client, rpc_url, &body).await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"));
    let value = match value {
        Some(v) if !v.is_null() => v,
        _ => return Err(format!("账户不存在或无法访问 (rpc: {rpc_url})")),
    };

    let lamports = value
        .get("lamports")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let info = value
        .get("data")
        .and_then(|d| d.get("parsed"))
        .and_then(|p| p.get("info"))
        .ok_or("无法解析 Stake 账户数据")?;

    let state_type = value
        .get("data")
        .and_then(|d| d.get("parsed"))
        .and_then(|p| p.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("unknown")
        .to_string();

    let meta = info.get("meta");
    let authorized_staker = meta
        .and_then(|m| m.get("authorized"))
        .and_then(|a| a.get("staker"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let authorized_withdrawer = meta
        .and_then(|m| m.get("authorized"))
        .and_then(|a| a.get("withdrawer"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let stake = info.get("stake");
    let delegated_vote_account = stake
        .and_then(|s| s.get("delegation"))
        .and_then(|d| d.get("voter"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let activation_epoch = stake
        .and_then(|s| s.get("delegation"))
        .and_then(|d| d.get("activationEpoch"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    let deactivation_epoch = stake
        .and_then(|s| s.get("delegation"))
        .and_then(|d| d.get("deactivationEpoch"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    let lockup = meta.and_then(|m| m.get("lockup"));
    let lockup_timestamp = lockup
        .and_then(|l| l.get("unixTimestamp"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let lockup_epoch = lockup
        .and_then(|l| l.get("epoch"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let lockup_custodian = lockup
        .and_then(|l| l.get("custodian"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(StakeAccountInfo {
        address: address.to_string(),
        state: state_type,
        delegated_vote_account,
        stake_lamports: lamports,
        authorized_staker,
        authorized_withdrawer,
        activation_epoch,
        deactivation_epoch,
        lockup_timestamp,
        lockup_epoch,
        lockup_custodian,
    })
}
