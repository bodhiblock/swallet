//! 多签 SPL 转账的账户校验、余额和指令准备；不签名、不广播。
use reqwest::Client;
use serde_json::{Value, json};
use solana_sdk::pubkey::Pubkey;

use super::proposals::{VaultInstruction, build_spl_transfer_instructions};
use crate::{
    chain::solana::{derive_ata_address, rpc_call},
    config::{SolanaChainConfig, SplTokenConfig},
    storage::data::ChainType,
    transfer::{AssetKind, TransferableAsset, parse_amount},
};

pub const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_2022: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const ATA: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

pub fn program(token: &SplTokenConfig) -> &'static str {
    if token.is_token_2022 {
        TOKEN_2022
    } else {
        TOKEN
    }
}

pub async fn fetch_accounts(
    client: &Client,
    rpc: &str,
    addresses: &[String],
) -> Result<Vec<Value>, String> {
    let mut accounts = Vec::with_capacity(addresses.len());
    for chunk in addresses.chunks(100) {
        let response = rpc_call(
            client,
            rpc,
            &json!({
                "jsonrpc": "2.0", "id": 1, "method": "getMultipleAccounts",
                "params": [chunk, {"encoding": "jsonParsed", "commitment": "confirmed"}],
            }),
        )
        .await?;
        if let Some(error) = response.get("error") {
            return Err(format!("账户查询失败: {error}"));
        }
        let values = response["result"]["value"]
            .as_array()
            .ok_or("账户查询响应无效")?;
        if values.len() != chunk.len() {
            return Err("账户查询响应数量不匹配".into());
        }
        accounts.extend(values.iter().cloned());
    }
    Ok(accounts)
}

pub fn info<'a>(account: &'a Value, pid: &str, kind: &str) -> Result<&'a Value, String> {
    if account.is_null() {
        return Err(format!("{kind} 账户不存在"));
    }
    if account["owner"].as_str() != Some(pid) {
        return Err("Token Program 不匹配".into());
    }
    let parsed = &account["data"]["parsed"];
    if parsed["type"].as_str() != Some(kind) || !parsed["info"].is_object() {
        return Err(format!("无法解析 {kind} 账户"));
    }
    Ok(&parsed["info"])
}

pub fn check_extensions(info: &Value) -> Result<(), String> {
    if let Some(extensions) = info.get("extensions") {
        for extension in extensions.as_array().ok_or("扩展数据无效")? {
            let name = extension["extension"].as_str().ok_or("无法识别代币扩展")?;
            if !matches!(
                name,
                "metadataPointer"
                    | "tokenMetadata"
                    | "groupPointer"
                    | "tokenGroup"
                    | "groupMemberPointer"
                    | "tokenGroupMember"
                    | "immutableOwner"
            ) {
                return Err(format!("暂不支持代币扩展: {name}"));
            }
        }
    }
    Ok(())
}

pub fn mint_decimals(account: &Value, pid: &str) -> Result<u8, String> {
    let mint = info(account, pid, "mint")?;
    if mint["isInitialized"].as_bool() != Some(true) {
        return Err("Mint 未初始化".into());
    }
    mint["decimals"]
        .as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or("Mint 精度无效".into())
}

fn check_mint(account: &Value, token: &SplTokenConfig) -> Result<(), String> {
    let decimals = mint_decimals(account, program(token))?;
    if decimals > 38 {
        return Err("代币精度超出支持范围".into());
    }
    if decimals != token.decimals {
        return Err(format!(
            "配置精度 {} 与链上精度 {decimals} 不匹配",
            token.decimals
        ));
    }
    check_extensions(info(account, program(token), "mint")?)
}

