<script lang="ts">
	import { api } from '$lib/api';
	import type { ProposalDto, FeePayerDto } from '$lib/types';

	let { walletIndex, onClose, onToast }: { walletIndex: number; onClose: () => void; onToast: (msg: string) => void } = $props();

	let tab: 'proposals' | 'create-proposal' = $state('proposals');
	let proposals: ProposalDto[] = $state([]);
	let feePayers: FeePayerDto[] = $state([]);
	let loadingProposals = $state(false);

	// Create proposal
	type ProposalKind = 'sol-transfer' | 'program-upgrade' | 'vote-manage' | 'stake-manage';
	const proposalKinds: { key: ProposalKind; label: string }[] = [
		{ key: 'sol-transfer', label: '原生币转账' },
		{ key: 'program-upgrade', label: '升级程序' },
		{ key: 'vote-manage', label: 'Vote 账户管理' },
		{ key: 'stake-manage', label: 'Stake 账户管理' },
	];
	let proposalKind: ProposalKind = $state('sol-transfer');

	// SOL transfer fields
	let proposalTo = $state('');
	let proposalAmount = $state('');

	// Program upgrade fields
	let upgradeProgram = $state('');
	let upgradeBuffer = $state('');

	// Vote/Stake manage fields
	type VsOp = 'vote-auth-voter' | 'vote-auth-withdrawer' | 'vote-withdraw' | 'stake-auth-staker' | 'stake-auth-withdrawer' | 'stake-delegate' | 'stake-deactivate' | 'stake-withdraw';
	const voteOps: { key: VsOp; label: string }[] = [
		{ key: 'vote-auth-voter', label: '修改 Voter 权限' },
		{ key: 'vote-auth-withdrawer', label: '修改 Withdrawer 权限' },
		{ key: 'vote-withdraw', label: '提取 (Withdraw)' },
	];
	const stakeOps: { key: VsOp; label: string }[] = [
		{ key: 'stake-auth-staker', label: '修改 Staker 权限' },
		{ key: 'stake-auth-withdrawer', label: '修改 Withdrawer 权限' },
		{ key: 'stake-delegate', label: '委托 (Delegate)' },
		{ key: 'stake-deactivate', label: '取消质押 (Deactivate)' },
		{ key: 'stake-withdraw', label: '提取 (Withdraw)' },
	];
	let vsOp: VsOp = $state('vote-auth-voter');
	let vsTarget = $state('');    // vote/stake account address
	let vsParam = $state('');     // new authority / vote account / to address
	let vsAmount = $state('');    // withdraw amount

	// Fee payer & password
	let selectedFeePayer = $state(0);
	let createPassword = $state('');
	let submitting = $state(false);

	// Vote dialog
	let voteDialog: { action: 'approve' | 'reject' | 'execute'; proposal: ProposalDto } | null = $state(null);
	let votePassword = $state('');

	$effect(() => {
		loadProposals();
		loadFeePayers();
	});

	async function loadProposals() {
		loadingProposals = true;
		try { proposals = await api.fetchProposals(walletIndex); } catch (e: any) { onToast(e?.message || '加载失败'); }
		loadingProposals = false;
	}

	async function loadFeePayers() {
		try { feePayers = await api.getFeePayers(); } catch (_) {}
	}

	// 判断 vs 操作是否需要参数/金额
	function vsNeedsParam(op: VsOp): boolean {
		return op !== 'stake-deactivate';
	}
	function vsParamLabel(op: VsOp): string {
		switch (op) {
			case 'vote-auth-voter': case 'vote-auth-withdrawer':
			case 'stake-auth-staker': case 'stake-auth-withdrawer':
				return '新权限地址';
			case 'stake-delegate': return 'Vote 账户地址';
			case 'vote-withdraw': case 'stake-withdraw': return '提取到地址';
			default: return '';
		}
	}
	function vsNeedsAmount(op: VsOp): boolean {
		return op === 'vote-withdraw' || op === 'stake-withdraw';
	}

	async function handleCreateProposal() {
		if (!createPassword) { onToast('请输入密码'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }

		// Validate inputs
		if (proposalKind === 'sol-transfer' && (!proposalTo.trim() || !proposalAmount.trim())) { onToast('请填写目标地址和金额'); return; }
		if (proposalKind === 'program-upgrade' && (!upgradeProgram.trim() || !upgradeBuffer.trim())) { onToast('请填写程序和 Buffer 地址'); return; }
		if ((proposalKind === 'vote-manage' || proposalKind === 'stake-manage') && !vsTarget.trim()) { onToast('请填写目标账户地址'); return; }

		submitting = true;
		const fp = feePayers[selectedFeePayer];
		try {
			// Map proposal kind to type index (matching ProposalType::for_chain order)
			// SolTransfer=0, TokenTransfer=1, ProgramUpgrade=2, VoteManage=3, StakeManage=4
			let typeIdx: number;
			switch (proposalKind) {
				case 'sol-transfer': typeIdx = 0; break;
				case 'program-upgrade': typeIdx = 2; break;
				case 'vote-manage': typeIdx = 3; break;
				case 'stake-manage': typeIdx = 4; break;
				default: typeIdx = 0;
			}

			// Map vsOp to index
			const vsOpMap: Record<string, number> = {
				'vote-auth-voter': 0, 'vote-auth-withdrawer': 1, 'vote-withdraw': 2,
				'stake-auth-staker': 3, 'stake-auth-withdrawer': 4, 'stake-delegate': 5, 'stake-deactivate': 6, 'stake-withdraw': 7,
			};
			const vsOpIdx = vsOpMap[vsOp] ?? 0;

			const sig = await api.createProposal(
				walletIndex, 0, typeIdx,
				proposalTo, proposalAmount,
				upgradeProgram, upgradeBuffer,
				vsOpIdx, vsTarget, vsParam, vsAmount,
				'', // chain_id (auto from wallet)
				createPassword, fp.wallet_index, fp.account_index,
			);
			onToast(`提案已创建: ${sig.slice(0, 16)}...`);
			createPassword = ''; proposalTo = ''; proposalAmount = '';
			upgradeProgram = ''; upgradeBuffer = '';
			vsTarget = ''; vsParam = ''; vsAmount = '';
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
		<button class:active={tab === 'proposals'} onclick={() => tab = 'proposals'}>提案列表</button>
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

	{:else if tab === 'create-proposal'}
		<div class="form">
			<!-- 提案类型选择 -->
			<p class="dim">提案类型</p>
			<div class="type-grid">
				{#each proposalKinds as kind}
					<button class="type-btn" class:active={proposalKind === kind.key}
						onclick={() => { proposalKind = kind.key; if (kind.key === 'vote-manage') vsOp = 'vote-auth-voter'; if (kind.key === 'stake-manage') vsOp = 'stake-auth-staker'; }}>
						{kind.label}
					</button>
				{/each}
			</div>

			<!-- SOL 转账 -->
			{#if proposalKind === 'sol-transfer'}
				<input bind:value={proposalTo} placeholder="目标地址" />
				<input bind:value={proposalAmount} placeholder="金额 (SOL)" type="text" inputmode="decimal" />

			<!-- 程序升级 -->
			{:else if proposalKind === 'program-upgrade'}
				<input bind:value={upgradeProgram} placeholder="程序地址" />
				<input bind:value={upgradeBuffer} placeholder="Buffer 地址" />

			<!-- Vote 管理 -->
			{:else if proposalKind === 'vote-manage'}
				<p class="dim">操作类型</p>
				<div class="type-grid">
					{#each voteOps as op}
						<button class="type-btn small" class:active={vsOp === op.key} onclick={() => vsOp = op.key}>{op.label}</button>
					{/each}
				</div>
				<input bind:value={vsTarget} placeholder="Vote 账户地址" />
				{#if vsNeedsParam(vsOp)}<input bind:value={vsParam} placeholder={vsParamLabel(vsOp)} />{/if}
				{#if vsNeedsAmount(vsOp)}<input bind:value={vsAmount} placeholder="金额 (SOL)" type="text" inputmode="decimal" />{/if}

			<!-- Stake 管理 -->
			{:else if proposalKind === 'stake-manage'}
				<p class="dim">操作类型</p>
				<div class="type-grid">
					{#each stakeOps as op}
						<button class="type-btn small" class:active={vsOp === op.key} onclick={() => vsOp = op.key}>{op.label}</button>
					{/each}
				</div>
				<input bind:value={vsTarget} placeholder="Stake 账户地址" />
				{#if vsNeedsParam(vsOp)}<input bind:value={vsParam} placeholder={vsParamLabel(vsOp)} />{/if}
				{#if vsNeedsAmount(vsOp)}<input bind:value={vsAmount} placeholder="金额 (SOL)" type="text" inputmode="decimal" />{/if}
			{/if}

			<!-- Fee Payer -->
			{#if feePayers.length > 0}
				<label class="dim">Fee Payer</label>
				<select bind:value={selectedFeePayer}>
					{#each feePayers as fp, i}<option value={i}>{fp.address.slice(0,8)}... ({fp.balance})</option>{/each}
				</select>
			{/if}

			<input type="password" bind:value={createPassword} placeholder="密码确认" />
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
	h3 { text-align: center; font-size: 18px; }
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
	.type-grid { display: flex; flex-wrap: wrap; gap: 6px; }
	.type-btn { padding: 8px 12px; border: 1px solid var(--border); border-radius: 6px; color: var(--text-dim); font-size: 13px; background: none; cursor: pointer; }
	.type-btn.small { padding: 6px 10px; font-size: 12px; }
	.type-btn.active { border-color: var(--accent); color: var(--accent); background: rgba(34,211,238,0.1); }
	select { border: 1px solid var(--border); background: var(--bg); color: var(--text); padding: 10px; border-radius: 8px; font-size: 14px; width: 100%; }
	label { font-size: 12px; }
	.btn-primary { width: 100%; padding: 12px; background: var(--accent); color: var(--bg); border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.btn-secondary { flex: 1; padding: 10px; color: var(--text-dim); font-size: 14px; background: none; border: none; cursor: pointer; }

	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	.dialog-actions { display: flex; gap: 8px; }
</style>
