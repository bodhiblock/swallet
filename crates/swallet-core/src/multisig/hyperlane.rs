//! Hyperlane Sealevel 管理指令编码
//!
//! Hyperlane 是原生 Solana 程序（非 Anchor），没有 IDL，必须手工 borsh 编码。
//!
//! 两种 discriminator 规则：
//! - **Rule A**: `[0x01;8]` 8字节固定前缀 + variant byte + payload
//!   用于 Multisig ISM、Warp Route
//! - **Rule B**: variant byte + payload（无前缀）
//!   用于 Mailbox
//!
//! 详见 ~/work/nara/nara-hyperlane/admin-instructions.md

use std::collections::HashMap;

use solana_sdk::pubkey::Pubkey;

use super::proposals::{VaultAccountMeta, VaultInstruction};

/// Hyperlane Rule A 固定前缀
const HYPERLANE_PREFIX: [u8; 8] = [0x01; 8];

const SYSTEM_PROGRAM: [u8; 32] = [0u8; 32];

/// 已知的 Hyperlane 部署 program IDs（用于跳过 Anchor 风格的 config 授权检查）
const HYPERLANE_PROGRAM_IDS: &[&str] = &[
    // Mailbox (Nara)
    "EjtLD3MCBJregFKAce2pQqPtSnnmBWK5oAZ3wBifHnaH",
    // Multisig ISM
    "2XenrKdmacQqSn3VAF9nbZNfhbe6YR2Way1WJmSL5Yrj", // Nara
    "6ExBzNNba9vAKMZyXfwE9CsTJmKsXPpdaQC4HxeUUQEJ", // Solana
    // USDC Warp
    "BC2j6WrdPs9xhU9CfBwJsYSnJrGq5Tcm4SEen9ENv7go", // Nara
    "4GcZJTa8s9vxtTz97Vj1RrwKMqPkT3DiiJkvUQDwsuZP", // Solana
    // SOL Warp
    "6bKmjEMbjcJUnqAiNw7AXuMvUALzw5XRKiV9dBsterxg", // Nara
    "46MmAWwKRAt9uvn7m44NXbVq2DCWBQE2r1TDw25nyXrt", // Solana
];

/// 检查给定 program_id 是否是 Hyperlane 部署
pub fn is_hyperlane_program(program_id: &[u8; 32]) -> bool {
    HYPERLANE_PROGRAM_IDS.iter().any(|s| {
        bs58::decode(s)
            .into_vec()
            .ok()
            .and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok())
            .map(|b| &b == program_id)
            .unwrap_or(false)
    })
}

// ==================== PDA 派生 ====================

/// Mailbox: Inbox PDA
fn mailbox_inbox_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"hyperlane", b"-", b"inbox"], program_id).0
}

/// Mailbox: Outbox PDA
fn mailbox_outbox_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"hyperlane", b"-", b"outbox"], program_id).0
}

/// Multisig ISM: Access control PDA
fn ism_access_control_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[b"multisig_ism_message_id", b"-", b"access_control"],
        program_id,
    )
    .0
}

/// Multisig ISM: Domain data PDA（每个 origin domain 一个）
fn ism_domain_data_pda(program_id: &Pubkey, domain: u32) -> Pubkey {
    let domain_bytes = domain.to_le_bytes();
    Pubkey::find_program_address(
        &[
            b"multisig_ism_message_id",
            b"-",
            &domain_bytes,
            b"-",
            b"domain_data",
        ],
        program_id,
    )
    .0
}

/// Warp Route: Token PDA（所有 warp routes 共用同一种 PDA seeds）
fn warp_token_pda(program_id: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"hyperlane_message_recipient",
            b"-",
            b"handle",
            b"-",
            b"account_metas",
        ],
        program_id,
    )
    .0
}

// ==================== 通用 helpers ====================