fn check_account(account: &Value, token: &SplTokenConfig, owner: &str) -> Result<u64, String> {
    let data = info(account, program(token), "account")?;
    if data["mint"].as_str() != Some(&token.mint_address) || data["owner"].as_str() != Some(owner) {
        return Err("代币账户 Mint 或持有人不匹配".into());
    }
    if data["state"].as_str() != Some("initialized") {
        return Err("代币账户被冻结或未初始化".into());
    }
    check_extensions(data)?;
    data["tokenAmount"]["amount"]
        .as_str()
        .ok_or("缺少余额")?
        .parse()
        .map_err(|_| "余额无效".into())
}

pub async fn load_assets(
    client: &Client,
    chain: &SolanaChainConfig,
    vault: &str,
) -> Result<Vec<(TransferableAsset, Option<String>)>, String> {
    vault.parse::<Pubkey>().map_err(|_| "Vault 地址无效")?;
    let mut addresses = Vec::new();
    for token in &chain.tokens {
        // 单个配置地址错误不影响其它代币的余额查询。
        addresses.push(
            token
                .mint_address
                .parse::<Pubkey>()
                .unwrap_or_default()
                .to_string(),
        );
        addresses.push(
            derive_ata_address(vault, &token.mint_address, token.is_token_2022)
                .unwrap_or_else(|| Pubkey::default().to_string()),
        );
    }
    let accounts = fetch_accounts(client, &chain.rpc_url, &addresses).await?;
    Ok(chain
        .tokens
        .iter()
        .enumerate()
        .map(|(i, token)| {
            let balance = token
                .mint_address
                .parse::<Pubkey>()
                .map_err(|_| "配置中 Mint 地址无效".to_string())
                .and_then(|_| check_mint(&accounts[i * 2], token))
                .and_then(|()| {
                    if accounts[i * 2 + 1].is_null() {
                        Ok(0)
                    } else {
                        check_account(&accounts[i * 2 + 1], token, vault)
                    }
                });
            let error = balance.as_ref().err().cloned();
            (
                TransferableAsset {
                    chain_type: ChainType::Solana,
                    chain_id: chain.id.clone(),
                    chain_name: chain.name.clone(),
                    rpc_url: chain.rpc_url.clone(),
                    evm_chain_id: None,
                    asset_kind: AssetKind::SplToken {
                        mint_address: token.mint_address.clone(),
                        is_token_2022: token.is_token_2022,
                    },
                    symbol: token.symbol.clone(),
                    decimals: token.decimals,
                    balance: balance.ok().map(u128::from),
                },
                error,
            )
        })
        .collect())
}

pub async fn prepare_transfer(
    client: &Client,
    rpc: &str,
    vault: &Pubkey,
    to: &str,
    amount: &str,
    token: &SplTokenConfig,
) -> Result<Vec<VaultInstruction>, String> {
    let recipient: Pubkey = to.parse().map_err(|_| "无效的收款钱包地址")?;
    let mint: Pubkey = token.mint_address.parse().map_err(|_| "无效的 Mint 地址")?;
    let amount: u64 = parse_amount(amount, token.decimals)?
        .try_into()
        .map_err(|_| "SPL 数量超出范围")?;
    let vault_str = vault.to_string();
    let source = derive_ata_address(&vault_str, &token.mint_address, token.is_token_2022)
        .ok_or("无法计算来源 ATA")?;
    let dest = derive_ata_address(to, &token.mint_address, token.is_token_2022)
        .ok_or("无法计算目标 ATA")?;
    let accounts = fetch_accounts(
        client,
        rpc,
        &[
            token.mint_address.clone(),
            source.clone(),
            dest.clone(),
            to.into(),
            vault_str.clone(),
        ],
    )
    .await?;
    check_mint(&accounts[0], token)?;
    let balance = check_account(&accounts[1], token, &vault_str)?;
    if amount > balance {
        return Err("Vault 代币余额不足".into());
    }
    if !accounts[2].is_null() {
        check_account(&accounts[2], token, to)?;
    }
    if matches!(accounts[3]["owner"].as_str(), Some(TOKEN | TOKEN_2022)) {
        return Err("请输入收款钱包地址，不是 Mint 或代币账户地址".into());
    }
    if accounts[2].is_null() {
        let rent = rpc_call(client, rpc, &json!({"jsonrpc": "2.0", "id": 1,
            "method": "getMinimumBalanceForRentExemption", "params": [if token.is_token_2022 { 170 } else { 165 }]
        })).await?["result"].as_u64().ok_or("无法查询 ATA 租金")?;
        if accounts[4]["lamports"]
            .as_u64()
            .ok_or("无法查询 Vault 原生币余额")?
            < rent
        {
            return Err("Vault 原生币余额不足以创建收款 ATA".into());
        }
    }
    Ok(build_spl_transfer_instructions(
        &vault.to_bytes(),
        &recipient.to_bytes(),
        &mint.to_bytes(),
        amount,
        token.decimals,
        &program(token).parse::<Pubkey>().unwrap().to_bytes(),
        &source.parse::<Pubkey>().unwrap().to_bytes(),
        &dest.parse::<Pubkey>().unwrap().to_bytes(),
        true,
    ))
}

