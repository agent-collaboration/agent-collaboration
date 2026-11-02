import { join } from 'node:path';

import { expect, test, vi } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import type { InputConsumptionExpectation } from '../sessions/input-consumption-schema.ts';
import { MarkerInputSession } from './marker-input-session.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { gridLines } from './terminal-grid.ts';

const fileNotifications = vi.hoisted(() => ({ suppressed: false }));
vi.mock('node:fs', async (importOriginal) => {
	const original = await importOriginal<typeof import('node:fs')>();
	return {
		...original,
		watch: (path: string, options: { persistent: boolean }, listener: () => void) =>
			original.watch(path, options, () => {
				if (!fileNotifications.suppressed) listener();
			}),
	};
});

const down: InputConsumptionExpectation = {
	event: { kind: 'key', code: { kind: 'fixed', name: 'Down' }, modifiers: [], phase: 'press' },
	surface: { kind: 'screen' },
	target: 'rows',
	action: null,
};
const quit: InputConsumptionExpectation = {
	event: {
		kind: 'key',
		code: { kind: 'shortcut', key: 'c' },
		modifiers: ['control'],
		phase: 'press',
	},
	surface: { kind: 'screen' },
	target: null,
	action: 'quit',
};
async function controlledDriver(
	mode: 'right' | 'wrong-target' | 'missing-marker',
): Promise<{ readonly driver: PtyGridDriver; readonly input: MarkerInputSession }> {
	const directory = temporaryRoot(),
		receipt = join(directory, 'input-dispatch.jsonl');
	const executable = controlledExecutable(
		directory,
		'marked-flow-control',
		`
const fs=require('node:fs'),readline=require('node:readline');
const receipt=${JSON.stringify(receipt)},mode=${JSON.stringify(mode)};fs.writeFileSync(receipt,'');
const emit=value=>process.stdout.write('TUI_GRID '+value+'\\n');
const frame=value=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+value+'\\x1b[?2026l';
const bytes=value=>emit('BYTES '+Buffer.from(value).toString('hex'));
let sequence=0;emit('READY 120 36 STREAM');bytes(frame('initial'));
readline.createInterface({input:process.stdin}).on('line',line=>{
 if(line.startsWith('write ')){
  sequence++;const expected=sequence===1?${JSON.stringify(down)}:${JSON.stringify(quit)};
  if(mode==='wrong-target'&&sequence===1)expected.target='foreign-rows';
  fs.appendFileSync(receipt,JSON.stringify({sequence,...expected})+'\\n');emit('OK');
  if(sequence===2||mode==='missing-marker'){emit('EOF');process.stdout.write('TUI_GRID FINISHED 0\\n',()=>process.exit(0));}
  else bytes(frame('marked result')+'\\x1b]7770;dispatch=1\\x1b\\\\'+frame('unrelated later frame'));
 }else if(line==='stop'){emit('EOF');process.stdout.write('TUI_GRID STOPPED 0\\n',()=>process.exit(0));}
 else if(line.startsWith('reply '))emit('OK');
});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'sessions-filters',
		keys: 'legacy',
		evidence: directory,
	});
	return { driver, input: new MarkerInputSession({ driver, receipt, profile: 'terminal-only' }) };
}
test('input-specific frame survives later output and loop-ending ACK settles without a promised marker', async () => {
	const { driver, input } = await controlledDriver('right');
	try {
		const observed = await input.press('Down', [down]);
		expect(observed.receipts.map((entry) => entry.sequence)).toEqual([1]);
		expect(gridLines(observed.frames[0]?.grid ?? driver.grid())[0]?.trim()).toBe('marked result');
		expect(gridLines(driver.grid())[0]?.trim()).toBe('unrelated later frame');
		const finished = await input.exit({
			expected: [quit],
			inject: () => driver.send(Buffer.from([3])),
		});
		expect(finished).toMatchObject({ exitCode: 0, receipts: [{ sequence: 2, action: 'quit' }] });
	} finally {
		await driver.stop();
	}
});
test('wrong consumed target is rejected even when a valid-looking frame follows', async () => {
	const { driver, input } = await controlledDriver('wrong-target');
	try {
		await expect(input.press('Down', [down])).rejects.toThrow('target mismatch');
	} finally {
		await driver.stop();
	}
});
test('a typed dispatch plus EOF cannot substitute for a missing continuing-input marker', async () => {
	const { driver, input } = await controlledDriver('missing-marker');
	try {
		await expect(input.press('Down', [down])).rejects.toThrow('marker absent');
	} finally {
		await driver.stop();
	}
});
test('exact markers and producer exit settle receipt reads even with every file notification suppressed', async () => {
	fileNotifications.suppressed = true;
	const { driver, input } = await controlledDriver('right');
	try {
		const observed = await input.press('Down', [down]);
		expect(observed.receipts.map((receipt) => receipt.sequence)).toEqual([1]);
		const finished = await input.exit({
			expected: [quit],
			inject: () => driver.send(Buffer.from([3])),
		});
		expect(finished.exitCode).toBe(0);
		expect(input.assertComplete()).toHaveLength(2);
	} finally {
		await driver.stop();
		fileNotifications.suppressed = false;
	}
});
