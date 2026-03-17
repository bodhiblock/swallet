<script lang="ts">
	import { api } from '$lib/api';
	import type { ProposalDto, FeePayerDto, PresetProgramDto, BalanceDto, VoteAccountDto, StakeAccountDto } from '$lib/types';

	import type { WalletDto, AccountDto } from '$lib/types';
	let { walletIndex, balances, wallets, onClose, onToast }: { walletIndex: number; balances: BalanceDto[]; wallets: WalletDto[]; onClose: () => void; onToast: (msg: string) => void } = $props();

	let msChainName = $derived(wallets[walletIndex]?.chain_name || 'SOL');
	let tab: 'proposals' | 'create-proposal' = $state('proposals');
	let proposals: ProposalDto[] = $state([]);
	let feePayers: FeePayerDto[] = $state([]);
	let loadingProposals = $state(false);

	// Proposal types
	type ProposalKind = 'sol-transfer' | 'program-upgrade' | 'program-call' | 'vote-manage' | 'stake-manage';
	const proposalKinds: { key: ProposalKind; label: string }[] = [
		{ key: 'sol-transfer', label: '原生币转账' },
		{ key: 'program-upgrade', label: '升级程序' },
		{ key: 'vote-manage', label: 'Vote 管理' },
		{ key: 'stake-manage', label: 'Stake 管理' },
		{ key: 'program-call', label: '调用程序' },
	];
	let proposalKind: ProposalKind = $state('sol-transfer');

	// Fields
	let proposalTo = $state('');
	let proposalAmount = $state('');
	let upgradeProgram = $state('');
	let upgradeBuffer = $state('');

	// Program call
	let presetPrograms: PresetProgramDto[] = $state([]);
	let selectedProgram = $state(0);
	let selectedInstruction = $state(0);
	let presetArgValues: string[] = $state([]);

	// Vote/Stake ops
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
	let vsTarget = $state('');
	let vsManualAddress = $state('');
	let vsParam = $state('');
	let vsAmount = $state('');

	// Vote/Stake account selection
	let vsAccounts: { address: string; type: 'vote' | 'stake'; label: string }[] = $state([]);
	let vsSelectedAccount = $state(-1);
	let vaultAddress = $state('');
	let msRpcUrl = $state('');
	let vsVerified = $state(false);
	let vsVerifyError = $state('');

	// 从主页钱包+余额数据获取所有 vote/stake 地址
	function getAccountOwner(address: string): string | null {
		return balances.find(b => b.address === address)?.account_owner || null;
	}
	let allVoteStakeAccounts = $derived.by(() => {
		const vote: string[] = [];
		const stake: string[] = [];
		for (const w of wallets) {
			for (const acc of w.accounts) {
				if (acc.hidden) continue;
				const owner = getAccountOwner(acc.address);
				if (owner === 'Vote111111111111111111111111111111111111111') vote.push(acc.address);
				else if (owner === 'Stake11111111111111111111111111111111111111') stake.push(acc.address);
			}
		}
		return { vote, stake };
	});

	// Fee payer
	let selectedFeePayer = $state(0);
	let submitting = $state(false);

	// Local addresses for vote check
	let localAddresses: string[] = $state([]);

	// Password dialog
	let passwordDialog: 'create' | 'vote' | null = $state(null);
	let dialogPassword = $state('');
	let voteDialog: { action: 'approve' | 'reject' | 'execute'; proposal: ProposalDto } | null = $state(null);

	$effect(() => { loadProposals(); loadFeePayers(); loadLocalAddresses(); });

	async function loadLocalAddresses() {
		try { localAddresses = await api.getLocalSolAddresses(); } catch (_) {}
	}

	function hasVoted(proposal: ProposalDto): boolean {
		return localAddresses.some(a => proposal.approved_addresses.includes(a) || proposal.rejected_addresses.includes(a));
	}

	async function loadProposals() {
		loadingProposals = true;
		try { proposals = await api.fetchProposals(walletIndex); } catch (e: any) { onToast(e?.message || '加载失败'); }
		loadingProposals = false;
	}
	async function loadFeePayers() { try { feePayers = await api.getFeePayers(); } catch (_) {} }

	async function loadPresets() {
		try {
			presetPrograms = await api.getPresetPrograms('nara-mainnet');
			selectedProgram = 0;
			selectedInstruction = 0;
			updateArgValues();
		} catch (_) { presetPrograms = []; }
	}

	function updateArgValues() {
		const ix = presetPrograms[selectedProgram]?.instructions[selectedInstruction];
		presetArgValues = ix ? ix.args.map(() => '') : [];
	}

	function vsNeedsParam(op: VsOp): boolean { return op !== 'stake-deactivate'; }
	function vsParamLabel(op: VsOp): string {
		switch (op) {
			case 'vote-auth-voter': case 'vote-auth-withdrawer':
			case 'stake-auth-staker': case 'stake-auth-withdrawer': return '新权限地址';
			case 'stake-delegate': return 'Vote 账户地址';
			case 'vote-withdraw': case 'stake-withdraw': return '提取到地址';
			default: return '';
		}
	}
	function vsNeedsAmount(op: VsOp): boolean { return op === 'vote-withdraw' || op === 'stake-withdraw'; }

	async function loadVsAccounts(type: 'vote' | 'stake') {
		vsVerified = false;
		vsVerifyError = '';
		vsManualAddress = '';
		try {
			vaultAddress = await api.getMultisigVaultAddress(walletIndex);
			msRpcUrl = await api.getMultisigRpcUrl(walletIndex);
		} catch (e: any) { onToast(e?.message || '加载失败'); }
	}

	async function verifyVsAuthority(): Promise<boolean> {
		const addr = vsManualAddress.trim();
		if (!addr) { onToast('请先选择或输入账户地址'); return false; }
		vsTarget = addr;
		try {
			// 确保 vaultAddress 已加载
			if (!vaultAddress) {
				vaultAddress = await api.getMultisigVaultAddress(walletIndex);
			}
			if (!vaultAddress) { onToast('无法获取 Vault 地址'); return false; }
			const rpcUrl = msRpcUrl || await api.getRpcUrlForAddress(addr);
			if (proposalKind === 'vote-manage') {
				const info: VoteAccountDto = await api.fetchVoteAccount(addr, rpcUrl);
				// vote-auth-voter 需要 voter 权限，其他(vote-auth-withdrawer, vote-withdraw) 需要 withdrawer 权限
				const needVoter = vsOp === 'vote-auth-voter';
				const authority = needVoter ? info.authorized_voter : info.authorized_withdrawer;
				const label = needVoter ? 'Voter' : 'Withdrawer';
				if (authority === vaultAddress) return true;
				onToast(`${label} 权限不匹配\nVault: ${vaultAddress}\n${label}: ${authority}`);
			} else {
				const info: StakeAccountDto = await api.fetchStakeAccount(addr, rpcUrl);
				// stake-auth-withdrawer, stake-withdraw 需要 withdrawer 权限，其他需要 staker 权限
				const needWithdrawer = vsOp === 'stake-auth-withdrawer' || vsOp === 'stake-withdraw';
				const authority = needWithdrawer ? info.authorized_withdrawer : info.authorized_staker;
				const label = needWithdrawer ? 'Withdrawer' : 'Staker';
				if (authority === vaultAddress) return true;
				onToast(`${label} 权限不匹配\nVault: ${vaultAddress}\n${label}: ${authority}`);
			}
		} catch (e: any) { onToast(e?.message || '验证失败'); }
		return false;
	}

	async function startCreateProposal() {
		if (proposalKind === 'sol-transfer' && (!proposalTo.trim() || !proposalAmount.trim())) { onToast('请填写目标地址和金额'); return; }
		if (proposalKind === 'program-upgrade' && (!upgradeProgram.trim() || !upgradeBuffer.trim())) { onToast('请填写程序和 Buffer 地址'); return; }
		if ((proposalKind === 'vote-manage' || proposalKind === 'stake-manage') && !vsManualAddress.trim()) { onToast('请先选择或输入账户地址'); return; }
		if (proposalKind === 'program-call' && presetPrograms.length === 0) { onToast('没有可用的预制程序'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }
		// Vote/Stake 提交前自动验证权限
		if (proposalKind === 'vote-manage' || proposalKind === 'stake-manage') {
			submitting = true;
			const ok = await verifyVsAuthority();
			submitting = false;
			if (!ok) return;
		}
		dialogPassword = '';
		passwordDialog = 'create';
	}

	async function confirmCreateProposal() {
		if (!dialogPassword) { onToast('请输入密码'); return; }
		submitting = true;
		const fp = feePayers[selectedFeePayer];
		const typeMap: Record<ProposalKind, number> = { 'sol-transfer': 0, 'program-upgrade': 2, 'vote-manage': 3, 'stake-manage': 4, 'program-call': 5 };
		const vsOpMap: Record<string, number> = {
			'vote-auth-voter': 0, 'vote-auth-withdrawer': 1, 'vote-withdraw': 2,
			'stake-auth-staker': 3, 'stake-auth-withdrawer': 4, 'stake-delegate': 5, 'stake-deactivate': 6, 'stake-withdraw': 7,
		};
		try {
			const sig = await api.createProposal({
				walletIndex, vaultIndex: 0, proposalTypeIdx: typeMap[proposalKind],
				toAddress: proposalTo, amount: proposalAmount,
				upgradeProgram, upgradeBuffer,
				presetProgramIdx: selectedProgram, presetInstructionIdx: selectedInstruction, presetArgs: presetArgValues,
				vsOpIdx: vsOpMap[vsOp] ?? 0, vsTarget, vsParam, vsAmount,
				chainId: '', password: dialogPassword, feePayerWi: fp.wallet_index, feePayerAi: fp.account_index,
			});
			onToast(`提案已创建: ${sig.slice(0, 16)}...`);
			passwordDialog = null;
			proposalTo = ''; proposalAmount = ''; upgradeProgram = ''; upgradeBuffer = ''; vsTarget = ''; vsParam = ''; vsAmount = '';
			vsVerified = false; vsSelectedAccount = -1;
			tab = 'proposals';
			await loadProposals();
		} catch (e: any) { onToast(e?.message || '创建失败'); }
		submitting = false;
	}

	function startVote(action: 'approve' | 'reject' | 'execute', proposal: ProposalDto) {
		voteDialog = { action, proposal };
		dialogPassword = '';
		passwordDialog = 'vote';
	}

	async function confirmVote() {
		if (!voteDialog || !dialogPassword) { onToast('请输入密码'); return; }
		if (feePayers.length === 0) { onToast('没有可用的 Fee Payer'); return; }
		submitting = true;
		const fp = feePayers[selectedFeePayer];
		const p = voteDialog.proposal;
		try {
			let sig: string;
			if (voteDialog.action === 'approve') sig = await api.approveProposal(walletIndex, p.transaction_index, dialogPassword, fp.wallet_index, fp.account_index);
			else if (voteDialog.action === 'reject') sig = await api.rejectProposal(walletIndex, p.transaction_index, dialogPassword, fp.wallet_index, fp.account_index);
			else sig = await api.executeProposal(walletIndex, p.transaction_index, 0, dialogPassword, fp.wallet_index, fp.account_index);
			onToast(`操作成功: ${sig.slice(0, 16)}...`);
			passwordDialog = null; voteDialog = null;
			await loadProposals();
		} catch (e: any) { onToast(e?.message || '操作失败'); }
		submitting = false;
	}

	function statusColor(s: string): string {
		switch (s) { case '投票中': return 'var(--yellow)'; case '已通过': return 'var(--green)'; case '已执行': return 'var(--accent)'; case '已拒绝': case '已取消': return 'var(--red)'; default: return 'var(--text-dim)'; }
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
		<button class:active={tab === 'create-proposal'} onclick={() => { tab = 'create-proposal'; loadPresets(); }}>创建提案</button>
	</div>

	{#if tab === 'proposals'}
		{#if loadingProposals}<p class="dim center-text">加载中...</p>
		{:else if proposals.length === 0}<p class="dim center-text">暂无提案</p>
		{:else}
			{#each proposals as proposal}
				<div class="proposal-card">
					<div class="proposal-header">
						<span>TX #{proposal.transaction_index}</span>
						<span style="color:{statusColor(proposal.status)}">{proposal.status}</span>
					</div>
					{#if proposal.summary}
						<div class="proposal-summary">
							{#each proposal.summary.replaceAll(' SOL', ' ' + msChainName).replace('SOL 转账', msChainName + ' 转账').split('\n') as line}
								<div>{line}</div>
							{/each}
						</div>
					{/if}
					<div class="proposal-votes"><span class="dim">通过: {proposal.approved_count} · 拒绝: {proposal.rejected_count}</span></div>
					{#if proposal.approved_addresses.length > 0}
						<div class="proposal-detail">
							<span class="dim">赞成:</span>
							{#each proposal.approved_addresses as addr}
								<div class="proposal-addr">{addr}</div>
							{/each}
						</div>
					{/if}
					{#if proposal.rejected_addresses.length > 0}
						<div class="proposal-detail">
							<span class="dim">反对:</span>
							{#each proposal.rejected_addresses as addr}
								<div class="proposal-addr">{addr}</div>
							{/each}
						</div>
					{/if}
					<div class="proposal-actions">
						{#if proposal.status === '投票中'}
							{#if hasVoted(proposal)}
								<span class="dim">已投票</span>
							{:else}
								<button class="btn-sm green" onclick={() => startVote('approve', proposal)}>审批</button>
								<button class="btn-sm red" onclick={() => startVote('reject', proposal)}>拒绝</button>
							{/if}
						{:else if proposal.status === '已通过'}
							<button class="btn-sm accent" onclick={() => startVote('execute', proposal)}>执行</button>
						{/if}
					</div>
				</div>
			{/each}
		{/if}
		<button class="btn-refresh" onclick={loadProposals}>刷新提案列表</button>

	{:else if tab === 'create-proposal'}
		<div class="form">
			<p class="dim">提案类型</p>
			<div class="type-grid">
				{#each proposalKinds as kind}
					<button class="type-btn" class:active={proposalKind === kind.key}
						onclick={() => {
							proposalKind = kind.key;
							if (kind.key === 'vote-manage') { vsOp = 'vote-auth-voter'; loadVsAccounts('vote'); }
							if (kind.key === 'stake-manage') { vsOp = 'stake-auth-staker'; loadVsAccounts('stake'); }
						}}>
						{kind.label}
					</button>
				{/each}
			</div>

			{#if proposalKind === 'sol-transfer'}
				<input bind:value={proposalTo} placeholder="目标地址" />
				<input bind:value={proposalAmount} placeholder="金额 ({msChainName})" type="text" inputmode="decimal" />

			{:else if proposalKind === 'program-upgrade'}
				<input bind:value={upgradeProgram} placeholder="程序地址" />
				<input bind:value={upgradeBuffer} placeholder="Buffer 地址" />

			{:else if proposalKind === 'program-call'}
				{#if presetPrograms.length === 0}
					<p class="dim">当前链没有可用的预制程序</p>
				{:else}
					<p class="dim">选择程序</p>
					<select bind:value={selectedProgram} onchange={() => { selectedInstruction = 0; updateArgValues(); }}>
						{#each presetPrograms as prog, i}<option value={i}>{prog.name}</option>{/each}
					</select>
					<p class="dim">选择指令</p>
					<select bind:value={selectedInstruction} onchange={updateArgValues}>
						{#each (presetPrograms[selectedProgram]?.instructions ?? []) as ix, i}
							<option value={i}>{ix.label} ({ix.name})</option>
						{/each}
					</select>
					{#each (presetPrograms[selectedProgram]?.instructions[selectedInstruction]?.args ?? []) as arg, i}
						<input bind:value={presetArgValues[i]} placeholder="{arg.label} ({arg.name})" />
					{/each}
				{/if}

			{:else if proposalKind === 'vote-manage' || proposalKind === 'stake-manage'}
				{@const currentVsAddrs = proposalKind === 'vote-manage' ? allVoteStakeAccounts.vote : allVoteStakeAccounts.stake}
				<p class="dim">选择 {proposalKind === 'vote-manage' ? 'Vote' : 'Stake'} 账户</p>
				<select bind:value={vsManualAddress} onchange={() => { vsSelectedAccount = -1; }}>
					<option value="">-- 选择账户 ({currentVsAddrs.length}) --</option>
					{#each currentVsAddrs as addr}
						<option value={addr}>{addr}</option>
					{/each}
				</select>
				<input bind:value={vsManualAddress} placeholder="或手动输入地址" />

				{#if vsManualAddress.trim()}
					<p class="dim">操作类型</p>
					<div class="type-grid">
						{#if proposalKind === 'vote-manage'}
							{#each voteOps as op}<button class="type-btn small" class:active={vsOp === op.key} onclick={() => vsOp = op.key}>{op.label}</button>{/each}
						{:else}
							{#each stakeOps as op}<button class="type-btn small" class:active={vsOp === op.key} onclick={() => vsOp = op.key}>{op.label}</button>{/each}
						{/if}
					</div>
					{#if vsNeedsParam(vsOp)}<input bind:value={vsParam} placeholder={vsParamLabel(vsOp)} />{/if}
					{#if vsNeedsAmount(vsOp)}<input bind:value={vsAmount} placeholder="金额 ({msChainName})" type="text" inputmode="decimal" />{/if}
				{/if}
			{/if}

			<button class="btn-primary" onclick={startCreateProposal} disabled={submitting}>{submitting ? '验证中...' : '创建提案'}</button>
		</div>
	{/if}
</div>

<!-- Password Dialog (create proposal or vote) -->
{#if passwordDialog}
	<div class="pw-overlay" onclick={() => { passwordDialog = null; }}>
		<div class="pw-dialog" onclick={(e) => e.stopPropagation()}>
			<h3>{passwordDialog === 'create' ? '确认创建提案' : voteDialog?.action === 'approve' ? '确认审批' : voteDialog?.action === 'reject' ? '确认拒绝' : '确认执行'}</h3>
			{#if passwordDialog === 'vote' && voteDialog}
				<p class="dim">TX #{voteDialog.proposal.transaction_index}</p>
			{/if}
			{#if feePayers.length > 0}
				<label class="dim">Fee Payer</label>
				<select bind:value={selectedFeePayer}>
					{#each feePayers as fp, i}<option value={i}>{fp.address.slice(0,8)}... ({fp.balance})</option>{/each}
				</select>
			{/if}
			<input type="password" bind:value={dialogPassword} placeholder="输入密码" autofocus
				onkeydown={(e) => e.key === 'Enter' && (passwordDialog === 'create' ? confirmCreateProposal() : confirmVote())} />
			<div class="pw-actions">
				<button class="pw-btn cancel" onclick={() => { passwordDialog = null; }}>取消</button>
				<button class="pw-btn confirm" onclick={() => passwordDialog === 'create' ? confirmCreateProposal() : confirmVote()} disabled={submitting}>{submitting ? '处理中...' : '确认'}</button>
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
	.proposal-summary { font-size: 12px; color: var(--accent); margin-bottom: 4px; }
	.proposal-summary div { word-break: break-all; }
	.proposal-summary div + div { color: var(--text-dim); font-family: monospace; font-size: 11px; margin-top: 2px; }
	.proposal-votes { font-size: 13px; margin-bottom: 4px; }
	.proposal-detail { font-size: 12px; margin-bottom: 4px; }
	.proposal-addr { font-family: monospace; font-size: 11px; color: var(--text-dim); word-break: break-all; margin-left: 8px; }
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

	/* Vote/Stake account selection */
	.vs-account-list { display: flex; flex-direction: column; gap: 4px; }
	.vs-account-item { width: 100%; text-align: left; padding: 10px 12px; border: 1px solid var(--border); border-radius: 8px; background: none; color: var(--text); font-size: 12px; font-family: monospace; cursor: pointer; word-break: break-all; }
	.vs-account-item.active { border-color: var(--accent); background: rgba(34,211,238,0.08); }
	.vs-addr { word-break: break-all; }

	.pw-overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 100; }
	.pw-dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	.pw-dialog h3 { text-align: center; font-size: 18px; }
	.pw-actions { display: flex; gap: 8px; }
	.pw-btn { flex: 1; padding: 12px; border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.pw-btn.confirm { background: var(--accent); color: var(--bg); }
	.pw-btn.cancel { background: var(--bg); color: var(--text-dim); border: 1px solid var(--border); }
</style>
