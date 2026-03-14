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

	async function copyAddr(addr: string) {
		try { await navigator.clipboard.writeText(addr); onToast('已复制'); } catch (_) {}
	}

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
					if (isVote) {
						sig = await api.voteWithdraw(walletIndex, accountIndex, rpcUrl, fp.wallet_index, fp.account_index, actionInput, actionAmount, actionPassword);
					} else {
						sig = await api.stakeWithdraw(walletIndex, accountIndex, rpcUrl, fp.wallet_index, fp.account_index, actionInput, actionAmount, actionPassword);
					}
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

	{#if loading}
		<p class="dim center-text">加载中...</p>
	{:else if isVote && voteInfo}
		<div class="info-card">
			<div class="info-item"><span class="info-label">地址</span><button class="addr-copy" onclick={() => copyAddr(address)}>{address}</button></div>
			<div class="info-item"><span class="info-label">Identity</span><button class="addr-copy" onclick={() => copyAddr(voteInfo.validator_identity)}>{voteInfo.validator_identity}</button></div>
			<div class="info-item"><span class="info-label">Voter</span><button class="addr-copy" onclick={() => copyAddr(voteInfo.authorized_voter)}>{voteInfo.authorized_voter}</button></div>
			<div class="info-item"><span class="info-label">Withdrawer</span><button class="addr-copy" onclick={() => copyAddr(voteInfo.authorized_withdrawer)}>{voteInfo.authorized_withdrawer}</button></div>
			<div class="info-item"><span class="info-label">Commission</span><span class="info-value">{voteInfo.commission}%</span></div>
			{#if voteInfo.credits}<div class="info-item"><span class="info-label">Credits</span><span class="info-value">{voteInfo.credits}</span></div>{/if}
		</div>

		<div class="actions">
			<button class="btn-action" onclick={() => { actionType = 'withdraw'; actionInput = ''; actionAmount = ''; actionPassword = ''; }}>提取</button>
		</div>
	{:else if isStake && stakeInfo}
		<div class="info-card">
			<div class="info-item"><span class="info-label">地址</span><button class="addr-copy" onclick={() => copyAddr(address)}>{address}</button></div>
			<div class="info-item"><span class="info-label">状态</span><span class="info-value">{stakeInfo.state}</span></div>
			<div class="info-item"><span class="info-label">质押数量</span><span class="info-value green">{stakeInfo.stake_lamports}</span></div>
			{#if stakeInfo.delegated_vote_account}
				<div class="info-item"><span class="info-label">委托 Vote</span><button class="addr-copy" onclick={() => copyAddr(stakeInfo.delegated_vote_account!)}>{stakeInfo.delegated_vote_account}</button></div>
			{/if}
			<div class="info-item"><span class="info-label">Staker</span><button class="addr-copy" onclick={() => copyAddr(stakeInfo.authorized_staker)}>{stakeInfo.authorized_staker}</button></div>
			<div class="info-item"><span class="info-label">Withdrawer</span><button class="addr-copy" onclick={() => copyAddr(stakeInfo.authorized_withdrawer)}>{stakeInfo.authorized_withdrawer}</button></div>
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
			<input type="password" bind:value={actionPassword} placeholder="输入密码" autofocus
				onkeydown={(e) => { if (e.key === 'Enter') handleAction(); }} />
			<div class="pw-actions">
				<button class="pw-btn cancel" onclick={() => { actionType = null; }}>取消</button>
				<button class="pw-btn confirm" onclick={handleAction} disabled={submitting}>{submitting ? '处理中...' : '确认'}</button>
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
	.mono { font-family: monospace; font-size: 13px; }

	.info-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; overflow: hidden; margin-bottom: 16px; }
	.info-item { display: flex; flex-direction: column; gap: 4px; padding: 10px 14px; border-bottom: 1px solid var(--border); }
	.info-item:last-child { border-bottom: none; }
	.info-label { color: var(--text-dim); font-size: 12px; }
	.info-value { font-size: 14px; }
	.addr-copy { font-family: monospace; font-size: 12px; color: var(--text); word-break: break-all; text-align: left; line-height: 1.4; background: none; border: none; cursor: pointer; padding: 0; }
	.addr-copy:hover { color: var(--accent); }

	.actions { display: flex; gap: 8px; }
	.btn-action { flex: 1; padding: 10px; border: 1px solid var(--border); border-radius: 8px; color: var(--accent); font-size: 13px; background: none; cursor: pointer; }
	.btn-action:hover { border-color: var(--accent); }

	label { font-size: 12px; }
	.pw-actions { display: flex; gap: 8px; }
	.pw-btn { flex: 1; padding: 12px; border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.pw-btn.confirm { background: var(--accent); color: var(--bg); }
	.pw-btn.cancel { background: var(--bg); color: var(--text-dim); border: 1px solid var(--border); }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	.dialog-actions { display: flex; gap: 8px; }
</style>
