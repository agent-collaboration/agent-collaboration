import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { controlledExecutable } from './controlled-child-fixtures.ts';
import { PtyGridDriver } from './pty-grid-driver.ts';
import { temporaryRoot } from './receipt-fixtures.ts';
import { gridLines } from './terminal-grid.ts';

const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

// A controlled stand-in for the Rust drive_terminal line protocol: READY, then one scripted reply
// per `read` (recording its bound), OK for writes, FINISHED on finish/stop. Never a real PTY.
const LINE_PROTOCOL_STANDIN = `
const { appendFileSync, readFileSync } = require('node:fs');
const replies = JSON.parse(readFileSync(process.env.TUI_STANDIN_REPLIES, 'utf8'));
process.stdout.write('TUI_GRID READY 120 36\\n');
let pending = '';
process.stdin.on('data', (chunk) => {
	pending += chunk.toString('utf8');
	while (pending.includes('\\n')) {
		const line = pending.slice(0, pending.indexOf('\\n'));
		pending = pending.slice(pending.indexOf('\\n') + 1);
		const [command, argument] = line.split(' ');
		if (command === 'read') {
			appendFileSync(process.env.TUI_STANDIN_READS, argument + '\\n');
			process.stdout.write('TUI_GRID ' + (replies.shift() ?? 'EOF') + '\\n');
		} else if (command === 'finish' || command === 'stop') {
			process.stdout.write('TUI_GRID FINISHED 0\\n');
			process.exit(0);
		} else process.stdout.write('TUI_GRID OK\\n');
	}
});
`;

async function standInDriver(replies: readonly string[]): Promise<{
	readonly driver: PtyGridDriver;
	readonly reads: () => number[];
}> {
	const root = temporaryRoot();
	roots.push(root);
	const script = controlledExecutable(root, 'drive-terminal', LINE_PROTOCOL_STANDIN);
	writeFileSync(join(root, 'replies.json'), JSON.stringify(replies));
	writeFileSync(join(root, 'reads.txt'), '');
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: script,
		surface: 'codex-acp',
		keys: 'legacy',
		evidence: root,
		publicEnvironment: {
			TUI_STANDIN_REPLIES: join(root, 'replies.json'),
			TUI_STANDIN_READS: join(root, 'reads.txt'),
		},
	});
	return {
		driver,
		reads: () =>
			readFileSync(join(root, 'reads.txt'), 'utf8').trim().split('\n').filter(Boolean).map(Number),
	};
}
const QUIET = 'READ_ERROR timed out waiting on channel';
const MARKER = `BYTES ${Buffer.from('ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED').toString('hex')}`;
const showsMarker = (grid: Parameters<typeof gridLines>[0]): boolean =>
	gridLines(grid).join('\n').includes('ACP_BRIDGE_CLAUDE_7FA28C|ALPHA|RED');

test('the default 8 s checkpoint still fails on its first quiet read', async () => {
	const { driver } = await standInDriver([QUIET, MARKER]);
	try {
		await expect(driver.waitForGrid('marker', showsMarker)).rejects.toThrow(QUIET);
	} finally {
		await driver.stop();
	}
});

test('a declared long checkpoint waits through quiet reads, each still bounded by 8 s', async () => {
	const { driver, reads } = await standInDriver([QUIET, QUIET, MARKER]);
	try {
		const grid = await driver.waitForGrid('marker', showsMarker, -1, 120_000);
		expect(showsMarker(grid)).toBe(true);
		expect(reads()).toHaveLength(3);
		expect(reads().every((bound) => bound >= 1 && bound <= 8_000)).toBe(true);
	} finally {
		await driver.stop();
	}
});

test('a declared long checkpoint still fails on any other read error', async () => {
	const { driver } = await standInDriver([
		'READ_ERROR channel is empty and sending half is closed',
	]);
	try {
		await expect(driver.waitForGrid('marker', showsMarker, -1, 120_000)).rejects.toThrow(
			'channel is empty and sending half is closed',
		);
	} finally {
		await driver.stop();
	}
});
