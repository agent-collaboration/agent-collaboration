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

// Original Codex 0.165 startup through its first duplicate mode reset; no Session data.
const CODEX_STARTUP_HEX =
	'2f6d6f64656c1b5b3f32303034681b5b3e343b306d1b5b3e35751b5b3f31303034681b5b366e1b5d31303b3f1b5c1b5d31313b3f1b5c1b5b3f751b5b631b5b3f32303236681b5b3f32303236681b5b313b31481b5b4a1b5b3f32356c1b5b313b32481b5b306d1b5b4b1b5b323b3239481b5b306d1b5b4b1b5b333b3133481b5b306d1b5b4b1b5b343b3230481b5b306d1b5b4b1b5b353b32481b5b306d1b5b4b1b5b363b3237481b5b306d1b5b4b1b5b373b32481b5b306d1b5b4b1b5b383b32481b5b306d1b5b4b1b5b313b3148201b5b323b314820203e5f201b5b316d4f70656e414920436f6465781b5b32326d1b5b326d1b5b326d202876302e3136352e30291b5b333b31481b5b32326d20202020201b5b326d6c6f6164696e671b5b343b31482020526573756d696e672073657373696f6ee280a61b5b353b31481b5b32326d201b5b363b31481b5b316de280ba1b5b32326d201b5b326d41736b20436f64657820746f20646f20616e797468696e671b5b373b31481b5b32326d201b5b383b3148201b5b306d1b5b313b31481b5b3020711b5b313b3148201b5b306d1b5b363b33481b5b3f3235681b5b3f323032366c1b5b3f323032366c';

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
		if (input === 'codex-startup') emit('BYTES ' + ${JSON.stringify(CODEX_STARTUP_HEX)});
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

test('the streamed driver accepts the original Codex startup with repeated synchronized mode writes', async () => {
	const { driver, root } = await standInDriver();
	try {
		await driver.send(Buffer.from('codex-startup'));
		const grid = await driver.waitForGrid('Codex startup', (observed) =>
			gridLines(observed).join('\n').includes('Resuming session…'),
		);
		expect(gridLines(grid).join('\n')).toContain('OpenAI Codex');
		await driver.send(Buffer.from('eof'));
		await driver.waitForEof('startup complete');
		expect(await driver.finish()).toBe(0);
		expect(readFileSync(join(root, 'terminal.ansi')).subarray(0, 444)).toEqual(
			Buffer.from(CODEX_STARTUP_HEX, 'hex'),
		);
	} finally {
		await driver.stop();
	}
});
