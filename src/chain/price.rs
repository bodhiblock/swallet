use std::collections::HashMap;

/// 代币价格信息
#[allow(dead_code)]
pub struct TokenPrice {
    pub symbol: String,
    pub usd_price: f64,
}

/// 价格缓存，key = 代币符号（大写）
pub type PriceCache = HashMap<String, f64>;

/// 获取价格（占位实现）
///
/// TODO: 接入 CoinGecko / Pyth 等价格 API
#[allow(dead_code)]
pub async fn fetch_prices(_symbols: &[&str]) -> PriceCache {
    // 占位：返回空缓存，后续接入真实价格 API
    HashMap::new()
}

/// 格式化 USD 金额
#[allow(dead_code)]
pub fn format_usd(amount: f64) -> String {
    if amount >= 1_000_000.0 {
        format!("${:.2}M", amount / 1_000_000.0)
    } else if amount >= 1_000.0 {
        format!("${:.2}K", amount / 1_000.0)
    } else if amount >= 0.01 {
        format!("${:.2}", amount)
    } else if amount > 0.0 {
        "$<0.01".to_string()
    } else {
        "$0.00".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_usd() {
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(1.5), "$1.50");
        assert_eq!(format_usd(1234.56), "$1.23K");
        assert_eq!(format_usd(1_234_567.89), "$1.23M");
        assert_eq!(format_usd(0.005), "$<0.01");
    }
}
