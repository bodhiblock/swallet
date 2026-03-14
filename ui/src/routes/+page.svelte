<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { WalletDto, BalanceDto } from '$lib/types';

	// Screen state
	let screen: 'loading' | 'unlock' | 'create' | 'confirm' | 'main' | 'add-wallet' | 'show-mnemonic' | 'input-name' = $state('loading');
	let password = $state('');
	let confirmPassword = $state('');
	let error = $state('');
	let toast = $state('');

	// Main data
	let wallets: WalletDto[] = $state([]);
	let balances: BalanceDto[] = $state([]);
	let loading = $state(false);

	// Add wallet flow
	let addWalletType: 'mnemonic' | 'import-mnemonic' | 'private-key' | 'watch' = $state('mnemonic');
	let mnemonicPhrase = $state('');
	let importInput = $state('');
	let walletName = $state('');
	let selectedChain: 'ethereum' | 'solana' = $state('solana');

	// Context menu
	let menuTarget: { type: 'wallet' | 'account'; walletIndex: number; accountIndex?: number; chainType?: string; walletType?: string } | null = $state(null);

	// Dialog
	let dialogType: 'rename' | 'relabel' | 'delete' | 'add-address' | null = $state(null);
	let dialogInput = $state('');
	let dialogPassword = $state('');

	onMount(async () => {
		try {
			const hasData = await api.hasDataFile();
			const unlocked = await api.isUnlocked();
			if (unlocked) { await loadMain(); }
			else if (hasData) { screen = 'unlock'; }
			else { screen = 'create'; }
		} catch (e: any) {
			error = `初始化失败: ${e?.message || e}`;
			screen = 'create';
		}
	});

	async function loadMain() {
		screen = 'main';
		await reloadWallets();
		refreshBalances();
		setInterval(refreshBalances, 60_000);
	}

	async function reloadWallets() {
		try { wallets = await api.getWallets(); } catch (_) {}
	}

	async function refreshBalances() {
		loading = true;
		try { balances = await api.refreshBalances(); } catch (_) {}
		loading = false;
	}

	// Auth
	async function handleCreate() {
		error = '';
		if (password.length < 1) { error = '请输入密码'; return; }
		if (screen === 'create') { screen = 'confirm'; confirmPassword = ''; return; }
		if (password !== confirmPassword) { error = '两次密码不一致'; return; }
		try { await api.createStore(password); await loadMain(); }
		catch (e: any) { error = e?.message || '创建失败'; }
	}

	async function handleUnlock() {
		error = '';
		if (password.length < 1) { error = '请输入密码'; return; }
		try { await api.unlock(password); await loadMain(); }
		catch (_) { error = '密码错误'; password = ''; }
	}

	// Add wallet
	async function startAddWallet(type: typeof addWalletType) {
		addWalletType = type;
		importInput = '';
		walletName = '';
		selectedChain = 'solana';
		if (type === 'mnemonic') {
			try { mnemonicPhrase = await api.generateMnemonic(); } catch (e: any) { showToast(e?.message || '生成失败'); return; }
			screen = 'show-mnemonic';
		} else {
			screen = 'add-wallet';
		}
	}

	function proceedToName() {
		if (addWalletType === 'import-mnemonic' && importInput.trim().split(/\s+/).length < 12) {
			showToast('助记词至少12个词'); return;
		}
		if (addWalletType === 'private-key' && importInput.trim().length < 10) {
			showToast('请输入有效的私钥'); return;
		}
		if (addWalletType === 'watch' && importInput.trim().length < 10) {
			showToast('请输入有效的地址'); return;
		}
		screen = 'input-name';
	}

	async function finishAddWallet() {
		if (!walletName.trim()) { showToast('请输入名称'); return; }
		try {
			if (addWalletType === 'mnemonic') {
				await api.addMnemonicWallet(walletName, mnemonicPhrase);
			} else if (addWalletType === 'import-mnemonic') {
				await api.addMnemonicWallet(walletName, importInput.trim());
			} else if (addWalletType === 'private-key') {
				await api.addPrivateKeyWallet(walletName, importInput.trim(), selectedChain);
			} else if (addWalletType === 'watch') {
				await api.addWatchWallet(walletName, importInput.trim(), selectedChain);
			}
			showToast('钱包添加成功');
			await reloadWallets();
			screen = 'main';
		} catch (e: any) { showToast(e?.message || '添加失败'); }
	}

	// Balance helpers
	function getBalance(address: string): { symbol: string; amount: string }[] {
		const b = balances.find(b => b.address === address);
		if (!b) return [];
		const result: { symbol: string; amount: string }[] = [];
		for (const chain of b.chains) {
			if (chain.native_balance !== '0' || !chain.rpc_failed) {
				result.push({ symbol: chain.native_symbol, amount: chain.native_balance });
			}
			for (const token of chain.tokens) {
				if (token.balance !== '0') result.push({ symbol: token.symbol, amount: token.balance });
			}
		}
		return result;
	}

	async function copyAddress(address: string) {
		try { await navigator.clipboard.writeText(address); showToast('已复制'); } catch (_) {}
	}

	// Context menu actions
	function openMenu(type: 'wallet' | 'account', walletIndex: number, accountIndex?: number, chainType?: string, walletType?: string, e?: MouseEvent) {
		e?.stopPropagation();
		menuTarget = { type, walletIndex, accountIndex, chainType, walletType };
	}

	function closeMenu() { menuTarget = null; }

	async function menuAction(action: string) {
		if (!menuTarget) return;
		const { walletIndex, accountIndex, chainType, walletType } = menuTarget;
		closeMenu();
		try {
			switch (action) {
				case 'rename':
					dialogType = 'rename';
					dialogInput = wallets[walletIndex]?.name || '';
					break;
				case 'relabel':
					dialogType = 'relabel';
					dialogInput = '';
					break;
				case 'move-up':
					await api.moveWallet(walletIndex, true); await reloadWallets(); break;
				case 'move-down':
					await api.moveWallet(walletIndex, false); await reloadWallets(); break;
				case 'hide-wallet':
					await api.hideWallet(walletIndex); await reloadWallets(); showToast('已隐藏'); break;
				case 'hide-address':
					if (chainType !== undefined && accountIndex !== undefined)
						await api.hideAddress(walletIndex, chainType, accountIndex);
					await reloadWallets(); showToast('已隐藏'); break;
				case 'delete':
					dialogType = 'delete'; dialogPassword = ''; break;
				case 'add-eth':
					await api.addDerivedAddress(walletIndex, 'ethereum'); await reloadWallets(); showToast('地址已添加'); break;
				case 'add-sol':
					await api.addDerivedAddress(walletIndex, 'solana'); await reloadWallets(); showToast('地址已添加'); break;
			}
		} catch (e: any) { showToast(e?.message || '操作失败'); }
	}

	async function confirmDialog() {
		if (!menuTarget) return;
		try {
			if (dialogType === 'rename') {
				await api.editWalletName(menuTarget.walletIndex, dialogInput);
			} else if (dialogType === 'relabel') {
				await api.editAddressLabel(menuTarget.walletIndex, menuTarget.chainType!, menuTarget.accountIndex!, dialogInput);
			} else if (dialogType === 'delete') {
				await api.deleteWallet(menuTarget.walletIndex, dialogPassword);
			}
			await reloadWallets();
			showToast('操作成功');
		} catch (e: any) { showToast(e?.message || '操作失败'); }
		dialogType = null;
		menuTarget = null;
	}

	async function restoreHidden() {
		try {
			const w = await api.restoreHiddenWallets();
			const a = await api.restoreHiddenAddresses();
			await reloadWallets();
			showToast(`恢复了 ${w} 个钱包, ${a} 个地址`);
		} catch (e: any) { showToast(e?.message || '恢复失败'); }
	}

	function showToast(msg: string) { toast = msg; setTimeout(() => { toast = ''; }, 2000); }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
{#if screen === 'loading'}
	<div class="container center"><p class="dim">loading...</p></div>

{:else if screen === 'unlock'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">输入密码解锁</p>
			<input type="password" bind:value={password} placeholder="密码" onkeydown={(e) => e.key === 'Enter' && handleUnlock()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleUnlock}>解锁</button>
		</div>
	</div>

{:else if screen === 'create'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">创建新钱包 - 设置密码</p>
			<input type="password" bind:value={password} placeholder="设置密码" onkeydown={(e) => e.key === 'Enter' && handleCreate()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleCreate}>下一步</button>
		</div>
	</div>

{:else if screen === 'confirm'}
	<div class="container center">
		<div class="card">
			<h2>swallet</h2>
			<p class="dim">再次输入密码确认</p>
			<input type="password" bind:value={confirmPassword} placeholder="确认密码" onkeydown={(e) => e.key === 'Enter' && handleCreate()} autofocus />
			{#if error}<p class="error">{error}</p>{/if}
			<button class="btn-primary" onclick={handleCreate}>创建</button>
		</div>
	</div>

{:else if screen === 'show-mnemonic'}
	<div class="container center">
		<div class="card">
			<h2>助记词</h2>
			<p class="dim">请安全保存以下助记词</p>
			<div class="mnemonic-box">{mnemonicPhrase}</div>
			<button class="btn-primary" onclick={() => { screen = 'input-name'; }}>已保存，继续</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'add-wallet'}
	<div class="container center">
		<div class="card">
			<h2>{addWalletType === 'import-mnemonic' ? '导入助记词' : addWalletType === 'private-key' ? '导入私钥' : '观察钱包'}</h2>
			{#if addWalletType === 'private-key' || addWalletType === 'watch'}
				<div class="chain-select">
					<button class:active={selectedChain === 'solana'} onclick={() => selectedChain = 'solana'}>SOL</button>
					<button class:active={selectedChain === 'ethereum'} onclick={() => selectedChain = 'ethereum'}>ETH</button>
				</div>
			{/if}
			<textarea bind:value={importInput} placeholder={addWalletType === 'import-mnemonic' ? '输入助记词（空格分隔）' : addWalletType === 'private-key' ? '输入私钥' : '输入地址'} rows="3"></textarea>
			<button class="btn-primary" onclick={proceedToName}>下一步</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'input-name'}
	<div class="container center">
		<div class="card">
			<h2>钱包名称</h2>
			<input bind:value={walletName} placeholder="输入钱包名称" onkeydown={(e) => e.key === 'Enter' && finishAddWallet()} autofocus />
			<button class="btn-primary" onclick={finishAddWallet}>完成</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'main'}
	<div class="container" onclick={closeMenu}>
		<header>
			<h1>swallet</h1>
			<div class="header-actions">
				{#if loading}<span class="dim">刷新中...</span>{/if}
				<button class="btn-icon" onclick={refreshBalances} title="刷新">↻</button>
				<button class="btn-icon" onclick={restoreHidden} title="恢复隐藏">👁</button>
			</div>
		</header>

		{#each wallets.filter(w => !w.hidden) as wallet, wi (wallet.id)}
			<div class="wallet-card">
				<div class="wallet-header" onclick={(e) => openMenu('wallet', wi, undefined, undefined, wallet.wallet_type, e)}>
					<span class="wallet-name">{wallet.name}</span>
					<span class="wallet-type">{wallet.wallet_type}</span>
				</div>
				{#each wallet.accounts.filter(a => !a.hidden) as account, ai}
					<div class="account-row">
						<button class="account-main" onclick={() => copyAddress(account.address)} title="点击复制">
							<span class="chain-badge">{account.chain_type === 'ethereum' ? 'ETH' : 'SOL'}</span>
							{#if account.label}<span class="label">{account.label}</span>{/if}
							<span class="address">{account.address.slice(0, 6)}...{account.address.slice(-4)}</span>
						</button>
						<div class="account-right">
							<div class="account-balances">
								{#each getBalance(account.address) as bal}
									<span class="balance">{bal.amount} {bal.symbol}</span>
								{/each}
							</div>
							<button class="btn-dots" onclick={(e) => openMenu('account', wi, ai, account.chain_type, wallet.wallet_type, e)} title="操作">⋮</button>
						</div>
					</div>
				{/each}
			</div>
		{/each}

		{#if wallets.length === 0}
			<div class="empty"><p class="dim">还没有钱包</p></div>
		{/if}

		<!-- Add wallet buttons -->
		<div class="add-buttons">
			<button class="btn-add" onclick={() => startAddWallet('mnemonic')}>+ 创建钱包</button>
			<button class="btn-add" onclick={() => startAddWallet('import-mnemonic')}>导入助记词</button>
			<button class="btn-add" onclick={() => startAddWallet('private-key')}>导入私钥</button>
			<button class="btn-add" onclick={() => startAddWallet('watch')}>观察钱包</button>
		</div>
	</div>

	<!-- Context Menu -->
	{#if menuTarget}
		<div class="overlay" onclick={closeMenu}>
			<div class="context-menu" onclick={(e) => e.stopPropagation()}>
				{#if menuTarget.type === 'wallet'}
					<button onclick={() => menuAction('rename')}>修改名称</button>
					{#if menuTarget.walletType === 'mnemonic'}
						<button onclick={() => menuAction('add-eth')}>添加 ETH 地址</button>
						<button onclick={() => menuAction('add-sol')}>添加 SOL 地址</button>
					{/if}
					<button onclick={() => menuAction('move-up')}>上移</button>
					<button onclick={() => menuAction('move-down')}>下移</button>
					<button onclick={() => menuAction('hide-wallet')}>隐藏</button>
					<button class="danger" onclick={() => menuAction('delete')}>删除</button>
				{:else}
					<button onclick={() => menuAction('relabel')}>修改标签</button>
					<button onclick={() => menuAction('hide-address')}>隐藏</button>
				{/if}
				<button onclick={closeMenu}>取消</button>
			</div>
		</div>
	{/if}

	<!-- Dialogs -->
	{#if dialogType}
		<div class="overlay">
			<div class="dialog">
				{#if dialogType === 'rename'}
					<h3>修改名称</h3>
					<input bind:value={dialogInput} autofocus />
				{:else if dialogType === 'relabel'}
					<h3>修改标签</h3>
					<input bind:value={dialogInput} placeholder="留空清除标签" autofocus />
				{:else if dialogType === 'delete'}
					<h3>确认删除</h3>
					<p class="dim">输入密码确认删除</p>
					<input type="password" bind:value={dialogPassword} autofocus />
				{/if}
				<div class="dialog-actions">
					<button class="btn-secondary" onclick={() => { dialogType = null; }}>取消</button>
					<button class="btn-primary" onclick={confirmDialog}>确认</button>
				</div>
			</div>
		</div>
	{/if}
{/if}

{#if toast}<div class="toast">{toast}</div>{/if}

<style>
	.container { max-width: 420px; margin: 0 auto; padding: 16px; min-height: 100vh; }
	.center { display: flex; align-items: center; justify-content: center; }
	.card { width: 100%; padding: 32px 24px; display: flex; flex-direction: column; gap: 16px; align-items: center; }
	h1, h2, h3 { text-align: center; }
	h2 { color: var(--accent); font-size: 24px; }
	h3 { font-size: 18px; }
	.dim { color: var(--text-dim); font-size: 14px; }
	.error { color: var(--red); font-size: 14px; }
	textarea { border: 1px solid var(--border); background: var(--bg); color: var(--text); padding: 10px; border-radius: 8px; font-size: 14px; width: 100%; resize: none; outline: none; font-family: monospace; }
	textarea:focus { border-color: var(--accent); }

	.btn-primary { width: 100%; padding: 12px; background: var(--accent); color: var(--bg); border-radius: 8px; font-size: 16px; font-weight: 600; }
	.btn-primary:hover { opacity: 0.9; }
	.btn-secondary { width: 100%; padding: 10px; color: var(--text-dim); font-size: 14px; }
	.btn-secondary:hover { color: var(--text); }
	.btn-icon { width: 32px; height: 32px; border-radius: 6px; font-size: 18px; color: var(--text-dim); display: flex; align-items: center; justify-content: center; }
	.btn-icon:hover { background: var(--bg-hover); color: var(--text); }
	.btn-dots { width: 24px; color: var(--text-dim); font-size: 18px; }
	.btn-dots:hover { color: var(--text); }

	header { display: flex; align-items: center; justify-content: space-between; padding: 8px 0 16px; }
	header h1 { font-size: 20px; color: var(--accent); }
	.header-actions { display: flex; align-items: center; gap: 4px; }

	.wallet-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; margin-bottom: 12px; overflow: hidden; }
	.wallet-header { padding: 12px 16px; display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border); cursor: pointer; }
	.wallet-header:hover { background: var(--bg-hover); }
	.wallet-name { font-weight: 600; font-size: 14px; }
	.wallet-type { color: var(--text-dim); font-size: 12px; }

	.account-row { padding: 8px 16px; display: flex; justify-content: space-between; align-items: center; border-bottom: 1px solid var(--border); }
	.account-row:last-child { border-bottom: none; }
	.account-main { display: flex; align-items: center; gap: 8px; flex: 1; text-align: left; }
	.account-main:hover { opacity: 0.8; }
	.account-right { display: flex; align-items: center; gap: 4px; }

	.chain-badge { background: var(--bg); color: var(--accent); padding: 2px 6px; border-radius: 4px; font-size: 11px; font-weight: 600; }
	.label { color: var(--yellow); font-size: 13px; }
	.address { color: var(--text-dim); font-size: 13px; font-family: monospace; }
	.account-balances { display: flex; flex-direction: column; align-items: flex-end; gap: 1px; }
	.balance { color: var(--green); font-size: 12px; font-family: monospace; }

	.mnemonic-box { background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; padding: 16px; font-family: monospace; font-size: 14px; line-height: 1.8; word-spacing: 8px; width: 100%; user-select: text; -webkit-user-select: text; }

	.chain-select { display: flex; gap: 8px; }
	.chain-select button { padding: 6px 16px; border-radius: 6px; border: 1px solid var(--border); color: var(--text-dim); font-size: 14px; }
	.chain-select button.active { border-color: var(--accent); color: var(--accent); background: rgba(34, 211, 238, 0.1); }

	.add-buttons { display: flex; flex-wrap: wrap; gap: 8px; padding: 8px 0 24px; }
	.btn-add { padding: 8px 14px; border: 1px solid var(--border); border-radius: 8px; color: var(--text-dim); font-size: 13px; }
	.btn-add:hover { border-color: var(--accent); color: var(--accent); }

	.empty { text-align: center; padding: 48px 0; }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: flex-end; justify-content: center; z-index: 50; }
	.context-menu { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px 12px 0 0; padding: 8px 0; width: 100%; max-width: 420px; }
	.context-menu button { display: block; width: 100%; padding: 14px 20px; text-align: left; font-size: 15px; color: var(--text); }
	.context-menu button:hover { background: var(--bg-hover); }
	.context-menu button.danger { color: var(--red); }

	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; margin: auto; display: flex; flex-direction: column; gap: 12px; }
	.dialog-actions { display: flex; gap: 8px; }
	.dialog-actions button { flex: 1; }

	.toast { position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%); background: var(--bg-card); border: 1px solid var(--border); color: var(--text); padding: 8px 20px; border-radius: 8px; font-size: 14px; z-index: 100; }
</style>
