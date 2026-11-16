import { join } from 'node:path';

import { expect, test } from 'vitest';

import { sourceBinding } from '../evidence/source-binding.ts';
import { initializedSourceRepository } from '../fixtures/controlled-child-fixtures.ts';
import { movedRunRecord } from './codex-acp-moved-run.ts';
import { codexAcpRunPlanSchema, codexJourneyFor, watcherFor } from './codex-acp-run.ts';
import { gridShowsCheckpoint } from './codex-acp-scenes.ts';
import { targetFixture } from './codex-acp-test-fixtures.ts';
import {
	WINDOW_B_ATTACH_SCENE,
	windowsJourneySchema,
	windowsJourneys,
} from './codex-acp-windows-scenes.ts';

const WINDOWS = {
	kind: 'windows',
	provider: 'codex',
	materialize: {
		prompt: 'Reply with only the word hyacinth in capitals',
		completion: 'HYACINTH OPENS',
	},
	send: { prompt: 'Reply with only the product of 4321 and 5678', completion: 'ANSWER 24534638' },
	reply_deadline_ms: 120_000,
} as const;
const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;

async function planFor(wireTee: boolean): Promise<ReturnType<typeof codexAcpRunPlanSchema.parse>> {
	const { root, target } = await targetFixture(false, 'codex-local');
	const repository = initializedSourceRepository();
	const output = join(root, 'run');
	return codexAcpRunPlanSchema.parse({
		repository,
		source_head: sourceBinding(repository).head,
		driver: target.codex,
		node: target.codex,
		chrome: target.codex,
		keys: 'legacy',
		output,
		target: {
			...target,
			wire_tee: wireTee
				? { listen_socket: join(root, 'tee.sock'), recording: join(output, 'wire.jsonl') }
				: null,
		},
		journey: WINDOWS,
	});
}

test('window A runs two turns; window B, attached after the first, must draw the second reply', () => {
	const { windowA, windowB } = windowsJourneys(WINDOWS);
	expect(windowA.steps.map((step) => step.checkpoint.scene)).toEqual([
		'window-a-first-composed',
		WINDOW_B_ATTACH_SCENE,
		'window-a-composed',
		'window-a-replied',
	]);
	expect(WINDOW_B_ATTACH_SCENE).toBe('window-a-first-replied');
	expect(windowB.steps.map((step) => step.checkpoint.scene)).toEqual(['window-b-sees-reply']);
	expect(windowB.steps[0]?.actions).toEqual([]);
	const seen = windowB.steps[0]?.checkpoint;
	if (seen === undefined) throw new Error('no B step');
	expect(gridShowsCheckpoint(['• ANSWER 24534638'], seen)).toBe(true);
	// B misses A's turn: its screen never draws the reply, or draws an error instead.
	expect(gridShowsCheckpoint([EMPTY_COMPOSER], seen)).toBe(false);
	expect(gridShowsCheckpoint(['ANSWER 24534638', '■ stream failed'], seen)).toBe(false);
});

test("the typed prompt is never window B's reply, and the schema refuses an echo", () => {
	expect(() =>
		windowsJourneys({ ...WINDOWS, send: { ...WINDOWS.send, completion: 'product of 4321' } }),
	).toThrow();
	expect(() => windowsJourneySchema.parse({ ...WINDOWS, extra: 1 })).toThrow();
	// The two turns cannot share a completion, and neither prompt may echo the other's.
	expect(() =>
		windowsJourneySchema.parse({
			...WINDOWS,
			materialize: { ...WINDOWS.materialize, completion: WINDOWS.send.completion },
		}),
	).toThrow('distinct completions');
	const { materialize: _materialize, ...oneTurn } = WINDOWS;
	expect(() => windowsJourneySchema.parse(oneTurn)).toThrow();
});

test('a two-window plan needs the wire tee, builds window A, and records both connections', async () => {
	await expect(planFor(false)).rejects.toThrow('records the wire');
	const plan = await planFor(true);
	expect(codexJourneyFor(plan).name).toBe('codex-acp-o5-codex-windows');
	expect(movedRunRecord(plan)).toEqual({
		kind: 'windows',
		wire_recording: join(plan.output, 'wire.jsonl'),
		window_a: 0,
		window_b: 1,
	});
});

test('both halves attach window B after turn 1, so the comparison stays symmetric (Correction 32)', async () => {
	// On real Codex, B resumes A's thread, whose rollout exists only after A's first message.
	const reference = await planFor(true);
	expect(watcherFor(reference)).toMatchObject({
		attach: WINDOW_B_ATTACH_SCENE,
		observe: 'window-a-replied',
	});
	const router = codexAcpRunPlanSchema.parse({
		...reference,
		target: {
			...(await targetFixture()).target,
			status_line: reference.target.status_line,
			wire_tee: reference.target.wire_tee,
		},
		journey: { ...WINDOWS, provider: 'cursor' },
	});
	expect(watcherFor(router)).toMatchObject({
		attach: WINDOW_B_ATTACH_SCENE,
		observe: 'window-a-replied',
	});
});
