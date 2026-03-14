/// 端到端测试: 创建多签 → 转 SOL 到 vault → 创建提案 → 审批 → 执行
///
/// 用法: cargo run --example test_proposal_flow
///
/// 测试地址:
///   Address 1 (creator): 9fc1pAKG4KJi9w68G8pSRFdQaCMGkhPaATnQNRADvR67
///   Address 2: AAqLFh4quxbA9nibweKDaE1gc4nVvUsGn2h62JDDNUUh

use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;
use std::str::FromStr;

const RPC_URL: &str = "https://mainnet-api.nara.build/";
const SQUADS_PROGRAM_ID: &str = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";

const SEED_PREFIX: &[u8] = b"multisig";
const SEED_VAULT: &[u8] = b"vault";
const SEED_TRANSACTION: &[u8] = b"transaction";
const SEED_PROPOSAL: &[u8] = b"proposal";

// 测试用 Keypair (base58 编码的 64 字节 keypair: secret[32] + public[32])
const KEYPAIR_1: &str = "63bKUMkwc5zCNxoT5XuEG6k5DPm2ZSg2aYeYcMNhsKiqmcK371oUoG6k4kUGd1k9rkV62DLwdCYkN8vm8euvWNtP";
const KEYPAIR_2: &str = "47Ar3mwHb231ApAV2eLnJ8YmVewybAKTu6BufTpwd1nATAgwzAWJZ4m5RSnzyKUeiNud546e2KQWzxvkeQqtBMoX";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let program_id = Pubkey::from_str(SQUADS_PROGRAM_ID)?;

    // 解码 keypairs
    let kp1_bytes = bs58::decode(KEYPAIR_1).into_vec()?;
    let kp2_bytes = bs58::decode(KEYPAIR_2).into_vec()?;
    let keypair1 = Keypair::try_from(kp1_bytes.as_slice())?;
    let keypair2 = Keypair::try_from(kp2_bytes.as_slice())?;

    println!("=== 端到端多签提案测试 ===\n");
    println!("地址 1 (creator): {}", keypair1.pubkey());
    println!("地址 2:           {}", keypair2.pubkey());
    println!();

    // 检查命令行参数：可传入已有多签地址跳过创建
    let args: Vec<String> = std::env::args().collect();
    let use_existing_multisig = args.get(1).map(|s| s.to_string());

    // ===== Step 1: 创建多签或使用已有 =====
    let multisig_pda = if let Some(existing) = &use_existing_multisig {
        println!("--- Step 1: 使用已有多签 ---");
        let pda = Pubkey::from_str(existing)?;
        println!("Multisig PDA: {pda}");
        pda
    } else {
        println!("--- Step 1: 创建多签 (threshold=2) ---");

        let create_key_keypair = Keypair::new();
        let create_key_pubkey = create_key_keypair.pubkey();

        let (pda, _) = Pubkey::find_program_address(
            &[SEED_PREFIX, SEED_PREFIX, create_key_pubkey.as_ref()],
            &program_id,
        );
        println!("Create key:   {create_key_pubkey}");
        println!("Multisig PDA: {pda}");

        // 获取 ProgramConfig treasury
        let (program_config_pda, _) = Pubkey::find_program_address(
            &[SEED_PREFIX, b"program_config"],
            &program_id,
        );
        let config_data = fetch_account_data(&client, &program_config_pda.to_string()).await?;
        let treasury = read_pubkey_at(&config_data, 48);
        println!("Treasury:     {treasury}");

        let create_multisig_disc = anchor_discriminator("global:multisig_create_v2");
        let mut create_data = create_multisig_disc.to_vec();
        create_data.push(0); // config_authority = None
        create_data.extend_from_slice(&2u16.to_le_bytes()); // threshold = 2
        create_data.extend_from_slice(&2u32.to_le_bytes()); // members.len() = 2
        create_data.extend_from_slice(&keypair1.pubkey().to_bytes());
        create_data.push(7); // mask = Initiate(1)+Vote(2)+Execute(4)
        create_data.extend_from_slice(&keypair2.pubkey().to_bytes());
        create_data.push(7);
        create_data.extend_from_slice(&0u32.to_le_bytes()); // time_lock = 0
        create_data.push(0); // rent_collector = None
        create_data.push(0); // memo = None

        let create_ix = SolInstruction {
            program_id: program_id.to_bytes(),
            accounts: vec![
                SolAccountMeta { pubkey: program_config_pda.to_bytes(), is_signer: false, is_writable: false },
                SolAccountMeta { pubkey: treasury.to_bytes(), is_signer: false, is_writable: true },
                SolAccountMeta { pubkey: pda.to_bytes(), is_signer: false, is_writable: true },
                SolAccountMeta { pubkey: create_key_pubkey.to_bytes(), is_signer: true, is_writable: false },
                SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: true },
                SolAccountMeta { pubkey: [0u8; 32], is_signer: false, is_writable: false },
            ],
            data: create_data,
        };

        let blockhash = get_latest_blockhash(&client).await?;
        let msg_bytes = build_and_serialize_message(
            &keypair1.pubkey().to_bytes(),
            &blockhash,
            &[create_ix],
        );
        let sig1 = keypair1.sign_message(&msg_bytes);
        let sig_ck = create_key_keypair.sign_message(&msg_bytes);
        let tx_bytes = build_transaction(
            &[to_sig_bytes(&sig1), to_sig_bytes(&sig_ck)],
            &msg_bytes,
        );
        let tx_sig = send_transaction(&client, &tx_bytes).await?;
        println!("创建多签 tx: {tx_sig}");
        wait_confirmation(&client, &tx_sig).await;
        pda
    };

    // ===== Step 2: 给 vault 转 SOL（如果余额不足） =====
    let (vault_pda, _) = Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_VAULT, &[0u8]],
        &program_id,
    );
    println!("\n--- Step 2: 检查 Vault 余额 ---");
    println!("Vault PDA: {vault_pda}");

    let vault_balance = get_balance(&client, &vault_pda.to_string()).await;
    println!("Vault 余额: {} lamports ({:.6} SOL)", vault_balance, vault_balance as f64 / 1e9);

    if vault_balance < 10_000_000 {
        println!("余额不足，转入 0.01 SOL...");
        let fund_amount = 10_000_000u64;
        let mut fund_data = vec![2, 0, 0, 0];
        fund_data.extend_from_slice(&fund_amount.to_le_bytes());
        let fund_ix = SolInstruction {
            program_id: [0u8; 32],
            accounts: vec![
                SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: true },
                SolAccountMeta { pubkey: vault_pda.to_bytes(), is_signer: false, is_writable: true },
            ],
            data: fund_data,
        };
        let blockhash = get_latest_blockhash(&client).await?;
        let msg_bytes = build_and_serialize_message(
            &keypair1.pubkey().to_bytes(),
            &blockhash,
            &[fund_ix],
        );
        let sig = keypair1.sign_message(&msg_bytes);
        let tx_bytes = build_transaction(&[to_sig_bytes(&sig)], &msg_bytes);
        let tx_sig = send_transaction(&client, &tx_bytes).await?;
        println!("转账 tx: {tx_sig}");
        wait_confirmation(&client, &tx_sig).await;
    } else {
        println!("余额充足，跳过转账");
    }

    // ===== Step 3: 创建 SOL 转账提案 + 自动审批 =====
    println!("\n--- Step 3: 创建 SOL 转账提案 (vault → address1, 0.005 SOL) ---");

    // 获取当前 transaction_index
    let ms_data = fetch_account_data(&client, &multisig_pda.to_string()).await?;
    // Multisig 账户: disc(8) + create_key(32) + config_authority(32) + threshold(2) + time_lock(4) + transaction_index(8)...
    // offset = 8 + 32 + 32 + 2 + 4 = 78 → transaction_index starts at 78
    let tx_index = u64::from_le_bytes(ms_data[78..86].try_into()?) + 1;
    println!("新 transaction_index: {tx_index}");

    let idx_bytes = tx_index.to_le_bytes();
    let (transaction_pda, _) = Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_TRANSACTION, &idx_bytes],
        &program_id,
    );
    let (proposal_pda, _) = Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pda.as_ref(), SEED_TRANSACTION, &idx_bytes, SEED_PROPOSAL],
        &program_id,
    );
    println!("Transaction PDA: {transaction_pda}");
    println!("Proposal PDA:    {proposal_pda}");

    // 构建 beet 格式的 vault transaction message（修复后的格式）
    let transfer_amount = 5_000_000u64; // 0.005 SOL
    let vault_message = build_sol_transfer_vault_message(
        &vault_pda.to_bytes(),
        &keypair1.pubkey().to_bytes(),
        transfer_amount,
    );
    println!("Vault message ({} bytes): {:02x?}", vault_message.len(), &vault_message[..std::cmp::min(32, vault_message.len())]);

    // 验证 beet 格式
    verify_beet_format(&vault_message);

    // 指令 1: vault_transaction_create
    let vtc_disc = anchor_discriminator("global:vault_transaction_create");
    let mut vtc_data = vtc_disc.to_vec();
    vtc_data.push(0); // vault_index = 0
    vtc_data.push(0); // ephemeral_signers = 0
    vtc_data.extend_from_slice(&(vault_message.len() as u32).to_le_bytes()); // Borsh Vec<u8> 前缀
    vtc_data.extend_from_slice(&vault_message);
    vtc_data.push(0); // memo = None

    let vtc_ix = SolInstruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            SolAccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: true },
            SolAccountMeta { pubkey: transaction_pda.to_bytes(), is_signer: false, is_writable: true },
            SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: false },
            SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: true },
            SolAccountMeta { pubkey: [0u8; 32], is_signer: false, is_writable: false },
        ],
        data: vtc_data,
    };

    // 指令 2: proposal_create
    let pc_disc = anchor_discriminator("global:proposal_create");
    let mut pc_data = pc_disc.to_vec();
    pc_data.extend_from_slice(&tx_index.to_le_bytes()); // transaction_index
    pc_data.push(0); // draft = false

    let pc_ix = SolInstruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            SolAccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: true },
            SolAccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
            SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: false },
            SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: true },
            SolAccountMeta { pubkey: [0u8; 32], is_signer: false, is_writable: false },
        ],
        data: pc_data,
    };

    // 指令 3: proposal_approve (by creator)
    let pa_disc = anchor_discriminator("global:proposal_approve");
    let mut pa_data = pa_disc.to_vec();
    pa_data.push(0); // memo = None

    let pa_ix = SolInstruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            SolAccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: false },
            SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: false },
            SolAccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
        ],
        data: pa_data,
    };

    let blockhash = get_latest_blockhash(&client).await?;
    let msg_bytes = build_and_serialize_message(
        &keypair1.pubkey().to_bytes(),
        &blockhash,
        &[vtc_ix, pc_ix, pa_ix],
    );
    let sig = keypair1.sign_message(&msg_bytes);
    let tx_bytes = build_transaction(&[to_sig_bytes(&sig)], &msg_bytes);
    let tx_sig = send_transaction(&client, &tx_bytes).await?;
    println!("创建提案+审批 tx: {tx_sig}");
    wait_confirmation(&client, &tx_sig).await;

    // ===== Step 4: 验证链上数据 =====
    println!("\n--- Step 4: 验证链上 VaultTransaction 数据 ---");
    let vt_data = fetch_account_data(&client, &transaction_pda.to_string()).await?;
    println!("VaultTransaction 数据大小: {} bytes", vt_data.len());
    verify_on_chain_message(&vt_data, &vault_pda);

    // ===== Step 5: 第二个成员审批 =====
    println!("\n--- Step 5: 第二个成员审批 ---");
    let pa2_disc = anchor_discriminator("global:proposal_approve");
    let mut pa2_data = pa2_disc.to_vec();
    pa2_data.push(0); // memo = None

    let pa2_ix = SolInstruction {
        program_id: program_id.to_bytes(),
        accounts: vec![
            SolAccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: false },
            SolAccountMeta { pubkey: keypair2.pubkey().to_bytes(), is_signer: true, is_writable: false },
            SolAccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
        ],
        data: pa2_data,
    };

    let blockhash = get_latest_blockhash(&client).await?;
    let msg_bytes = build_and_serialize_message(
        &keypair2.pubkey().to_bytes(),
        &blockhash,
        &[pa2_ix],
    );
    let sig = keypair2.sign_message(&msg_bytes);
    let tx_bytes = build_transaction(&[to_sig_bytes(&sig)], &msg_bytes);
    let tx_sig = send_transaction(&client, &tx_bytes).await?;
    println!("审批 tx: {tx_sig}");
    wait_confirmation(&client, &tx_sig).await;

    // ===== Step 6: 执行 =====
    println!("\n--- Step 6: 执行 VaultTransactionExecute ---");
    let vte_disc = anchor_discriminator("global:vault_transaction_execute");
    let vte_data = vte_disc.to_vec(); // 无额外 args

    let mut vte_accounts = vec![
        SolAccountMeta { pubkey: multisig_pda.to_bytes(), is_signer: false, is_writable: true },
        SolAccountMeta { pubkey: proposal_pda.to_bytes(), is_signer: false, is_writable: true },
        SolAccountMeta { pubkey: transaction_pda.to_bytes(), is_signer: false, is_writable: false },
        SolAccountMeta { pubkey: keypair1.pubkey().to_bytes(), is_signer: true, is_writable: false },
    ];

    // 从链上数据构建 remaining accounts
    let remaining = build_remaining_accounts(&vt_data, &vault_pda, &program_id, &multisig_pda)?;
    println!("Remaining accounts ({}):", remaining.len());
    for (i, acc) in remaining.iter().enumerate() {
        let pk = Pubkey::new_from_array(acc.pubkey);
        let w = if acc.is_writable { "W" } else { "R" };
        let is_vault = pk == vault_pda;
        let marker = if is_vault { " ← VAULT" } else { "" };
        println!("  [{i}] {pk} ({w}){marker}");
    }
    vte_accounts.extend(remaining);

    let vte_ix = SolInstruction {
        program_id: program_id.to_bytes(),
        accounts: vte_accounts,
        data: vte_data,
    };

    let blockhash = get_latest_blockhash(&client).await?;
    let msg_bytes = build_and_serialize_message(
        &keypair1.pubkey().to_bytes(),
        &blockhash,
        &[vte_ix],
    );
    let sig = keypair1.sign_message(&msg_bytes);
    let tx_bytes = build_transaction(&[to_sig_bytes(&sig)], &msg_bytes);
    let tx_sig = send_transaction(&client, &tx_bytes).await?;
    println!("执行 tx: {tx_sig}");

    println!("\n=== 测试完成! ===");
    Ok(())
}

