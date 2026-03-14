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

export interface AssetDto {
	index: number;
	chain_name: string;
	symbol: string;
	decimals: number;
	balance: string;
	balance_raw: string;
	asset_type: string;
}

export interface MultisigDetailDto {
	address: string;
	threshold: number;
	members: { address: string }[];
	transaction_index: number;
}

export interface ProposalDto {
	index: number;
	transaction_index: number;
	status: string;
	approved_count: number;
	rejected_count: number;
	approved_addresses: string[];
	rejected_addresses: string[];
}

export interface FeePayerDto {
	address: string;
	label: string;
	balance: string;
	wallet_index: number;
	account_index: number;
}

export interface ChainDto {
	id: string;
	name: string;
	rpc_url: string;
}

export interface VoteAccountDto {
	address: string;
	validator_identity: string;
	authorized_voter: string;
	authorized_withdrawer: string;
	commission: number;
	credits: string | null;
}

export interface PresetProgramDto {
	name: string;
	instructions: PresetInstructionDto[];
}

export interface PresetInstructionDto {
	name: string;
	label: string;
	args: PresetArgDto[];
}

export interface PresetArgDto {
	name: string;
	label: string;
	arg_type: string;
}

export interface StakeAccountDto {
	address: string;
	state: string;
	delegated_vote_account: string | null;
	stake_lamports: string;
	authorized_staker: string;
	authorized_withdrawer: string;
	lockup_timestamp: number;
}
