/**
 * The shared running-turn signal (assignment Correction 65), over controlled frames and recorded
 * tee frames shaped like D0 r1 c64; never a live run, and no real time passes.
 */
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { compareJourney, compareJourneySchema } from './codex-acp-compare-scenes.ts';
import { driveCodexStep } from './codex-acp-run.ts';
import { CODEX_0_165_TEXT, gridShowsCheckpoint, showsRunningTurn } from './codex-acp-scenes.ts';
import { LAUNCHED_THREAD, referenceStartRecording } from './codex-acp-test-fixtures.ts';
import { wireTurnReading, wireTurnRunning } from './codex-acp-turn-signal.ts';

const STEP = compareJourney(
	compareJourneySchema.parse({
		kind: 'compare',
		provider: 'codex',
		send: { prompt: 'Reply with only the product of 4321 and 5678', completion: 'ANSWER 24534638' },
		steer: {
			prompt:
				'Write 150 short numbered sentences about rivers, one per line, then FOXTROT LONG DONE. No tools.',
			steer_text:
				'Also finish with the words GOLF, STEERED, OK joined by underscores in uppercase.',
			completion: 'GOLF_STEERED_OK',
		},
		model: {
			catalogue_page: ['4. GPT-6-Luna (current)'],
			stale_rows: [],
			model_navigation: [],
			target: { display_name: 'GPT-6-Luna', number: 4, is_current: true },
			effort: null,
			apply: 'session',
			expected_outcome: ['for this session only'],
			outcome_deadline_ms: 20_000,
		},
		scripted_settings: { model: 'gpt-6-luna', effort: 'low', service_tier: 'default' },
		initial_selection: null,
		status: { footer_cells: ['GPT-6-Luna low', '732K window'] },
		reply_deadline_ms: 120_000,
	}),
).steps.find((step) => step.checkpoint.scene === 'steer-turn-running');
if (STEP === undefined) throw new Error('no steer-turn-running step');
const RUNNING = STEP;

const directories: string[] = [];
afterEach(() => {
	for (const directory of directories.splice(0))
		rmSync(directory, { recursive: true, force: true });
});
function recording(text: string): string {
	const directory = mkdtempSync(join(tmpdir(), 'turn-signal-'));
	directories.push(directory);
	const path = join(directory, 'wire.jsonl');
	writeFileSync(path, text);
	return path;
}

let nextSeq = 10;
function frame(direction: 'cliToServer' | 'serverToCli', message: unknown): string {
	nextSeq += 1;
	return `${JSON.stringify({ seq: nextSeq, connection: 0, direction, message })}\n`;
}
const turnStarted = (thread: string, turn: string): string =>
	frame('serverToCli', {
		method: 'turn/started',
		params: { threadId: thread, turn: { ['id']: turn, status: 'inProgress' } },
	});
const turnCompleted = (thread: string, turn: string): string =>
	frame('serverToCli', {
		method: 'turn/completed',
		params: { threadId: thread, turn: { ['id']: turn, status: 'completed' } },
	});
const turnStart = (thread: string, clientId: string | null): string =>
	frame('cliToServer', {
		['id']: `t-${nextSeq}`,
		method: 'turn/start',
		params: { threadId: thread, clientUserMessageId: clientId },
	});

// The c64 shape: turn 1 complete, then the steer prompt's turn 2 started and streaming.
const START = referenceStartRecording(LAUNCHED_THREAD);
const REFERENCE = { kind: 'reference' } as const;
const TURN_ONE =
	turnStart(LAUNCHED_THREAD, 'c1') +
	turnStarted(LAUNCHED_THREAD, 'T1') +
	turnCompleted(LAUNCHED_THREAD, 'T1');
const TURN_TWO_RUNNING = turnStart(LAUNCHED_THREAD, 'c2') + turnStarted(LAUNCHED_THREAD, 'T2');

async function drawn(lines: readonly string[]): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(lines.map((line, row) => `\x1b[${row + 1};1H${line}`).join(''));
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}
// The c64 frame: the reply streams, so neither Working nor the hint is on screen.
const STREAMING = ['  37. Rivers carry silt.', '  38. Rivers', '› Ask Codex to do anything'];

test('the wire reading: the latest launch-thread turn started and not completed', () => {
	expect(wireTurnRunning(recording(START + TURN_ONE + TURN_TWO_RUNNING), REFERENCE)).toBe(true);
	expect(
		wireTurnRunning(
			recording(START + TURN_ONE + TURN_TWO_RUNNING + turnCompleted(LAUNCHED_THREAD, 'T2')),
			REFERENCE,
		),
	).toBe(false);
	expect(wireTurnRunning(join(tmpdir(), 'absent-turn-signal.jsonl'), REFERENCE)).toBe(false);
});