/// 精度来自链上，可大于 u128 的十进制范围，使用字符串移动小数点。
pub fn format_amount(amount: u64, decimals: u8) -> String {
    let mut digits = format!("{:0>width$}", amount, width = decimals as usize + 1);
    if decimals > 0 {
        digits.insert(digits.len() - decimals as usize, '.');
        digits = digits
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
    }
    digits
}

pub fn describe_transfer(
    pid: &str,
    data: &[u8],
    keys: &[String],
    accounts: &std::collections::HashMap<String, Value>,
    created: &std::collections::HashMap<String, (String, String, String)>,
    tokens: &[SplTokenConfig],
) -> Result<String, String> {
    let checked = data.first() == Some(&12);
    let (dest_i, authority_i) = if checked { (2, 3) } else { (1, 2) };
    if !matches!(data.first(), Some(3 | 12))
        || data.len() != if checked { 10 } else { 9 }
        || keys.len() != authority_i + 1
    {
        return Err("暂不支持此 SPL 指令或多授权人转账".into());
    }
    let null = Value::Null;
    let source = info(accounts.get(&keys[0]).unwrap_or(&null), pid, "account")?;
    let mint = source["mint"].as_str().ok_or("来源账户缺少 Mint")?;
    if checked && keys[1] != mint {
        return Err("指令 Mint 与来源账户不匹配".into());
    }
    let mint_account = accounts.get(mint).unwrap_or(&null);
    let decimals = mint_decimals(mint_account, pid)?;
    if checked && data[9] != decimals {
        return Err("指令精度与链上 Mint 不匹配".into());
    }
    check_extensions(info(mint_account, pid, "mint")?)?;
    check_extensions(source)?;
    let dest = accounts.get(&keys[dest_i]).unwrap_or(&null);
    let recipient = if dest.is_null() {
        let (owner, dest_mint, dest_program) = created
            .get(&keys[dest_i])
            .ok_or("目标账户不存在，且无对应 ATA 创建指令")?;
        if dest_mint != mint || dest_program != pid {
            return Err("ATA 创建与转账 Mint/Program 不匹配".into());
        }
        owner.to_string()
    } else {
        let dest = info(dest, pid, "account")?;
        if dest["mint"].as_str() != Some(mint) {
            return Err("来源和目标 Mint 不匹配".into());
        }
        check_extensions(dest)?;
        dest["owner"]
            .as_str()
            .ok_or("目标账户缺少持有人")?
            .to_string()
    };
    let amount = u64::from_le_bytes(data[1..9].try_into().unwrap());
    let symbol = tokens
        .iter()
        .find(|t| t.mint_address == mint && program(t) == pid)
        .map(|t| t.symbol.as_str())
        .unwrap_or("未知代币");
    let owner = source["owner"].as_str().ok_or("来源账户缺少持有人")?;
    Ok(format!(
        "SPL 转账: {} {symbol}\nMint: {mint}\nToken Program: {pid}\n来源持有人: {owner}\n来源代币账户: {}\n收款钱包: {recipient}\n目标代币账户: {}\n转账授权人: {}\n原始数量: {amount} (精度 {decimals})",
        format_amount(amount, decimals),
        keys[0],
        keys[dest_i],
        keys[authority_i]
    ))
}

