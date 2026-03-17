<script lang="ts">
	let { onUnlock }: { onUnlock: () => void } = $props();

	let display = $state('0');
	let prevValue = $state<number | null>(null);
	let operator = $state<string | null>(null);
	let waitingForOperand = $state(false);
	let keySequence = $state('');

	function checkUnlock(digit: string) {
		keySequence += digit;
		if (keySequence.length > 10) keySequence = keySequence.slice(-10);
		if (keySequence.includes('1024')) {
			keySequence = '';
			onUnlock();
		}
	}

	function inputDigit(d: string) {
		checkUnlock(d);
		if (waitingForOperand) {
			display = d;
			waitingForOperand = false;
		} else {
			display = display === '0' ? d : display + d;
		}
	}

	function inputDot() {
		if (waitingForOperand) {
			display = '0.';
			waitingForOperand = false;
			return;
		}
		if (!display.includes('.')) display += '.';
	}

	function clear() {
		display = '0';
		prevValue = null;
		operator = null;
		waitingForOperand = false;
	}

	function toggleSign() {
		const v = parseFloat(display);
		display = String(-v);
	}

	function percent() {
		display = String(parseFloat(display) / 100);
	}

	function performOp(op: string) {
		const current = parseFloat(display);
		if (prevValue !== null && operator && !waitingForOperand) {
			let result: number;
			switch (operator) {
				case '+': result = prevValue + current; break;
				case '-': result = prevValue - current; break;
				case '×': result = prevValue * current; break;
				case '÷': result = current !== 0 ? prevValue / current : NaN; break;
				case 'xʸ': result = Math.pow(prevValue, current); break;
				default: result = current;
			}
			display = isNaN(result) || !isFinite(result) ? 'Error' : formatNum(result);
			prevValue = isNaN(result) || !isFinite(result) ? null : result;
		} else {
			prevValue = current;
		}
		operator = op;
		waitingForOperand = true;
	}

	function equals() {
		if (prevValue === null || !operator) return;
		performOp(operator);
		operator = null;
		prevValue = null;
	}

	function sciFunc(fn: string) {
		const v = parseFloat(display);
		let result: number;
		switch (fn) {
			case 'sin': result = Math.sin(v * Math.PI / 180); break;
			case 'cos': result = Math.cos(v * Math.PI / 180); break;
			case 'tan': result = Math.tan(v * Math.PI / 180); break;
			case 'ln': result = Math.log(v); break;
			case 'log': result = Math.log10(v); break;
			case '√': result = Math.sqrt(v); break;
			case 'x²': result = v * v; break;
			case '1/x': result = 1 / v; break;
			case 'x!': result = factorial(v); break;
			default: result = v;
		}
		display = isNaN(result) || !isFinite(result) ? 'Error' : formatNum(result);
	}

	function insertConst(c: string) {
		if (c === 'π') display = formatNum(Math.PI);
		else if (c === 'e') display = formatNum(Math.E);
		waitingForOperand = false;
	}

	function factorial(n: number): number {
		if (n < 0 || n % 1 !== 0) return NaN;
		if (n > 170) return Infinity;
		let r = 1;
		for (let i = 2; i <= n; i++) r *= i;
		return r;
	}

	function formatNum(n: number): string {
		if (Number.isInteger(n) && Math.abs(n) < 1e15) return String(n);
		const s = n.toPrecision(10);
		return parseFloat(s).toString();
	}
</script>

