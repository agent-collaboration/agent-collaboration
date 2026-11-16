/**
 * Baseline continuation after a named historical red, over controlled frames drawn by the real
 * terminal core. The steps are the r3 Haiku compare declaration's own model-apply and status
 * steps; the frames reproduce the r3-c17f baseline grid (95ab03b56), never a live run.
 */
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, expect, test } from 'vitest';

import { createTerminalCore, snapshotGrid } from '../terminal/terminal-grid.ts';
import type { TerminalGrid } from '../terminal/terminal-grid.ts';
import { compareJourney, compareJourneySchema } from './codex-acp-compare-scenes.ts';
import {
	HISTORICAL_REDS_FILE,
	HistoricalRedContinuation,
	requireRedsInJourney,
} from './codex-acp-historical-reds.ts';
import type { HistoricalRed } from './codex-acp-historical-reds.ts';
import { driveCodexStep } from './codex-acp-run.ts';
import type { CodexJourney } from './codex-acp-scenes.ts';

const R3_HAIKU = compareJourneySchema.parse({
	kind: 'compare',
	provider: 'claude',
	send: {
		prompt: 'Reply with three uppercase words joined by underscores: DELTA, SEND, CHECK. No tools.',
		completion: 'DELTA_SEND_CHECK',
	},
	steer: {
		prompt: 'Count from 1 to 2000, one number per line, then write FOXTROT LONG DONE. No tools.',
		steer_text: 'Also finish with the words GOLF, STEERED, OK joined by underscores in uppercase.',
		completion: 'GOLF_STEERED_OK',
	},
	model: {
		catalogue_page: ['1. Default (recommended)', '5. Haiku 5.5 (current)'],
		stale_rows: [],
		model_navigation: [],
		target: { display_name: 'Haiku 5.5', number: 5, is_current: true },
		effort: {
			navigation: [
				{ kind: 'key', key: 'Up' },
				{ kind: 'key', key: 'Up' },
			],
			value: 'low',
			number: 2,
			is_default: false,
			is_current: false,
		},
		apply: 'session',
		expected_outcome: ['for this session only'],
		outcome_deadline_ms: 120_000,
	},
	scripted_settings: { model: 'haiku', effort: 'low', service_tier: null },
	initial_selection: {
		model: 'haiku',
		selectOnCreate: false,
		selectOnRestore: false,
		requiredOptions: {},
	},
	status: { footer_cells: ['haiku low', '770K window'] },
	reply_deadline_ms: 300_000,
});
const STEPS = compareJourney(R3_HAIKU).steps;
function stepOf(scene: string): CodexJourney['steps'][number] {
	const step = STEPS.find((candidate) => candidate.checkpoint.scene === scene);
	if (step === undefined) throw new Error(`no ${scene} step`);
	return step;
}
// The two steps from the red on: the session apply and the footer.
const TAIL: CodexJourney = {
	name: 'codex-acp-o5-claude-compare',
	steps: [stepOf('model-applied'), stepOf('status-footer')],
};

const RED: HistoricalRed = {
	scene: 'model-applied',
	reason: 'thread/settings/update with serviceTier refused, -32000; fixed by da58f7bec (#242)',
	error_line: 'Failed to update thread settings: thread/settings/update failed in TUI',
};

// Rows of the r3-c17f baseline final grid, after the refused session apply.
const REFUSED = [
	'  GOLF_STEERED_OK',
	'',
	'■ Failed to update thread settings: thread/settings/update failed in TUI',
	'',
	'› Ask Codex to do anything',
	'',
	'  Haiku 5.5 high · 770K window · ← for agents',
];
// What the head is expected to draw instead: the apply succeeds and the footer follows it.
const APPLIED = [
	'  GOLF_STEERED_OK',
	'',
	'• Model changed to Haiku 5.5 low for this session only',
	'',
	'› Ask Codex to do anything',
	'',
	'  haiku low · 770K window · ← for agents',
];

async function drawn(lines: readonly string[]): Promise<TerminalGrid> {
	const core = await createTerminalCore(120, 36);
	try {
		core.writeString(lines.map((line, row) => `\x1b[${row + 1};1H${line}`).join(''));
		// The composer cursor: row of "› Ask Codex", column 2.
		const composer = lines.findIndex((line) => line.startsWith('› Ask Codex'));
		core.writeString(`\x1b[${composer + 1};3H`);
		return snapshotGrid(core);
	} finally {
		core.dispose();
	}
}

