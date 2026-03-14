use reqwest::Client;
use serde_json::{json, Value};

use crate::config::EvmChainConfig;

use super::{ChainBalance, TokenBalance};

/// 查询某个地址在某条 EVM 链上的所有资产
pub async fn query_evm_balance(
    client: &Client,
    config: &EvmChainConfig,
    address: &str,
) -> Result<ChainBalance, String> {
    // 查询原生币余额
    let native_balance = get_native_balance(client, &config.rpc_url, address).await?;

    // 查询所有配置的 ERC20 代币
    let mut tokens = Vec::new();
    for token_config in &config.tokens {
        match get_erc20_balance(client, &config.rpc_url, &token_config.contract_address, address)
            .await
        {
            Ok(balance) => {
                if balance > 0 {
                    tokens.push(TokenBalance {
                        symbol: token_config.symbol.clone(),
                        decimals: token_config.decimals,
                        balance,
                    });
                }
            }
            Err(_) => {
                // 单个代币查询失败不影响整体
            }
        }
    }

    Ok(ChainBalance {
        chain_id: config.id.clone(),
        chain_name: config.name.clone(),
        native_symbol: config.native_symbol.clone(),
        native_decimals: config.native_decimals,
        native_balance,
        staked_balance: 0,
        tokens,
        rpc_failed: false,
    })
}

/// eth_getBalance
async fn get_native_balance(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<u128, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1
    });

    let resp: Value = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("RPC 请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))?;

    parse_hex_balance(&resp)
}

/// ERC20 balanceOf(address)
async fn get_erc20_balance(
    client: &Client,
    rpc_url: &str,
    contract: &str,
    address: &str,
) -> Result<u128, String> {
    // balanceOf(address) = 0x70a08231 + address padded to 32 bytes
    let addr_clean = address.strip_prefix("0x").unwrap_or(address);
    let data = format!("0x70a08231{:0>64}", addr_clean);

    let body = json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{"to": contract, "data": data}, "latest"],
        "id": 1
    });

    let resp: Value = client
        .post(rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("RPC 请求失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {e}"))?;

    parse_hex_balance(&resp)
}

/// 解析 JSON-RPC 返回的 hex 余额
fn parse_hex_balance(resp: &Value) -> Result<u128, String> {
    if let Some(error) = resp.get("error") {
        return Err(format!("RPC 错误: {error}"));
    }

    let result = resp
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or("响应缺少 result 字段")?;

    let hex_str = result.strip_prefix("0x").unwrap_or(result);
    if hex_str.is_empty() || hex_str == "0" {
        return Ok(0);
    }

    u128::from_str_radix(hex_str, 16).map_err(|e| format!("解析余额失败: {e}"))
}