// ========== Vault Transaction Message (beet 格式) ==========

/// 构建 SOL 转账的 vault transaction message（beet 格式）
fn build_sol_transfer_vault_message(
    vault_pubkey: &[u8; 32],
    to_pubkey: &[u8; 32],
    lamports: u64,
) -> Vec<u8> {
    let system_program = [0u8; 32];

    // System Transfer instruction data
    let mut ix_data = vec![2, 0, 0, 0]; // instruction index 2
    ix_data.extend_from_slice(&lamports.to_le_bytes());

    // account_keys 排序: writable signers, readonly signers, writable non-signers, readonly non-signers
    // vault: writable signer
    // to: writable non-signer
    // system_program: readonly non-signer
    let account_keys: Vec<[u8; 32]> = vec![*vault_pubkey, *to_pubkey, system_program];

    let mut buf = Vec::new();

    // num_signers: u8 = 1 (vault)
    buf.push(1);
    // num_writable_signers: u8 = 1 (vault)
    buf.push(1);
    // num_writable_non_signers: u8 = 1 (to)
    buf.push(1);

    // account_keys: u8 长度前缀 + N * 32 bytes
    buf.push(account_keys.len() as u8);
    for key in &account_keys {
        buf.extend_from_slice(key);
    }

    // instructions: u8 长度前缀 = 1
    buf.push(1);
    // program_id_index = 2 (system_program)
    buf.push(2);
    // account_indexes: u8 长度前缀 = 2, then [0, 1]
    buf.push(2);
    buf.push(0); // vault
    buf.push(1); // to
    // data: u16 LE 长度前缀
    buf.extend_from_slice(&(ix_data.len() as u16).to_le_bytes());
    buf.extend_from_slice(&ix_data);

    // address_table_lookups: u8 = 0
    buf.push(0);

    buf
}