<div class="calc">
	<div class="display">
		<div class="display-text">{display}</div>
	</div>

	<div class="sci-row">
		<button class="sci" onclick={() => sciFunc('sin')}>sin</button>
		<button class="sci" onclick={() => sciFunc('cos')}>cos</button>
		<button class="sci" onclick={() => sciFunc('tan')}>tan</button>
		<button class="sci" onclick={() => sciFunc('ln')}>ln</button>
		<button class="sci" onclick={() => sciFunc('log')}>log</button>
	</div>
	<div class="sci-row">
		<button class="sci" onclick={() => sciFunc('√')}>√</button>
		<button class="sci" onclick={() => sciFunc('x²')}>x²</button>
		<button class="sci" onclick={() => performOp('xʸ')}>xʸ</button>
		<button class="sci" onclick={() => sciFunc('x!')}>x!</button>
		<button class="sci" onclick={() => sciFunc('1/x')}>1/x</button>
	</div>
	<div class="sci-row">
		<button class="sci" onclick={() => insertConst('π')}>π</button>
		<button class="sci" onclick={() => insertConst('e')}>e</button>
		<button class="sci" onclick={() => { display = String(Math.random()); waitingForOperand = false; }}>Rand</button>
		<button class="sci" onclick={() => { display = 'Error'; }}>EE</button>
		<button class="sci" onclick={() => percent()}>%</button>
	</div>

	<div class="grid">
		<button class="fn" onclick={clear}>AC</button>
		<button class="fn" onclick={toggleSign}>±</button>
		<button class="fn" onclick={percent}>%</button>
		<button class="op" onclick={() => performOp('÷')}>÷</button>

		<button class="num" onclick={() => inputDigit('7')}>7</button>
		<button class="num" onclick={() => inputDigit('8')}>8</button>
		<button class="num" onclick={() => inputDigit('9')}>9</button>
		<button class="op" onclick={() => performOp('×')}>×</button>

		<button class="num" onclick={() => inputDigit('4')}>4</button>
		<button class="num" onclick={() => inputDigit('5')}>5</button>
		<button class="num" onclick={() => inputDigit('6')}>6</button>
		<button class="op" onclick={() => performOp('-')}>−</button>

		<button class="num" onclick={() => inputDigit('1')}>1</button>
		<button class="num" onclick={() => inputDigit('2')}>2</button>
		<button class="num" onclick={() => inputDigit('3')}>3</button>
		<button class="op" onclick={() => performOp('+')}>+</button>

		<button class="num zero" onclick={() => inputDigit('0')}>0</button>
		<button class="num" onclick={inputDot}>.</button>
		<button class="op eq" onclick={equals}>=</button>
	</div>
</div>

<style>
	.calc {
		max-width: 420px;
		margin: 0 auto;
		padding: 12px;
		height: 100vh;
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 6px;
	}
	.display {
		background: var(--bg-card);
		border-radius: 12px;
		padding: 16px;
		text-align: right;
		margin-bottom: 4px;
		min-height: 70px;
		display: flex;
		align-items: flex-end;
		justify-content: flex-end;
	}
	.display-text {
		font-size: 38px;
		font-weight: 300;
		font-family: -apple-system, BlinkMacSystemFont, 'SF Pro Display', sans-serif;
		color: var(--text);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 100%;
	}
	.sci-row {
		display: grid;
		grid-template-columns: repeat(5, 1fr);
		gap: 5px;
	}
	.sci {
		background: var(--bg-card);
		color: var(--text-dim);
		border-radius: 8px;
		padding: 8px 0;
		font-size: 13px;
		font-weight: 500;
	}
	.sci:active {
		background: var(--bg-hover);
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(4, 1fr);
		gap: 8px;
	}
	.grid button {
		height: 56px;
		border-radius: 16px;
		font-size: 20px;
		font-weight: 400;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.num {
		background: #2a2d36;
		color: var(--text);
	}
	.num:active {
		background: #3a3d46;
	}
	.fn {
		background: #505258;
		color: var(--text);
	}
	.fn:active {
		background: #606268;
	}
	.op {
		background: var(--accent);
		color: var(--bg);
		font-weight: 600;
	}
	.op:active {
		background: #19a8bd;
	}
	.eq {
		background: var(--green);
	}
	.eq:active {
		background: #1ba34e;
	}
	.zero {
		grid-column: span 2;
	}
</style>
