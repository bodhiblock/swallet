<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { WalletDto, BalanceDto } from '$lib/types';

	let screen: 'loading' | 'unlock' | 'create' | 'confirm' | 'main' = $state('loading');
	let password = $state('');
	let confirmPassword = $state('');
	let error = $state('');
	let wallets: WalletDto[] = $state([]);
	let balances: BalanceDto[] = $state([]);
	let loading = $state(false);

	onMount(async () => {
		try {
			const hasData = await api.hasDataFile();
			const unlocked = await api.isUnlocked();
			if (unlocked) {
				await loadMain();
			} else if (hasData) {
				screen = 'unlock';
			} else {
				screen = 'create';
			}
		} catch (e: any) {
			error = `初始化失败: ${e?.message || e}`;
			screen = 'create';
		}
	});

	async function loadMain() {
		screen = 'main';
		wallets = await api.getWallets();
		loading = true;
		try {
			balances = await api.refreshBalances();
		} catch (e) {
			console.error('refresh balances failed:', e);
		}
		loading = false;
		// 60s auto refresh
		setInterval(async () => {
			try {
				balances = await api.refreshBalances();
			} catch (_) {}
		}, 60_000);
	}

	async function handleCreate() {
		if (password.length < 1) { error = '请输入密码'; return; }
		if (screen === 'create') { screen = 'confirm'; confirmPassword = ''; error = ''; return; }
		if (password !== confirmPassword) { error = '两次密码不一致'; return; }
		try {
			await api.createStore(password);
			await loadMain();
		} catch (e: any) {
			error = e?.message || '创建失败';
		}
	}

	async function handleUnlock() {
		if (password.length < 1) { error = '请输入密码'; return; }
		try {
			await api.unlock(password);
			await loadMain();
		} catch (_) {
			error = '密码错误';
			password = '';
		}
	}

	function getBalance(address: string): { symbol: string; amount: string }[] {
		const b = balances.find(b => b.address === address);
		if (!b) return [];
		const result: { symbol: string; amount: string }[] = [];
		for (const chain of b.chains) {
			if (chain.native_balance !== '0' || !chain.rpc_failed) {
				result.push({ symbol: chain.native_symbol, amount: chain.native_balance });
			}
			for (const token of chain.tokens) {
				if (token.balance !== '0') {
					result.push({ symbol: token.symbol, amount: token.balance });
				}
			}
		}
		return result;
	}
</script>

{#if screen === 'loading'}
	<div class="container center">
		<p class="dim">loading...</p>
	</div>

{:else if screen === 'unlock'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">输入密码解锁</p>
			<input type="password" bind:value={password} placeholder="密码"
				onkeydown={(e) => e.key === 'Enter' && handleUnlock()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleUnlock}>解锁</button>
		</div>
	</div>

{:else if screen === 'create'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">创建新钱包 - 设置密码</p>
			<input type="password" bind:value={password} placeholder="设置密码"
				onkeydown={(e) => e.key === 'Enter' && handleCreate()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleCreate}>下一步</button>
		</div>
	</div>

{:else if screen === 'confirm'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">再次输入密码确认</p>
			<input type="password" bind:value={confirmPassword} placeholder="确认密码"
				onkeydown={(e) => e.key === 'Enter' && handleCreate()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleCreate}>创建</button>
		</div>
	</div>

{:else if screen === 'main'}
	<div class="container">
		<header>
			<h1>swallet</h1>
			{#if loading}<span class="dim">刷新中...</span>{/if}
		</header>

		{#each wallets.filter(w => !w.hidden) as wallet (wallet.id)}
			<div class="wallet-card">
				<div class="wallet-header">
					<span class="wallet-name">{wallet.name}</span>
					<span class="wallet-type">{wallet.wallet_type}</span>
				</div>
				{#each wallet.accounts.filter(a => !a.hidden) as account}
					<div class="account-row">
						<div class="account-info">
							<span class="chain-badge">{account.chain_type === 'ethereum' ? 'ETH' : 'SOL'}</span>
							{#if account.label}<span class="label">{account.label}</span>{/if}
							<span class="address">{account.address.slice(0, 6)}...{account.address.slice(-4)}</span>
						</div>
						<div class="account-balances">
							{#each getBalance(account.address) as bal}
								<span class="balance">{bal.amount} {bal.symbol}</span>
							{/each}
						</div>
					</div>
				{/each}
			</div>
		{/each}

		{#if wallets.length === 0}
			<div class="empty">
				<p class="dim">还没有钱包</p>
			</div>
		{/if}
	</div>
{/if}

<style>
	.container {
		max-width: 420px;
		margin: 0 auto;
		padding: 16px;
		min-height: 100vh;
	}
	.center {
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.card {
		width: 100%;
		padding: 32px 24px;
		display: flex;
		flex-direction: column;
		gap: 16px;
		align-items: center;
	}
	h1, h2 { text-align: center; }
	h2 { color: var(--accent); font-size: 28px; }
	.dim { color: var(--text-dim); font-size: 14px; }
	.error { color: var(--red); font-size: 14px; }

	.btn-primary {
		width: 100%;
		padding: 12px;
		background: var(--accent);
		color: var(--bg);
		border-radius: 8px;
		font-size: 16px;
		font-weight: 600;
	}
	.btn-primary:hover { opacity: 0.9; }

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 8px 0 16px;
	}
	header h1 { font-size: 20px; color: var(--accent); }

	.wallet-card {
		background: var(--bg-card);
		border: 1px solid var(--border);
		border-radius: 12px;
		margin-bottom: 12px;
		overflow: hidden;
	}
	.wallet-header {
		padding: 12px 16px;
		display: flex;
		justify-content: space-between;
		align-items: center;
		border-bottom: 1px solid var(--border);
	}
	.wallet-name { font-weight: 600; font-size: 14px; }
	.wallet-type { color: var(--text-dim); font-size: 12px; }

	.account-row {
		padding: 10px 16px;
		display: flex;
		justify-content: space-between;
		align-items: center;
		border-bottom: 1px solid var(--border);
	}
	.account-row:last-child { border-bottom: none; }
	.account-info { display: flex; align-items: center; gap: 8px; }

	.chain-badge {
		background: var(--bg);
		color: var(--accent);
		padding: 2px 6px;
		border-radius: 4px;
		font-size: 11px;
		font-weight: 600;
	}
	.label { color: var(--yellow); font-size: 13px; }
	.address { color: var(--text-dim); font-size: 13px; font-family: monospace; }

	.account-balances {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 2px;
	}
	.balance { color: var(--green); font-size: 13px; font-family: monospace; }

	.empty {
		text-align: center;
		padding: 48px 0;
	}
</style>