/// 验证 beet 格式序列化
fn verify_beet_format(msg: &[u8]) {
    let mut off = 0;
    let num_signers = msg[off]; off += 1;
    let num_writable_signers = msg[off]; off += 1;
    let num_writable_non_signers = msg[off]; off += 1;

    // u8 prefix for account_keys
    let ak_len = msg[off] as usize; off += 1;
    println!("  验证: num_signers={num_signers}, num_writable_signers={num_writable_signers}, num_writable_non_signers={num_writable_non_signers}");
    println!("  验证: account_keys_len={ak_len} (u8 前缀 @byte 3)");

    for i in 0..ak_len {
        let key = Pubkey::try_from(&msg[off..off + 32]).unwrap();
        off += 32;
        println!("  验证: account_keys[{i}] = {key}");
    }

    // u8 prefix for instructions
    let ix_len = msg[off] as usize; off += 1;
    println!("  验证: instructions_len={ix_len} (u8 前缀)");

    for i in 0..ix_len {
        let pid_idx = msg[off]; off += 1;
        let ai_len = msg[off] as usize; off += 1;
        off += ai_len;
        let data_len = u16::from_le_bytes([msg[off], msg[off + 1]]) as usize; off += 2;
        off += data_len;
        println!("  验证: ix[{i}] program_id_index={pid_idx}, accounts={ai_len}, data_len={data_len}");
    }

    let atl_len = msg[off] as usize; off += 1;
    println!("  验证: address_table_lookups={atl_len}");
    println!("  验证: 已解析 {off}/{} bytes ✓", msg.len());
    assert_eq!(off, msg.len(), "beet 格式验证失败: 字节数不匹配");
}

