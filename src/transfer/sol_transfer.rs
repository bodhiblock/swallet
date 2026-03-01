use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signer::keypair::Keypair;
use solana_sdk::signer::Signer;

/// 发送 SOL 原生转账
pub async fn send_sol_native(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    to: &str,
    amount_lamports: u64,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let from_pubkey = keypair.pubkey().to_bytes();

    let to_pubkey: [u8; 32] = bs58::decode(to)
        .into_vec()
        .map_err(|e| format!("无效的目标地址: {e}"))?
        .try_into()
        .map_err(|_| "目标地址长度无效".to_string())?;

    let recent_blockhash = get_latest_blockhash(client, rpc_url).await?;

    // System program: all zeros
    let system_program = [0u8; 32];

    // Transfer instruction: index=2 + u64 LE lamports
    let mut ix_data = vec![2, 0, 0, 0];
    ix_data.extend_from_slice(&amount_lamports.to_le_bytes());

    let message = build_message(
        &from_pubkey,
        &recent_blockhash,
        &[Instruction {
            program_id: system_program,
            accounts: vec![
                AccountMeta {
                    pubkey: from_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to_pubkey,
                    is_signer: false,
                    is_writable: true,
                },
            ],
            data: ix_data,
        }],
    );

    let message_bytes = serialize_message(&message);
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = build_transaction(&[sig_bytes], &message_bytes);

    send_transaction(client, rpc_url, &tx_bytes).await
}

