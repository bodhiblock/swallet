<script lang="ts">
	let { title = '确认操作', onConfirm, onCancel }:
		{ title?: string; onConfirm: (password: string) => void; onCancel: () => void } = $props();

	let password = $state('');
</script>

<div class="overlay" onclick={onCancel}>
	<div class="dialog" onclick={(e) => e.stopPropagation()}>
		<h3>{title}</h3>
		<input type="password" bind:value={password} placeholder="输入密码" autofocus
			onkeydown={(e) => { if (e.key === 'Enter' && password) onConfirm(password); }} />
		<div class="dialog-actions">
			<button class="btn-secondary" onclick={onCancel}>取消</button>
			<button class="btn-primary" onclick={() => { if (password) onConfirm(password); }}>确认</button>
		</div>
	</div>
</div>

<style>
	.overlay { position: fixed; inset: 0; background: rgba(0,0,0,0.5); display: flex; align-items: center; justify-content: center; z-index: 100; }
	.dialog { background: var(--bg-card); border: 1px solid var(--border); border-radius: 12px; padding: 24px; width: 90%; max-width: 360px; display: flex; flex-direction: column; gap: 12px; }
	h3 { text-align: center; font-size: 18px; }
	.dialog-actions { display: flex; gap: 8px; }
	.dialog-actions button { flex: 1; padding: 12px; border-radius: 8px; font-size: 16px; font-weight: 600; border: none; cursor: pointer; }
	.btn-primary { background: var(--accent); color: var(--bg); }
	.btn-secondary { background: var(--bg); color: var(--text-dim); border: 1px solid var(--border) !important; }
</style>