/// 验证链上存储的 VaultTransaction 数据（标准 Borsh u32 格式）
///
/// 注意：输入用 beet (u8/u16)，但链上存储用标准 Borsh (u32)
fn verify_on_chain_message(data: &[u8], expected_vault: &Pubkey) {
    let mut off = 83; // skip header

    // ephemeral_signer_bumps: Vec<u8> (Borsh u32 前缀)
    let esb_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4 + esb_len;
    println!("  ephemeral_signer_bumps: len={esb_len}");

    // message (标准 Borsh u32 格式)
    let msg_off = off;
    let num_signers = data[off]; off += 1;
    let num_writable_signers = data[off]; off += 1;
    let num_writable_non_signers = data[off]; off += 1;
    println!("  num_signers={num_signers}, num_writable_signers={num_writable_signers}, num_writable_non_signers={num_writable_non_signers}");

    // account_keys: u32 前缀
    let ak_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4;
    println!("  account_keys_len={ak_len} (u32 前缀)");

    for i in 0..ak_len {
        let key = Pubkey::try_from(&data[off..off + 32]).unwrap();
        off += 32;
        let is_vault = key == *expected_vault;
        let marker = if is_vault { " ← VAULT PDA ✓" } else { "" };
        println!("  account_keys[{i}] = {key}{marker}");
    }

    // instructions: u32 前缀
    let ix_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4;
    println!("  instructions_len={ix_len}");

    for i in 0..ix_len {
        let pid_idx = data[off]; off += 1;
        // account_indexes: u32 前缀
        let ai_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        let ai: Vec<u8> = data[off..off + ai_len].to_vec(); off += ai_len;
        // data: u32 前缀
        let data_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        off += data_len;
        println!("  ix[{i}]: program={pid_idx}, accounts={ai:?}, data_len={data_len}");
    }

    // address_table_lookups: u32 前缀
    let atl_len = u32::from_le_bytes(data[off..off + 4].try_into().unwrap()) as usize;
    off += 4;
    println!("  address_table_lookups={atl_len}");

    println!("  解析完成: message offset {msg_off}..{off}, 数据总长 {}", data.len());
}

