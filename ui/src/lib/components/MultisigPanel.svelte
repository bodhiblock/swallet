<script lang="ts">
	import { api } from '$lib/api';
	import type { ProposalDto, FeePayerDto, ChainDto } from '$lib/types';

	let { walletIndex, onClose, onToast }: { walletIndex: number; onClose: () => void; onToast: (msg: string) => void } = $props();

	let tab: 'proposals' | 'import' | 'create-proposal' = $state('proposals');
	let proposals: ProposalDto[] = $state([]);
	let feePayers: FeePayerDto[] = $state([]);
	let chains: ChainDto[] = $state([]);
	let loadingProposals = $state(false);

	// Import
	let importChainId = $state('');
	let importAddress = $state('');

	// Create proposal
	let proposalTo = $state('');
	let proposalAmount = $state('');

	// Vote dialog
	let voteDialog: { action: 'approve' | 'reject' | 'execute'; proposal: ProposalDto } | null = $state(null);
	let votePassword = $state('');
	let selectedFeePayer = $state(0);
	let submitting = $state(false);

	$effect(() => {
		loadProposals();
		loadFeePayers();
		loadChains();
	});

	async function loadProposals() {
		loadingProposals = true;
		try { proposals = await api.fetchProposals(walletIndex); } catch (e: any) { onToast(e?.message || '加载失败'); }
		loadingProposals = false;
	}

	async function loadFeePayers() {
		try { feePayers = await api.getFeePayers(); } catch (_) {}
	}

	async function loadChains() {
		try { chains = await api.getSolanaChains(); if (chains.length > 0) importChainId = chains[0].id; } catch (_) {}
	}

	async function handleImport() {
		if (!importAddress.trim()) { onToast('请输入多签地址'); return; }
		try {
			await api.importMultisig(importChainId, importAddress.trim());
			onToast('导入成功');
			onClose();
		} catch (e: any) { onToast(e?.message || '导入失败'); }
	}

	async function handleCreateProposal() {
		if (!proposalTo.trim() || !proposalAmount.trim()) { onToast('请填写完整'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }
		voteDialog = null;
		votePassword = '';
		submitting = true;
		const fp = feePayers[selectedFeePayer];
		try {
			const sig = await api.createSolTransferProposal(walletIndex, 0, proposalTo, proposalAmount, votePassword, fp.wallet_index, fp.account_index);
			onToast(`提案已创建: ${sig.slice(0, 16)}...`);
			proposalTo = ''; proposalAmount = '';
			tab = 'proposals';
			await loadProposals();
		} catch (e: any) { onToast(e?.message || '创建失败'); }
		submitting = false;
	}

	async function handleVote() {
		if (!voteDialog || !votePassword) { onToast('请输入密码'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }
		submitting = true;
		const fp = feePayers[selectedFeePayer];
		const p = voteDialog.proposal;
		try {
			let sig: string;
			if (voteDialog.action === 'approve') {
				sig = await api.approveProposal(walletIndex, p.transaction_index, votePassword, fp.wallet_index, fp.account_index);
			} else if (voteDialog.action === 'reject') {
				sig = await api.rejectProposal(walletIndex, p.transaction_index, votePassword, fp.wallet_index, fp.account_index);
			} else {
				sig = await api.executeProposal(walletIndex, p.transaction_index, 0, votePassword, fp.wallet_index, fp.account_index);
			}
			onToast(`操作成功: ${sig.slice(0, 16)}...`);
			voteDialog = null;
			await loadProposals();
		} catch (e: any) { onToast(e?.message || '操作失败'); }
		submitting = false;
	}

	function statusColor(status: string): string {
		switch (status) {
			case '投票中': return 'var(--yellow)';
			case '已通过': return 'var(--green)';
			case '已执行': return 'var(--accent)';
			case '已拒绝': case '已取消': return 'var(--red)';
			default: return 'var(--text-dim)';
		}
	}
</script>

<div class="panel">
	<header>
		<button class="btn-back" onclick={onClose}>← 返回</button>
		<h2>多签管理</h2>
		<div></div>
	</header>

	<div class="tabs">
		<button class:active={tab === 'proposals'} onclick={() => tab = 'proposals'}>提案</button>
		<button class:active={tab === 'import'} onclick={() => tab = 'import'}>导入</button>
		<button class:active={tab === 'create-proposal'} onclick={() => tab = 'create-proposal'}>创建提案</button>
	</div>

	{#if tab === 'proposals'}
		{#if loadingProposals}
			<p class="dim center-text">加载中...</p>
		{:else if proposals.length === 0}
			<p class="dim center-text">暂无提案</p>
		{:else}
			{#each proposals as proposal}
				<div class="proposal-card">
					<div class="proposal-header">
						<span>TX #{proposal.transaction_index}</span>
						<span style="color:{statusColor(proposal.status)}">{proposal.status}</span>
					</div>
					<div class="proposal-votes">
						<span class="dim">通过: {proposal.approved_count} · 拒绝: {proposal.rejected_count}</span>
					</div>
					<div class="proposal-actions">
						{#if proposal.status === '投票中'}
							<button class="btn-sm green" onclick={() => { voteDialog = { action: 'approve', proposal }; votePassword = ''; }}>审批</button>
							<button class="btn-sm red" onclick={() => { voteDialog = { action: 'reject', proposal }; votePassword = ''; }}>拒绝</button>
						{:else if proposal.status === '已通过'}
							<button class="btn-sm accent" onclick={() => { voteDialog = { action: 'execute', proposal }; votePassword = ''; }}>执行</button>
						{/if}
					</div>
				</div>
			{/each}
		{/if}
		<button class="btn-refresh" onclick={loadProposals}>刷新提案列表</button>

	{:else if tab === 'import'}
		<div class="form">
			{#if chains.length > 1}
				<select bind:value={importChainId}>
					{#each chains as chain}<option value={chain.id}>{chain.name}</option>{/each}
				</select>
			{/if}
			<input bind:value={importAddress} placeholder="输入多签地址 (Base58)" />
			<button class="btn-primary" onclick={handleImport}>导入</button>
		</div>

	{:else if tab === 'create-proposal'}
		<div class="form">
			<p class="dim">创建 SOL 转账提案</p>
			<input bind:value={proposalTo} placeholder="目标地址" />
			<input bind:value={proposalAmount} placeholder="金额 (SOL)" type="text" inputmode="decimal" />
			{#if feePayers.length > 0}
				<label class="dim">Fee Payer</label>
				<select bind:value={selectedFeePayer}>
					{#each feePayers as fp, i}<option value={i}>{fp.address.slice(0,8)}... ({fp.balance})</option>{/each}
				</select>
			{/if}
			<input type="password" bind:value={votePassword} placeholder="密码确认" />
			<button class="btn-primary" onclick={handleCreateProposal} disabled={submitting}>{submitting ? '提交中...' : '创建提案'}</button>
		</div>
	{/if}
</div>

<!-- Vote Dialog -->
{#if voteDialog}
	<div class="overlay" onclick={() => { voteDialog = null; }}>
		<div class="dialog" onclick={(e) => e.stopPropagation()}>
			<h3>{voteDialog.action === 'approve' ? '审批提案' : voteDialog.action === 'reject' ? '拒绝提案' : '执行提案'}</h3>
			<p class="dim">TX #{voteDialog.proposal.transaction_index}</p>
			{#if feePayers.length > 0}
				<label class="dim">Fee Payer</label>
				<select bind:value={selectedFeePayer}>
					{#each feePayers as fp, i}<option value={i}>{fp.address.slice(0,8)}... ({fp.balance})</option>{/each}
				</select>
			{/if}
			<input type="password" bind:value={votePassword} placeholder="输入密码" autofocus />
			<div class="dialog-actions">
				<button class="btn-secondary" onclick={() => { voteDialog = null; }}>取消</button>
				<button class="btn-primary" onclick={handleVote} disabled={submitting}>{submitting ? '处理中...' : '确认'}</button>
			</div>
		</div>
	</div>
{/if}

<style>
	.panel { padding: 16px; max-width: 420px; margin: 0 auto; }
	header { display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; }
	h2 { font-size: 18px; color: var(--accent); }
	.btn-back { color: var(--accent); font-size: 14px; cursor: pointer; background: none; border: none; }
	.tabs { display: flex; gap: 4px; margin-bottom: 16px; }
	.tabs button { flex: 1; padding: 8px; border-radius: 6px; border: 1px solid var(--border); color: var(--text-dim); font-size: 13px; background: none; cursor: pointer; }
	.tabs button.active { border-color: var(--accent); color: var(--accent); background: rgba(34,211,238,0.1); }
	.dim { color: var(--text-dim); font-size: 14px; }
	.center-text { text-align: center; padding: 24px 0; }

	.proposal-card { background: var(--bg-card); border: 1px solid var(--border); border-radius: 8px; padding: 12px; margin-bottom: 8px; }
	.proposal-header { display: flex; justify-content: space-between; font-size: 14px; margin-bottom: 4px; }
	.proposal-votes { font-size: 13px; margin-bottom: 8px; }
	.proposal-actions { display: flex; gap: 8px; }
	.btn-sm { padding: 4px 12px; border-radius: 4px; font-size: 12px; border: 1px solid; cursor: pointer; background: none; }
	.btn-sm.green { color: var(--green); border-color: var(--green); }
	.btn-sm.red { color: var(--red); border-color: var(--red); }
	.btn-sm.accent { color: var(--accent); border-color: var(--accent); }
	.btn-refresh { width: 100%; padding: 8px; margin-top: 8px; color: var(--text-dim); font-size: 13px; border: 1px solid var(--border); border-radius: 8px; background: none; cursor: pointer; }

	.form { display: flex; flex-direction: column; gap: 12px; }
	select { border: 1px solid var(--border); background: var(--bg); color: var(--text); padding: 10px; border-radius: 8px; font-size: 14px; }
	label { font-size: 12px; }
	.btn-primary { width: 100%; padding: 12px; background: var(--accent); color: var(--bg); border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.btn-secondary { flex: 1; padding: 10px; color: var(--text-dim); font-size: 14px; background: none; border: none; cursor: pointer; }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	.dialog h3 { text-align: center; font-size: 18px; }
	.dialog-actions { display: flex; gap: 8px; }
</style>
