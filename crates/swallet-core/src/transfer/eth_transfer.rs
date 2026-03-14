use k256::ecdsa::SigningKey;
use reqwest::Client;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};

/// 发送 ETH 原生转账
pub async fn send_eth_native(
    client: &Client,
    rpc_url: &str,
    chain_id: u64,
    private_key: &[u8],
    to: &str,
    amount_wei: u128,
) -> Result<String, String> {
    let from = crate::crypto::eth_keys::private_key_to_eth_address(private_key)
        .map_err(|e| format!("地址计算失败: {e}"))?;

    let nonce = get_nonce(client, rpc_url, &from).await?;
    let gas_price = get_gas_price(client, rpc_url).await?;
    let gas_limit: u128 = 21000;

    let tx_bytes = encode_and_sign_legacy_tx(
        nonce, gas_price, gas_limit, to, amount_wei, &[], chain_id, private_key,
    )?;

    send_raw_transaction(client, rpc_url, &tx_bytes).await
}

/// 发送 ERC20 代币转账
pub async fn send_erc20(
    client: &Client,
    rpc_url: &str,
    chain_id: u64,
    private_key: &[u8],
    contract: &str,
    to: &str,
    amount: u128,
) -> Result<String, String> {
    let from = crate::crypto::eth_keys::private_key_to_eth_address(private_key)
        .map_err(|e| format!("地址计算失败: {e}"))?;

    // transfer(address,uint256) = 0xa9059cbb
    let to_clean = to.strip_prefix("0x").unwrap_or(to);
    let data_hex = format!("a9059cbb{:0>64}{:064x}", to_clean, amount);
    let data = hex::decode(&data_hex).map_err(|e| format!("数据编码失败: {e}"))?;

    let nonce = get_nonce(client, rpc_url, &from).await?;
    let gas_price = get_gas_price(client, rpc_url).await?;
    let gas_limit: u128 = 65000;

    let tx_bytes = encode_and_sign_legacy_tx(
        nonce, gas_price, gas_limit, contract, 0, &data, chain_id, private_key,
    )?;

    send_raw_transaction(client, rpc_url, &tx_bytes).await
}

// ========== RLP 编码 ==========

/// RLP 编码字节数组
fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return vec![0x80];
    }
    if data.len() == 1 && data[0] < 0x80 {
        return data.to_vec();
    }
    if data.len() <= 55 {
        let mut result = vec![0x80 + data.len() as u8];
        result.extend_from_slice(data);
        result
    } else {
        let len_bytes = to_be_bytes_trimmed(data.len() as u128);
        let mut result = vec![0xb7 + len_bytes.len() as u8];
        result.extend_from_slice(&len_bytes);
        result.extend_from_slice(data);
        result
    }
}

/// RLP 编码列表
fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut content: Vec<u8> = Vec::new();
    for item in items {
        content.extend(item);
    }

    if content.len() <= 55 {
        let mut result = vec![0xc0 + content.len() as u8];
        result.extend(content);
        result
    } else {
        let len_bytes = to_be_bytes_trimmed(content.len() as u128);
        let mut result = vec![0xf7 + len_bytes.len() as u8];
        result.extend_from_slice(&len_bytes);
        result.extend(content);
        result
    }
}

/// u128 转大端字节，去除前导零
fn to_be_bytes_trimmed(value: u128) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }
    let bytes = value.to_be_bytes();
    let first_non_zero = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
    bytes[first_non_zero..].to_vec()
}

/// RLP 编码 u128
fn rlp_encode_u128(value: u128) -> Vec<u8> {
    let bytes = to_be_bytes_trimmed(value);
    rlp_encode_bytes(&bytes)
}

/// RLP 编码以太坊地址（20 bytes）
fn rlp_encode_address(addr: &str) -> Result<Vec<u8>, String> {
    let clean = addr.strip_prefix("0x").unwrap_or(addr);
    if clean.len() != 40 || !clean.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("无效的 ETH 地址: {addr}"));
    }
    let bytes = hex::decode(clean).map_err(|e| format!("地址解码失败: {e}"))?;
    if bytes.len() != 20 {
        return Err(format!("地址长度无效: 期望 20 字节，实际 {} 字节", bytes.len()));
    }
    Ok(rlp_encode_bytes(&bytes))
}