pub fn describe_ata(data: &[u8], keys: &[String]) -> Result<(String, String, String), String> {
    if !matches!(data, [] | [0] | [1]) || keys.len() < 6 || keys.len() > 7 {
        return Err("暂不支持此 ATA 指令".into());
    }
    if !matches!(keys[5].as_str(), TOKEN | TOKEN_2022) || keys[4] != Pubkey::default().to_string() {
        return Err("ATA 指令 Program 无效".into());
    }
    let expected =
        derive_ata_address(&keys[2], &keys[3], keys[5] == TOKEN_2022).ok_or("ATA 地址无效")?;
    if keys[1] != expected {
        return Err("ATA 地址与钱包/Mint 不匹配".into());
    }
    Ok((keys[2].clone(), keys[3].clone(), keys[5].clone()))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    pub fn mint(pid: &str, decimals: u8) -> Value {
        json!({"owner": pid, "data": {"parsed": {"type": "mint", "info": {
            "isInitialized": true, "decimals": decimals
        }}}})
    }

    pub fn account(pid: &str, mint: &str, owner: &str, amount: u64) -> Value {
        json!({"owner": pid, "data": {"parsed": {"type": "account", "info": {
            "mint": mint, "owner": owner, "state": "initialized", "tokenAmount": {"amount": amount.to_string()}
        }}}})
    }

    /// Local JSON-RPC server: captures requests, never contacts a real chain.
    pub fn rpc(results: Vec<Value>) -> (String, thread::JoinHandle<Vec<Value>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for result in results {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                let request = loop {
                    let count = stream.read(&mut buffer).unwrap();
                    assert_ne!(count, 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = headers
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break serde_json::from_slice(&bytes[end + 4..end + 4 + length])
                                .unwrap();
                        }
                    }
                };
                requests.push(request);
                let body = json!({"jsonrpc":"2.0", "id":1, "result":result}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        (url, handle)
    }

    #[tokio::test]
    async fn spl_preparation_and_validation() {
        for is_token_2022 in [false, true] {
            for decimals in [0, 6, 9] {
                for exists in [false, true] {
                    let vault = Pubkey::new_unique();
                    let to = Pubkey::new_unique().to_string();
                    let token = SplTokenConfig {
                        mint_address: Pubkey::new_unique().to_string(),
                        symbol: "SAME".into(),
                        decimals,
                        is_token_2022,
                    };
                    let pid = program(&token);
                    let unit = 10u64.pow(decimals as u32);
                    let values = json!([mint(pid, decimals), account(pid, &token.mint_address, &vault.to_string(), 2 * unit),
                        if exists { account(pid, &token.mint_address, &to, 0) } else { Value::Null }, null, {"lamports": 10_000_000}]);
                    let mut results = vec![json!({"value":values})];
                    if !exists {
                        results.push(json!(2_000_000));
                    }
                    let (url, server) = rpc(results);
                    let instructions =
                        prepare_transfer(&Client::new(), &url, &vault, &to, "1", &token)
                            .await
                            .unwrap();
                    let requests = server.join().unwrap();
                    assert_eq!(requests[0]["method"], "getMultipleAccounts");
                    if !exists {
                        assert_eq!(
                            requests[1]["params"][0],
                            if is_token_2022 { 170 } else { 165 }
                        );
                    }
                    assert_eq!(instructions.len(), 2);
                    assert_eq!(instructions[0].data, [1]);
                    assert_eq!(instructions[0].accounts[0].pubkey, vault.to_bytes());
                    let ix = &instructions[1];
                    assert_eq!(ix.program_id, pid.parse::<Pubkey>().unwrap().to_bytes());
                    assert_eq!(ix.data[0], 12);
                    assert_eq!(u64::from_le_bytes(ix.data[1..9].try_into().unwrap()), unit);
                    assert_eq!(ix.data[9], decimals);
                    assert_eq!(
                        ix.accounts[1].pubkey,
                        token.mint_address.parse::<Pubkey>().unwrap().to_bytes()
                    );
                    assert_eq!(ix.accounts[3].pubkey, vault.to_bytes());
                    assert!(ix.accounts[3].is_signer);
                }
            }
        }
        let token = SplTokenConfig {
            mint_address: Pubkey::new_unique().to_string(),
            symbol: "T".into(),
            decimals: 6,
            is_token_2022: true,
        };
        let owner = Pubkey::new_unique().to_string();
        assert!(check_mint(&mint(TOKEN, 6), &token).is_err());
        assert!(check_mint(&mint(TOKEN_2022, 9), &token).is_err());
        let mut frozen = account(TOKEN_2022, &token.mint_address, &owner, 0);
        frozen["data"]["parsed"]["info"]["state"] = json!("frozen");
        assert!(check_account(&frozen, &token, &owner).is_err());
        assert!(check_account(&account(TOKEN_2022, &owner, &owner, 1), &token, &owner).is_err());
        for name in [
            "transferFeeConfig",
            "transferHook",
            "nonTransferable",
            "memoTransfer",
            "cpiGuard",
            "unknown",
        ] {
            assert!(check_extensions(&json!({"extensions":[{"extension":name}]})).is_err());
        }
        assert!(check_extensions(&json!({"extensions":[{"extension":"metadataPointer"},{"extension":"immutableOwner"}]})).is_ok());
        for amount in ["0", "-1", "1.0000001", "18446744073709551616"] {
            assert!(
                prepare_transfer(
                    &Client::new(),
                    "http://127.0.0.1:1",
                    &Pubkey::new_unique(),
                    &owner,
                    amount,
                    &token
                )
                .await
                .is_err()
            );
        }
        assert!(parse_amount("1", 255).is_err());
        assert_eq!(format_amount(1, 6), "0.000001");
        assert_eq!(format_amount(u64::MAX, 0), u64::MAX.to_string());

        // Same symbols do not share cached balances; identify assets by their mint/ATA.
        let second = SplTokenConfig {
            mint_address: Pubkey::new_unique().to_string(),
            ..token.clone()
        };
        let mut chain = crate::config::defaults::default_config()
            .chains
            .solana
            .remove(0);
        chain.tokens = vec![token.clone(), second.clone()];
        let (url, server) = rpc(vec![
            json!({"value": [mint(TOKEN_2022, 6), account(TOKEN_2022, &token.mint_address, &owner, 3),
            mint(TOKEN_2022, 6), account(TOKEN_2022, &second.mint_address, &owner, 7)]}),
        ]);
        chain.rpc_url = url;
        let assets = load_assets(&Client::new(), &chain, &owner).await.unwrap();
        server.join().unwrap();
        assert_eq!(assets[0].0.balance, Some(3));
        assert_eq!(assets[1].0.balance, Some(7));

        for (balance, lamports, error) in [
            (0, 10_000_000, "代币余额不足"),
            (2_000_000, 0, "原生币余额不足"),
        ] {
            let mut results = vec![
                json!({"value": [mint(TOKEN_2022, 6), account(TOKEN_2022, &token.mint_address, &owner, balance), null, null, {"lamports":lamports}]}),
            ];
            if balance > 0 {
                results.push(json!(2_000_000));
            }
            let (url, server) = rpc(results);
            let result = prepare_transfer(
                &Client::new(),
                &url,
                &owner.parse().unwrap(),
                &Pubkey::new_unique().to_string(),
                "1",
                &token,
            )
            .await;
            server.join().unwrap();
            assert!(result.err().unwrap().contains(error));
        }
    }
}
