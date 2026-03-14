/// 调试 VaultTransactionExecute 的 remaining accounts
///
/// 用法: cargo run --example debug_execute

use base64::Engine;
use reqwest::Client;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

const RPC_URL: &str = "https://mainnet-api.nara.build/";
const MULTISIG_ADDRESS: &str = "893sDN8LftCSv3MP34JqSSiqBidrGRFK7U3K792vWa7Q";
const SQUADS_PROGRAM_ID: &str = "SQDS4ep65T869zMMBKyuUq6aD6EgTu8psMjkvj52pCf";

const SEED_PREFIX: &[u8] = b"multisig";
const SEED_VAULT: &[u8] = b"vault";
const SEED_TRANSACTION: &[u8] = b"transaction";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();
    let program_id = Pubkey::from_str(SQUADS_PROGRAM_ID)?;
    let multisig_pubkey = Pubkey::from_str(MULTISIG_ADDRESS)?;

    println!("=== 调试 VaultTransactionExecute ===\n");

    // 推导 vault PDA
    let (vault_pda, vault_pda_bump) = Pubkey::find_program_address(
        &[SEED_PREFIX, multisig_pubkey.as_ref(), SEED_VAULT, &[0u8]],
        &program_id,
    );
    println!("Vault PDA (vault_index=0): {vault_pda} (bump={vault_pda_bump})");
    println!();

    // 分析每个 VaultTransaction
    for tx_index in 1u64..=2 {
        println!("{}", "=".repeat(60));
        println!("=== VaultTransaction #{tx_index} ===");
        let idx_bytes = tx_index.to_le_bytes();
        let (transaction_pda, _) = Pubkey::find_program_address(
            &[SEED_PREFIX, multisig_pubkey.as_ref(), SEED_TRANSACTION, &idx_bytes],
            &program_id,
        );
        println!("Transaction PDA: {transaction_pda}");

        let tx_data = match fetch_account_data(&client, &transaction_pda.to_string()).await {
            Ok(data) => data,
            Err(e) => {
                println!("  获取失败: {e}\n");
                continue;
            }
        };
        println!("数据大小: {} bytes", tx_data.len());

        // Hex dump message 部分
        // VaultTransaction header: disc(8) + multisig(32) + creator(32) + index(8) + bump(1) + vault_index(1) + vault_bump(1) + esb(4+N)
        let mut off = 8;
        let vt_multisig = read_pubkey(&tx_data, &mut off);
        let vt_creator = read_pubkey(&tx_data, &mut off);
        let vt_index = read_u64(&tx_data, &mut off);
        let vt_bump = tx_data[off]; off += 1;
        let vt_vault_index = tx_data[off]; off += 1;
        let vt_vault_bump = tx_data[off]; off += 1;

        let esb_len = read_u32(&tx_data, &mut off) as usize;
        let esb: Vec<u8> = tx_data[off..off + esb_len].to_vec();
        off += esb_len;

        println!("multisig:    {vt_multisig}");
        println!("creator:     {vt_creator}");
        println!("index:       {vt_index}");
        println!("bump:        {vt_bump}");
        println!("vault_index: {vt_vault_index}");
        println!("vault_bump:  {vt_vault_bump}");
        println!("ephemeral_signer_bumps: {esb:?}");

        // 推导对应的 vault PDA
        let (this_vault_pda, this_vault_bump) = Pubkey::find_program_address(
            &[SEED_PREFIX, multisig_pubkey.as_ref(), SEED_VAULT, &[vt_vault_index]],
            &program_id,
        );
        println!("推导 vault PDA: {this_vault_pda} (bump={this_vault_bump})");

        // message 原始字节 hex dump
        let msg_start = off;
        println!("\n--- message 原始字节 (offset={msg_start}, 剩余 {} bytes) ---", tx_data.len() - msg_start);

        // 打印前 128 字节 hex
        let msg_end = std::cmp::min(tx_data.len(), msg_start + 128);
        for chunk_start in (msg_start..msg_end).step_by(16) {
            let chunk_end = std::cmp::min(msg_end, chunk_start + 16);
            let hex: String = tx_data[chunk_start..chunk_end]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            println!("  @{chunk_start:04}: {hex}");
        }
        println!();

        // 解析 message
        let num_signers = tx_data[off]; off += 1;
        let num_writable_signers = tx_data[off]; off += 1;
        let num_writable_non_signers = tx_data[off]; off += 1;
        let ak_len = read_u32(&tx_data, &mut off) as usize;

        println!("num_signers:              {num_signers}");
        println!("num_writable_signers:     {num_writable_signers}");
        println!("num_writable_non_signers: {num_writable_non_signers}");
        println!("account_keys_len:         {ak_len}");

        let mut account_keys = Vec::new();
        for i in 0..ak_len {
            if off + 32 > tx_data.len() {
                println!("  [!] 数据不足，无法读取 account_keys[{i}]");
                break;
            }
            let key = read_pubkey(&tx_data, &mut off);
            let is_vault = key == this_vault_pda;
            let marker = if is_vault { " ← VAULT PDA" } else { "" };
            println!("  account_keys[{i}]: {key}{marker}");
            account_keys.push(key);
        }

        // instructions
        if off + 4 <= tx_data.len() {
            let ix_len = read_u32(&tx_data, &mut off) as usize;
            println!("instructions_len:         {ix_len}");

            for ix_i in 0..ix_len {
                if off >= tx_data.len() {
                    println!("  [!] 数据不足，无法读取 instruction #{ix_i}");
                    break;
                }
                let pid_idx = tx_data[off]; off += 1;
                let ai_len = read_u32(&tx_data, &mut off) as usize;
                let ai: Vec<u8> = tx_data[off..off + ai_len].to_vec();
                off += ai_len;
                let data_len = read_u32(&tx_data, &mut off) as usize;
                let data: Vec<u8> = tx_data[off..off + data_len].to_vec();
                off += data_len;

                let program_name = if (pid_idx as usize) < account_keys.len() {
                    let pk = account_keys[pid_idx as usize];
                    if pk == Pubkey::default() { "System Program".to_string() }
                    else { pk.to_string() }
                } else {
                    format!("INVALID({pid_idx})")
                };
                println!("  ix[{ix_i}]: program={program_name} accounts={ai:?} data({})={data:?}", data.len());
            }
        } else {
            println!("instructions: [数据不足]");
        }

        // atl
        if off + 4 <= tx_data.len() {
            let atl_len = read_u32(&tx_data, &mut off);
            println!("address_table_lookups:    {atl_len}");
        }

        println!("解析结束 offset={off}, 数据总长={}", tx_data.len());

        // 比较
        println!("\n--- 比较 ---");
        if !account_keys.is_empty() {
            if account_keys[0] == this_vault_pda {
                println!("✓ account_keys[0] == vault PDA");
            } else {
                println!("✗ account_keys[0] != vault PDA");
                println!("  account_keys[0] = {}", account_keys[0]);
                println!("  vault PDA       = {this_vault_pda}");
            }
        }

        // 构建正确的 remaining accounts
        println!("\n--- remaining accounts (应该传入) ---");
        for (i, key) in account_keys.iter().enumerate() {
            let is_writable = if i < num_signers as usize {
                i < num_writable_signers as usize
            } else {
                (i - num_signers as usize) < num_writable_non_signers as usize
            };
            let is_vault = *key == this_vault_pda;
            let is_signer = (i < num_signers as usize) && !is_vault;
            println!("  [{i}] {key} (writable={is_writable}, signer={is_signer})");
        }

        println!();
    }

    // 额外检查: 错误中的地址
    println!("=== 错误中的地址分析 ===");
    let left = "7vRLdYL3svyp48WGeLZsNjmuCb9gWygdHtMjTUK5WtUQ";
    let right = "1115fmJbZucbrecjf3b3m4TXP7aYWnpMq1FkWeCQjKR";
    println!("Left (provided):  {left}");
    println!("Right (expected): {right}");
    println!("Vault PDA:        {vault_pda}");

    if left == vault_pda.to_string() {
        println!("→ Left IS the vault PDA (我们手动添加的)");
    }
    println!("→ Right 的 hex 表示:");
    let right_key = Pubkey::from_str(right)?;
    let right_bytes = right_key.to_bytes();
    println!("  {:02x?}", &right_bytes);

    Ok(())
}

fn read_pubkey(data: &[u8], offset: &mut usize) -> Pubkey {
    let key = Pubkey::try_from(&data[*offset..*offset + 32]).unwrap_or_default();
    *offset += 32;
    key
}

fn read_u32(data: &[u8], offset: &mut usize) -> u32 {
    let val = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
    *offset += 4;
    val
}

fn read_u64(data: &[u8], offset: &mut usize) -> u64 {
    let val = u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    val
}

async fn fetch_account_data(
    client: &Client,
    address: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64"}],
        "id": 1
    });

    let resp: serde_json::Value = client
        .post(RPC_URL)
        .json(&body)
        .send()
        .await?
        .json()
        .await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .ok_or("账户不存在")?;

    if value.is_null() {
        return Err(format!("账户 {address} 不存在").into());
    }

    let data_arr = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("缺少 data 字段")?;

    let base64_str = data_arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or("无效的数据格式")?;

    Ok(base64::engine::general_purpose::STANDARD.decode(base64_str)?)
}