// ========== 交易签名 ==========

/// 构造并签名 Legacy 交易（EIP-155）
#[allow(clippy::too_many_arguments)]
fn encode_and_sign_legacy_tx(
    nonce: u128,
    gas_price: u128,
    gas_limit: u128,
    to: &str,
    value: u128,
    data: &[u8],
    chain_id: u64,
    private_key: &[u8],
) -> Result<Vec<u8>, String> {
    // 验证目标地址
    let to_encoded = rlp_encode_address(to)?;

    // EIP-155 签名消息: RLP([nonce, gasPrice, gasLimit, to, value, data, chainId, 0, 0])
    let signing_items = vec![
        rlp_encode_u128(nonce),
        rlp_encode_u128(gas_price),
        rlp_encode_u128(gas_limit),
        to_encoded.clone(),
        rlp_encode_u128(value),
        rlp_encode_bytes(data),
        rlp_encode_u128(chain_id as u128),
        rlp_encode_u128(0),
        rlp_encode_u128(0),
    ];
    let signing_rlp = rlp_encode_list(&signing_items);

    // Keccak256 哈希
    let hash = Keccak256::digest(&signing_rlp);

    // ECDSA 签名 (with recovery ID)
    let signing_key = SigningKey::from_bytes(private_key.into())
        .map_err(|e| format!("无效的私钥: {e}"))?;

    let (signature, recovery_id) = signing_key
        .sign_prehash_recoverable(hash.as_slice())
        .map_err(|e| format!("签名失败: {e}"))?;

    let sig_bytes = signature.to_bytes();
    let r_bytes = &sig_bytes[..32];
    let s_bytes = &sig_bytes[32..];
    let v = chain_id * 2 + 35 + recovery_id.to_byte() as u64;

    // 最终交易: RLP([nonce, gasPrice, gasLimit, to, value, data, v, r, s])
    let tx_items = vec![
        rlp_encode_u128(nonce),
        rlp_encode_u128(gas_price),
        rlp_encode_u128(gas_limit),
        to_encoded,
        rlp_encode_u128(value),
        rlp_encode_bytes(data),
        rlp_encode_u128(v as u128),
        rlp_encode_bytes(r_bytes),
        rlp_encode_bytes(s_bytes),
    ];

    Ok(rlp_encode_list(&tx_items))
}

// ========== RPC 辅助 ==========

async fn get_nonce(client: &Client, rpc_url: &str, address: &str) -> Result<u128, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "pending"],
        "id": 1
    });
    let resp = rpc_call(client, rpc_url, &body).await?;
    parse_hex_u128(&resp)
}

async fn get_gas_price(client: &Client, rpc_url: &str) -> Result<u128, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });
    let resp = rpc_call(client, rpc_url, &body).await?;
    parse_hex_u128(&resp)
}

async fn send_raw_transaction(
    client: &Client,
    rpc_url: &str,
    tx_bytes: &[u8],
) -> Result<String, String> {
    let tx_hex = format!("0x{}", hex::encode(tx_bytes));
    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [tx_hex],
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
        .ok_or("未收到交易哈希".into())
}

fn parse_hex_u128(resp: &Value) -> Result<u128, String> {
    if let Some(error) = resp.get("error") {
        return Err(format!("RPC 错误: {error}"));
    }
    let result = resp
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or("响应缺少 result")?;
    let hex_str = result.strip_prefix("0x").unwrap_or(result);
    if hex_str.is_empty() || hex_str == "0" {
        return Ok(0);
    }
    u128::from_str_radix(hex_str, 16).map_err(|e| format!("解析失败: {e}"))
}

async fn rpc_call(client: &Client, rpc_url: &str, body: &Value) -> Result<Value, String> {
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
