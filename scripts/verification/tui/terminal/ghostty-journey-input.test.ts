import { readFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, test } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { encodeGhosttyMouse, encodeGhosttyText } from './ghostty-input-encoding.ts';
import type { TerminalInputModes } from './ghostty-input-encoding.ts';
import { GhosttyJourneyInput, requireDelivered } from './ghostty-journey-input.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';

const modes: TerminalInputModes = {
	kittyFlags: 1,
	cursorKeysApplication: false,
	mouseTracking: 1000,
	mouseEncoding: 'sgr',
	columns: 120,
	rows: 36,
};
test('example Ghostty keybinds consume navigation and replace Shift Enter before encoding', async () => {
	const writes: string[] = [];
	const input = new GhosttyJourneyInput(
		{
			inputModes: () => modes,
			send: async (bytes: Uint8Array): Promise<void> => {
				writes.push(Buffer.from(bytes).toString('hex'));
			},
		},
		'example-ghostty-keybinds',
	);
	await ['PageUp', 'PageDown', 'Home', 'End'].reduce<Promise<void>>(async (previous, key) => {
		await previous;
		const record = await input.key(key);
		expect(record.delivery.kind).toBe('consumed');
		expect(() => requireDelivered(record)).toThrow('input prerequisite finding');
	}, Promise.resolve());
	expect(writes).toEqual([]);
	await input.key('Shift+Enter');
	await input.key('Ctrl+Enter');
	expect(writes).toEqual(['0a', '1b5b31333b3575']);
});
test('SGR1006 uses one-based press/release and wheel codes only when actually enabled', () => {
	for (const [kind, bytes] of [
		['press', '\x1b[<0;120;36M'],
		['release', '\x1b[<0;120;36m'],
	] as const)
		expect(encodeGhosttyMouse({ kind, button: 'left', column: 119, row: 35 }, modes)).toEqual({
			kind: 'bytes',
			bytes: Buffer.from(bytes),
		});
	for (const tracking of [1000, 1002, 1003] as const)
		for (const [direction, code] of [
			['up', 64],
			['down', 65],
		] as const)
			expect(
				encodeGhosttyMouse(
					{ kind: 'wheel', direction, column: 0, row: 0 },
					{ ...modes, mouseTracking: tracking },
				),
			).toEqual({ kind: 'bytes', bytes: Buffer.from(`\x1b[<${code};1;1M`) });
	expect(
		encodeGhosttyMouse(
			{ kind: 'wheel', direction: 'up', column: 0, row: 0 },
			{ ...modes, mouseTracking: 0 },
		).kind,
	).toBe('consumed');
	expect(
		encodeGhosttyMouse(
			{ kind: 'wheel', direction: 'up', column: 0, row: 0 },
			{ ...modes, mouseEncoding: 'sgr-pixels' },
		).kind,
	).toBe('unsupported');
	expect(() =>
		encodeGhosttyMouse(
			{ kind: 'press', button: 'left', column: 32, row: 0 },
			{ ...modes, columns: 32 },
		),
	).toThrow('outside');
	expect(encodeGhosttyText('a', { ...modes, kittyFlags: 31 }).toString()).toBe('\x1b[97;;97u');
});
test('real protocol subprocess feeds modes into existing driver and opt-in input across resize', async () => {
	const root = temporaryRoot();
	const writes = join(root, 'writes.txt');
	const executable = controlledExecutable(
		root,
		'mode-driver',
		`
const fs=require('node:fs'); const readline=require('node:readline');
let bytes='\\x1b[>1u\\x1b[?1002h\\x1b[?1006h';
process.stdout.write('TUI_GRID READY 120 36 STREAM\\n');
const emitBytes=()=>process.stdout.write('TUI_GRID BYTES '+Buffer.from(bytes).toString('hex')+'\\n');emitBytes();
const lines=readline.createInterface({input:process.stdin});
lines.on('line',line=>{const [command,hex]=line.split(' ');
if(command==='read'){process.stdout.write('TUI_GRID OK\\n');}
else if(command==='write'){fs.appendFileSync(${JSON.stringify(writes)},hex+'\\n');process.stdout.write('TUI_GRID OK\\n');}
else if(command==='resize'){bytes='\\x1b[>31u';process.stdout.write('TUI_GRID RESIZED '+line.slice(7)+'\\nTUI_GRID OK\\n');emitBytes();}
else if(command==='reply'){process.stdout.write('TUI_GRID OK\\n');}
else if(command==='stop'||command==='finish'){process.stdout.write('TUI_GRID FINISHED 0\\n');lines.close();process.stdin.destroy();}
});`,
	);
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: executable,
		surface: 'sessions-filters',
		keys: 'kitty',
		evidence: root,
	});
	try {
		await driver.receive();
		const input = new GhosttyJourneyInput(driver, 'terminal-only');
		await input.key('Tab');
		await input.click(119, 35);
		await input.mouse({ kind: 'wheel', direction: 'up', column: 0, row: 0 });
		await input.mouse({ kind: 'wheel', direction: 'down', column: 0, row: 0 });
		const grids = [
			[76, 30],
			[48, 24],
			[48, 12],
			[40, 24],
			[32, 24],
		] as const;
		await grids.reduce<Promise<void>>(async (previous, [columns, rows]) => {
			await previous;
			await driver.resize(columns, rows);
			await driver.receive();
			await input.key('Tab');
			await input.click(columns - 1, rows - 1);
			await input.mouse({ kind: 'wheel', direction: 'up', column: 0, row: 0 });
			await input.mouse({ kind: 'wheel', direction: 'down', column: 0, row: 0 });
		}, Promise.resolve());
		expect(input.history().at(-1)?.modes).toMatchObject({
			columns: 32,
			rows: 24,
			kittyFlags: 31,
			mouseTracking: 1002,
			mouseEncoding: 'sgr',
		});
		expect(
			readFileSync(writes, 'utf8')
				.trim()
				.split('\n')
				.map((hex) => Buffer.from(hex, 'hex').toString()),
		).toEqual([
			'\t',
			'\x1b[<0;120;36M',
			'\x1b[<0;120;36m',
			'\x1b[<64;1;1M',
			'\x1b[<65;1;1M',
			...grids.flatMap(([columns, rows]) => [
				'\x1b[9u',
				`\x1b[<0;${columns};${rows}M`,
				`\x1b[<0;${columns};${rows}m`,
				'\x1b[<64;1;1M',
				'\x1b[<65;1;1M',
			]),
		]);
	} finally {
		expect(await driver.stop()).toBe(0);
	}
});

