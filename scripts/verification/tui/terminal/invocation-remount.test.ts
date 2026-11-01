/** A controlled stream qualifies child-free remount observation, never real quota persistence. */
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import type { InputConsumptionExpectation } from '../sessions/input-consumption-schema.ts';
import type { LoopEndingHandoff } from './loop-ending-handoff.ts';
import { MarkerInputSession } from './marker-input-session.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { gridLines } from './terminal-grid.ts';

const quit: InputConsumptionExpectation = {
	event: {
		kind: 'key',
		code: { kind: 'shortcut', key: 'c' },
		modifiers: ['control'],
		phase: 'press',
	},
	surface: { kind: 'screen' },
	target: null,
	action: 'quota:quit',
};
const barrier: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'F12' }, modifiers: [], phase: 'press' },
	surface: { kind: 'screen' },
	target: null,
	action: null,
};
type ControlMode =
	| 'right'
	| 'missing-settlement'
	| 'wrong-invocation'
	| 'crossed-mount'
	| 'missing-start'
	| 'stale-mount'
	| 'skipped-mount'
	| 'changed-store'
	| 'fake-child'
	| 'wrong-marked-frame';

async function controlledRemount(mode: ControlMode): Promise<{
	driver: PtyGridDriver;
	input: MarkerInputSession;
	handoff: LoopEndingHandoff;
	directory: string;
	phases: string[];
}> {
	const directory = temporaryRoot(),
		receipt = join(directory, 'input-dispatch.jsonl'),
		storePath = join(directory, 'agent-proxy-ui.sqlite');
	const executable = controlledExecutable(
		directory,
		'invocation-remount',
		`
const fs=require('node:fs'),readline=require('node:readline');let sequence=0;
const root=${JSON.stringify(directory)},receipt=${JSON.stringify(receipt)},store=${JSON.stringify(storePath)},mode=${JSON.stringify(mode)};
fs.writeFileSync(receipt,'');fs.writeFileSync(root+'/commands.json','[]');const commands=[];
const emit=value=>process.stdout.write('TUI_GRID '+value+'\\n');
const frame=text=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+text+'\\x1b[?2026l';
const bytes=value=>emit('BYTES '+Buffer.from(value).toString('hex'));
emit('READY 120 36 STREAM');bytes(frame('old quota mount'));
readline.createInterface({input:process.stdin}).on('line',line=>{
 if(line==='stop'){emit('EOF');process.stdout.write('TUI_GRID STOPPED 0\\n',()=>process.exit(0));return;}
 if(line.startsWith('reply ')){emit('OK');return;}
 if(!line.startsWith('write '))throw Error('unexpected command '+line);
 commands.push(line);fs.writeFileSync(root+'/commands.json',JSON.stringify(commands));sequence++;
 const expected=sequence===2?${JSON.stringify(barrier)}:${JSON.stringify(quit)};
 fs.appendFileSync(receipt,JSON.stringify({sequence,...expected})+'\\n');
 if(sequence===1){
  const settled=mode==='missing-settlement'?null:mode==='fake-child'?{arguments:['login'],terminal:{kind:'interactive'}}:{invocation:mode==='wrong-invocation'?2:1,initialMount:1,finalMount:mode==='crossed-mount'?2:1};
  const started=mode==='missing-start'?null:{invocation:2,mount:mode==='stale-mount'?1:mode==='skipped-mount'?3:2,storePath:mode==='changed-store'?store+'.other':store};
  fs.writeFileSync(root+'/facts.json',JSON.stringify({settled,started}));
  bytes(frame('fresh quota mount'));emit('OK');return;
 }
 emit('OK');
 if(sequence===3){emit('EOF');process.stdout.write('TUI_GRID FINISHED 0\\n',()=>process.exit(0));return;}
 bytes(frame(mode==='wrong-marked-frame'?'wrong quota mount':'fresh quota mount')+'\\x1b]7770;dispatch='+sequence+'\\x1b\\\\'+frame(mode==='wrong-marked-frame'?'fresh quota mount':'unrelated later frame'));
});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'quota-effects',
		keys: 'legacy',
		evidence: directory,
	});
	const phases: string[] = [];
	const fact = (name: 'settled' | 'started'): unknown => {
		const value: unknown = JSON.parse(readFileSync(join(directory, 'facts.json'), 'utf8'));
		if (typeof value !== 'object' || value === null || !(name in value))
			throw new Error('fact absent');
		return name === 'settled' && 'settled' in value
			? value.settled
			: 'started' in value
				? value.started
				: null;
	};
	return {
		driver,
		input: new MarkerInputSession({ driver, receipt, profile: 'terminal-only' }),
		directory,
		phases,
		handoff: {
			kind: 'invocation-remount',
			previousMount: 1,
			previousInvocation: 1,
			storePath,
			invocationSettled: async () => {
				phases.push('settled');
				return fact('settled');
			},
			invocationStarted: async () => {
				phases.push('started');
				return fact('started');
			},
			ready: (grid) => gridLines(grid)[0]?.trim() === 'fresh quota mount',
			firstFrame: { key: 'F12', expected: [barrier] },
		},
	};
}

test('first Quit settles its invocation and observes the next marked mount without a child or EOF', async () => {
	const { driver, input, handoff, directory, phases } = await controlledRemount('right');
	try {
		const owner = driver.processId();
		const proof = await input.handoff({
			...handoff,
			expected: [quit],
			inject: () => driver.send(Buffer.from([3])),
		});
		expect(proof.kind).toBe('invocation-remount');
		expect(proof.driverPid).toBe(owner);
		expect(proof.remount.mount).toBe(2);
		expect('childEntry' in proof || 'childExit' in proof).toBe(false);
		expect(proof.receipts.map((entry) => entry.sequence)).toEqual([1]);
		expect(proof.firstFrame.frames.map((frame) => frame.binding.sequence)).toEqual([2]);
		expect(gridLines(proof.firstFrame.frames[0]?.grid ?? driver.grid())[0]?.trim()).toBe(
			'fresh quota mount',
		);
		expect(gridLines(driver.grid())[0]?.trim()).toBe('unrelated later frame');
		expect(phases).toEqual(['settled', 'started']);
		expect(JSON.parse(readFileSync(join(directory, 'commands.json'), 'utf8'))).toEqual([
			'write 03',
			'write 1b5b32347e',
		]);
		await input.exit({ expected: [quit], inject: () => driver.send(Buffer.from([3])) });
		expect(input.assertComplete()).toHaveLength(3);
	} finally {
		await driver.stop();
	}
});

test.each([
	'missing-settlement',
	'wrong-invocation',
	'crossed-mount',
	'missing-start',
	'stale-mount',
	'skipped-mount',
	'changed-store',
	'fake-child',
] satisfies readonly ControlMode[])(
	'child-free remount rejects %s before sending a first-frame input',
	async (mode) => {
		const { driver, input, handoff, directory } = await controlledRemount(mode);
		try {
			await expect(
				input.handoff({
					...handoff,
					expected: [quit],
					inject: () => driver.send(Buffer.from([3])),
				}),
			).rejects.toThrow();
			expect(JSON.parse(readFileSync(join(directory, 'commands.json'), 'utf8'))).toEqual([
				'write 03',
			]);
		} finally {
			await driver.stop();
		}
	},
);

test('later convenient output cannot replace the fresh invocation barrier marked frame', async () => {
	const { driver, input, handoff, directory } = await controlledRemount('wrong-marked-frame');
	try {
		await expect(
			input.handoff({ ...handoff, expected: [quit], inject: () => driver.send(Buffer.from([3])) }),
		).rejects.toThrow('first frame');
		expect(JSON.parse(readFileSync(join(directory, 'commands.json'), 'utf8'))).toEqual([
			'write 03',
			'write 1b5b32347e',
		]);
	} finally {
		await driver.stop();
	}
});