/// 从链上 VaultTransaction 数据构建 remaining accounts（标准 Borsh u32 格式）
fn build_remaining_accounts(
    data: &[u8],
    vault_pda: &Pubkey,
    _program_id: &Pubkey,
    _multisig_pda: &Pubkey,
) -> Result<Vec<SolAccountMeta>, Box<dyn std::error::Error>> {
    let mut off = 83; // skip header

    // ephemeral_signer_bumps: u32 前缀
    let esb_len = u32::from_le_bytes(data[off..off + 4].try_into()?) as usize;
    off += 4 + esb_len;

    // message (标准 Borsh u32 格式)
    let num_signers = data[off] as usize; off += 1;
    let num_writable_signers = data[off] as usize; off += 1;
    let num_writable_non_signers = data[off] as usize; off += 1;

    // account_keys: u32 前缀
    let ak_len = u32::from_le_bytes(data[off..off + 4].try_into()?) as usize;
    off += 4;
    let mut account_keys = Vec::with_capacity(ak_len);
    for _ in 0..ak_len {
        let key = Pubkey::try_from(&data[off..off + 32])?;
        off += 32;
        account_keys.push(key);
    }

    let mut remaining = Vec::new();
    for (i, key) in account_keys.iter().enumerate() {
        let is_signer_key = i < num_signers;
        let is_writable = if i < num_signers {
            i < num_writable_signers
        } else {
            (i - num_signers) < num_writable_non_signers
        };

        // signer key[0] = vault PDA
        let actual_key = if is_signer_key && i == 0 {
            *vault_pda
        } else {
            *key
        };

        remaining.push(SolAccountMeta {
            pubkey: actual_key.to_bytes(),
            is_signer: false,
            is_writable,
        });
    }

    Ok(remaining)
}

