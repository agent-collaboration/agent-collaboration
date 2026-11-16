/**
 * A turn error ends the reference's waiting step at once (assignment Correction 33). Recorded tee
 * frames shaped like the D0 r1 c32 wire, controlled terminal frames and a scripted frame source;
 * never a live run, and no real time passes.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { driveCodexStep } from './codex-acp-run.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';
import { LAUNCHED_THREAD, referenceStartRecording } from './codex-acp-test-fixtures.ts';
import {
	referenceTurnFailure,
	undeclaredErrorCell,
	wireTurnFailure,
} from './codex-acp-turn-failure.ts';

const START = referenceStartRecording(LAUNCHED_THREAD);
const TITLE_THREAD = '019a0000-0000-7000-8000-00000000717e';

function turnError(seq: number, threadId: string, willRetry: boolean): string {
	return `${JSON.stringify({
		seq,
		connection: 0,
		direction: 'serverToCli',
		message: {
			method: 'error',
			params: {
				error: {
					message: 'You’ve hit your usage limit. Try again later.',
					codexErrorInfo: 'usageLimitExceeded',
				},
				willRetry,
				threadId,
				turnId: 'turn-1',
			},
		},
	})}\n`;
}

// Codex's own title thread: its start (threadSource thread_title) and reply.
const TITLE_START = [
	{
		seq: 10,
		connection: 0,
		direction: 'cliToServer',
		message: { ['id']: 'temp-1', method: 'thread/start', params: { threadSource: 'thread_title' } },
	},
	{
		seq: 11,
		connection: 0,
		direction: 'serverToCli',
		message: { ['id']: 'temp-1', result: { thread: { ['id']: TITLE_THREAD } } },
	},
]
	.map((frame) => `${JSON.stringify(frame)}\n`)
	.join('');

const directories: string[] = [];
afterEach(() => {
	for (const directory of directories.splice(0))
		rmSync(directory, { recursive: true, force: true });
});
function recordingPath(text?: string): string {
	const directory = mkdtempSync(join(tmpdir(), 'turn-failure-'));
	directories.push(directory);
	const path = join(directory, 'wire.jsonl');
	if (text !== undefined) writeFileSync(path, text);
	return path;
}

async function drawn(lines: readonly string[]): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(lines.map((line, row) => `\x1b[${row + 1};1H${line}`).join(''));
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}

const SEND_REPLIED: CodexJourney['steps'][number] = {
	actions: [],
	checkpoint: {
		scene: 'send-replied',
		require: ['DELTA_SEND_CHECK'],
		forbid: ['■ '],
		deadline_ms: 300_000,
	},
};

test('the wire names a turn error Codex will not retry; a retried one, or the title thread, is not', () => {
	expect(wireTurnFailure(recordingPath())).toBeNull();
	expect(wireTurnFailure(recordingPath(START + turnError(4, LAUNCHED_THREAD, false)))).toBe(
		'turn error usageLimitExceeded on turn turn-1 (wire seq 4): You’ve hit your usage limit. Try again later.',
	);
	expect(wireTurnFailure(recordingPath(START + turnError(4, LAUNCHED_THREAD, true)))).toBeNull();
	expect(
		wireTurnFailure(recordingPath(START + TITLE_START + turnError(12, TITLE_THREAD, false))),
	).toBeNull();
});

test('an error cell the step did not declare stops it; a declared one does not', async () => {
	const grid = await drawn(['■ You’ve hit your usage limit. Try again later.']);
	expect(undeclaredErrorCell(grid, SEND_REPLIED.checkpoint)).toBe(
		'error cell on screen: ■ You’ve hit your usage limit. Try again later.',
	);
	expect(
		undeclaredErrorCell(grid, {
			...SEND_REPLIED.checkpoint,
			error_line: 'You’ve hit your usage limit. Try again later.',
		}),
	).toBeNull();
});

test('a recorded turn error ends the waiting step with its name, before any deadline', async () => {
	const path = recordingPath(START);
	const waiting = await drawn(['› Reply with three uppercase words', '  Working']);
	const judged: string[] = [];
	// The terminal never draws the reply; the error reaches the wire with no redraw, and the tee's
	// wake makes the wait judge the current frame again (review F9).
	const driver = {
		frameGeneration: (): number => 1,
		send: async (): Promise<void> => undefined,
		waitForGrid: async (
			name: string,
			matches: (grid: TerminalGrid) => boolean,
		): Promise<TerminalGrid> => {
			judged.push('frame');
			if (matches(waiting)) return waiting;
			writeFileSync(path, START + turnError(4, LAUNCHED_THREAD, false));
			judged.push('wake');
			if (matches(waiting)) return waiting;
			throw new Error(`grid checkpoint unreachable: ${name}`);
		},
	};
	await expect(
		driveCodexStep(driver, SEND_REPLIED, 'legacy', referenceTurnFailure(path)),
	).rejects.toThrow(
		'send-replied stopped at once: turn error usageLimitExceeded on turn turn-1 (wire seq 4)',
	);
	expect(judged).toEqual(['frame', 'wake']);
});

const LIMIT = 'You’ve hit your usage limit. Try again later.';

test('a wire error that is the step declared expected error does not stop it (F13)', async () => {
	const hook = referenceTurnFailure(recordingPath(START + turnError(4, LAUNCHED_THREAD, false)));
	const grid = await drawn([`■ ${LIMIT}`]);
	expect(hook(grid, { ...SEND_REPLIED.checkpoint, error_line: LIMIT })).toBeNull();
	// The same error without the declaration still stops the step at once.
	expect(hook(grid, SEND_REPLIED.checkpoint)).toContain('turn error usageLimitExceeded');
});

test('an error Codex marked for retry is exempt on screen too; an unmatched cell still stops (F14)', async () => {
	const hook = referenceTurnFailure(recordingPath(START + turnError(4, LAUNCHED_THREAD, true)));
	expect(hook(await drawn([`■ ${LIMIT}`]), SEND_REPLIED.checkpoint)).toBeNull();
	expect(hook(await drawn(['■ stream disconnected']), SEND_REPLIED.checkpoint)).toBe(
		'send-replied stopped at once: error cell on screen: ■ stream disconnected; no retry (assignment Correction 33)',
	);
});
