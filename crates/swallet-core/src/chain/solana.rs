use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use spl_token::solana_program::program_pack::Pack;
use std::str::FromStr;

use crate::config::SolanaChainConfig;

use super::{ChainBalance, TokenBalance};

const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const SPL_TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const ATA_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const STAKE_PROGRAM: &str = "Stake11111111111111111111111111111111111111";
pub const VOTE_PROGRAM: &str = "Vote111111111111111111111111111111111111111";
pub const SYSTEM_PROGRAM_STR: &str = "11111111111111111111111111111111";

/// getMultipleAccounts 返回的账户信息
#[derive(Debug, Clone)]
pub struct RawAccountInfo {
    pub lamports: u64,
    pub owner: String,
    pub data: Vec<u8>,
}

/// 批量查询账户信息（自动分批，每批最多 100 个）
pub async fn get_multiple_accounts(
    client: &Client,
    rpc_url: &str,
    addresses: &[String],
) -> Result<Vec<Option<RawAccountInfo>>, String> {
    if addresses.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_results: Vec<Option<RawAccountInfo>> = Vec::with_capacity(addresses.len());

    // 每批最多 100 个地址
    for chunk in addresses.chunks(100) {
        let addr_values: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        let body = json!({
            "jsonrpc": "2.0",
            "method": "getMultipleAccounts",
            "params": [addr_values, {"encoding": "base64", "commitment": "confirmed"}],
            "id": 1
        });

        let resp: Value = rpc_call(client, rpc_url, &body).await?;

        let values = resp
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_array())
            .ok_or("解析 getMultipleAccounts 失败")?;

        for val in values {
            if val.is_null() {
                all_results.push(None);
                continue;
            }

            let lamports = val.get("lamports").and_then(|l| l.as_u64()).unwrap_or(0);
            let owner = val
                .get("owner")
                .and_then(|o| o.as_str())
                .unwrap_or("")
                .to_string();

            let data = if let Some(data_arr) = val.get("data").and_then(|d| d.as_array()) {
                let b64_str = data_arr
                    .first()
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(b64_str)
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            all_results.push(Some(RawAccountInfo {
                lamports,
                owner,
                data,
            }));
        }
    }

    Ok(all_results)
}

/// 计算 ATA (Associated Token Account) 地址
pub fn derive_ata_address(wallet: &str, mint: &str, is_token_2022: bool) -> Option<String> {
    let wallet_pubkey = Pubkey::from_str(wallet).ok()?;
    let mint_pubkey = Pubkey::from_str(mint).ok()?;
    let token_program = if is_token_2022 {
        Pubkey::from_str(SPL_TOKEN_2022_PROGRAM).ok()?
    } else {
        Pubkey::from_str(SPL_TOKEN_PROGRAM).ok()?
    };
    let ata_program = Pubkey::from_str(ATA_PROGRAM).ok()?;

    let (ata, _bump) = Pubkey::find_program_address(
        &[
            wallet_pubkey.as_ref(),
            token_program.as_ref(),
            mint_pubkey.as_ref(),
        ],
        &ata_program,
    );
    Some(ata.to_string())
}

/// 批量查询结果
pub struct SolBalanceBatchResult {
    /// (地址, ChainBalance, account_owner)
    pub balances: Vec<(String, ChainBalance, Option<String>)>,
}

