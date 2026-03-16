import { invoke } from '@tauri-apps/api/core';
import type {
	WalletDto, BalanceDto, AssetDto, MultisigDetailDto,
	ProposalDto, FeePayerDto, ChainDto, VoteAccountDto, StakeAccountDto,
	PresetProgramDto
} from './types';

export const api = {
	// Auth
	hasDataFile: () => invoke<boolean>('has_data_file'),
	createStore: (password: string) => invoke<void>('create_store', { password }),
	unlock: (password: string) => invoke<void>('unlock', { password }),
	verifyPassword: (password: string) => invoke<boolean>('verify_password', { password }),
	isUnlocked: () => invoke<boolean>('is_unlocked'),

	// Wallet
	getWallets: () => invoke<WalletDto[]>('get_wallets'),
	generateMnemonic: () => invoke<string>('generate_mnemonic'),
	addMnemonicWallet: (name: string, phrase: string) => invoke<void>('add_mnemonic_wallet', { name, phrase }),
	addPrivateKeyWallet: (name: string, privateKey: string, chainType: string) => invoke<void>('add_private_key_wallet', { name, privateKey, chainType }),
	addWatchWallet: (name: string, address: string, chainType: string) => invoke<void>('add_watch_wallet', { name, address, chainType }),
	addDerivedAddress: (walletIndex: number, chainType: string) => invoke<string>('add_derived_address', { walletIndex, chainType }),
	editWalletName: (walletIndex: number, name: string) => invoke<void>('edit_wallet_name', { walletIndex, name }),
	editAddressLabel: (walletIndex: number, chainType: string, accountIndex: number, label: string) => invoke<void>('edit_address_label', { walletIndex, chainType, accountIndex, label }),
	moveWallet: (walletIndex: number, up: boolean) => invoke<void>('move_wallet', { walletIndex, up }),
	hideWallet: (walletIndex: number) => invoke<void>('hide_wallet', { walletIndex }),
	hideAddress: (walletIndex: number, chainType: string, accountIndex: number) => invoke<void>('hide_address', { walletIndex, chainType, accountIndex }),
	deleteWallet: (walletIndex: number, password: string) => invoke<void>('delete_wallet', { walletIndex, password }),
	restoreHiddenWallets: () => invoke<number>('restore_hidden_wallets'),
	restoreHiddenAddresses: () => invoke<number>('restore_hidden_addresses'),

	// Balance
	getRpcUrlForAddress: (address: string) => invoke<string>('get_rpc_url_for_address', { address }),
	getCachedBalances: () => invoke<BalanceDto[]>('get_cached_balances'),
	refreshBalances: () => invoke<BalanceDto[]>('refresh_balances'),

	// Transfer
	buildTransferAssets: (address: string, chainType: string) => invoke<AssetDto[]>('build_transfer_assets', { address, chainType }),
	executeTransfer: (password: string, walletIndex: number, accountIndex: number, chainType: string, assetIndex: number, toAddress: string, amount: string) =>
		invoke<string>('execute_transfer', { password, walletIndex, accountIndex, chainType, assetIndex, toAddress, amount }),

	// Multisig
	getLocalSolAddresses: () => invoke<string[]>('get_local_sol_addresses'),
	getSolanaChains: () => invoke<ChainDto[]>('get_solana_chains'),
	getFeePayers: () => invoke<FeePayerDto[]>('get_fee_payers'),
	importMultisig: (chainId: string, address: string) => invoke<MultisigDetailDto>('import_multisig', { chainId, address }),
	fetchProposals: (walletIndex: number) => invoke<ProposalDto[]>('fetch_proposals', { walletIndex }),
	createSolTransferProposal: (walletIndex: number, vaultIndex: number, toAddress: string, amount: string, password: string, feePayerWi: number, feePayerAi: number) =>
		invoke<string>('create_sol_transfer_proposal', { walletIndex, vaultIndex, toAddress, amount, password, feePayerWi, feePayerAi }),
	createProposal: (params: {
		walletIndex: number; vaultIndex: number; proposalTypeIdx: number;
		toAddress: string; amount: string; upgradeProgram: string; upgradeBuffer: string;
		presetProgramIdx: number; presetInstructionIdx: number; presetArgs: string[];
		vsOpIdx: number; vsTarget: string; vsParam: string; vsAmount: string;
		chainId: string; password: string; feePayerWi: number; feePayerAi: number;
	}) => invoke<string>('create_proposal', params),
	getPresetPrograms: (chainId: string) => invoke<PresetProgramDto[]>('get_preset_programs', { chainId }),
	approveProposal: (walletIndex: number, txIndex: number, password: string, feePayerWi: number, feePayerAi: number) =>
		invoke<string>('approve_proposal', { walletIndex, txIndex, password, feePayerWi, feePayerAi }),
	rejectProposal: (walletIndex: number, txIndex: number, password: string, feePayerWi: number, feePayerAi: number) =>
		invoke<string>('reject_proposal', { walletIndex, txIndex, password, feePayerWi, feePayerAi }),
	executeProposal: (walletIndex: number, txIndex: number, vaultIndex: number, password: string, feePayerWi: number, feePayerAi: number) =>
		invoke<string>('execute_proposal', { walletIndex, txIndex, vaultIndex, password, feePayerWi, feePayerAi }),
	createMultisig: (chainId: string, creatorAddress: string, members: string[], threshold: number, password: string, seed?: string) =>
		invoke<string>('create_multisig', { chainId, creatorAddress, members, threshold, password, seed }),
	getMultisigVaultAddress: (walletIndex: number) => invoke<string>('get_multisig_vault_address', { walletIndex }),

	// Staking
	fetchVoteAccount: (address: string, rpcUrl: string) => invoke<VoteAccountDto>('fetch_vote_account', { address, rpcUrl }),
	fetchStakeAccount: (address: string, rpcUrl: string) => invoke<StakeAccountDto>('fetch_stake_account', { address, rpcUrl }),
	createVoteAccount: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, identity: string, withdrawer: string, password: string) =>
		invoke<string>('create_vote_account', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, identity, withdrawer, password }),
	createStakeAccount: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, amount: string, lockupDays: number, password: string) =>
		invoke<string>('create_stake_account', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, amount, lockupDays, password }),
	stakeDelegate: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, voteAccount: string, password: string) =>
		invoke<string>('stake_delegate', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, voteAccount, password }),
	stakeDeactivate: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, password: string) =>
		invoke<string>('stake_deactivate', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, password }),
	stakeWithdraw: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, toAddress: string, amount: string, password: string) =>
		invoke<string>('stake_withdraw', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, toAddress, amount, password }),
	voteWithdraw: (walletIndex: number, accountIndex: number, rpcUrl: string, feePayerWi: number, feePayerAi: number, toAddress: string, amount: string, password: string) =>
		invoke<string>('vote_withdraw', { walletIndex, accountIndex, rpcUrl, feePayerWi, feePayerAi, toAddress, amount, password }),
};
