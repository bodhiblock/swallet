pub mod eth_transfer;
pub mod sol_transfer;

use crate::chain::{format_balance, BalanceCache};
use crate::config::AppConfig;
use crate::storage::data::ChainType;

/// 可转账资产
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TransferableAsset {
    pub chain_type: ChainType,
    pub chain_id: String,
    pub chain_name: String,
    pub rpc_url: String,
    pub evm_chain_id: Option<u64>,
    pub asset_kind: AssetKind,
    pub symbol: String,
    pub decimals: u8,
    /// 当前余额（最小单位），None 表示未查询
    pub balance: Option<u128>,
}

/// 资产类型
#[derive(Debug, Clone)]
pub enum AssetKind {
    Native,
    Erc20 { contract_address: String },
    SplToken { mint_address: String, is_token_2022: bool },
}

impl TransferableAsset {
    pub fn display_label(&self) -> String {
        let bal = match self.balance {
            Some(b) => format_balance(b, self.decimals),
            None => "-".to_string(),
        };
        format!("[{}] {} {}", self.chain_name, bal, self.symbol)
    }
}

/// 从 BalanceCache 中查找某地址在某链上某资产的余额
fn lookup_balance(
    cache: &BalanceCache,
    address: &str,
    chain_id: &str,
    symbol: &str,
    is_native: bool,
) -> Option<u128> {
    let portfolio = cache.get(address)?;
    let chain_bal = portfolio.chains.iter().find(|c| c.chain_id == chain_id)?;
    if chain_bal.rpc_failed {
        return None;
    }
    if is_native {
        Some(chain_bal.native_balance)
    } else {
        chain_bal
            .tokens
            .iter()
            .find(|t| t.symbol == symbol)
            .map(|t| t.balance)
    }
}

/// 构建 EVM 链可转账资产列表
pub fn build_eth_assets(
    config: &AppConfig,
    address: &str,
    cache: &BalanceCache,
) -> Vec<TransferableAsset> {
    let mut assets = Vec::new();
    for chain in &config.chains.ethereum {
        assets.push(TransferableAsset {
            chain_type: ChainType::Ethereum,
            chain_id: chain.id.clone(),
            chain_name: chain.name.clone(),
            rpc_url: chain.rpc_url.clone(),
            evm_chain_id: Some(chain.chain_id),
            asset_kind: AssetKind::Native,
            symbol: chain.native_symbol.clone(),
            decimals: chain.native_decimals,
            balance: lookup_balance(cache, address, &chain.id, "", true),
        });
        for token in &chain.tokens {
            assets.push(TransferableAsset {
                chain_type: ChainType::Ethereum,
                chain_id: chain.id.clone(),
                chain_name: chain.name.clone(),
                rpc_url: chain.rpc_url.clone(),
                evm_chain_id: Some(chain.chain_id),
                asset_kind: AssetKind::Erc20 {
                    contract_address: token.contract_address.clone(),
                },
                symbol: token.symbol.clone(),
                decimals: token.decimals,
                balance: lookup_balance(cache, address, &chain.id, &token.symbol, false),
            });
        }
    }
    assets
}

/// 构建 Solana 链可转账资产列表
pub fn build_sol_assets(
    config: &AppConfig,
    address: &str,
    cache: &BalanceCache,
) -> Vec<TransferableAsset> {
    let mut assets = Vec::new();
    for chain in &config.chains.solana {
        assets.push(TransferableAsset {
            chain_type: ChainType::Solana,
            chain_id: chain.id.clone(),
            chain_name: chain.name.clone(),
            rpc_url: chain.rpc_url.clone(),
            evm_chain_id: None,
            asset_kind: AssetKind::Native,
            symbol: chain.native_symbol.clone(),
            decimals: chain.native_decimals,
            balance: lookup_balance(cache, address, &chain.id, "", true),
        });
        for token in &chain.tokens {
            assets.push(TransferableAsset {
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
                balance: lookup_balance(cache, address, &chain.id, &token.symbol, false),
            });
        }
    }
    assets
}

/// 解析用户输入的数量为最小单位
pub fn parse_amount(input: &str, decimals: u8) -> Result<u128, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("数量不能为空".into());
    }

    let parts: Vec<&str> = input.split('.').collect();
    if parts.len() > 2 {
        return Err("无效的数量格式".into());
    }

    let integer_part: u128 = parts[0]
        .parse()
        .map_err(|_| "无效的整数部分".to_string())?;
    let fractional_str = if parts.len() == 2 { parts[1] } else { "" };

    if fractional_str.len() > decimals as usize {
        return Err(format!("小数位数不能超过 {decimals} 位"));
    }

    let padded = format!("{:0<width$}", fractional_str, width = decimals as usize);
    let fractional_part: u128 = if padded.is_empty() {
        0
    } else {
        padded.parse().map_err(|_| "无效的小数部分".to_string())?
    };

    let divisor = 10u128.pow(decimals as u32);
    let total = integer_part
        .checked_mul(divisor)
        .and_then(|v| v.checked_add(fractional_part))
        .ok_or("数量溢出")?;

    if total == 0 {
        return Err("数量必须大于 0".into());
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_amount() {
        assert_eq!(parse_amount("1.5", 18).unwrap(), 1_500_000_000_000_000_000);
        assert_eq!(parse_amount("1", 18).unwrap(), 1_000_000_000_000_000_000);
        assert_eq!(parse_amount("0.001", 18).unwrap(), 1_000_000_000_000_000);
        assert_eq!(parse_amount("1", 9).unwrap(), 1_000_000_000);
        assert_eq!(parse_amount("0.5", 9).unwrap(), 500_000_000);
        assert_eq!(parse_amount("100", 6).unwrap(), 100_000_000);
        assert_eq!(parse_amount("1.23", 6).unwrap(), 1_230_000);

        assert!(parse_amount("0", 18).is_err());
        assert!(parse_amount("", 18).is_err());
        assert!(parse_amount("abc", 18).is_err());
        assert!(parse_amount("1.1234567890123456789", 18).is_err());
    }
}
