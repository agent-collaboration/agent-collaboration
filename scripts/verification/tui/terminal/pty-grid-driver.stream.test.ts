/** Controlled protocol child checks command/stream ordering, never product behavior. */
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';
import { z } from 'zod';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { InputConsumptionReadback } from '../sessions/input-consumption-readback.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { gridLines } from './terminal-grid.ts';

test('old request-driven bridge is rejected and its controlled process is stopped', async () => {
	const directory = temporaryRoot();
	const executable = controlledExecutable(
		directory,
		'old-protocol',
		`
process.stdout.write('TUI_GRID READY 120 36\\n');
process.stdin.on('data',()=>{process.stdout.write('TUI_GRID STOPPED 0\\n');process.exit(0);});`,
	);
	await expect(
		PtyGridDriver.start({
			repository: directory,
			driver: executable,
			surface: 'sessions-filters',
			keys: 'legacy',
			evidence: directory,
		}),
	).rejects.toThrow('streaming PTY driver required');
	const output = readFileSync(join(directory, 'driver.stdout'), 'utf8');
	expect(output).toContain('STOPPED 0');
});

test('stream drains while a file ACK is awaited; commands during a burst retain ordered answers and no-op frame', async () => {
	const directory = temporaryRoot();
	const receiptPath = join(directory, 'input-dispatch.jsonl');
	const commandsPath = join(directory, 'commands.jsonl');
	const executable = controlledExecutable(
		directory,
		'stream-control',
		`
const fs=require('node:fs');const readline=require('node:readline');
fs.writeFileSync(${JSON.stringify(receiptPath)},'');
const emit=x=>process.stdout.write('TUI_GRID '+x+'\\n');
const frame=x=>'\\x1b[?2026h\\x1b[2J\\x1b[H'+x+'\\x1b[?2026l';
const bytes=x=>emit('BYTES '+Buffer.from(x).toString('hex'));
let burst=true;
emit('READY 120 36 STREAM');bytes(frame('first'));
readline.createInterface({input:process.stdin}).on('line',line=>{
fs.appendFileSync(${JSON.stringify(commandsPath)},JSON.stringify({line,burst})+'\\n');
if(line.startsWith('write ')){
 fs.appendFileSync(${JSON.stringify(receiptPath)},JSON.stringify({sequence:1,event:{kind:'key',code:{kind:'fixed',name:'Up'},modifiers:['shift','alt'],phase:'press'},surface:{kind:'screen'},target:'sessions-list',action:null})+'\\n');
 // The no-op is still a complete new frame. A newer unrelated frame in
 // the same output burst must not replace the one preceding marker 1.
 emit('OK');bytes(frame('first')+'\\x1b]7770;dispatch=1\\x1b\\\\'+frame('later unrelated'));
}else if(line==='running'){emit(burst?'RUNNING':'EXITED');burst=false;}
else if(line.startsWith('resize ')){emit('RESIZED '+line.slice(7));emit('OK');bytes(frame('resized'));}
else if(line==='stop'){emit('EOF');emit('STOPPED 0');process.exit(0);}
else if(line.startsWith('reply ')||line.startsWith('read '))emit('OK');
});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'sessions-filters',
		keys: 'legacy',
		evidence: directory,
	});
	try {
		await driver.waitForGrid('burst began', (grid) => gridLines(grid)[0]?.trim() === 'first', 0);
		const reader = InputConsumptionReadback.open(receiptPath);
		const observed = reader.observeInput({
			expected: [
				{
					event: {
						kind: 'key',
						code: { kind: 'fixed', name: 'Up' },
						modifiers: ['shift', 'alt'],
						phase: 'press',
					},
					surface: { kind: 'screen' },
					target: 'sessions-list',
					action: null,
				},
			],
			inject: () => driver.send(Buffer.from('\x1b[1;4A')),
			timeoutMs: 1000,
		});
		const running = driver.running();
		const [[ack], isRunning] = await Promise.all([observed, running]);
		expect(isRunning).toBe(true);
		if (ack === undefined) throw new Error('controlled ACK absent');
		const marked = await driver.waitForDispatchFrame(ack.sequence);
		expect(gridLines(marked.grid)[0]?.trim()).toBe('first');
		expect(gridLines(driver.grid())[0]?.trim()).toBe('later unrelated');
		reader.assertComplete();
		await driver.resize(32, 24);
		expect(driver.grid()).toMatchObject({ columns: 32, rows: 24 });
	} finally {
		await driver.stop();
	}
	const commandSchema = z.strictObject({ line: z.string(), burst: z.boolean() });
	const commands = readFileSync(commandsPath, 'utf8')
		.trim()
		.split('\n')
		.map((line) => commandSchema.parse(JSON.parse(line)));
	const write = commands.find((command) => command.line.startsWith('write '));
	const running = commands.find((command) => command.line === 'running');
	const resize = commands.find((command) => command.line.startsWith('resize '));
	if (write === undefined || running === undefined || resize === undefined)
		throw new Error('controlled command absent');
	// Queue order is caller order: running was commissioned before injection's
	// microtask. Each answer must still reach the matching caller.
	expect(running.burst).toBe(true);
	expect(commands.some((command) => command.line.startsWith('read '))).toBe(false);
});

test('component dispatch receipt nomination reaches only its controlled component driver', async () => {
	const directory = temporaryRoot();
	const observedPath = join(directory, 'receipt-nomination.json');
	const receiptPath = join(directory, 'component-input-dispatch.jsonl');
	const executable = controlledExecutable(
		directory,
		'component-receipt-control',
		`
const fs=require('node:fs');fs.writeFileSync(${JSON.stringify(observedPath)},JSON.stringify({path:process.env.TUI_GRID_COMPONENT_DISPATCH_RECEIPT,scenario:process.env.TUI_GRID_COMPONENT_SCENARIO}));
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
process.stdin.on('data',()=>{process.stdout.write('TUI_GRID STOPPED 0\\n');process.exit(0);});`,
	);
	const driver = await PtyGridDriver.start({
		repository: directory,
		driver: executable,
		surface: 'normal',
		keys: 'legacy',
		evidence: directory,
		componentBinary: '/declared/component-libtest',
		componentDispatchReceipt: receiptPath,
		componentScenario: 'save-failed',
	});
	try {
		expect(JSON.parse(readFileSync(observedPath, 'utf8'))).toEqual({
			path: receiptPath,
			scenario: 'save-failed',
		});
	} finally {
		await driver.stop();
	}
});

test('component receipt rejects occupied and foreign paths before any controlled child launch', async () => {
	const directory = temporaryRoot(),
		foreign = temporaryRoot();
	const launched = join(directory, 'launched');
	const executable = controlledExecutable(
		directory,
		'rejected-receipt-control',
		`require('node:fs').writeFileSync(${JSON.stringify(launched)},'wrong launch');`,
	);
	const occupied = join(directory, 'occupied.jsonl');
	writeFileSync(occupied, 'existing');
	await Promise.all(
		[occupied, join(foreign, 'foreign.jsonl'), 'relative.jsonl'].map((receipt) =>
			expect(
				PtyGridDriver.start({
					repository: directory,
					driver: executable,
					surface: 'normal',
					keys: 'legacy',
					evidence: directory,
					componentBinary: '/declared/component',
					componentDispatchReceipt: receipt,
				}),
			).rejects.toThrow(/component receipt/u),
		),
	);
	await expect(
		PtyGridDriver.start({
			repository: directory,
			driver: executable,
			surface: 'sessions-filters',
			keys: 'legacy',
			evidence: directory,
			componentBinary: '/declared/component',
			componentDispatchReceipt: join(directory, 'wrong-surface.jsonl'),
		}),
	).rejects.toThrow('requires a component fixture');
	expect(existsSync(launched)).toBe(false);
});
