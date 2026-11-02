import { readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';
import { z } from 'zod';

import { controlledExecutable } from '../fixtures/controlled-child-fixtures.ts';
import { temporaryRoot } from '../fixtures/receipt-fixtures.ts';
import { PtyGridDriver } from '../terminal/pty-grid-driver.ts';
import { gridLines } from '../terminal/terminal-grid.ts';
import {
	observeCompletedFrames,
	requireNoWholeRunError,
	requireReplayMatchesLiveGrid,
	settleWholeRunFrames,
} from './codex-acp-frame-observation.ts';

// Literal frames as Codex draws them: each draw is one DECSET 2026 synchronized update
// (crossterm SynchronizedUpdate, tui.rs:1304-1356), here a home, a clear and the frame's text.
const BEGIN = '\u001b[?2026h';
const END = '\u001b[?2026l';
const frame = (...lines: string[]): string =>
	`${BEGIN}\u001b[H\u001b[2J${lines.join('\r\n')}${END}`;
const SAVE_ERROR = '■ Failed to save default model: Saving Codex defaults is not supported';
const SUCCESS = '• Model changed to claude-haiku-5-5 high';
const bytes = (text: string): Uint8Array => Buffer.from(text, 'utf8');
const roots: string[] = [];
afterEach(() => {
	for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

test('an error frame followed by a clean frame in one write is still seen, with its place', async () => {
	const run = frame(SAVE_ERROR) + frame(SUCCESS);
	const observed = await observeCompletedFrames(bytes(run), []);
	expect(observed.completedFrames).toBe(2);
	expect(observed.errors).toEqual([
		{ frame: 1, byteOffset: Buffer.byteLength(frame(SAVE_ERROR)), line: SAVE_ERROR },
	]);
	// The final screen alone, which a single snapshot per write reads, is clean.
	expect(observed.finalLines.some((line) => line.startsWith('■ '))).toBe(false);
	expect(() => requireNoWholeRunError(observed)).toThrow(
		`error cell in the whole run (frame 1, after byte ${Buffer.byteLength(frame(SAVE_ERROR))}): ${SAVE_ERROR}`,
	);
});

test('a clean run passes, and a repainted success cell is not an error', async () => {
	const observed = await observeCompletedFrames(
		bytes(frame('› /model') + frame(SUCCESS) + frame(SUCCESS)),
		[],
	);
	expect(observed.completedFrames).toBe(3);
	expect(observed.errors).toEqual([]);
	expect(() => requireNoWholeRunError(observed)).not.toThrow();
});

test('a partial observation is never a pass', async () => {
	const unfinished = await observeCompletedFrames(bytes(frame(SUCCESS) + BEGIN + 'half'), []);
	expect(unfinished.unterminatedFrame).toBe(true);
	expect(() => requireNoWholeRunError(unfinished)).toThrow('unfinished frame');
	const noFrames = await observeCompletedFrames(bytes(SUCCESS), []);
	expect(noFrames.completedFrames).toBe(0);
	expect(() => requireNoWholeRunError(noFrames)).toThrow('no completed frame');
	await expect(
		observeCompletedFrames(bytes(frame(SUCCESS)), [{ byteOffset: 999, columns: 80, rows: 24 }]),
	).rejects.toThrow('invalid terminal resize boundary');
});

test('an error pushed into scrollback inside one frame, or drawn outside any frame, is seen', async () => {
	const pushed = frame(SAVE_ERROR, ...Array.from({ length: 40 }, (_, row) => `row ${row}`));
	const scrolled = await observeCompletedFrames(bytes(pushed), []);
	expect(scrolled.completedFrames).toBe(1);
	expect(scrolled.errors).toEqual([
		{ frame: 0, byteOffset: Buffer.byteLength(pushed), line: SAVE_ERROR },
	]);
	const outside = `${frame(SUCCESS)}\r\n${SAVE_ERROR}`;
	const unbracketed = await observeCompletedFrames(bytes(outside), []);
	expect(unbracketed.errors).toEqual([
		{ frame: 0, byteOffset: Buffer.byteLength(outside), line: SAVE_ERROR },
	]);
});

test('resize boundaries replay at their byte offsets', async () => {
	const first = frame(SUCCESS);
	const observed = await observeCompletedFrames(bytes(first + frame('narrow')), [
		{ byteOffset: Buffer.byteLength(first), columns: 40, rows: 12 },
	]);
	expect(observed.completedFrames).toBe(2);
	expect(observed.finalLines).toHaveLength(12);
	expect(observed.finalLines[0]?.trimEnd()).toBe('narrow');
});

test('settling keeps the evidence first, then refuses only a forbidding journey', async () => {
	const run = bytes(frame(SAVE_ERROR) + frame(SUCCESS));
	const live = (await observeCompletedFrames(run, [])).finalLines;
	const forbid = temporaryRoot();
	const record = temporaryRoot();
	roots.push(forbid, record);
	await expect(
		settleWholeRunFrames({
			bytes: run,
			resizes: [],
			liveLines: live,
			policy: 'forbid',
			output: forbid,
		}),
	).rejects.toThrow(SAVE_ERROR);
	const kept = z
		.looseObject({
			completed_frames: z.number(),
			whole_run_errors: z.string(),
			errors: z.array(z.looseObject({ frame: z.number(), line: z.string() })),
		})
		.parse(JSON.parse(readFileSync(join(forbid, 'frame-observation.json'), 'utf8')));
	expect(kept.completed_frames).toBe(2);
	expect(kept.whole_run_errors).toBe('forbid');
	expect(kept.errors.map((error) => ({ frame: error.frame, line: error.line }))).toEqual([
		{ frame: 1, line: SAVE_ERROR },
	]);
	await expect(
		settleWholeRunFrames({
			bytes: run,
			resizes: [],
			liveLines: live,
			policy: 'record',
			output: record,
		}),
	).resolves.toMatchObject({ completedFrames: 2 });
	// A replay that does not end on the live grid is refused even when nothing is forbidden.
	const other = temporaryRoot();
	roots.push(other);
	await expect(
		settleWholeRunFrames({
			bytes: run,
			resizes: [],
			liveLines: ['another grid'],
			policy: 'record',
			output: other,
		}),
	).rejects.toThrow('does not end on the live grid');
});

// The Rust drive_terminal line protocol, scripted: READY, one reply per read, OK for writes.
const LINE_PROTOCOL_STANDIN = `
const { readFileSync } = require('node:fs');
const replies = JSON.parse(readFileSync(process.env.TUI_STANDIN_REPLIES, 'utf8'));
process.stdout.write('TUI_GRID READY 120 36\\n');
let pending = '';
process.stdin.on('data', (chunk) => {
	pending += chunk.toString('utf8');
	while (pending.includes('\\n')) {
		const line = pending.slice(0, pending.indexOf('\\n'));
		pending = pending.slice(pending.indexOf('\\n') + 1);
		const [command] = line.split(' ');
		if (command === 'read') process.stdout.write('TUI_GRID ' + (replies.shift() ?? 'EOF') + '\\n');
		else if (command === 'finish' || command === 'stop') {
			process.stdout.write('TUI_GRID FINISHED 0\\n');
			process.exit(0);
		} else process.stdout.write('TUI_GRID OK\\n');
	}
});
`;

test('a forbidden error cannot hide behind a later clean checkpoint in the same read', async () => {
	const root = temporaryRoot();
	roots.push(root);
	const run = Buffer.from(frame(SAVE_ERROR) + frame(SUCCESS), 'utf8');
	// Split the run inside the second frame's opening sequence, across two driver reads.
	const split = Buffer.byteLength(frame(SAVE_ERROR)) + 3;
	writeFileSync(
		join(root, 'replies.json'),
		JSON.stringify([
			`BYTES ${run.subarray(0, split).toString('hex')}`,
			`BYTES ${run.subarray(split).toString('hex')}`,
		]),
	);
	const driver = await PtyGridDriver.start({
		repository: root,
		driver: controlledExecutable(root, 'drive-terminal', LINE_PROTOCOL_STANDIN),
		surface: 'codex-acp',
		keys: 'legacy',
		evidence: root,
		publicEnvironment: { TUI_STANDIN_REPLIES: join(root, 'replies.json') },
	});
	try {
		// The checkpoint only ever sees each read's last state: the clean success frame.
		const grid = await driver.waitForGrid('success', (candidate) =>
			gridLines(candidate).some((line) => line.trimEnd() === SUCCESS),
		);
		expect(gridLines(grid).some((line) => line.startsWith('■ '))).toBe(false);
		const observed = await observeCompletedFrames(driver.bytes(), driver.resizeHistory());
		requireReplayMatchesLiveGrid(observed, gridLines(driver.grid()));
		expect(observed.errors.map((error) => error.line)).toEqual([SAVE_ERROR]);
		expect(() => requireNoWholeRunError(observed)).toThrow(SAVE_ERROR);
		expect(() => requireReplayMatchesLiveGrid(observed, ['another grid'])).toThrow(
			'does not end on the live grid',
		);
	} finally {
		await driver.stop();
	}
});
