import { invoke } from '@tauri-apps/api/core';
import type { WalletDto, BalanceDto, AssetDto } from './types';

export const api = {
	// Auth
	hasDataFile: () => invoke<boolean>('has_data_file'),
	createStore: (password: string) => invoke<void>('create_store', { password }),
	unlock: (password: string) => invoke<void>('unlock', { password }),
	verifyPassword: (password: string) => invoke<boolean>('verify_password', { password }),
	isUnlocked: () => invoke<boolean>('is_unlocked'),

	// Wallet - Query
	getWallets: () => invoke<WalletDto[]>('get_wallets'),
	generateMnemonic: () => invoke<string>('generate_mnemonic'),

	// Wallet - Create
	addMnemonicWallet: (name: string, phrase: string) => invoke<void>('add_mnemonic_wallet', { name, phrase }),
	addPrivateKeyWallet: (name: string, privateKey: string, chainType: string) => invoke<void>('add_private_key_wallet', { name, privateKey, chainType }),
	addWatchWallet: (name: string, address: string, chainType: string) => invoke<void>('add_watch_wallet', { name, address, chainType }),
	addDerivedAddress: (walletIndex: number, chainType: string) => invoke<string>('add_derived_address', { walletIndex, chainType }),

	// Wallet - Manage
	editWalletName: (walletIndex: number, name: string) => invoke<void>('edit_wallet_name', { walletIndex, name }),
	editAddressLabel: (walletIndex: number, chainType: string, accountIndex: number, label: string) => invoke<void>('edit_address_label', { walletIndex, chainType, accountIndex, label }),
	moveWallet: (walletIndex: number, up: boolean) => invoke<void>('move_wallet', { walletIndex, up }),
	hideWallet: (walletIndex: number) => invoke<void>('hide_wallet', { walletIndex }),
	hideAddress: (walletIndex: number, chainType: string, accountIndex: number) => invoke<void>('hide_address', { walletIndex, chainType, accountIndex }),
	deleteWallet: (walletIndex: number, password: string) => invoke<void>('delete_wallet', { walletIndex, password }),
	restoreHiddenWallets: () => invoke<number>('restore_hidden_wallets'),
	restoreHiddenAddresses: () => invoke<number>('restore_hidden_addresses'),

	// Balance
	getCachedBalances: () => invoke<BalanceDto[]>('get_cached_balances'),
	refreshBalances: () => invoke<BalanceDto[]>('refresh_balances'),

	// Transfer
	buildTransferAssets: (address: string, chainType: string) => invoke<AssetDto[]>('build_transfer_assets', { address, chainType }),
	executeTransfer: (password: string, walletIndex: number, accountIndex: number, chainType: string, assetIndex: number, toAddress: string, amount: string) =>
		invoke<string>('execute_transfer', { password, walletIndex, accountIndex, chainType, assetIndex, toAddress, amount }),
};