/// 批量查询一组 SOL 地址在某条链上的余额（native + tokens + owner）
pub async fn query_sol_balance_batch(
    client: &Client,
    config: &SolanaChainConfig,
    addresses: &[String],
) -> SolBalanceBatchResult {
    if addresses.is_empty() {
        return SolBalanceBatchResult {
            balances: Vec::new(),
        };
    }

    // 构建需要批量查询的地址列表:
    // 1. 钱包地址本身
    // 2. 每个钱包地址对应每个配置代币的 ATA 地址
    let mut all_query_addresses: Vec<String> = Vec::new();
    // 记录每个钱包地址在 all_query_addresses 中的位置
    let mut wallet_indices: Vec<usize> = Vec::new();
    // 记录 ATA 地址映射: (wallet_index_in_addresses, token_config_index, ata_address_index_in_all)
    let mut ata_mappings: Vec<(usize, usize, usize)> = Vec::new();

    for (i, addr) in addresses.iter().enumerate() {
        wallet_indices.push(all_query_addresses.len());
        all_query_addresses.push(addr.clone());

        // 为每个配置的代币计算 ATA 地址
        for (ti, token_config) in config.tokens.iter().enumerate() {
            if let Some(ata_addr) =
                derive_ata_address(addr, &token_config.mint_address, token_config.is_token_2022)
            {
                let ata_idx = all_query_addresses.len();
                all_query_addresses.push(ata_addr);
                ata_mappings.push((i, ti, ata_idx));
            }
        }
    }

    // 批量查询所有地址
    let account_infos = match get_multiple_accounts(client, &config.rpc_url, &all_query_addresses).await {
        Ok(infos) => infos,
        Err(_) => {
            // RPC 失败，返回所有地址的失败结果
            return SolBalanceBatchResult {
                balances: addresses
                    .iter()
                    .map(|addr| {
                        (
                            addr.clone(),
                            ChainBalance {
                                chain_id: config.id.clone(),
                                chain_name: config.name.clone(),
                                native_symbol: config.native_symbol.clone(),
                                native_decimals: config.native_decimals,
                                native_balance: 0,
                                staked_balance: 0,
                                tokens: Vec::new(),
                                rpc_failed: true,
                            },
                            None,
                        )
                    })
                    .collect(),
            };
        }
    };

    // 解析结果
    let mut results: Vec<(String, ChainBalance, Option<String>)> = Vec::new();

    for (i, addr) in addresses.iter().enumerate() {
        let wallet_idx = wallet_indices[i];
        let account_info = account_infos.get(wallet_idx).and_then(|a| a.as_ref());

        let (native_balance, account_owner) = if let Some(info) = account_info {
            (info.lamports as u128, Some(info.owner.clone()))
        } else {
            (0u128, None)
        };

        // 判断 staked_balance: 如果地址本身是 Stake 账户，其 lamports 就是质押余额
        let is_stake = account_owner.as_deref() == Some(STAKE_PROGRAM);
        let staked_balance = if is_stake { native_balance } else { 0 };
        let display_native = if is_stake { 0 } else { native_balance };

        // 解析 ATA 代币余额
        let mut tokens: Vec<TokenBalance> = Vec::new();
        for &(wallet_i, token_i, ata_idx) in &ata_mappings {
            if wallet_i != i {
                continue;
            }
            if let Some(Some(ata_info)) = account_infos.get(ata_idx)
                && let Ok(token_account) = spl_token::state::Account::unpack(&ata_info.data)
                && token_account.amount > 0
            {
                let tc = &config.tokens[token_i];
                tokens.push(TokenBalance {
                    symbol: tc.symbol.clone(),
                    decimals: tc.decimals,
                    balance: token_account.amount as u128,
                });
            }
        }

        results.push((
            addr.clone(),
            ChainBalance {
                chain_id: config.id.clone(),
                chain_name: config.name.clone(),
                native_symbol: config.native_symbol.clone(),
                native_decimals: config.native_decimals,
                native_balance: display_native,
                staked_balance,
                tokens,
                rpc_failed: false,
            },
            account_owner,
        ));
    }

    // 对 System 程序拥有的地址，额外查询外部 stake 账户
    for result in &mut results {
        let owner = result.2.as_deref().unwrap_or(SYSTEM_PROGRAM_STR);
        if owner == SYSTEM_PROGRAM_STR
            && let Ok(extra_staked) = get_stake_balance(client, &config.rpc_url, &result.0).await
            && extra_staked > 0
        {
            result.1.staked_balance = extra_staked;
        }
    }

    SolBalanceBatchResult { balances: results }
}



/// 查询质押余额（遍历 stake 账户，查找以该地址为 staker authority 的外部 stake 账户）
async fn get_stake_balance(
    client: &Client,
    rpc_url: &str,
    address: &str,
) -> Result<u128, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "method": "getProgramAccounts",
        "params": [
            STAKE_PROGRAM,
            {
                "encoding": "jsonParsed",
                "commitment": "confirmed",
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

/// 通用 RPC 调用
pub async fn rpc_call(client: &Client, rpc_url: &str, body: &Value) -> Result<Value, String> {
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