/** A controlled terminal whose frame after any key is `afterKeys`; a wait fails on no match. */
function scriptedDriver(afterKeys: TerminalGrid): {
	readonly frameGeneration: () => number;
	readonly send: () => Promise<void>;
	readonly waitForGrid: (
		name: string,
		matches: (grid: TerminalGrid) => boolean,
		after?: number,
	) => Promise<TerminalGrid>;
	readonly sent: () => number;
} {
	let generation = 0;
	let keys = 0;
	return {
		frameGeneration: (): number => generation,
		send: async (): Promise<void> => {
			keys += 1;
			generation += 1;
		},
		waitForGrid: async (name, matches, after = -1): Promise<TerminalGrid> => {
			if (generation > after && matches(afterKeys)) return afterKeys;
			// Deterministic stand-in for the PTY deadline: no further frame ever arrives.
			throw new Error(`grid checkpoint unreachable: ${name}`);
		},
		sent: (): number => keys,
	};
}

const outputs: string[] = [];
afterEach(() => {
	for (const directory of outputs.splice(0)) rmSync(directory, { recursive: true, force: true });
});
function freshOutput(): string {
	const directory = mkdtempSync(join(tmpdir(), 'reds-'));
	outputs.push(directory);
	return directory;
}

async function driveTail(
	continuation: HistoricalRedContinuation,
	frame: TerminalGrid,
): Promise<{ scenes: string[]; recorded: string[] }> {
	const driver = scriptedDriver(frame);
	const recorded: string[] = [];
	const scenes: string[] = [];
	await TAIL.steps.reduce<Promise<void>>(async (previous, step) => {
		await previous;
		scenes.push(
			await continuation.driveStep({
				driver,
				step,
				driveStep: (stepDriver, declared) => driveCodexStep(stepDriver, declared, 'legacy'),
				record: async (recordedStep): Promise<void> => {
					recorded.push(recordedStep.checkpoint.scene);
				},
			}),
		);
	}, Promise.resolve());
	return { scenes, recorded };
}

test('the named baseline refusal is recorded as a red and the footer step still runs and is judged', async () => {
	const continuation = new HistoricalRedContinuation([RED]);
	const { scenes, recorded } = await driveTail(continuation, await drawn(REFUSED));
	expect(scenes).toEqual(['model-applied-historical-red', 'status-footer']);
	expect(recorded).toEqual(scenes);
	expect(scenes).toEqual(continuation.expectedScenes(TAIL));
	const directory = freshOutput();
	continuation.settle(directory);
	const record: unknown = JSON.parse(readFileSync(join(directory, HISTORICAL_REDS_FILE), 'utf8'));
	expect(record).toEqual({
		build: 'baseline',
		reds: [
			{
				scene: 'model-applied',
				reason: RED.reason,
				error_line: RED.error_line,
				step_failure: 'grid checkpoint unreachable: model-applied',
			},
		],
		// The 770K window cell is decided in the same run; the effort never changed.
		continued_steps: [
			{
				scene: 'status-footer',
				passed: false,
				present_require: ['770K window'],
				missing_require: ['haiku low'],
				present_forbid: ['■ '],
			},
		],
	});
	expect(continuation.expectedErrorCells()).toEqual([RED.error_line]);
});

test('a named red that passes at this build is the stop condition, not a continuation', async () => {
	const continuation = new HistoricalRedContinuation([RED]);
	await expect(driveTail(continuation, await drawn(APPLIED))).rejects.toThrow(
		'historical red did not reproduce: model-applied passed at this build',
	);
});

test('a named step that fails without its error cell stops the run', async () => {
	const continuation = new HistoricalRedContinuation([RED]);
	const other = REFUSED.map((line) =>
		line.startsWith('■ ') ? '■ Failed to update thread settings: provider is busy' : line,
	);
	await expect(driveTail(continuation, await drawn(other))).rejects.toThrow(
		'model-applied failed without its named error cell',
	);
});

test('with no named reds the head path stays strict: the refused apply stops the run', async () => {
	const continuation = new HistoricalRedContinuation([]);
	expect(continuation.active).toBe(false);
	await expect(driveTail(continuation, await drawn(REFUSED))).rejects.toThrow(
		'grid checkpoint unreachable: model-applied',
	);
});

test('a red must name one declared step, once', () => {
	expect(() => requireRedsInJourney([{ ...RED, scene: 'model-saved' }], TAIL)).toThrow(
		'names no declared step',
	);
	expect(() => requireRedsInJourney([RED, RED], TAIL)).toThrow('named twice');
	expect(() => requireRedsInJourney([RED], TAIL)).not.toThrow();
});

test('settle refuses a run whose named red was never reached', () => {
	const continuation = new HistoricalRedContinuation([RED]);
	expect(() => continuation.settle(freshOutput())).toThrow('named historical reds never reached');
});