// ========== 交易构建辅助 ==========

struct SolAccountMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
}

struct SolInstruction {
    program_id: [u8; 32],
    accounts: Vec<SolAccountMeta>,
    data: Vec<u8>,
}

struct AccountKeyMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
    is_fee_payer: bool,
}

fn account_sort_order(key: &AccountKeyMeta) -> u8 {
    if key.is_fee_payer { return 0; }
    match (key.is_signer, key.is_writable) {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 3,
        (false, false) => 4,
    }
}

fn add_key(keys: &mut Vec<AccountKeyMeta>, pubkey: &[u8; 32], is_signer: bool, is_writable: bool) {
    if let Some(existing) = keys.iter_mut().find(|k| k.pubkey == *pubkey) {
        existing.is_signer |= is_signer;
        existing.is_writable |= is_writable;
    } else {
        keys.push(AccountKeyMeta {
            pubkey: *pubkey,
            is_signer,
            is_writable,
            is_fee_payer: keys.is_empty(),
        });
    }
}

fn build_and_serialize_message(
    fee_payer: &[u8; 32],
    recent_blockhash: &[u8; 32],
    instructions: &[SolInstruction],
) -> Vec<u8> {
    let mut keys: Vec<AccountKeyMeta> = Vec::new();
    add_key(&mut keys, fee_payer, true, true);
    for ix in instructions {
        for meta in &ix.accounts {
            add_key(&mut keys, &meta.pubkey, meta.is_signer, meta.is_writable);
        }
        add_key(&mut keys, &ix.program_id, false, false);
    }
    keys.sort_by_key(account_sort_order);

    let num_signers = keys.iter().filter(|k| k.is_signer).count() as u8;
    let num_readonly_signed = keys.iter().filter(|k| k.is_signer && !k.is_writable).count() as u8;
    let num_readonly_unsigned = keys.iter().filter(|k| !k.is_signer && !k.is_writable).count() as u8;
    let account_keys: Vec<[u8; 32]> = keys.iter().map(|k| k.pubkey).collect();

    let compiled: Vec<(u8, Vec<u8>, Vec<u8>)> = instructions
        .iter()
        .map(|ix| {
            let pid_idx = account_keys.iter().position(|k| k == &ix.program_id).unwrap() as u8;
            let ai: Vec<u8> = ix.accounts.iter()
                .map(|m| account_keys.iter().position(|k| k == &m.pubkey).unwrap() as u8)
                .collect();
            (pid_idx, ai, ix.data.clone())
        })
        .collect();

    let mut buf = Vec::new();
    buf.push(num_signers);
    buf.push(num_readonly_signed);
    buf.push(num_readonly_unsigned);
    encode_compact_u16(&mut buf, account_keys.len() as u16);
    for key in &account_keys {
        buf.extend_from_slice(key);
    }
    buf.extend_from_slice(recent_blockhash);
    encode_compact_u16(&mut buf, compiled.len() as u16);
    for (pid_idx, ai, data) in &compiled {
        buf.push(*pid_idx);
        encode_compact_u16(&mut buf, ai.len() as u16);
        buf.extend_from_slice(ai);
        encode_compact_u16(&mut buf, data.len() as u16);
        buf.extend_from_slice(data);
    }
    buf
}

