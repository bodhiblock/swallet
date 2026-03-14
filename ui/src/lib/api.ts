import { invoke } from '@tauri-apps/api/core';
import type { WalletDto, BalanceDto } from './types';

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

	// Balance
	getCachedBalances: () => invoke<BalanceDto[]>('get_cached_balances'),
	refreshBalances: () => invoke<BalanceDto[]>('refresh_balances'),
};
