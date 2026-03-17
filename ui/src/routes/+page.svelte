<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { WalletDto, BalanceDto, AssetDto } from '$lib/types';
	import MultisigPanel from '$lib/components/MultisigPanel.svelte';
	import StakingPanel from '$lib/components/StakingPanel.svelte';
	import PasswordDialog from '$lib/components/PasswordDialog.svelte';

	// Global password dialog
	let pwDialogTitle = $state('');
	let pwDialogCallback: ((pw: string) => void) | null = $state(null);

	function requestPassword(title: string, callback: (pw: string) => void) {
		pwDialogTitle = title;
		pwDialogCallback = callback;
	}

	function closePwDialog() { pwDialogCallback = null; }

	// Screen state
	let screen: string = $state('loading');
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
	let menuTarget: { type: 'wallet' | 'account'; walletIndex: number; accountIndex?: number; chainType?: string; walletType?: string; address?: string } | null = $state(null);
	let showMainMenu = $state(false);

	// Dialog
	let dialogType: 'rename' | 'relabel' | null = $state(null);
	let dialogInput = $state('');

	// Transfer
	let txWalletIndex = $state(0);
	let txAccountIndex = $state(0);
	let txChainType = $state('');
	let txAddress = $state('');
	let txAssets: AssetDto[] = $state([]);
	let txSelectedAsset = $state(0);
	let txToAddress = $state('');
	let txAmount = $state('');
	let txSending = $state(false);
	let txResult = $state<{ success: boolean; message: string } | null>(null);

	// Import multisig
	let importMsChains: { id: string; name: string; rpc_url: string }[] = $state([]);
	let importMsChainId = $state('');
	let importMsAddress = $state('');

	// Create vote/stake
	let createStakingType: 'vote' | 'stake' = $state('vote');
	let createStakingChainId = $state('');
	let createStakingRpcUrl = $state('');
	let createStakingIdentity = $state('');
	let createStakingWithdrawer = $state('');
	let createStakingAmount = $state('');
	let createStakingLockupDays = $state('0');
	let createStakingPassword = $state('');

	// Create multisig
	let createMsMembers: string[] = $state([]);
	let createMsMemberInput = $state('');
	let createMsThreshold = $state('2');
	let createMsPassword = $state('');
	let createMsSeed = $state('');
	let createMsUseSeed = $state(false);

	// Multisig/Staking
	let msWalletIndex = $state(0);
	let stakingAddress = $state('');
	let stakingRpcUrl = $state('');
	let stakingWalletIndex = $state(0);
	let stakingAccountIndex = $state(0);
	let stakingAccountOwner = $state<string | null>(null);

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
			result.push({ symbol: chain.native_symbol, amount: chain.rpc_failed ? '-' : chain.native_balance });
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
	function openMenu(type: 'wallet' | 'account', walletIndex: number, accountIndex?: number, chainType?: string, walletType?: string, address?: string, e?: MouseEvent) {
		e?.stopPropagation();
		menuTarget = { type, walletIndex, accountIndex, chainType, walletType, address };
	}

	function closeMenu() { menuTarget = null; }

	async function menuAction(action: string) {
		if (!menuTarget) return;
		const { walletIndex, accountIndex, chainType, walletType } = menuTarget;
		closeMenu();
		try {
			switch (action) {
				case 'transfer':
					await startTransfer(walletIndex, accountIndex!, chainType!);
					break;
				case 'multisig':
					msWalletIndex = walletIndex;
					screen = 'multisig';
					break;
				case 'staking': {
					const w = wallets[walletIndex];
					const acc = w?.accounts[accountIndex!];
					if (acc) {
						stakingAddress = acc.address;
						stakingWalletIndex = walletIndex;
						stakingAccountIndex = accountIndex!;
						const b = balances.find(b => b.address === acc.address);
						stakingAccountOwner = b?.account_owner || null;
						try { stakingRpcUrl = await api.getRpcUrlForAddress(acc.address); } catch (_) {}
						screen = 'staking';
					}
					break;
				}
				case 'create-vote':
				case 'create-stake': {
					createStakingType = action === 'create-vote' ? 'vote' : 'stake';
					createStakingIdentity = '';
					createStakingWithdrawer = '';
					createStakingAmount = '';
					createStakingLockupDays = '0';
					createStakingPassword = '';
					try {
						const chains = await api.getSolanaChains();
						importMsChains = chains;
						createStakingChainId = chains[0]?.id || '';
						createStakingRpcUrl = chains[0]?.rpc_url || '';
					} catch (_) {}
					screen = 'create-staking';
					break;
				}
				case 'create-multisig':
				case 'create-multisig-seed': {
					createMsUseSeed = action === 'create-multisig-seed';
					createMsSeed = '';
					createMsMembers = [];
					createMsMemberInput = '';
					createMsThreshold = '2';
					createMsPassword = '';
					// Auto-add current address as first member
					const cw = wallets[walletIndex];
					const ca = cw?.accounts[accountIndex!];
					if (ca) createMsMembers = [ca.address];
					try {
						const chains = await api.getSolanaChains();
						importMsChains = chains;
						importMsChainId = chains[0]?.id || '';
					} catch (_) {}
					screen = 'create-multisig';
					break;
				}
				case 'import-multisig':
					try {
						const chains = await api.getSolanaChains();
						importMsChains = chains;
						importMsChainId = chains[0]?.id || '';
						importMsAddress = '';
						screen = 'import-multisig';
					} catch (_) { showToast('加载链配置失败'); }
					break;
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
					requestPassword('确认删除钱包', async (pw) => {
						closePwDialog();
						try { await api.deleteWallet(walletIndex, pw); await reloadWallets(); showToast('已删除'); }
						catch (e: any) { showToast(e?.message || '删除失败'); }
					});
					break;
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

	// Transfer flow
	async function startTransfer(walletIndex: number, accountIndex: number, chainType: string) {
		txWalletIndex = walletIndex;
		txChainType = chainType;
		// accountIndex is the position in the combined accounts array
		// Need to compute the chain-specific index
		const wallet = wallets[walletIndex];
		const allAccs = wallet?.accounts || [];
		const acc = allAccs[accountIndex];
		if (!acc) { showToast('无效的地址'); return; }
		txAddress = acc.address;
		// Compute chain-specific index
		const chainAccs = allAccs.filter(a => a.chain_type === chainType);
		txAccountIndex = chainAccs.findIndex(a => a.address === acc.address);
		if (txAccountIndex < 0) txAccountIndex = 0;
		txToAddress = '';
		txAmount = '';
		txPassword = '';
		txResult = null;
		try {
			txAssets = await api.buildTransferAssets(txAddress, chainType);
			txSelectedAsset = 0;
			screen = 'transfer-assets';
		} catch (e: any) { showToast(e?.message || '加载资产失败'); }
	}

	async function submitTransfer() {
		if (!txToAddress.trim()) { showToast('请输入目标地址'); return; }
		if (!txAmount.trim()) { showToast('请输入金额'); return; }
		requestPassword('确认转账', async (pw) => {
			closePwDialog();
			txSending = true;
			try {
				const sig = await api.executeTransfer(pw, txWalletIndex, txAccountIndex, txChainType, txSelectedAsset, txToAddress, txAmount);
				txResult = { success: true, message: sig };
			} catch (e: any) {
				txResult = { success: false, message: e?.message || '转账失败' };
			}
			txSending = false;
			screen = 'transfer-result';
		});
	}

	function getAccountOwner(address: string): string | null {
		return balances.find(b => b.address === address)?.account_owner || null;
	}

	function isVoteOrStake(address: string): boolean {
		const owner = getAccountOwner(address);
		return owner === 'Vote111111111111111111111111111111111111111' || owner === 'Stake11111111111111111111111111111111111111';
	}

	function walletTypeLabel(t: string): string {
		switch (t) {
			case 'mnemonic': return '助记词钱包';
			case 'private_key': return '私钥钱包';
			case 'watch_only': return '观察钱包';
			case 'multisig': return '多签钱包';
			default: return t;
		}
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
				<button class="btn-icon" onclick={(e) => { e.stopPropagation(); showMainMenu = true; }} title="菜单">☰</button>
			</div>
		</header>

		{#each wallets.filter(w => !w.hidden) as wallet, wi (wallet.id)}
			<div class="wallet-card">
				<div class="wallet-header" onclick={(e) => openMenu('wallet', wi, undefined, undefined, wallet.wallet_type, undefined, e)}>
					<span class="wallet-name">{wallet.name}</span>
					<span class="wallet-type">{walletTypeLabel(wallet.wallet_type)}</span>
				</div>
				{#each wallet.accounts.filter(a => !a.hidden) as account, ai}
					<div class="account-row" onclick={() => copyAddress(account.address)} title="点击复制" role="button" tabindex="0">
						<div class="account-top">
							<span class="chain-badge">{account.chain_type === 'ethereum' ? 'ETH' : 'SOL'}</span>
							{#if isVoteOrStake(account.address)}
								<span class="tag-vote-stake">{getAccountOwner(account.address) === 'Vote111111111111111111111111111111111111111' ? 'Vote' : 'Stake'}</span>
							{/if}
							{#if account.label}<span class="label">{account.label}</span>{/if}
							<button class="btn-dots" onclick={(e) => openMenu('account', wi, ai, account.chain_type, wallet.wallet_type, account.address, e)} title="操作">⋮</button>
						</div>
						<div class="address">{account.address}</div>
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
			<div class="empty"><p class="dim">还没有钱包</p></div>
		{/if}

		<!-- padding bottom for scroll -->
		<div style="height:24px"></div>
	</div>

	<!-- Context Menu -->
	{#if menuTarget}
		<div class="overlay" onclick={closeMenu}>
			<div class="context-menu" onclick={(e) => e.stopPropagation()}>
				{#if menuTarget.type === 'wallet'}
					{#if menuTarget.walletType === 'multisig'}
						<button onclick={() => menuAction('multisig')}>多签管理</button>
					{/if}
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
					{#if menuTarget.walletType === 'multisig'}
						<button onclick={() => menuAction('multisig')}>多签管理</button>
					{:else if menuTarget.address && isVoteOrStake(menuTarget.address)}
						<button onclick={() => menuAction('staking')}>账户详情</button>
					{:else}
						<button onclick={() => menuAction('transfer')}>转账</button>
						{#if menuTarget.chainType === 'solana'}
							<button onclick={() => menuAction('create-vote')}>创建 Vote 账户</button>
							<button onclick={() => menuAction('create-stake')}>创建 Stake 账户</button>
							<button onclick={() => menuAction('create-multisig')}>创建多签地址（随机）</button>
							<button onclick={() => menuAction('create-multisig-seed')}>创建多签地址（指定种子）</button>
						{/if}
					{/if}
					<button onclick={() => menuAction('relabel')}>修改标签</button>
					<button onclick={() => menuAction('hide-address')}>隐藏</button>
				{/if}
				<button onclick={closeMenu}>取消</button>
			</div>
		</div>
	{/if}

	<!-- Main Menu -->
	{#if showMainMenu}
		<div class="overlay" onclick={() => { showMainMenu = false; }}>
			<div class="context-menu" onclick={(e) => e.stopPropagation()}>
				<button onclick={() => { showMainMenu = false; startAddWallet('mnemonic'); }}>创建助记词钱包</button>
				<button onclick={() => { showMainMenu = false; startAddWallet('import-mnemonic'); }}>导入助记词</button>
				<button onclick={() => { showMainMenu = false; startAddWallet('private-key'); }}>导入私钥</button>
				<button onclick={() => { showMainMenu = false; startAddWallet('watch'); }}>添加观察钱包</button>
				<button onclick={async () => { showMainMenu = false; try { const chains = await api.getSolanaChains(); importMsChains = chains; importMsChainId = chains[0]?.id || ''; importMsAddress = ''; screen = 'import-multisig'; } catch(_) { showToast('加载链配置失败'); } }}>导入多签地址</button>
				<button onclick={() => { showMainMenu = false; restoreHidden(); }}>恢复隐藏项</button>
				<button onclick={() => { showMainMenu = false; }}>取消</button>
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
				{/if}
				<div class="dialog-actions">
					<button class="btn-secondary" onclick={() => { dialogType = null; }}>取消</button>
					<button class="btn-primary" onclick={confirmDialog}>确认</button>
				</div>
			</div>
		</div>
	{/if}

{:else if screen === 'transfer-assets'}
	<div class="container">
		<header>
			<button class="btn-back" onclick={() => { screen = 'main'; }}>← 返回</button>
			<h1>选择资产</h1>
			<div></div>
		</header>
		<p class="dim" style="margin-bottom:12px">从 {txAddress.slice(0,6)}...{txAddress.slice(-4)}</p>
		{#each txAssets as asset}
			<button class="asset-row" class:selected={txSelectedAsset === asset.index}
				onclick={() => { txSelectedAsset = asset.index; screen = 'transfer-input'; }}>
				<span class="asset-info">
					<span class="chain-badge">{asset.chain_name}</span>
					<span>{asset.symbol}</span>
				</span>
				<span class="balance">{asset.balance} {asset.symbol}</span>
			</button>
		{/each}
		{#if txAssets.length === 0}<p class="dim" style="text-align:center;padding:24px">无可用资产</p>{/if}
	</div>

{:else if screen === 'transfer-input'}
	<div class="container center">
		<div class="card">
			<h2>转账 {txAssets[txSelectedAsset]?.symbol}</h2>
			<p class="dim">{txAssets[txSelectedAsset]?.chain_name} · 余额: {txAssets[txSelectedAsset]?.balance}</p>
			<input bind:value={txToAddress} placeholder="目标地址" />
			<input bind:value={txAmount} placeholder="金额" type="text" inputmode="decimal" />
			<button class="btn-primary" onclick={submitTransfer}>确认转账</button>
			<button class="btn-secondary" onclick={() => { screen = 'transfer-assets'; }}>返回</button>
		</div>
	</div>

{:else if screen === 'create-staking'}
	<div class="container center">
		<div class="card">
			<h2>{createStakingType === 'vote' ? '创建 Vote 账户' : '创建 Stake 账户'}</h2>
			{#if importMsChains.length > 1}
				<div class="chain-select">
					{#each importMsChains as chain}
						<button class:active={createStakingChainId === chain.id} onclick={() => { createStakingChainId = chain.id; createStakingRpcUrl = chain.rpc_url; }}>{chain.name}</button>
					{/each}
				</div>
			{/if}
			{#if createStakingType === 'vote'}
				<input bind:value={createStakingIdentity} placeholder="Identity 私钥 (可选)" />
				<input bind:value={createStakingWithdrawer} placeholder="Withdrawer 地址 (可选，默认当前地址)" />
			{:else}
				<input bind:value={createStakingAmount} placeholder="质押数量" type="text" inputmode="decimal" />
				<input bind:value={createStakingLockupDays} placeholder="锁仓天数 (0=不锁仓)" type="text" inputmode="numeric" />
			{/if}
			<button class="btn-primary" onclick={async () => {
				if (!menuTarget) { showToast('无效操作'); return; }
				const fp = (await api.getFeePayers())[0];
				if (!fp) { showToast('没有可用的 Fee Payer'); return; }
				const mt = menuTarget;
				requestPassword(createStakingType === 'vote' ? '确认创建 Vote 账户' : '确认创建 Stake 账户', async (pw) => {
					closePwDialog();
					try {
						let sig: string;
						if (createStakingType === 'vote') {
							sig = await api.createVoteAccount(mt.walletIndex, mt.accountIndex!, createStakingRpcUrl, fp.wallet_index, fp.account_index, createStakingIdentity, createStakingWithdrawer, pw);
						} else {
							sig = await api.createStakeAccount(mt.walletIndex, mt.accountIndex!, createStakingRpcUrl, fp.wallet_index, fp.account_index, createStakingAmount, parseInt(createStakingLockupDays) || 0, pw);
						}
						showToast(`创建成功: ${sig.slice(0,16)}...`);
						screen = 'main';
						refreshBalances();
					} catch (e: any) { showToast(e?.message || '创建失败'); }
				});
			}}>创建</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'create-multisig'}
	<div class="container center">
		<div class="card">
			<h2>创建多签地址</h2>
			{#if importMsChains.length > 1}
				<div class="chain-select">
					{#each importMsChains as chain}
						<button class:active={importMsChainId === chain.id} onclick={() => importMsChainId = chain.id}>{chain.name}</button>
					{/each}
				</div>
			{/if}
			<p class="dim">成员地址 ({createMsMembers.length})</p>
			{#each createMsMembers as m, i}
				<div class="member-row">
					<span class="mono dim">{m.slice(0,8)}...{m.slice(-4)}</span>
					{#if i > 0}<button class="btn-sm-x" onclick={() => { createMsMembers = createMsMembers.filter((_, j) => j !== i); }}>✕</button>{/if}
				</div>
			{/each}
			<div class="member-input-row">
				<input bind:value={createMsMemberInput} placeholder="添加成员地址" style="flex:1" />
				<button class="btn-add-member" onclick={() => {
					const addr = createMsMemberInput.trim();
					if (addr && !createMsMembers.includes(addr)) { createMsMembers = [...createMsMembers, addr]; createMsMemberInput = ''; }
				}}>+</button>
			</div>
			<input bind:value={createMsThreshold} placeholder="阈值" type="text" inputmode="numeric" />
			<label class="dim" style="display:flex;align-items:center;gap:8px">
				<input type="checkbox" bind:checked={createMsUseSeed} /> 使用种子
			</label>
			{#if createMsUseSeed}
				<input bind:value={createMsSeed} placeholder="种子私钥 (Base58)" />
			{/if}
			<button class="btn-primary" onclick={async () => {
				if (createMsMembers.length < 2) { showToast('至少需要2个成员'); return; }
				const t = parseInt(createMsThreshold);
				if (!t || t < 1 || t > createMsMembers.length) { showToast('无效的阈值'); return; }
				requestPassword('确认创建多签地址', async (pw) => {
					closePwDialog();
					try {
						const addr = await api.createMultisig(importMsChainId, createMsMembers[0], createMsMembers, t, pw, createMsUseSeed ? createMsSeed : undefined);
						showToast(`多签已创建: ${addr.slice(0,12)}...`);
						await reloadWallets();
						screen = 'main';
					} catch (e: any) { showToast(e?.message || '创建失败'); }
				});
			}}>创建多签地址</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'import-multisig'}
	<div class="container center">
		<div class="card">
			<h2>导入多签地址</h2>
			{#if importMsChains.length > 1}
				<div class="chain-select">
					{#each importMsChains as chain}
						<button class:active={importMsChainId === chain.id} onclick={() => importMsChainId = chain.id}>{chain.name}</button>
					{/each}
				</div>
			{/if}
			<input bind:value={importMsAddress} placeholder="多签地址 (Base58)" />
			<button class="btn-primary" onclick={async () => {
				if (!importMsAddress.trim()) { showToast('请输入地址'); return; }
				try { await api.importMultisig(importMsChainId, importMsAddress.trim()); showToast('导入成功'); await reloadWallets(); screen = 'main'; }
				catch (e) { showToast((e as any)?.message || '导入失败'); }
			}}>导入</button>
			<button class="btn-secondary" onclick={() => { screen = 'main'; }}>取消</button>
		</div>
	</div>

{:else if screen === 'multisig'}
	<MultisigPanel walletIndex={msWalletIndex} onClose={() => { screen = 'main'; reloadWallets(); }} onToast={showToast} />

{:else if screen === 'staking'}
	<StakingPanel address={stakingAddress} rpcUrl={stakingRpcUrl} walletIndex={stakingWalletIndex} accountIndex={stakingAccountIndex} accountOwner={stakingAccountOwner} onClose={() => { screen = 'main'; }} onToast={showToast} />

{:else if screen === 'transfer-result'}
	<div class="container center">
		<div class="card">
			{#if txResult?.success}
				<h2 style="color:var(--green)">转账成功</h2>
				<p class="dim" style="word-break:break-all;font-family:monospace;font-size:12px">{txResult.message}</p>
			{:else}
				<h2 style="color:var(--red)">转账失败</h2>
				<p class="error">{txResult?.message}</p>
			{/if}
			<button class="btn-primary" onclick={() => { screen = 'main'; refreshBalances(); }}>返回</button>
		</div>
	</div>
{/if}

{#if pwDialogCallback}
	<PasswordDialog title={pwDialogTitle} onConfirm={pwDialogCallback} onCancel={closePwDialog} />
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

	.account-row { display: flex; flex-direction: column; gap: 4px; padding: 10px 16px; border-bottom: 1px solid var(--border); cursor: pointer; }
	.account-row:last-child { border-bottom: none; }
	.account-row:hover { background: var(--bg-hover); }
	.account-top { display: flex; align-items: center; gap: 6px; }
	.account-top .btn-dots { margin-left: auto; }

	.chain-badge { background: var(--bg); color: var(--accent); padding: 2px 6px; border-radius: 4px; font-size: 11px; font-weight: 600; flex-shrink: 0; }
	.tag-vote-stake { color: var(--accent); font-size: 11px; font-weight: 600; background: rgba(34,211,238,0.15); padding: 2px 6px; border-radius: 4px; flex-shrink: 0; }
	.label { color: var(--yellow); font-size: 13px; }
	.address { color: var(--text-dim); font-size: 12px; font-family: monospace; word-break: break-all; line-height: 1.4; }
	.account-balances { display: flex; flex-wrap: wrap; gap: 4px 12px; }
	.balance { color: var(--green); font-size: 13px; font-family: monospace; }

	.mnemonic-box { background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; padding: 16px; font-family: monospace; font-size: 14px; line-height: 1.8; word-spacing: 8px; width: 100%; user-select: text; -webkit-user-select: text; }

	.chain-select { display: flex; gap: 8px; }
	.chain-select button { padding: 6px 16px; border-radius: 6px; border: 1px solid var(--border); color: var(--text-dim); font-size: 14px; }
	.chain-select button.active { border-color: var(--accent); color: var(--accent); background: rgba(34, 211, 238, 0.1); }

	.empty { text-align: center; padding: 48px 0; }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: flex-end; justify-content: center; z-index: 50; }
	.context-menu { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px 12px 0 0; padding: 8px 0; width: 100%; max-width: 420px; }
	.context-menu button { display: block; width: 100%; padding: 14px 20px; text-align: left; font-size: 15px; color: var(--text); }
	.context-menu button:hover { background: var(--bg-hover); }
	.context-menu button.danger { color: var(--red); }

	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; margin: auto; display: flex; flex-direction: column; gap: 12px; }
	.dialog-actions { display: flex; gap: 8px; }
	.dialog-actions button { flex: 1; }

	.btn-back { color: var(--accent); font-size: 14px; }
	.asset-row { display: flex; justify-content: space-between; align-items: center; width: 100%; padding: 12px 16px; background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; margin-bottom: 8px; text-align: left; transition: border-color 0.15s; }
	.asset-row:hover { border-color: var(--accent); }
	.asset-row.selected { border-color: var(--accent); background: rgba(34,211,238,0.05); }
	.asset-info { display: flex; align-items: center; gap: 8px; }
	.confirm-detail { background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 12px 16px; width: 100%; font-size: 14px; line-height: 1.8; }
	.confirm-detail .mono { font-family: monospace; font-size: 13px; }

	.member-row { display: flex; justify-content: space-between; align-items: center; padding: 6px 10px; background: var(--bg); border-radius: 6px; width: 100%; }
	.member-input-row { display: flex; gap: 8px; width: 100%; }
	.btn-add-member { width: 36px; height: 36px; border: 1px solid var(--accent); border-radius: 8px; color: var(--accent); font-size: 18px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; }
	.btn-sm-x { color: var(--red); font-size: 14px; }

	.toast { position: fixed; bottom: 24px; left: 50%; transform: translateX(-50%); background: var(--bg-card); border: 1px solid var(--border); color: var(--text); padding: 8px 20px; border-radius: 8px; font-size: 14px; z-index: 100; }
</style>