/// 发送 SPL Token 转账
pub async fn send_spl_token(
    client: &Client,
    rpc_url: &str,
    private_key: &[u8],
    mint: &str,
    to_wallet: &str,
    amount: u64,
    token_program_id: &str,
) -> Result<String, String> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| "私钥长度必须为 32 字节".to_string())?;
    let keypair = Keypair::new_from_array(key_bytes);
    let from_pubkey = keypair.pubkey().to_bytes();

    let mint_pubkey: [u8; 32] = bs58::decode(mint)
        .into_vec()
        .map_err(|e| format!("无效的 mint 地址: {e}"))?
        .try_into()
        .map_err(|_| "mint 地址长度无效".to_string())?;

    let to_wallet_pubkey: [u8; 32] = bs58::decode(to_wallet)
        .into_vec()
        .map_err(|e| format!("无效的目标地址: {e}"))?
        .try_into()
        .map_err(|_| "目标地址长度无效".to_string())?;

    let token_program: [u8; 32] = bs58::decode(token_program_id)
        .into_vec()
        .map_err(|e| format!("无效的 token program: {e}"))?
        .try_into()
        .map_err(|_| "token program 地址长度无效".to_string())?;

    let ata_program: [u8; 32] = bs58::decode("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
        .into_vec()
        .unwrap()
        .try_into()
        .unwrap();

    let system_program = [0u8; 32];

    // Derive ATAs
    let source_ata =
        find_associated_token_address(&from_pubkey, &mint_pubkey, &token_program, &ata_program)?;
    let dest_ata = find_associated_token_address(
        &to_wallet_pubkey,
        &mint_pubkey,
        &token_program,
        &ata_program,
    )?;

    let recent_blockhash = get_latest_blockhash(client, rpc_url).await?;

    // Check if dest ATA exists
    let dest_ata_exists =
        account_exists(client, rpc_url, &bs58::encode(dest_ata).into_string()).await;

    let mut instructions = Vec::new();

    // Create dest ATA if needed
    if !dest_ata_exists {
        instructions.push(Instruction {
            program_id: ata_program,
            accounts: vec![
                AccountMeta {
                    pubkey: from_pubkey,
                    is_signer: true,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: dest_ata,
                    is_signer: false,
                    is_writable: true,
                },
                AccountMeta {
                    pubkey: to_wallet_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: mint_pubkey,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: system_program,
                    is_signer: false,
                    is_writable: false,
                },
                AccountMeta {
                    pubkey: token_program,
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

    instructions.push(Instruction {
        program_id: token_program,
        accounts: vec![
            AccountMeta {
                pubkey: source_ata,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: dest_ata,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: from_pubkey,
                is_signer: true,
                is_writable: false,
            },
        ],
        data: transfer_data,
    });

    let message = build_message(&from_pubkey, &recent_blockhash, &instructions);
    let message_bytes = serialize_message(&message);
    let sig = keypair.sign_message(&message_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(sig.as_ref());
    let tx_bytes = build_transaction(&[sig_bytes], &message_bytes);

    send_transaction(client, rpc_url, &tx_bytes).await
}

// ========== Solana 交易构建 ==========

pub(crate) struct AccountMeta {
    pub pubkey: [u8; 32],
    pub is_signer: bool,
    pub is_writable: bool,
}

pub(crate) struct Instruction {
    pub program_id: [u8; 32],
    pub accounts: Vec<AccountMeta>,
    pub data: Vec<u8>,
}

struct Message {
    num_required_signatures: u8,
    num_readonly_signed: u8,
    num_readonly_unsigned: u8,
    account_keys: Vec<[u8; 32]>,
    recent_blockhash: [u8; 32],
    instructions: Vec<CompiledInstruction>,
}

struct CompiledInstruction {
    program_id_index: u8,
    account_indices: Vec<u8>,
    data: Vec<u8>,
}

struct AccountKeyMeta {
    pubkey: [u8; 32],
    is_signer: bool,
    is_writable: bool,
    is_fee_payer: bool,
}

fn account_sort_order(key: &AccountKeyMeta) -> u8 {
    if key.is_fee_payer {
        return 0;
    }
    match (key.is_signer, key.is_writable) {
        (true, true) => 1,
        (true, false) => 2,
        (false, true) => 3,
        (false, false) => 4,
    }
}

fn add_key(
    keys: &mut Vec<AccountKeyMeta>,
    pubkey: &[u8; 32],
    is_signer: bool,
    is_writable: bool,
) {
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

fn build_message(
    fee_payer: &[u8; 32],
    recent_blockhash: &[u8; 32],
    instructions: &[Instruction],
) -> Message {
    let mut keys: Vec<AccountKeyMeta> = Vec::new();

    // Fee payer first
    add_key(&mut keys, fee_payer, true, true);

    for ix in instructions {
        for meta in &ix.accounts {
            add_key(&mut keys, &meta.pubkey, meta.is_signer, meta.is_writable);
        }
        add_key(&mut keys, &ix.program_id, false, false);
    }

    // Sort: fee_payer first, then writable signers, readonly signers, writable non-signers, readonly non-signers
    keys.sort_by_key(account_sort_order);

    let num_readonly_signed = keys
        .iter()
        .filter(|k| k.is_signer && !k.is_writable)
        .count() as u8;
    let num_readonly_unsigned = keys
        .iter()
        .filter(|k| !k.is_signer && !k.is_writable)
        .count() as u8;
    let num_signers = keys.iter().filter(|k| k.is_signer).count() as u8;

    let account_keys: Vec<[u8; 32]> = keys.iter().map(|k| k.pubkey).collect();

    let compiled = instructions
        .iter()
        .map(|ix| {
            let program_id_index = account_keys
                .iter()
                .position(|k| k == &ix.program_id)
                .unwrap() as u8;
            let account_indices: Vec<u8> = ix
                .accounts
                .iter()
                .map(|m| {
                    account_keys
                        .iter()
                        .position(|k| k == &m.pubkey)
                        .unwrap() as u8
                })
                .collect();
            CompiledInstruction {
                program_id_index,
                account_indices,
                data: ix.data.clone(),
            }
        })
        .collect();

    Message {
        num_required_signatures: num_signers,
        num_readonly_signed,
        num_readonly_unsigned,
        account_keys,
        recent_blockhash: *recent_blockhash,
        instructions: compiled,
    }
}

fn serialize_message(msg: &Message) -> Vec<u8> {
    let mut buf = Vec::new();

    // Header
    buf.push(msg.num_required_signatures);
    buf.push(msg.num_readonly_signed);
    buf.push(msg.num_readonly_unsigned);

    // Account keys
    encode_compact_u16(&mut buf, msg.account_keys.len() as u16);
    for key in &msg.account_keys {
        buf.extend_from_slice(key);
    }

    // Recent blockhash
    buf.extend_from_slice(&msg.recent_blockhash);

    // Instructions
    encode_compact_u16(&mut buf, msg.instructions.len() as u16);
    for ix in &msg.instructions {
        buf.push(ix.program_id_index);
        encode_compact_u16(&mut buf, ix.account_indices.len() as u16);
        buf.extend_from_slice(&ix.account_indices);
        encode_compact_u16(&mut buf, ix.data.len() as u16);
        buf.extend_from_slice(&ix.data);
    }

    buf
}

pub(crate) fn build_transaction(signatures: &[[u8; 64]], message_bytes: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_compact_u16(&mut buf, signatures.len() as u16);
    for sig in signatures {
        buf.extend_from_slice(sig);
    }
    buf.extend_from_slice(message_bytes);
    buf
}

/// 构建并序列化 Solana 消息（一步完成，避免暴露 Message 类型）
pub(crate) fn build_and_serialize_message(
    fee_payer: &[u8; 32],
    recent_blockhash: &[u8; 32],
    instructions: &[Instruction],
) -> Vec<u8> {
    let message = build_message(fee_payer, recent_blockhash, instructions);
    serialize_message(&message)
}

/// Solana compact-u16 编码
pub(crate) fn encode_compact_u16(buf: &mut Vec<u8>, value: u16) {
    let mut val = value;
    loop {
        let mut byte = (val & 0x7f) as u8;
        val >>= 7;
        if val > 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if val == 0 {
            break;
        }
    }
}

/// 计算 Associated Token Address (PDA)
pub(crate) fn find_associated_token_address(
    wallet: &[u8; 32],
    mint: &[u8; 32],
    token_program: &[u8; 32],
    ata_program: &[u8; 32],
) -> Result<[u8; 32], String> {
    let ata_program_id = Pubkey::new_from_array(*ata_program);
    let (pda, _bump) = Pubkey::find_program_address(
        &[wallet, token_program, mint],
        &ata_program_id,
    );
    Ok(pda.to_bytes())
}

// ========== RPC 辅助 ==========

pub(crate) async fn get_latest_blockhash(client: &Client, rpc_url: &str) -> Result<[u8; 32], String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getLatestBlockhash",
        "params": [{"commitment": "finalized"}],
        "id": 1
    });
    let resp = rpc_call(client, rpc_url, &body).await?;

    let blockhash_str = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.get("blockhash"))
        .and_then(|b| b.as_str())
        .ok_or("获取 blockhash 失败")?;

    let bytes = bs58::decode(blockhash_str)
        .into_vec()
        .map_err(|e| format!("解析 blockhash 失败: {e}"))?;

    bytes
        .try_into()
        .map_err(|_| "blockhash 长度无效".to_string())
}

pub(crate) async fn send_transaction(
    client: &Client,
    rpc_url: &str,
    tx_bytes: &[u8],
) -> Result<String, String> {
    use base64::Engine;
    let tx_base64 = base64::engine::general_purpose::STANDARD.encode(tx_bytes);

    let body = json!({
        "jsonrpc": "2.0",
        "method": "sendTransaction",
        "params": [tx_base64, {"encoding": "base64", "skipPreflight": true}],
        "id": 1
    });
    let resp = rpc_call(client, rpc_url, &body).await?;

    if let Some(error) = resp.get("error") {
        let msg = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("交易失败: {msg}"));
    }

    resp.get("result")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or("未收到交易签名".into())
}

pub(crate) async fn account_exists(client: &Client, rpc_url: &str, address: &str) -> bool {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64"}],
        "id": 1
    });
    if let Ok(resp) = rpc_call(client, rpc_url, &body).await {
        return resp
            .get("result")
            .and_then(|r| r.get("value"))
            .map(|v| !v.is_null())
            .unwrap_or(false);
    }
    false
}

pub(crate) async fn rpc_call(client: &Client, rpc_url: &str, body: &Value) -> Result<Value, String> {
    client
        .post(rpc_url)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("RPC 请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))
}
