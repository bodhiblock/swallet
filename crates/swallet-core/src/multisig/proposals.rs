/// Vault 交易中的一条指令
pub struct VaultInstruction {
    pub program_id: [u8; 32],
    pub accounts: Vec<VaultAccountMeta>,
    pub data: Vec<u8>,
}

/// Vault 交易指令中的账户
pub struct VaultAccountMeta {
    pub pubkey: [u8; 32],
    pub is_signer: bool,
    pub is_writable: bool,
}

/// 构建 SOL 转账的 Vault 指令
pub fn build_sol_transfer_instruction(
    vault_pubkey: &[u8; 32],
    to_pubkey: &[u8; 32],
    lamports: u64,
) -> VaultInstruction {
    let system_program = [0u8; 32];

    // System Program Transfer: index=2 + u64 LE
    let mut data = vec![2, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());

    VaultInstruction {
        program_id: system_program,
        accounts: vec![
            VaultAccountMeta {
                pubkey: *vault_pubkey,
                is_signer: true,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *to_pubkey,
                is_signer: false,
                is_writable: true,
            },
        ],
        data,
    }
}

/// 构建 SPL Token 转账的 Vault 指令（含可选的 ATA 创建）
#[allow(dead_code, clippy::too_many_arguments)]
pub fn build_spl_transfer_instructions(
    vault_pubkey: &[u8; 32],
    to_wallet_pubkey: &[u8; 32],
    mint_pubkey: &[u8; 32],
    amount: u64,
    token_program: &[u8; 32],
    source_ata: &[u8; 32],
    dest_ata: &[u8; 32],
    create_dest_ata: bool,
) -> Vec<VaultInstruction> {
    let ata_program: [u8; 32] = bs58::decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();
    let system_program = [0u8; 32];

    let mut instructions = Vec::new();

    // 如果目标 ATA 不存在，先创建
    if create_dest_ata {
        instructions.push(VaultInstruction {
            program_id: ata_program,
            accounts: vec![
                VaultAccountMeta {
                    pubkey: *vault_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                VaultAccountMeta {
                    pubkey: *dest_ata,
                    is_signer: false,
                    is_writable: true,
                },
                VaultAccountMeta {
                    pubkey: *to_wallet_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
                VaultAccountMeta {
                    pubkey: *mint_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
                VaultAccountMeta {
                    pubkey: system_program,
                    is_signer: false,
                    is_writable: false,
                },
                VaultAccountMeta {
                    pubkey: *token_program,
                    is_signer: false,
                    is_writable: false,
                },
            ],
            data: vec![],
        });
    }

    // SPL Token Transfer: instruction index 3 + u64 amount LE
    let mut transfer_data = vec![3u8];
    transfer_data.extend_from_slice(&amount.to_le_bytes());

    instructions.push(VaultInstruction {
        program_id: *token_program,
        accounts: vec![
            VaultAccountMeta {
                pubkey: *source_ata,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *dest_ata,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *vault_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ],
        data: transfer_data,
    });

    instructions
}

/// 序列化 vault 交易消息（Squads v4 beet 格式）
///
/// Squads v4 使用自定义的 "beet" 序列化格式（非标准 Borsh），
/// 长度前缀使用 u8/u16 而非 Borsh 的 u32：
/// - num_signers (u8), num_writable_signers (u8), num_writable_non_signers (u8)
/// - account_keys: u8 长度 + N * 32 bytes
/// - instructions: u8 长度 + compiled instructions
///   - 每条: program_id_index (u8) + account_indexes (u8 长度 + bytes) + data (u16 LE 长度 + bytes)
/// - address_table_lookups: u8 长度 + lookups
pub fn serialize_vault_transaction_message(
    _vault_index: u8,
    instructions: &[VaultInstruction],
) -> Vec<u8> {
    // 收集所有唯一的账户 key，并确定其角色
    let mut account_keys: Vec<AccountKeyEntry> = Vec::new();

    for ix in instructions {
        for meta in &ix.accounts {
            add_account_key(&mut account_keys, &meta.pubkey, meta.is_signer, meta.is_writable);
        }
        // Program ID 作为只读非签名者
        add_account_key(&mut account_keys, &ix.program_id, false, false);
    }

    // 排序：writable signers, readonly signers, writable non-signers, readonly non-signers
    account_keys.sort_by_key(|k| match (k.is_signer, k.is_writable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    });

    let num_signers = account_keys.iter().filter(|k| k.is_signer).count();
    let num_writable_signers = account_keys.iter().filter(|k| k.is_signer && k.is_writable).count();
    let num_writable_non_signers = account_keys.iter().filter(|k| !k.is_signer && k.is_writable).count();

    // 编译指令（将 pubkey 替换为索引）
    let compiled_instructions: Vec<CompiledVaultInstruction> = instructions
        .iter()
        .map(|ix| {
            let program_id_index = account_keys
                .iter()
                .position(|k| k.pubkey == ix.program_id)
                .unwrap() as u8;
            let account_indices: Vec<u8> = ix
                .accounts
                .iter()
                .map(|m| {
                    account_keys
                        .iter()
                        .position(|k| k.pubkey == m.pubkey)
                        .unwrap() as u8
                })
                .collect();
            CompiledVaultInstruction {
                program_id_index,
                account_indices,
                data: ix.data.clone(),
            }
        })
        .collect();

    // Squads v4 beet 格式序列化（u8/u16 长度前缀）
    let mut buf = vec![
        num_signers as u8,
        num_writable_signers as u8,
        num_writable_non_signers as u8,
        account_keys.len() as u8,
    ];
    for key in &account_keys {
        buf.extend_from_slice(&key.pubkey);
    }

    // instructions: u8 长度前缀 + compiled instructions
    buf.push(compiled_instructions.len() as u8);
    for ix in &compiled_instructions {
        // program_id_index: u8
        buf.push(ix.program_id_index);
        // account_indices: u8 长度前缀 + bytes
        buf.push(ix.account_indices.len() as u8);
        buf.extend_from_slice(&ix.account_indices);
        // data: u16 LE 长度前缀 + bytes
        buf.extend_from_slice(&(ix.data.len() as u16).to_le_bytes());
        buf.extend_from_slice(&ix.data);
    }

    // address_table_lookups: u8 长度前缀 (0 = empty)
    buf.push(0u8);

    buf
}

// ========== 内部辅助 ==========

struct AccountKeyEntry {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

fn add_account_key(
    keys: &mut Vec<AccountKeyEntry>,
    pubkey: &[u8; 32],
    is_signer: bool,
    is_writable: bool,
) {
    if let Some(existing) = keys.iter_mut().find(|k| k.pubkey == *pubkey) {
        existing.is_signer |= is_signer;
        existing.is_writable |= is_writable;
    } else {
        keys.push(AccountKeyEntry {
            pubkey: *pubkey,
            is_signer,
            is_writable,
        });
    }
}

struct CompiledVaultInstruction {
    program_id_index: u8,
    account_indices: Vec<u8>,
    data: Vec<u8>,
}

/// 构建 BPF Loader Upgradeable 的 ExtendProgram 指令
///
/// discriminator = 6 (u32 LE), 参数: additional_len (u32 LE)
/// 账户: ProgramData(writable), Program(writable), SystemProgram, Payer(signer, writable, optional)
pub fn build_extend_program_instruction(
    program_pubkey: &[u8; 32],
    payer_pubkey: &[u8; 32],
    additional_len: u32,
) -> VaultInstruction {
    use solana_sdk::pubkey::Pubkey;

    let bpf_loader_upgradeable: [u8; 32] =
        bs58::decode("BPFLoaderUpgradeab1e11111111111111111111111")
            .into_vec()
            .unwrap()
            .try_into()
            .unwrap();

    let program_pk = Pubkey::new_from_array(*program_pubkey);
    let bpf_pk = Pubkey::new_from_array(bpf_loader_upgradeable);
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_pk);

    let mut data = vec![6, 0, 0, 0]; // ExtendProgram discriminator
    data.extend_from_slice(&additional_len.to_le_bytes());

    VaultInstruction {
        program_id: bpf_loader_upgradeable,
        accounts: vec![
            VaultAccountMeta {
                pubkey: programdata_pda.to_bytes(),
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *program_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: solana_sdk::system_program::ID.to_bytes(),
                is_signer: false,
                is_writable: false,
            },
            VaultAccountMeta {
                pubkey: *payer_pubkey,
                is_signer: true,
                is_writable: true,
            },
        ],
        data,
    }
}

/// 构建 BPF Loader Upgradeable 的 Upgrade + Close 指令
///
/// 返回两条指令：
/// 1. Upgrade: 将 buffer 中的程序代码写入 programdata
/// 2. Close: 关闭 buffer 账户，回收剩余租金到 vault
pub fn build_program_upgrade_instructions(
    program_pubkey: &[u8; 32],
    buffer_pubkey: &[u8; 32],
    spill_pubkey: &[u8; 32],
    authority_pubkey: &[u8; 32],
) -> Vec<VaultInstruction> {
    use solana_sdk::pubkey::Pubkey;

    let bpf_loader_upgradeable: [u8; 32] =
        bs58::decode("BPFLoaderUpgradeab1e11111111111111111111111")
            .into_vec()
            .unwrap()
            .try_into()
            .unwrap();

    // 推导 ProgramData PDA: seeds = [program_id], program = BPF Loader Upgradeable
    let program_pk = Pubkey::new_from_array(*program_pubkey);
    let bpf_pk = Pubkey::new_from_array(bpf_loader_upgradeable);
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_pk);

    let rent_sysvar: [u8; 32] = bs58::decode("SysvarRent111111111111111111111111111111111")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    let clock_sysvar: [u8; 32] = bs58::decode("SysvarC1ock11111111111111111111111111111111")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    // 指令 1: Upgrade (discriminator = 3, u32 LE)
    // 账户: ProgramData, Program, Buffer, Spill, Rent, Clock, Authority
    let upgrade_ix = VaultInstruction {
        program_id: bpf_loader_upgradeable,
        accounts: vec![
            VaultAccountMeta {
                pubkey: programdata_pda.to_bytes(),
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *program_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *buffer_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *spill_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: rent_sysvar,
                is_signer: false,
                is_writable: false,
            },
            VaultAccountMeta {
                pubkey: clock_sysvar,
                is_signer: false,
                is_writable: false,
            },
            VaultAccountMeta {
                pubkey: *authority_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ],
        data: vec![3, 0, 0, 0],
    };

    // 指令 2: Close (discriminator = 5, u32 LE)
    // 关闭 buffer 账户，回收剩余租金
    // 账户: Buffer(close target), Recipient(lamports), Authority(signer)
    let close_ix = VaultInstruction {
        program_id: bpf_loader_upgradeable,
        accounts: vec![
            VaultAccountMeta {
                pubkey: *buffer_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *spill_pubkey,
                is_signer: false,
                is_writable: true,
            },
            VaultAccountMeta {
                pubkey: *authority_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ],
        data: vec![5, 0, 0, 0],
    };

    vec![upgrade_ix, close_ix]
}

// ========== Vote/Stake 管理指令 ==========

pub fn decode_bs58_pubkey(s: &str) -> Option<[u8; 32]> {
    bs58::decode(s)
        .into_vec()
        .ok()
        .and_then(|v| v.try_into().ok())
}

fn decode_bs58_pubkey_or_default(s: &str) -> [u8; 32] {
    decode_bs58_pubkey(s).unwrap_or([0u8; 32])
}

const VOTE_PROGRAM: &str = "Vote111111111111111111111111111111111111111";
const STAKE_PROGRAM: &str = "Stake11111111111111111111111111111111111111";
const CLOCK_SYSVAR: &str = "SysvarC1ock11111111111111111111111111111111";
const STAKE_HISTORY_SYSVAR: &str = "SysvarStakeHistory1111111111111111111111111";
const STAKE_CONFIG: &str = "StakeConfig11111111111111111111111111111111";

/// Vote Authorize: 修改 voter 或 withdrawer 权限
/// authorize_type: 0=Voter, 1=Withdrawer
pub fn build_vote_authorize_instruction(
    vote_account: &[u8; 32],
    vault_pubkey: &[u8; 32], // 当前权限持有者
    new_authority: &[u8; 32],
    authorize_type: u32,
) -> VaultInstruction {
    let vote_program = decode_bs58_pubkey_or_default(VOTE_PROGRAM);
    let clock_sysvar = decode_bs58_pubkey_or_default(CLOCK_SYSVAR);

    // data: [1,0,0,0] + new_authority(32) + authorize_type(4)
    let mut data = vec![1, 0, 0, 0];
    data.extend_from_slice(new_authority);
    data.extend_from_slice(&authorize_type.to_le_bytes());

    VaultInstruction {
        program_id: vote_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *vote_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data,
    }
}

/// Vote Withdraw: 从 Vote 账户提取
pub fn build_vote_withdraw_instruction(
    vote_account: &[u8; 32],
    to_pubkey: &[u8; 32],
    vault_pubkey: &[u8; 32], // authorized withdrawer
    lamports: u64,
) -> VaultInstruction {
    let vote_program = decode_bs58_pubkey_or_default(VOTE_PROGRAM);

    // Vote Withdraw (index 3)
    // data: [3,0,0,0] + lamports(8)
    let mut data = vec![3, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());

    VaultInstruction {
        program_id: vote_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *vote_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: *to_pubkey, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data,
    }
}

/// Stake Authorize: 修改 staker 或 withdrawer 权限
/// authorize_type: 0=Staker, 1=Withdrawer
pub fn build_stake_authorize_instruction(
    stake_account: &[u8; 32],
    vault_pubkey: &[u8; 32], // 当前权限持有者
    new_authority: &[u8; 32],
    authorize_type: u32,
) -> VaultInstruction {
    let stake_program = decode_bs58_pubkey_or_default(STAKE_PROGRAM);
    let clock_sysvar = decode_bs58_pubkey_or_default(CLOCK_SYSVAR);

    // data: [1,0,0,0] + new_authority(32) + stake_authorize_type(4)
    let mut data = vec![1, 0, 0, 0];
    data.extend_from_slice(new_authority);
    data.extend_from_slice(&authorize_type.to_le_bytes());

    VaultInstruction {
        program_id: stake_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *stake_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data,
    }
}

/// Stake Delegate: 委托 stake 账户到 vote 账户
pub fn build_stake_delegate_instruction(
    stake_account: &[u8; 32],
    vote_account: &[u8; 32],
    vault_pubkey: &[u8; 32], // staker 权限
) -> VaultInstruction {
    let stake_program = decode_bs58_pubkey_or_default(STAKE_PROGRAM);
    let clock_sysvar = decode_bs58_pubkey_or_default(CLOCK_SYSVAR);
    let stake_history = decode_bs58_pubkey_or_default(STAKE_HISTORY_SYSVAR);
    let stake_config = decode_bs58_pubkey_or_default(STAKE_CONFIG);

    VaultInstruction {
        program_id: stake_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *stake_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: *vote_account, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: stake_history, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: stake_config, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data: vec![2, 0, 0, 0], // Delegate instruction index
    }
}

/// Stake Deactivate: 取消质押
pub fn build_stake_deactivate_instruction(
    stake_account: &[u8; 32],
    vault_pubkey: &[u8; 32], // staker 权限
) -> VaultInstruction {
    let stake_program = decode_bs58_pubkey_or_default(STAKE_PROGRAM);
    let clock_sysvar = decode_bs58_pubkey_or_default(CLOCK_SYSVAR);

    VaultInstruction {
        program_id: stake_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *stake_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data: vec![5, 0, 0, 0], // Deactivate instruction index
    }
}

/// Stake Withdraw: 从 stake 账户提取
pub fn build_stake_withdraw_instruction(
    stake_account: &[u8; 32],
    to_pubkey: &[u8; 32],
    vault_pubkey: &[u8; 32], // withdrawer 权限
    lamports: u64,
) -> VaultInstruction {
    let stake_program = decode_bs58_pubkey_or_default(STAKE_PROGRAM);
    let clock_sysvar = decode_bs58_pubkey_or_default(CLOCK_SYSVAR);
    let stake_history = decode_bs58_pubkey_or_default(STAKE_HISTORY_SYSVAR);

    // data: [4,0,0,0] + lamports(8)
    let mut data = vec![4, 0, 0, 0];
    data.extend_from_slice(&lamports.to_le_bytes());

    VaultInstruction {
        program_id: stake_program,
        accounts: vec![
            VaultAccountMeta { pubkey: *stake_account, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: *to_pubkey, is_signer: false, is_writable: true },
            VaultAccountMeta { pubkey: clock_sysvar, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: stake_history, is_signer: false, is_writable: false },
            VaultAccountMeta { pubkey: *vault_pubkey, is_signer: true, is_writable: false },
        ],
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sol_transfer_instruction() {
        let vault = [1u8; 32];
        let to = [2u8; 32];
        let ix = build_sol_transfer_instruction(&vault, &to, 1_000_000_000);

        assert_eq!(ix.program_id, [0u8; 32]); // System program
        assert_eq!(ix.accounts.len(), 2);
        assert!(ix.accounts[0].is_signer); // vault is signer
        assert!(!ix.accounts[1].is_signer); // to is not signer
        // Data: 4 bytes (instruction index) + 8 bytes (amount)
        assert_eq!(ix.data.len(), 12);
    }

    #[test]
    fn test_serialize_vault_transaction_message() {
        let vault = [1u8; 32];
        let to = [2u8; 32];
        let ix = build_sol_transfer_instruction(&vault, &to, 1_000_000);

        let msg = serialize_vault_transaction_message(0, &[ix]);
        assert!(!msg.is_empty());

        // 验证 beet 格式序列化
        let mut off = 0;

        // num_signers: u8 = 1 (vault)
        assert_eq!(msg[off], 1); off += 1;
        // num_writable_signers: u8 = 1 (vault)
        assert_eq!(msg[off], 1); off += 1;
        // num_writable_non_signers: u8 = 1 (to)
        assert_eq!(msg[off], 1); off += 1;

        // account_keys: u8 长度前缀 = 3 (vault, to, system_program)
        assert_eq!(msg[off], 3); off += 1;
        // 验证第一个 key 是 vault (writable signer)
        assert_eq!(&msg[off..off+32], &vault); off += 32;
        // 第二个 key 是 to (writable non-signer)
        assert_eq!(&msg[off..off+32], &to); off += 32;
        // 第三个 key 是 system_program (readonly non-signer)
        assert_eq!(&msg[off..off+32], &[0u8; 32]); off += 32;

        // instructions: u8 长度前缀 = 1
        assert_eq!(msg[off], 1); off += 1;
        // program_id_index: u8 = 2 (system_program)
        assert_eq!(msg[off], 2); off += 1;
        // account_indices: u8 长度前缀 = 2
        assert_eq!(msg[off], 2); off += 1;
        assert_eq!(msg[off], 0); off += 1; // vault index
        assert_eq!(msg[off], 1); off += 1; // to index

        // data: u16 LE 长度前缀 = 12
        let data_len = u16::from_le_bytes([msg[off], msg[off+1]]);
        assert_eq!(data_len, 12); off += 2;
        off += 12; // skip data bytes

        // address_table_lookups: u8 长度前缀 = 0
        assert_eq!(msg[off], 0); off += 1;

        // 应该恰好读完所有字节
        assert_eq!(off, msg.len());
    }

    #[test]
    fn test_build_program_upgrade_instructions() {
        let program = [3u8; 32];
        let buffer = [4u8; 32];
        let spill = [5u8; 32];
        let authority = [1u8; 32];
        let ixs = build_program_upgrade_instructions(&program, &buffer, &spill, &authority);

        assert_eq!(ixs.len(), 2);

        // === Upgrade 指令 ===
        let upgrade = &ixs[0];
        let expected_pid: [u8; 32] =
            bs58::decode("BPFLoaderUpgradeab1e11111111111111111111111")
                .into_vec()
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(upgrade.program_id, expected_pid);
        assert_eq!(upgrade.accounts.len(), 7);
        assert!(upgrade.accounts[0].is_writable);  // ProgramData
        assert_eq!(upgrade.accounts[1].pubkey, program);
        assert_eq!(upgrade.accounts[2].pubkey, buffer);
        assert_eq!(upgrade.accounts[3].pubkey, spill);
        assert!(upgrade.accounts[6].is_signer);    // Authority
        assert_eq!(upgrade.data, vec![3, 0, 0, 0]);

        // === Close 指令 ===
        let close = &ixs[1];
        assert_eq!(close.program_id, expected_pid);
        assert_eq!(close.accounts.len(), 3);
        assert_eq!(close.accounts[0].pubkey, buffer);   // close target
        assert!(close.accounts[0].is_writable);
        assert_eq!(close.accounts[1].pubkey, spill);     // recipient
        assert!(close.accounts[1].is_writable);
        assert_eq!(close.accounts[2].pubkey, authority);  // authority
        assert!(close.accounts[2].is_signer);
        assert_eq!(close.data, vec![5, 0, 0, 0]);
    }

    #[test]
    fn test_serialize_program_upgrade_message() {
        let program = [3u8; 32];
        let buffer = [4u8; 32];
        let vault = [1u8; 32];
        let ixs = build_program_upgrade_instructions(&program, &buffer, &vault, &vault);

        let msg = serialize_vault_transaction_message(0, &ixs);
        assert!(!msg.is_empty());
        // num_signers >= 1 (vault as authority)
        assert!(msg[0] >= 1);
    }
}
