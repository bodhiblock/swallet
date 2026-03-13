pub mod ethereum;
pub mod price;
pub mod registry;
pub mod solana;

use std::collections::HashMap;

/// 某个地址在某条链上的资产快照
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ChainBalance {
    pub chain_id: String,
    pub chain_name: String,
    pub native_symbol: String,
    pub native_decimals: u8,
    /// 原生币余额（最小单位，如 wei / lamports）
    pub native_balance: u128,
    /// 质押余额（仅 SOL 系）
    pub staked_balance: u128,
    /// 代币余额
    pub tokens: Vec<TokenBalance>,
    /// RPC 查询是否失败
    pub rpc_failed: bool,
}

#[derive(Debug, Clone)]
pub struct TokenBalance {
    pub symbol: String,
    pub decimals: u8,
    /// 余额（最小单位）
    pub balance: u128,
}

/// 一个地址的全部资产（跨所有链）
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct AddressPortfolio {
    pub address: String,
    pub chains: Vec<ChainBalance>,
    /// 账户 owner 程序地址（用于区分 Vote/Stake/普通账户）
    pub account_owner: Option<String>,
    /// account_owner 来源链 ID
    pub account_owner_chain_id: Option<String>,
}

/// 全部钱包的余额缓存，key = 地址
pub type BalanceCache = HashMap<String, AddressPortfolio>;

/// 格式化余额：最小单位 -> 可读字符串（去掉尾部零）
pub fn format_balance(amount: u128, decimals: u8) -> String {
    if decimals == 0 {
        return amount.to_string();
    }
    let divisor = 10u128.pow(decimals as u32);
    let integer_part = amount / divisor;
    let fractional_part = amount % divisor;

    if fractional_part == 0 {
        return integer_part.to_string();
    }

    let frac_str = format!("{:0>width$}", fractional_part, width = decimals as usize);
    let trimmed = frac_str.trim_end_matches('0');
    format!("{integer_part}.{trimmed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_balance() {
        // 1.5 ETH = 1500000000000000000 wei (18 decimals)
        assert_eq!(format_balance(1_500_000_000_000_000_000, 18), "1.5");
        // 1 SOL = 1000000000 lamports (9 decimals)
        assert_eq!(format_balance(1_000_000_000, 9), "1");
        // 0.001 SOL
        assert_eq!(format_balance(1_000_000, 9), "0.001");
        // 100 USDT (6 decimals)
        assert_eq!(format_balance(100_000_000, 6), "100");
        // 0 balance
        assert_eq!(format_balance(0, 18), "0");
        // 0 decimals
        assert_eq!(format_balance(42, 0), "42");
    }
}
