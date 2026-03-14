export interface WalletDto {
	id: string;
	name: string;
	wallet_type: string;
	sort_order: number;
	hidden: boolean;
	accounts: AccountDto[];
}

export interface AccountDto {
	address: string;
	label: string | null;
	chain_type: string;
	hidden: boolean;
}

export interface BalanceDto {
	address: string;
	account_owner: string | null;
	chains: ChainBalanceDto[];
}

export interface ChainBalanceDto {
	chain_id: string;
	chain_name: string;
	native_symbol: string;
	native_balance: string;
	native_balance_raw: string;
	staked_balance: string;
	tokens: TokenBalanceDto[];
	rpc_failed: boolean;
}

export interface TokenBalanceDto {
	symbol: string;
	balance: string;
	balance_raw: string;
	decimals: number;
}