test("the reference's own title turn running does not count as its turn running", () => {
	const title = [
		frame('cliToServer', {
			['id']: 'temp-1',
			method: 'thread/start',
			params: { threadSource: 'thread_title' },
		}),
		frame('serverToCli', { ['id']: 'temp-1', result: { thread: { ['id']: 'title-thread' } } }),
		turnStart('title-thread', null),
		turnStarted('title-thread', 'title-turn'),
	].join('');
	expect(wireTurnRunning(recording(START + TURN_ONE + title), REFERENCE)).toBe(false);
});

test('the c64 frame passes steer-turn-running through the step driver and the wire reading', async () => {
	const grid = await drawn(STREAMING);
	const path = recording(START + TURN_ONE + TURN_TWO_RUNNING);
	const driver = {
		frameGeneration: (): number => 0,
		send: async (): Promise<void> => undefined,
		waitForGrid: async (
			name: string,
			matches: (candidate: TerminalGrid) => boolean,
		): Promise<TerminalGrid> => {
			if (matches(grid)) return grid;
			throw new Error(`grid checkpoint unreachable: ${name}`);
		},
	};
	await expect(
		driveCodexStep(
			driver,
			RUNNING,
			'legacy',
			null,
			wireTurnReading(path, { reference_model: {}, native_thread_id: null }),
		),
	).resolves.toBe(grid);
	// After turn/completed, with no Working on screen, the same frame fails.
	const ended = recording(
		START + TURN_ONE + TURN_TWO_RUNNING + turnCompleted(LAUNCHED_THREAD, 'T2'),
	);
	await expect(
		driveCodexStep(
			driver,
			RUNNING,
			'legacy',
			null,
			wireTurnReading(ended, { reference_model: {}, native_thread_id: null }),
		),
	).rejects.toThrow('grid checkpoint unreachable: steer-turn-running');
});

test('the screen signals: Working, and the in-turn hint only while the composer holds text', () => {
	const { checkpoint } = RUNNING;
	// The old Working frame (and the Router half's, with or without a tee) passes on the screen.
	expect(gridShowsCheckpoint(['◦ Working (0s • esc to interrupt)'], checkpoint)).toBe(true);
	expect(
		gridShowsCheckpoint(['◦ Working (0s • esc to interrupt)'], checkpoint, { turnRunning: false }),
	).toBe(true);
	// No signal at all fails.
	expect(gridShowsCheckpoint(STREAMING, checkpoint)).toBe(false);
	expect(gridShowsCheckpoint(STREAMING, checkpoint, { turnRunning: false })).toBe(false);
	const hint = `  ${CODEX_0_165_TEXT.inTurnComposerHint}`;
	expect(showsRunningTurn(['› Also finish with GOLF', hint])).toBe(true);
	expect(showsRunningTurn(['› Ask Codex to do anything', hint])).toBe(false);
});

test("an unrelated thread's turn never proves the judged thread is running (F17)", () => {
	// The Reviewer's case: a completed launch turn, then turn/started for another thread.
	const other = START + TURN_ONE + turnStarted('other-thread', 'X1');
	expect(wireTurnRunning(recording(other), REFERENCE)).toBe(false);
	// A Router target judges the Session thread it resumed, and nothing else.
	const session = { kind: 'session', threadId: LAUNCHED_THREAD } as const;
	expect(wireTurnRunning(recording(TURN_ONE + TURN_TWO_RUNNING), session)).toBe(true);
	expect(wireTurnRunning(recording(TURN_ONE + turnStarted('other-thread', 'X1')), session)).toBe(
		false,
	);
	expect(
		wireTurnReading(recording(TURN_ONE + TURN_TWO_RUNNING), {
			reference_model: null,
			native_thread_id: LAUNCHED_THREAD,
		})(),
	).toBe(true);
});

test('user history above an empty active composer is not typed text (F18)', () => {
	const hint = `  ${CODEX_0_165_TEXT.inTurnComposerHint}`;
	expect(
		showsRunningTurn([
			'› Earlier user request retained in history',
			'› Ask Codex to do anything',
			hint,
		]),
	).toBe(false);
	expect(showsRunningTurn(['› Earlier user request', '› Also finish with GOLF', hint])).toBe(true);
});
