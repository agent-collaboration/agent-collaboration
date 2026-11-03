/** Controlled stream/fact producers qualify the apparatus; no quota/login product or real PTY runs. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test, vi } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import type { InputConsumptionExpectation } from '../sessions/input-consumption-schema.ts';
import type { ChildLoginHandoff } from './loop-ending-handoff.ts';
import { MarkerInputSession } from './marker-input-session.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { gridLines } from './terminal-grid.ts';

const login: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'Return' }, phase: 'press', modifiers: [] },
	surface: { kind: 'overlay', overlayId: '1' },
	target: 'action:quota:login',
	action: 'quota:login',
};
const barrier: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'F12' }, phase: 'press', modifiers: [] },
	surface: { kind: 'screen' },
	target: null,
	action: null,
};
const quit: InputConsumptionExpectation = {
	event: {
		kind: 'key',
		code: { kind: 'shortcut', key: 'c' },
		phase: 'press',
		modifiers: ['control'],
	},
	surface: { kind: 'screen' },
	target: null,
	action: 'quota:quit',
};
const argv = [
	'account',
	'login',
	'--provider',
	'openai',
	'--label',
	'effect-alpha',
	'--router-root',
	'/private/tmp/owned-fake-root',
];
type ControlMode =
	| 'right'
	| 'missing-remount'
	| 'never-restored'
	| 'stale-remount'
	| 'planned-exit-only'
	| 'wrong-exit';
async function controlledHandoff(
	mode: ControlMode,
	exitCode: 0 | 130 | 7,
): Promise<{
	driver: PtyGridDriver;
	input: MarkerInputSession;
	handoff: ChildLoginHandoff;
	directory: string;
	phases: string[];
}> {
	const directory = temporaryRoot(),
		receipt = join(directory, 'input-dispatch.jsonl');
	const executable = controlledExecutable(
		directory,
		'handoff-control',
		`
const fs=require('node:fs'),readline=require('node:readline');let sequence=0;
const root=${JSON.stringify(directory)},receipt=${JSON.stringify(receipt)},mode=${JSON.stringify(mode)},code=${exitCode};fs.writeFileSync(receipt,'');const commands=[];fs.writeFileSync(root+'/received-commands.json','[]');
const emit=value=>process.stdout.write('TUI_GRID '+value+'\\n');
const frame=value=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+value+'\\x1b[?2026l';
const bytes=value=>emit('BYTES '+Buffer.from(value).toString('hex'));
emit('READY 120 36 STREAM');bytes(frame('old quota mount'));
readline.createInterface({input:process.stdin}).on('line',line=>{
 if(line==='stop'){emit('EOF');process.stdout.write('TUI_GRID STOPPED 0\\n',()=>process.exit(0));return;}
 if(line.startsWith('reply ')){emit('OK');return;}
 if(!line.startsWith('write '))throw new Error('unexpected command '+line);
 commands.push(line);fs.writeFileSync(root+'/received-commands.json',JSON.stringify(commands));
 sequence++;const expected=sequence===1?${JSON.stringify(login)}:sequence===2?${JSON.stringify(barrier)}:${JSON.stringify(quit)};
 fs.appendFileSync(receipt,JSON.stringify({sequence,...expected})+'\\n');
 if(sequence===1){
  fs.writeFileSync(root+'/entry.json',JSON.stringify({arguments:${JSON.stringify(argv)},terminal:{kind:'interactive',canonical:mode!=='never-restored',echo:true,signals:true,foreground:true},exitCode:code}));
  fs.writeFileSync(root+'/exit.json',JSON.stringify(mode==='planned-exit-only'?{exitCode:code}:{kind:'exited',code:mode==='wrong-exit'?7:code}));
  fs.writeFileSync(root+'/mount.json',JSON.stringify(mode==='missing-remount'?null:{mount:mode==='stale-remount'?1:2}));
  if(mode!=='missing-remount')bytes(frame('fresh quota '+code));
  emit('OK');return; // loop-ending action deliberately has NO continuing marker/EOF.
 }
 emit('OK');
 if(sequence===3){emit('EOF');process.stdout.write('TUI_GRID FINISHED 0\\n',()=>process.exit(0));return;}
 bytes(frame('fresh quota '+code)+'\\x1b]7770;dispatch='+sequence+'\\x1b\\\\'+frame('unrelated later output'));
});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'quota-effects',
		keys: 'legacy',
		evidence: directory,
	});
	let input: MarkerInputSession;
	try {
		input = new MarkerInputSession({ driver, receipt, profile: 'terminal-only' });
	} catch (error) {
		await driver.stop();
		throw error;
	}
	const phases: string[] = [];
	const fact = (name: string): unknown =>
		JSON.parse(readFileSync(join(directory, name + '.json'), 'utf8'));
	return {
		driver,
		input,
		directory,
		phases,
		handoff: {
			kind: 'child-login',
			previousMount: 1,
			expectedChildArguments: argv,
			expectedChildExitCode: exitCode,
			childEntered: async () => {
				phases.push('child-entered');
				return fact('entry');
			},
			childExited: async () => {
				phases.push('child-exited');
				return fact('exit');
			},
			remounted: async () => {
				phases.push('remounted');
				return fact('mount');
			},
			ready: (grid) => gridLines(grid)[0]?.trim() === 'fresh quota ' + exitCode,
			firstFrame: { key: 'F12', expected: [barrier] },
		},
	};
}
test.each([0, 130, 7] as const)(
	'loop-ending handoff retains exact ACK, child outcome %i and its fresh first frame on one owner',
	async (exitCode) => {
		const { driver, input, handoff, directory, phases } = await controlledHandoff(
			'right',
			exitCode,
		);
		const pid = driver.processId();
		try {
			const proof = await input.handoff({
				...handoff,
				expected: [login],
				inject: () => driver.send(Buffer.from('\r')),
			});
			if (proof.kind !== 'child-login') throw new Error('login returned a different lifecycle');
			expect(proof.driverPid).toBe(pid);
			expect(proof.receipts.map((receipt) => receipt.sequence)).toEqual([1]);
			expect(proof.childExit).toEqual({ kind: 'exited', code: exitCode });
			expect(proof.remount.mount).toBe(2);
			expect(proof.firstFrame.frames.map((frame) => frame.binding.sequence)).toEqual([2]);
			expect(gridLines(proof.firstFrame.frames[0]?.grid ?? driver.grid())[0]?.trim()).toBe(
				'fresh quota ' + exitCode,
			);
			expect(gridLines(driver.grid())[0]?.trim()).toBe('unrelated later output');
			expect(phases).toEqual(['child-entered', 'child-exited', 'remounted']);
			expect(JSON.parse(readFileSync(join(directory, 'received-commands.json'), 'utf8'))).toEqual([
				'write 0d',
				'write 1b5b32347e',
			]);
			await input.exit({ expected: [quit], inject: () => driver.send(Buffer.from([3])) });
			expect(input.assertComplete()).toHaveLength(3);
		} finally {
			await driver.stop();
		}
	},
);
test.each([
	{ mode: 'missing-remount', message: 'no fresh quota remount' },
	{ mode: 'never-restored', message: 'without terminal restoration' },
	{ mode: 'stale-remount', message: 'no fresh quota remount' },
	{ mode: 'wrong-exit', message: 'parent-observed child exit differs' },
] satisfies readonly { mode: ControlMode; message: string }[])(
	'handoff rejects $mode without sending a first-frame input',
	async ({ mode, message }) => {
		const { driver, input, handoff, directory } = await controlledHandoff(mode, 0);
		try {
			await expect(
				input.handoff({
					...handoff,
					expected: [login],
					inject: () => driver.send(Buffer.from('\r')),
				}),
			).rejects.toThrow(message);
			expect(JSON.parse(readFileSync(join(directory, 'received-commands.json'), 'utf8'))).toEqual([
				'write 0d',
			]);
			expect(input.assertComplete()).toHaveLength(1);
		} finally {
			await driver.stop();
		}
	},
);
test('an intended exit code from the child cannot masquerade as the parent wait result', async () => {
	const { driver, input, handoff, directory } = await controlledHandoff('planned-exit-only', 0);
	try {
		await expect(
			input.handoff({
				...handoff,
				expected: [login],
				inject: () => driver.send(Buffer.from('\r')),
			}),
		).rejects.toThrow();
		expect(JSON.parse(readFileSync(join(directory, 'received-commands.json'), 'utf8'))).toEqual([
			'write 0d',
		]);
	} finally {
		await driver.stop();
	}
});
test('a remount that never publishes fails its guard and cancels the owned watcher', async () => {
	const { driver, input, handoff } = await controlledHandoff('right', 0);
	let entered: (() => void) | null = null;
	const waiting = new Promise<void>((resolve) => {
		entered = resolve;
	});
	let aborted = false;
	vi.useFakeTimers();
	try {
		const result = input.handoff({
			...handoff,
			expected: [login],
			inject: () => driver.send(Buffer.from('\r')),
			remounted: async (signal) => {
				signal.addEventListener(
					'abort',
					() => {
						aborted = true;
					},
					{ once: true },
				);
				if (entered === null) throw new Error('remount waiter absent');
				entered();
				return new Promise<never>(() => {});
			},
		});
		const rejected = expect(result).rejects.toThrow('handoff lifecycle fact unavailable');
		await waiting;
		await vi.advanceTimersByTimeAsync(8000);
		await rejected;
		expect(aborted).toBe(true);
	} finally {
		vi.useRealTimers();
		await driver.stop();
	}
});