fn ix(
    program_id: &[u8; 32],
    accounts: Vec<VaultAccountMeta>,
    data: Vec<u8>,
) -> VaultInstruction {
    VaultInstruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

fn meta(pubkey: [u8; 32], is_signer: bool, is_writable: bool) -> VaultAccountMeta {
    VaultAccountMeta {
        pubkey,
        is_signer,
        is_writable,
    }
}

// ==================== 参数解析 ====================

/// 解析 Pubkey（base58）
fn parse_pubkey(s: &str) -> Result<Pubkey, String> {
    s.trim()
        .parse::<Pubkey>()
        .map_err(|e| format!("无效的地址 {s}: {e}"))
}

/// 解析 Option<Pubkey>：空字符串 / "none" / "null" → None
fn parse_optional_pubkey(s: &str) -> Result<Option<Pubkey>, String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("null") {
        Ok(None)
    } else {
        Ok(Some(parse_pubkey(s)?))
    }
}

/// 解析 H160（20 字节 EVM 地址，hex）
fn parse_h160(s: &str) -> Result<[u8; 20], String> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    let bytes = hex::decode(s).map_err(|e| format!("无效的 EVM 地址 hex: {e}"))?;
    if bytes.len() != 20 {
        return Err(format!("EVM 地址必须 20 字节，收到 {}", bytes.len()));
    }
    let mut out = [0u8; 20];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// 解析逗号分隔的 Vec<H160>
fn parse_h160_list(s: &str) -> Result<Vec<[u8; 20]>, String> {
    s.split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(parse_h160)
        .collect()
}