fn build_transaction(signatures: &[[u8; 64]], message_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_compact_u16(&mut buf, signatures.len() as u16);
    for sig in signatures {
        buf.extend_from_slice(sig);
    }
    buf.extend_from_slice(message_bytes);
    buf
}

fn encode_compact_u16(buf: &mut Vec<u8>, value: u16) {
    let mut val = value;
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val > 0 { byte |= 0x80; }
        buf.push(byte);
        if val == 0 { break; }
    }
}

fn to_sig_bytes(sig: &solana_sdk::signature::Signature) -> [u8; 64] {
    let mut bytes = [0u8; 64];
    bytes.copy_from_slice(sig.as_ref());
    bytes
}

// ========== Anchor 辅助 ==========

fn anchor_discriminator(name: &str) -> [u8; 8] {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let hash = hasher.finalize();
    let mut disc = [0u8; 8];
    disc.copy_from_slice(&hash[..8]);
    disc
}

fn read_pubkey_at(data: &[u8], offset: usize) -> Pubkey {
    Pubkey::try_from(&data[offset..offset + 32]).unwrap()
}

// ========== RPC 辅助 ==========

async fn fetch_account_data(
    client: &Client,
    address: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let resp: Value = client.post(RPC_URL).json(&body).send().await?.json().await?;
    let value = resp.get("result").and_then(|r| r.get("value"))
        .ok_or("账户不存在")?;
    if value.is_null() {
        return Err(format!("账户 {address} 不存在").into());
    }
    let base64_str = value.get("data").and_then(|d| d.as_array())
        .and_then(|a| a.first()).and_then(|v| v.as_str())
        .ok_or("无效的数据格式")?;
    Ok(base64::engine::general_purpose::STANDARD.decode(base64_str)?)
}

