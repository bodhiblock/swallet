use reqwest::Client;
use serde_json::{json, Value};

use crate::config::SolanaChainConfig;

use super::{ChainBalance, TokenBalance};

/// 查询某个地址在某条 Solana 链上的所有资产
pub async fn query_sol_balance(
    client: &Client,
    config: &SolanaChainConfig,
    address: &str,
) -> Result<ChainBalance, String> {
    // 原生币余额
    let native_balance = get_sol_balance(client, &config.rpc_url, address).await?;

    // 质押余额
    let staked_balance = get_stake_balance(client, &config.rpc_url, address)
        .await
        .unwrap_or(0);

    // SPL Token 余额
    let mut tokens = Vec::new();

    // 标准 SPL Token 程序
    let spl_accounts = get_token_accounts(
        client,
        &config.rpc_url,
        address,
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    )
    .await
    .unwrap_or_default();

    // Token-2022 程序
    let spl2022_accounts = get_token_accounts(
        client,
        &config.rpc_url,
        address,
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    )
    .await
    .unwrap_or_default();

    let all_accounts: Vec<_> = spl_accounts
        .into_iter()
        .chain(spl2022_accounts)
        .collect();

    // 匹配配置中的代币
    for token_config in &config.tokens {
        for (mint, balance, _decimals) in &all_accounts {
            if mint == &token_config.mint_address && *balance > 0 {
                tokens.push(TokenBalance {
                    symbol: token_config.symbol.clone(),
                    decimals: token_config.decimals,
                    balance: *balance,
                });
                break;
            }
        }
    }

    Ok(ChainBalance {
        chain_id: config.id.clone(),
        chain_name: config.name.clone(),
        native_symbol: config.native_symbol.clone(),
        native_decimals: config.native_decimals,
        native_balance,
        staked_balance,
        tokens,
        rpc_failed: false,
    })
}

/// getBalance
async fn get_sol_balance(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<u128, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getBalance",
        "params": [address],
        "id": 1
    });

    let resp: Value = rpc_call(client, rpc_url, &body).await?;

    resp.get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u128)
        .ok_or("解析余额失败".into())
}

/// 查询质押余额（简化版：遍历 stake 账户）
async fn get_stake_balance(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<u128, String> {
    // 使用 getProgramAccounts 查询用户的 stake 账户
    // Stake program ID: Stake11111111111111111111111111111111111111
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getProgramAccounts",
        "params": [
            "Stake11111111111111111111111111111111111111",
            {
                "encoding": "jsonParsed",
                "filters": [
                    {
                        "memcmp": {
                            "offset": 12,
                            "bytes": address
                        }
                    }
                ]
            }
        ],
        "id": 1
    });

    let resp: Value = rpc_call(client, rpc_url, &body).await?;

    let accounts = resp
        .get("result")
        .and_then(|r| r.as_array())
        .ok_or("解析质押账户失败")?;

    let mut total_staked: u128 = 0;
    for account in accounts {
        if let Some(lamports) = account
            .get("account")
            .and_then(|a| a.get("lamports"))
            .and_then(|l| l.as_u64())
        {
            total_staked += lamports as u128;
        }
    }

    Ok(total_staked)
}

/// getTokenAccountsByOwner
async fn get_token_accounts(
    client: &Client,
    rpc_url: &str,
    address: &str,
    program_id: &str,
) -> Result<Vec<(String, u128, u8)>, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getTokenAccountsByOwner",
        "params": [
            address,
            {"programId": program_id},
            {"encoding": "jsonParsed"}
        ],
        "id": 1
    });

    let resp: Value = rpc_call(client, rpc_url, &body).await?;

    let accounts = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_array())
        .ok_or("解析 Token 账户失败")?;

    let mut result = Vec::new();
    for account in accounts {
        let info = account
            .get("account")
            .and_then(|a| a.get("data"))
            .and_then(|d| d.get("parsed"))
            .and_then(|p| p.get("info"));

        if let Some(info) = info {
            let mint = info.get("mint").and_then(|m| m.as_str()).unwrap_or("");
            let token_amount = info.get("tokenAmount");

            if let Some(ta) = token_amount {
                let amount_str = ta.get("amount").and_then(|a| a.as_str()).unwrap_or("0");
                let decimals = ta.get("decimals").and_then(|d| d.as_u64()).unwrap_or(0) as u8;
                let balance: u128 = amount_str.parse().unwrap_or(0);

                if balance > 0 {
                    result.push((mint.to_string(), balance, decimals));
                }
            }
        }
    }

    Ok(result)
}

/// 通用 RPC 调用
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
