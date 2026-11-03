import { readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test, vi } from 'vitest';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { gridLines } from './terminal-grid.ts';

const roots: string[] = [];
afterEach(() => {
	vi.useRealTimers();
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

// The peer publishes output without read commands. Inputs release a frame, a stream error or EOF.
const STREAM_PEER = `
const fs = require('node:fs');
const readline = require('node:readline');
const emit = (record) => process.stdout.write('TUI_GRID ' + record + '\\n');
emit('READY 120 36 STREAM');
readline.createInterface({input: process.stdin}).on('line', (line) => {
	fs.appendFileSync(process.env.TUI_STANDIN_LOG, line + '\\n');
	const [command, argument] = line.split(' ');
	if (command === 'finish' || command === 'stop') { emit('FINISHED 0'); process.exit(0); }
	if (command === 'write') {
		const input = Buffer.from(argument, 'hex').toString();
		if (input === 'marker') emit('BYTES ' + Buffer.from('ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED').toString('hex'));
		if (input === 'fail') { emit('READ_ERROR disconnected before EOF'); return; }
		if (input === 'eof') emit('EOF');
	}
	emit('OK');
});
`;

async function standInDriver(): Promise<{ readonly driver: PtyGridDriver; readonly root: string }> {
	const root = temporaryRoot();
	roots.push(root);
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: controlledExecutable(root, 'stream-peer', STREAM_PEER),
		surface: 'codex-acp',
		keys: 'legacy',
		evidence: root,
		publicEnvironment: { TUI_STANDIN_LOG: join(root, 'commands.txt') },
	});
	return { driver, root };
}
const showsMarker = (grid: Parameters<typeof gridLines>[0]): boolean =>
	gridLines(grid).join('\n').includes('ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED');

test('the default checkpoint fails at its unchanged eight-second deadline', async () => {
	const { driver } = await standInDriver();
	try {
		vi.useFakeTimers();
		const rejected = expect(driver.waitForGrid('marker', showsMarker)).rejects.toThrow(
			'PTY output deadline',
		);
		await vi.advanceTimersByTimeAsync(8_000);
		await rejected;
	} finally {
		vi.useRealTimers();
		await driver.stop();
	}
});

test('a declared long checkpoint accepts a later streamed frame without issuing read commands', async () => {
	const { driver, root } = await standInDriver();
	try {
		vi.useFakeTimers();
		let settled = false;
		const waiting = driver.waitForGrid('marker', showsMarker, -1, 120_000).then((grid) => {
			settled = true;
			return grid;
		});
		await vi.advanceTimersByTimeAsync(8_000);
		expect(settled).toBe(false);
		await driver.send(Buffer.from('marker'));
		expect(showsMarker(await waiting)).toBe(true);
		expect(readFileSync(join(root, 'commands.txt'), 'utf8')).toBe('write 6d61726b6572\n');
	} finally {
		vi.useRealTimers();
		await driver.stop();
	}
});

test('a declared long checkpoint still rejects an actual stream failure', async () => {
	const { driver } = await standInDriver();
	try {
		const rejected = expect(driver.waitForGrid('marker', showsMarker, -1, 120_000)).rejects.toThrow(
			'disconnected before EOF',
		);
		await expect(driver.send(Buffer.from('fail'))).rejects.toThrow('disconnected before EOF');
		await rejected;
	} finally {
		await driver.stop();
	}
});

test('EOF completion comes from the stream and does not require a read command', async () => {
	const { driver } = await standInDriver();
	try {
		await driver.send(Buffer.from('eof'));
		await driver.waitForEof('normal quit');
		expect(await driver.finish()).toBe(0);
	} finally {
		await driver.stop();
	}
});