async fn get_latest_blockhash(client: &Client) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getLatestBlockhash",
        "params": [{"commitment": "finalized"}],
        "id": 1
    });
    let resp: Value = client.post(RPC_URL).json(&body).send().await?.json().await?;
    let hash_str = resp.get("result").and_then(|r| r.get("value"))
        .and_then(|v| v.get("blockhash")).and_then(|b| b.as_str())
        .ok_or("获取 blockhash 失败")?;
    let bytes = bs58::decode(hash_str).into_vec()?;
    Ok(bytes.try_into().map_err(|_| "blockhash 长度无效")?)
}

async fn send_transaction(
    client: &Client,
    tx_bytes: &[u8],
) -> Result<String, Box<dyn std::error::Error>> {
    let tx_base64 = base64::engine::general_purpose::STANDARD.encode(tx_bytes);
    let body = json!({
        "jsonrpc": "2.0",
        "method": "sendTransaction",
        "params": [tx_base64, {"encoding": "base64", "skipPreflight": true}],
        "id": 1
    });
    let resp: Value = client.post(RPC_URL).json(&body).send().await?.json().await?;
    if let Some(error) = resp.get("error") {
        let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("未知错误");
        let logs = error.get("data").and_then(|d| d.get("logs"));
        if let Some(logs) = logs {
            eprintln!("交易日志: {}", serde_json::to_string_pretty(logs)?);
        }
        return Err(format!("交易失败: {msg}").into());
    }
    Ok(resp.get("result").and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or("未收到交易签名")?)
}

async fn get_balance(client: &Client, address: &str) -> u64 {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address, {"commitment": "confirmed"}],
        "id": 1
    });
    if let Ok(resp) = client.post(RPC_URL).json(&body).send().await {
        if let Ok(val) = resp.json::<Value>().await {
            return val.get("result").and_then(|r| r.get("value"))
                .and_then(|v| v.as_u64()).unwrap_or(0);
        }
    }
    0
}

async fn wait_confirmation(client: &Client, signature: &str) {
    println!("  等待确认...");
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let body = json!({
            "jsonrpc": "2.0",
            "method": "getSignatureStatuses",
            "params": [[signature]],
            "id": 1
        });
        if let Ok(resp) = client.post(RPC_URL).json(&body).send().await {
            if let Ok(val) = resp.json::<Value>().await {
                if let Some(status) = val.get("result").and_then(|r| r.get("value"))
                    .and_then(|v| v.as_array()).and_then(|a| a.first()) {
                    if !status.is_null() {
                        // 检查是否有错误
                        if let Some(err) = status.get("err") {
                            if !err.is_null() {
                                println!("  交易失败: {err}");
                                // 打印日志
                                print_tx_logs(client, signature).await;
                                return;
                            }
                        }
                        let conf = status.get("confirmationStatus")
                            .and_then(|c| c.as_str()).unwrap_or("unknown");
                        println!("  确认状态: {conf}");
                        if conf == "confirmed" || conf == "finalized" {
                            return;
                        }
                    }
                }
            }
        }
    }
    println!("  超时，继续...");
}

async fn print_tx_logs(client: &Client, signature: &str) {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getTransaction",
        "params": [signature, {"encoding": "json", "maxSupportedTransactionVersion": 0}],
        "id": 1
    });
    if let Ok(resp) = client.post(RPC_URL).json(&body).send().await {
        if let Ok(val) = resp.json::<Value>().await {
            if let Some(logs) = val.get("result").and_then(|r| r.get("meta"))
                .and_then(|m| m.get("logMessages")).and_then(|l| l.as_array()) {
                println!("  程序日志:");
                for log in logs {
                    if let Some(s) = log.as_str() {
                        println!("    {s}");
                    }
                }
            }
        }
    }
}