test.each([0, 1, 3, 31])(
	'example Ghostty consumes original paging keys while four Option aliases encode at flags %i',
	async (flags) => {
		const writes: string[] = [];
		const input = new GhosttyJourneyInput(
			{
				inputModes: () => ({ ...modes, kittyFlags: flags, cursorKeysApplication: true }),
				send: async (bytes: Uint8Array): Promise<void> => {
					writes.push(Buffer.from(bytes).toString('latin1'));
				},
			},
			'example-ghostty-keybinds',
		);
		await ['PageUp', 'PageDown', 'Home', 'End'].reduce<Promise<void>>(async (previous, key) => {
			await previous;
			expect((await input.key(key)).delivery.kind).toBe('consumed');
		}, Promise.resolve());
		expect(writes).toEqual([]);
		await ['Alt+Up', 'Alt+Down', 'Alt+Shift+Up', 'Alt+Shift+Down'].reduce<Promise<void>>(
			async (previous, key) => {
				await previous;
				requireDelivered(await input.key(key));
			},
			Promise.resolve(),
		);
		const phase = flags === 3 || flags === 31 ? ':1' : '';
		expect(writes).toEqual([
			`\x1b[1;3${phase}A`,
			`\x1b[1;3${phase}B`,
			`\x1b[1;4${phase}A`,
			`\x1b[1;4${phase}B`,
		]);
		expect(
			input
				.history()
				.slice(4)
				.every(
					(record) =>
						record.profile === 'example-ghostty-keybinds' &&
						record.delivery.kind === 'sent' &&
						record.delivery.route === 'encoder',
				),
		).toBe(true);
	},
);
