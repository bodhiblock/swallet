use super::*;

pub fn default_config() -> AppConfig {
    AppConfig {
        version: 1,
        chains: ChainsConfig {
            ethereum: default_evm_chains(),
            solana: default_solana_chains(),
        },
    }
}

fn default_evm_chains() -> Vec<EvmChainConfig> {
    vec![
        EvmChainConfig {
            id: "eth-mainnet".into(),
            name: "Ethereum".into(),
            rpc_url: "https://eth.llamarpc.com".into(),
            chain_id: 1,
            native_symbol: "ETH".into(),
            native_decimals: 18,
            explorer_url: Some("https://etherscan.io".into()),
            tokens: vec![
                Erc20TokenConfig {
                    contract_address: "0xdAC17F958D2ee523a2206206994597C13D831ec7".into(),
                    symbol: "USDT".into(),
                    decimals: 6,
                },
                Erc20TokenConfig {
                    contract_address: "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".into(),
                    symbol: "USDC".into(),
                    decimals: 6,
                },
            ],
            is_default: true,
        },
        EvmChainConfig {
            id: "bsc-mainnet".into(),
            name: "BSC".into(),
            rpc_url: "https://bsc-dataseed.binance.org".into(),
            chain_id: 56,
            native_symbol: "BNB".into(),
            native_decimals: 18,
            explorer_url: Some("https://bscscan.com".into()),
            tokens: vec![],
            is_default: true,
        },
        EvmChainConfig {
            id: "base-mainnet".into(),
            name: "Base".into(),
            rpc_url: "https://mainnet.base.org".into(),
            chain_id: 8453,
            native_symbol: "ETH".into(),
            native_decimals: 18,
            explorer_url: Some("https://basescan.org".into()),
            tokens: vec![],
            is_default: true,
        },
    ]
}

fn default_solana_chains() -> Vec<SolanaChainConfig> {
    vec![
        SolanaChainConfig {
            id: "solana-mainnet".into(),
            name: "Solana".into(),
            rpc_url: "https://api.mainnet-beta.solana.com".into(),
            native_symbol: "SOL".into(),
            native_decimals: 9,
            explorer_url: Some("https://explorer.solana.com".into()),
            tokens: vec![SplTokenConfig {
                mint_address: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
                symbol: "USDC".into(),
                decimals: 6,
                is_token_2022: false,
            }],
            is_default: true,
        },
        SolanaChainConfig {
            id: "nara-mainnet".into(),
            name: "Nara".into(),
            rpc_url: "https://mainnet-api.nara.build/".into(),
            native_symbol: "NARA".into(),
            native_decimals: 9,
            explorer_url: None,
            tokens: vec![
                SplTokenConfig {
                    mint_address: "7fKh7DqPZmsYPHdGvt9Qw2rZkSEGp9F5dBa3XuuuhavU".into(),
                    symbol: "SOL".into(),
                    decimals: 9,
                    is_token_2022: true,
                },
                SplTokenConfig {
                    mint_address: "8yQSyqC85A9Vcqz8gTU2Bk5Y63bnC5378sgx1biTKsjd".into(),
                    symbol: "USDT".into(),
                    decimals: 6,
                    is_token_2022: true,
                },
                SplTokenConfig {
                    mint_address: "8P7UGWjq86N3WUmwEgKeGHJZLcoMJqr5jnRUmeBN7YwR".into(),
                    symbol: "USDC".into(),
                    decimals: 6,
                    is_token_2022: true,
                },
            ],
            is_default: true,
        },
    ]
}
