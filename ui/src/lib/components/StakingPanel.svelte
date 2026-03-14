<script lang="ts">
	import { api } from '$lib/api';
	import type { VoteAccountDto, StakeAccountDto, FeePayerDto } from '$lib/types';

	let { address, rpcUrl, walletIndex, accountIndex, accountOwner, onClose, onToast }:
		{ address: string; rpcUrl: string; walletIndex: number; accountIndex: number; accountOwner: string | null; onClose: () => void; onToast: (msg: string) => void } = $props();

	let isVote = $derived(accountOwner === 'Vote111111111111111111111111111111111111111');
	let isStake = $derived(accountOwner === 'Stake11111111111111111111111111111111111111');

	let voteInfo: VoteAccountDto | null = $state(null);
	let stakeInfo: StakeAccountDto | null = $state(null);
	let loading = $state(true);
	let feePayers: FeePayerDto[] = $state([]);
	let selectedFeePayer = $state(0);

	// Action dialog
	let actionType: 'delegate' | 'deactivate' | 'withdraw' | null = $state(null);
	let actionInput = $state('');
	let actionAmount = $state('');
	let actionPassword = $state('');
	let submitting = $state(false);

	$effect(() => {
		loadAccount();
		loadFeePayers();
	});

	async function loadAccount() {
		loading = true;
		try {
			if (isVote) { voteInfo = await api.fetchVoteAccount(address, rpcUrl); }
			else if (isStake) { stakeInfo = await api.fetchStakeAccount(address, rpcUrl); }
		} catch (e: any) { onToast(e?.message || '加载失败'); }
		loading = false;
	}

	async function loadFeePayers() {
		try { feePayers = await api.getFeePayers(); } catch (_) {}
	}

	async function handleAction() {
		if (!actionPassword) { onToast('请输入密码'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }
		submitting = true;
		const fp = feePayers[selectedFeePayer];
		try {
			let sig: string;
			switch (actionType) {
				case 'delegate':
					sig = await api.stakeDelegate(walletIndex, accountIndex, rpcUrl, fp.wallet_index, fp.account_index, actionInput, actionPassword);
					break;
				case 'deactivate':
					sig = await api.stakeDeactivate(walletIndex, accountIndex, rpcUrl, fp.wallet_index, fp.account_index, actionPassword);
					break;
				case 'withdraw':
					sig = await api.stakeWithdraw(walletIndex, accountIndex, rpcUrl, fp.wallet_index, fp.account_index, actionInput, actionAmount, actionPassword);
					break;
				default: return;
			}
			onToast(`操作成功: ${sig.slice(0, 16)}...`);
			actionType = null;
			await loadAccount();
		} catch (e: any) { onToast(e?.message || '操作失败'); }
		submitting = false;
	}
</script>

<div class="panel">
	<header>
		<button class="btn-back" onclick={onClose}>← 返回</button>
		<h2>{isVote ? 'Vote 账户' : 'Stake 账户'}</h2>
		<div></div>
	</header>

	<p class="address">{address}</p>

	{#if loading}
		<p class="dim center-text">加载中...</p>
	{:else if isVote && voteInfo}
		<div class="info-card">
			<div class="info-row"><span class="dim">Identity</span><span class="mono">{voteInfo.validator_identity.slice(0,12)}...</span></div>
			<div class="info-row"><span class="dim">Voter</span><span class="mono">{voteInfo.authorized_voter.slice(0,12)}...</span></div>
			<div class="info-row"><span class="dim">Withdrawer</span><span class="mono">{voteInfo.authorized_withdrawer.slice(0,12)}...</span></div>
			<div class="info-row"><span class="dim">Commission</span><span>{voteInfo.commission}%</span></div>
			{#if voteInfo.credits}<div class="info-row"><span class="dim">Credits</span><span>{voteInfo.credits}</span></div>{/if}
		</div>
	{:else if isStake && stakeInfo}
		<div class="info-card">
			<div class="info-row"><span class="dim">状态</span><span>{stakeInfo.state}</span></div>
			<div class="info-row"><span class="dim">质押数量</span><span class="green">{stakeInfo.stake_lamports}</span></div>
			{#if stakeInfo.delegated_vote_account}
				<div class="info-row"><span class="dim">委托 Vote</span><span class="mono">{stakeInfo.delegated_vote_account.slice(0,12)}...</span></div>
			{/if}
			<div class="info-row"><span class="dim">Staker</span><span class="mono">{stakeInfo.authorized_staker.slice(0,12)}...</span></div>
			<div class="info-row"><span class="dim">Withdrawer</span><span class="mono">{stakeInfo.authorized_withdrawer.slice(0,12)}...</span></div>
		</div>

		<div class="actions">
			<button class="btn-action" onclick={() => { actionType = 'delegate'; actionInput = ''; actionPassword = ''; }}>委托</button>
			<button class="btn-action" onclick={() => { actionType = 'deactivate'; actionPassword = ''; }}>取消质押</button>
			<button class="btn-action" onclick={() => { actionType = 'withdraw'; actionInput = ''; actionAmount = ''; actionPassword = ''; }}>提取</button>
		</div>
	{:else}
		<p class="dim center-text">无法加载账户信息</p>
	{/if}
</div>

{#if actionType}
	<div class="overlay" onclick={() => { actionType = null; }}>
		<div class="dialog" onclick={(e) => e.stopPropagation()}>
			<h3>{actionType === 'delegate' ? '委托质押' : actionType === 'deactivate' ? '取消质押' : '提取质押'}</h3>
			{#if actionType === 'delegate'}
				<input bind:value={actionInput} placeholder="Vote 账户地址" />
			{:else if actionType === 'withdraw'}
				<input bind:value={actionInput} placeholder="提取到地址" />
				<input bind:value={actionAmount} placeholder="提取金额" type="text" inputmode="decimal" />
			{/if}
			{#if feePayers.length > 0}
				<label class="dim">Fee Payer</label>
				<select bind:value={selectedFeePayer}>
					{#each feePayers as fp, i}<option value={i}>{fp.address.slice(0,8)}... ({fp.balance})</option>{/each}
				</select>
			{/if}
			<input type="password" bind:value={actionPassword} placeholder="密码确认" autofocus />
			<div class="dialog-actions">
				<button class="btn-secondary" onclick={() => { actionType = null; }}>取消</button>
				<button class="btn-primary" onclick={handleAction} disabled={submitting}>{submitting ? '处理中...' : '确认'}</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.panel { padding: 16px; max-width: 420px; margin: 0 auto; }
	header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; }
	h2 { font-size: 18px; color: var(--accent); }
	h3 { text-align: center; font-size: 18px; }
	.btn-back { color: var(--accent); font-size: 14px; cursor: pointer; background: none; border: none; }
	.dim { color: var(--text-dim); font-size: 14px; }
	.green { color: var(--green); }
	.center-text { text-align: center; padding: 24px 0; }
	.address { font-family: monospace; font-size: 12px; color: var(--text-dim); word-break: break-all; margin-bottom: 16px; }
	.mono { font-family: monospace; font-size: 13px; }

	.info-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; overflow: hidden; margin-bottom: 16px; }
	.info-row { display: flex; justify-content: space-between; padding: 10px 14px; border-bottom: 1px solid var(--border); font-size: 13px; }
	.info-row:last-child { border-bottom: none; }

	.actions { display: flex; gap: 8px; }
	.btn-action { flex: 1; padding: 10px; border: 1px solid var(--border); border-radius: 8px; color: var(--accent); font-size: 13px; background: none; cursor: pointer; }
	.btn-action:hover { border-color: var(--accent); }

	select { border: 1px solid var(--border); background: var(--bg); color: var(--text); padding: 10px; border-radius: 8px; font-size: 14px; width: 100%; }
	label { font-size: 12px; }
	.btn-primary { width: 100%; padding: 12px; background: var(--accent); color: var(--bg); border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.btn-secondary { flex: 1; padding: 10px; color: var(--text-dim); font-size: 14px; background: none; border: none; cursor: pointer; }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	.dialog-actions { display: flex; gap: 8px; }
</style>