/// 解析 H256（32 字节，hex）—— Warp Route remote router 地址
fn parse_h256(s: &str) -> Result<[u8; 32], String> {
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    let bytes = hex::decode(s).map_err(|e| format!("无效的 32 字节 hex: {e}"))?;
    if bytes.len() != 32 {
        return Err(format!("H256 必须 32 字节，收到 {}", bytes.len()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_u32(s: &str) -> Result<u32, String> {
    s.trim()
        .parse::<u32>()
        .map_err(|e| format!("无效的 u32: {e}"))
}

fn parse_u8(s: &str) -> Result<u8, String> {
    s.trim()
        .parse::<u8>()
        .map_err(|e| format!("无效的 u8: {e}"))
}

// ==================== 编码 helpers ====================

/// Rule A: `[0x01;8] + variant + payload`
fn encode_rule_a(variant: u8, payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(8 + 1 + payload.len());
    data.extend_from_slice(&HYPERLANE_PREFIX);
    data.push(variant);
    data.extend_from_slice(payload);
    data
}

/// Rule B: `variant + payload`
fn encode_rule_b(variant: u8, payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(1 + payload.len());
    data.push(variant);
    data.extend_from_slice(payload);
    data
}

fn encode_option_pubkey(opt: &Option<Pubkey>) -> Vec<u8> {
    match opt {
        Some(pk) => {
            let mut v = Vec::with_capacity(33);
            v.push(1);
            v.extend_from_slice(pk.as_ref());
            v
        }
        None => vec![0],
    }
}

// ==================== Mailbox 指令构建 ====================

/// Mailbox.TransferOwnership(Option<Pubkey>) — variant 9
///
/// args: [new_owner: Optional Pubkey]
pub fn build_mailbox_transfer_ownership(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() {
        return Err("参数不足".into());
    }
    let new_owner = parse_optional_pubkey(&args[0])?;
    let pid = Pubkey::new_from_array(*program_id);
    let outbox = mailbox_outbox_pda(&pid);

    let payload = encode_option_pubkey(&new_owner);
    let data = encode_rule_b(9, &payload);

    Ok(vec![ix(
        program_id,
        vec![
            meta(outbox.to_bytes(), false, true),
            meta(*vault, true, true),
        ],
        data,
    )])
}

/// Mailbox.InboxSetDefaultIsm(Pubkey) — variant 2
///
/// args: [default_ism: Pubkey]
pub fn build_mailbox_set_default_ism(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() {
        return Err("参数不足".into());
    }
    let new_ism = parse_pubkey(&args[0])?;
    let pid = Pubkey::new_from_array(*program_id);
    let inbox = mailbox_inbox_pda(&pid);
    let outbox = mailbox_outbox_pda(&pid);

    let data = encode_rule_b(2, new_ism.as_ref());

    Ok(vec![ix(
        program_id,
        vec![
            meta(inbox.to_bytes(), false, true),
            meta(outbox.to_bytes(), false, false),
            meta(*vault, true, true),
        ],
        data,
    )])
}

// ==================== Multisig ISM 指令构建 ====================

/// Multisig ISM.SetValidatorsAndThreshold — variant 1
///
/// args: [domain: u32, validators: comma-sep H160 list, threshold: u8]
pub fn build_ism_set_validators_and_threshold(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 3 {
        return Err("参数不足：domain, validators, threshold".into());
    }
    let domain = parse_u32(&args[0])?;
    let validators = parse_h160_list(&args[1])?;
    let threshold = parse_u8(&args[2])?;

    if validators.is_empty() {
        return Err("validators 不能为空".into());
    }
    if threshold == 0 || (threshold as usize) > validators.len() {
        return Err(format!(
            "threshold 必须在 1..={} 之间",
            validators.len()
        ));
    }

    // payload: [domain u32 LE][Vec<H160>: u32 LE len + 20*N][threshold u8]
    let mut payload =
        Vec::with_capacity(4 + 4 + 20 * validators.len() + 1);
    payload.extend_from_slice(&domain.to_le_bytes());
    payload.extend_from_slice(&(validators.len() as u32).to_le_bytes());
    for v in &validators {
        payload.extend_from_slice(v);
    }
    payload.push(threshold);

    let data = encode_rule_a(1, &payload);

    let pid = Pubkey::new_from_array(*program_id);
    let access_control = ism_access_control_pda(&pid);
    let domain_data = ism_domain_data_pda(&pid, domain);

    Ok(vec![ix(
        program_id,
        vec![
            meta(*vault, true, true),
            meta(access_control.to_bytes(), false, false),
            meta(domain_data.to_bytes(), false, true),
            // system_program 仅首次创建 domain PDA 时需要，无脑加上无害
            meta(SYSTEM_PROGRAM, false, false),
        ],
        data,
    )])
}

/// Multisig ISM.TransferOwnership(Option<Pubkey>) — variant 3
pub fn build_ism_transfer_ownership(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() {
        return Err("参数不足".into());
    }
    let new_owner = parse_optional_pubkey(&args[0])?;
    let pid = Pubkey::new_from_array(*program_id);
    let access_control = ism_access_control_pda(&pid);

    let payload = encode_option_pubkey(&new_owner);
    let data = encode_rule_a(3, &payload);

    Ok(vec![ix(
        program_id,
        vec![
            meta(*vault, true, true),
            meta(access_control.to_bytes(), false, true),
        ],
        data,
    )])
}

// ==================== Warp Route 指令构建 ====================

/// Warp Route.SetInterchainSecurityModule(Option<Pubkey>) — variant 5
pub fn build_warp_set_ism(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() {
        return Err("参数不足".into());
    }
    let new_ism = parse_optional_pubkey(&args[0])?;
    let pid = Pubkey::new_from_array(*program_id);
    let token = warp_token_pda(&pid);

    let payload = encode_option_pubkey(&new_ism);
    let data = encode_rule_a(5, &payload);

    Ok(vec![ix(
        program_id,
        vec![
            meta(token.to_bytes(), false, true),
            meta(*vault, true, false),
        ],
        data,
    )])
}

/// Warp Route.TransferOwnership(Option<Pubkey>) — variant 7
pub fn build_warp_transfer_ownership(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.is_empty() {
        return Err("参数不足".into());
    }
    let new_owner = parse_optional_pubkey(&args[0])?;
    let pid = Pubkey::new_from_array(*program_id);
    let token = warp_token_pda(&pid);

    let payload = encode_option_pubkey(&new_owner);
    let data = encode_rule_a(7, &payload);

    Ok(vec![ix(
        program_id,
        vec![
            meta(token.to_bytes(), false, true),
            meta(*vault, true, false),
        ],
        data,
    )])
}

/// Warp Route.EnrollRemoteRouter(RemoteRouterConfig) — variant 2
///
/// args: [domain: u32, router: H256 (留空 = None 注销)]
pub fn build_warp_enroll_remote_router(
    vault: &[u8; 32],
    program_id: &[u8; 32],
    args: &[String],
) -> Result<Vec<VaultInstruction>, String> {
    if args.len() < 2 {
        return Err("参数不足：domain, router".into());
    }
    let domain = parse_u32(&args[0])?;
    let router_str = args[1].trim();
    let router_opt: Option<[u8; 32]> = if router_str.is_empty()
        || router_str.eq_ignore_ascii_case("none")
        || router_str.eq_ignore_ascii_case("null")
    {
        None
    } else {
        Some(parse_h256(router_str)?)
    };

    // payload: [domain u32 LE][Option<H256>]
    let mut payload = Vec::with_capacity(4 + 1 + 32);
    payload.extend_from_slice(&domain.to_le_bytes());
    match router_opt {
        Some(r) => {
            payload.push(1);
            payload.extend_from_slice(&r);
        }
        None => payload.push(0),
    }

    let data = encode_rule_a(2, &payload);

    let pid = Pubkey::new_from_array(*program_id);
    let token = warp_token_pda(&pid);

    Ok(vec![ix(
        program_id,
        vec![
            meta(SYSTEM_PROGRAM, false, false),
            meta(token.to_bytes(), false, true),
            meta(*vault, true, false),
        ],
        data,
    )])
}

// ==================== 链上 config 值获取（用于参数输入提示） ====================

/// 判断 program_id 属于哪种 Hyperlane 部署
fn program_type(program_id: &[u8; 32]) -> Option<HlProgramType> {
    let id_str = bs58::encode(program_id).into_string();
    match id_str.as_str() {
        "EjtLD3MCBJregFKAce2pQqPtSnnmBWK5oAZ3wBifHnaH" => Some(HlProgramType::Mailbox),
        "2XenrKdmacQqSn3VAF9nbZNfhbe6YR2Way1WJmSL5Yrj"
        | "6ExBzNNba9vAKMZyXfwE9CsTJmKsXPpdaQC4HxeUUQEJ" => Some(HlProgramType::Ism),
        "BC2j6WrdPs9xhU9CfBwJsYSnJrGq5Tcm4SEen9ENv7go"
        | "4GcZJTa8s9vxtTz97Vj1RrwKMqPkT3DiiJkvUQDwsuZP"
        | "6bKmjEMbjcJUnqAiNw7AXuMvUALzw5XRKiV9dBsterxg"
        | "46MmAWwKRAt9uvn7m44NXbVq2DCWBQE2r1TDw25nyXrt" => Some(HlProgramType::Warp),
        _ => None,
    }
}

enum HlProgramType {
    Mailbox,
    Ism,
    Warp,
}

/// 通过 RPC 获取单个账户的 base64 原始数据
async fn fetch_account_base64(
    client: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<Option<Vec<u8>>, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "getAccountInfo",
        "params": [address, {"encoding": "base64", "commitment": "confirmed"}],
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

    let value = &resp["result"]["value"];
    if value.is_null() {
        return Ok(None);
    }
    let b64 = value["data"][0]
        .as_str()
        .ok_or("account data missing")?;
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 解码失败: {e}"))?;
    Ok(Some(raw))
}

/// 从 AccountData<T> 封装的字节里读一个 Option<Pubkey>
/// 返回格式化的 bs58 字符串，或 "None"
fn read_option_pubkey(data: &[u8], offset: usize) -> Option<String> {
    if data.len() <= offset {
        return None;
    }
    match data[offset] {
        0 => Some("None".to_string()),
        1 => {
            if data.len() < offset + 1 + 32 {
                return None;
            }
            let pk: [u8; 32] = data[offset + 1..offset + 33].try_into().ok()?;
            Some(bs58::encode(pk).into_string())
        }
        _ => None,
    }
}

/// 从字节里读一个 Pubkey（32 字节）
fn read_pubkey(data: &[u8], offset: usize) -> Option<String> {
    if data.len() < offset + 32 {
        return None;
    }
    let pk: [u8; 32] = data[offset..offset + 32].try_into().ok()?;
    Some(bs58::encode(pk).into_string())
}

/// 获取 Hyperlane 程序当前的 config 值（用于提示）
///
/// 根据 program_id 识别类型并分派到不同 PDA 解析。
/// 对于 domain 特定的指令（SetValidatorsAndThreshold / EnrollRemoteRouter），
/// 因为需要先知道 domain 才能派生 PDA，这里不返回相关字段。
pub async fn fetch_hyperlane_config_values(
    client: &reqwest::Client,
    rpc_url: &str,
    program_id: &[u8; 32],
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let Some(ty) = program_type(program_id) else {
        return Ok(map);
    };
    let pid = Pubkey::new_from_array(*program_id);

    match ty {
        HlProgramType::Mailbox => {
            // Outbox: init(1) + local_domain(4) + bump(1) + owner Option<Pubkey>
            let outbox = mailbox_outbox_pda(&pid);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &outbox.to_string()).await
                && let Some(owner) = read_option_pubkey(&data, 6)
            {
                map.insert("owner".to_string(), owner);
            }

            // Inbox: init(1) + local_domain(4) + bump(1) + default_ism(32) + processed_count(8)
            let inbox = mailbox_inbox_pda(&pid);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &inbox.to_string()).await
                && let Some(ism) = read_pubkey(&data, 6)
            {
                map.insert("default_ism".to_string(), ism);
            }
        }
        HlProgramType::Ism => {
            // AccessControl: init(1) + bump(1) + owner Option<Pubkey>
            let access_control = ism_access_control_pda(&pid);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &access_control.to_string()).await
                && let Some(owner) = read_option_pubkey(&data, 2)
            {
                map.insert("owner".to_string(), owner);
            }
        }
        HlProgramType::Warp => {
            // HyperlaneToken:
            // offset 0: init(1)
            // offset 1: bump(1)
            // offset 2..34: mailbox(32)
            // offset 34..66: mailbox_process_authority(32)
            // offset 66: dispatch_authority_bump(1)
            // offset 67: decimals(1)
            // offset 68: remote_decimals(1)
            // offset 69: owner Option<Pubkey>
            //   if Some: 70..102 = owner pubkey, next field starts at 102
            //   if None: next field starts at 70
            // next: interchain_security_module Option<Pubkey>
            let token = warp_token_pda(&pid);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &token.to_string()).await
            {
                // mailbox
                if let Some(mb) = read_pubkey(&data, 2) {
                    map.insert("mailbox".to_string(), mb);
                }
                // decimals
                if data.len() > 67 {
                    map.insert("decimals".to_string(), data[67].to_string());
                }
                // owner at offset 69
                let owner_tag_offset = 69;
                let ism_offset = if data.len() > owner_tag_offset {
                    match data[owner_tag_offset] {
                        0 => {
                            map.insert("owner".to_string(), "None".to_string());
                            Some(owner_tag_offset + 1)
                        }
                        1 => {
                            if let Some(owner) = read_pubkey(&data, owner_tag_offset + 1) {
                                map.insert("owner".to_string(), owner);
                            }
                            Some(owner_tag_offset + 1 + 32)
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                // interchain_security_module
                if let Some(ism_off) = ism_offset
                    && let Some(ism) = read_option_pubkey(&data, ism_off)
                {
                    map.insert("interchain_security_module".to_string(), ism);
                }
            }
        }
    }

    Ok(map)
}

/// 解析 ISM DomainData PDA 的字节，返回 (validators_newline_separated, threshold)
///
/// 布局（AccountData<DomainData>）:
/// offset 0: initialized(1) + bump(1) + Vec<H160>(4 len + 20*N) + threshold(1)
///
/// 多个 validator 用 `\n` 分隔，渲染层据此换行显示
fn parse_domain_data(data: &[u8]) -> Option<(String, u8)> {
    if data.len() < 6 {
        return None;
    }
    // skip initialized(1) + bump(1)
    let len = u32::from_le_bytes(data[2..6].try_into().ok()?) as usize;
    let needed = 6 + 20 * len + 1;
    if data.len() < needed {
        return None;
    }
    let mut validators = Vec::with_capacity(len);
    for i in 0..len {
        let off = 6 + i * 20;
        let slice = &data[off..off + 20];
        validators.push(format!("0x{}", hex::encode(slice)));
    }
    let threshold = data[6 + 20 * len];
    Some((validators.join("\n"), threshold))
}

/// 在 Token PDA 字节里跳过 owner/ism/igp/destination_gas，
/// 定位到 remote_routers HashMap，查找指定 domain 的 H256 并返回 hex
fn parse_warp_remote_router(data: &[u8], target_domain: u32) -> Option<String> {
    // fixed header offsets (see accounts.rs):
    // 0: init(1), 1: bump(1), 2..34: mailbox(32), 34..66: mba(32),
    // 66: dispatch_authority_bump(1), 67: decimals(1), 68: remote_decimals(1)
    // 69: owner Option<Pubkey>
    let mut off = 69;

    // owner Option<Pubkey>
    if data.len() <= off {
        return None;
    }
    off += 1;
    if data[off - 1] == 1 {
        off += 32;
    }

    // interchain_security_module Option<Pubkey>
    if data.len() <= off {
        return None;
    }
    off += 1;
    if data[off - 1] == 1 {
        off += 32;
    }

    // interchain_gas_paymaster Option<(Pubkey, InterchainGasPaymasterType)>
    // Some: 32 (pubkey) + 1 (igp enum variant) + 32 (inner pubkey) = 65
    if data.len() <= off {
        return None;
    }
    off += 1;
    if data[off - 1] == 1 {
        off += 65;
    }

    // destination_gas HashMap<u32, u64>: 4 bytes len + 12 * len
    if data.len() < off + 4 {
        return None;
    }
    let dg_len = u32::from_le_bytes(data[off..off + 4].try_into().ok()?) as usize;
    off += 4 + 12 * dg_len;

    // remote_routers HashMap<u32, H256>: 4 bytes len + 36 * len
    if data.len() < off + 4 {
        return None;
    }
    let rr_len = u32::from_le_bytes(data[off..off + 4].try_into().ok()?) as usize;
    off += 4;
    for _ in 0..rr_len {
        if data.len() < off + 4 + 32 {
            return None;
        }
        let domain = u32::from_le_bytes(data[off..off + 4].try_into().ok()?);
        off += 4;
        if domain == target_domain {
            let router = &data[off..off + 32];
            return Some(format!("0x{}", hex::encode(router)));
        }
        off += 32;
    }
    None
}

/// 动态 hints：根据当前指令名和已输入参数，返回额外的 config hints
///
/// 支持的场景：
/// - ISM.SetValidatorsAndThreshold: 输入 domain 后返回当前 validators + threshold
/// - Warp.EnrollRemoteRouter: 输入 domain 后返回当前 router
pub async fn fetch_dynamic_hints(
    client: &reqwest::Client,
    rpc_url: &str,
    program_id: &[u8; 32],
    instruction_name: &str,
    args_so_far: &[String],
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let Some(ty) = program_type(program_id) else {
        return Ok(map);
    };

    // 所有支持的动态 hint 指令第一个参数都是 domain
    if args_so_far.is_empty() {
        return Ok(map);
    }
    let Ok(domain) = args_so_far[0].parse::<u32>() else {
        return Ok(map);
    };

    let pid = Pubkey::new_from_array(*program_id);

    match (ty, instruction_name) {
        (HlProgramType::Ism, "set_validators_and_threshold") => {
            let domain_pda = ism_domain_data_pda(&pid, domain);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &domain_pda.to_string()).await
                && let Some((validators, threshold)) = parse_domain_data(&data)
            {
                map.insert("validators".to_string(), validators);
                map.insert("threshold".to_string(), threshold.to_string());
            }
        }
        (HlProgramType::Warp, "enroll_remote_router") => {
            let token_pda = warp_token_pda(&pid);
            if let Ok(Some(data)) =
                fetch_account_base64(client, rpc_url, &token_pda.to_string()).await
                && let Some(router) = parse_warp_remote_router(&data, domain)
            {
                map.insert("router".to_string(), router);
            }
        }
        _ => {}
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 SetValidatorsAndThreshold 编码与文档示例 hex 一致
    /// 文档 section 2.1 示例:
    /// - domain = 4077895904 (Nara)
    /// - validators = 3 个 EVM 地址
    /// - threshold = 2
    #[test]
    fn test_set_validators_and_threshold_golden() {
        let vault = [0u8; 32];
        let program_id = [0u8; 32];
        let args = vec![
            "4077895904".to_string(),
            "0x8707e152a0824335a60e57161cf8d138201527ae,0x952a8a35dca62d857183644897e1700b35f2511f,0xe305aac5b48ecaf730b997128ddcaf12905fc280".to_string(),
            "2".to_string(),
        ];
        let ixs = build_ism_set_validators_and_threshold(&vault, &program_id, &args).unwrap();
        assert_eq!(ixs.len(), 1);
        let data = &ixs[0].data;

        // 期望长度 = 8 (prefix) + 1 (variant) + 4 (domain) + 4 (vec len) + 60 (3*20) + 1 (threshold) = 78
        assert_eq!(data.len(), 78);

        // 检查前缀 + variant
        assert_eq!(&data[0..8], &[0x01u8; 8]);
        assert_eq!(data[8], 0x01); // variant 1

        // 检查 domain LE
        let domain_bytes = &data[9..13];
        assert_eq!(domain_bytes, &[0xe0, 0xc0, 0x0f, 0xf3]);

        // Vec len = 3
        assert_eq!(&data[13..17], &[0x03, 0x00, 0x00, 0x00]);

        // 第一个 validator
        let v1_expected = hex::decode("8707e152a0824335a60e57161cf8d138201527ae").unwrap();
        assert_eq!(&data[17..37], v1_expected.as_slice());

        // threshold
        assert_eq!(data[77], 2);
    }

    #[test]
    fn test_transfer_ownership_some() {
        let vault = [1u8; 32];
        let program_id = [2u8; 32];
        let owner_pk = Pubkey::new_unique();
        let args = vec![owner_pk.to_string()];

        let ixs = build_mailbox_transfer_ownership(&vault, &program_id, &args).unwrap();
        assert_eq!(ixs.len(), 1);
        let data = &ixs[0].data;
        // Rule B: variant(9) + Some(1) + 32 bytes pubkey
        assert_eq!(data.len(), 1 + 1 + 32);
        assert_eq!(data[0], 9);
        assert_eq!(data[1], 1);
        assert_eq!(&data[2..34], owner_pk.as_ref());
    }

    #[test]
    fn test_transfer_ownership_none() {
        let vault = [1u8; 32];
        let program_id = [2u8; 32];
        let args = vec!["".to_string()];

        let ixs = build_mailbox_transfer_ownership(&vault, &program_id, &args).unwrap();
        let data = &ixs[0].data;
        assert_eq!(data.len(), 2);
        assert_eq!(data[0], 9);
        assert_eq!(data[1], 0);
    }

    #[test]
    fn test_warp_set_ism() {
        let vault = [1u8; 32];
        let program_id = [2u8; 32];
        let ism_pk = Pubkey::new_unique();
        let args = vec![ism_pk.to_string()];

        let ixs = build_warp_set_ism(&vault, &program_id, &args).unwrap();
        let data = &ixs[0].data;
        // Rule A: 8 prefix + variant(5) + Some(1) + 32 pubkey = 42
        assert_eq!(data.len(), 8 + 1 + 1 + 32);
        assert_eq!(&data[0..8], &[0x01u8; 8]);
        assert_eq!(data[8], 5);
        assert_eq!(data[9], 1);
        assert_eq!(&data[10..42], ism_pk.as_ref());
    }

    #[test]
    fn test_h160_parsing() {
        let h = parse_h160("0x8707e152a0824335a60e57161cf8d138201527ae").unwrap();
        assert_eq!(h.len(), 20);
        assert_eq!(h[0], 0x87);
        assert_eq!(h[19], 0xae);

        // 不带 0x 前缀
        let h2 = parse_h160("8707e152a0824335a60e57161cf8d138201527ae").unwrap();
        assert_eq!(h, h2);

        // 错误长度
        assert!(parse_h160("0x1234").is_err());
    }

    #[test]
    fn test_h160_list_parsing() {
        let list = parse_h160_list(
            "0x8707e152a0824335a60e57161cf8d138201527ae, 0x952a8a35dca62d857183644897e1700b35f2511f",
        )
        .unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_optional_pubkey_parsing() {
        assert!(parse_optional_pubkey("").unwrap().is_none());
        assert!(parse_optional_pubkey("none").unwrap().is_none());
        assert!(parse_optional_pubkey("NULL").unwrap().is_none());
        assert!(parse_optional_pubkey("11111111111111111111111111111111").unwrap().is_some());
    }
}
