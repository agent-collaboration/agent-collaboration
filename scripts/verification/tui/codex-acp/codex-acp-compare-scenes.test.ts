import { expect, test } from 'vitest';

import {
	COMPARE_ACTION_SCRIPT,
	compareJourney,
	compareJourneySchema,
} from './codex-acp-compare-scenes.ts';
import type { CompareJourneyDeclaration } from './codex-acp-compare-scenes.ts';
import {
	CODEX_0_165_TEXT,
	codexCheckpointSchema,
	gridShowsCheckpoint,
} from './codex-acp-scenes.ts';
import type { CodexCheckpoint } from './codex-acp-scenes.ts';

// Each prompt asks for a reply line that is never typed: the expected text only the reply makes.
const DECLARATION: CompareJourneyDeclaration = {
	kind: 'compare',
	provider: 'codex',
	send: { prompt: 'Reply with only the product of 4321 and 5678', completion: 'ANSWER 24534638' },
	steer: {
		prompt: 'Count from 1 to 300, one number per line, then stop',
		steer_text: 'Stop counting and reply with only the word hyacinth',
		completion: 'STEERED hyacinth',
	},
	model: {
		catalogue_page: ['gpt-5.5', 'gpt-5.5-mini'],
		stale_rows: [],
		model_navigation: [{ kind: 'key', key: 'Down' }],
		target: { display_name: 'gpt-5.5-mini', number: 2, is_current: false },
		effort: null,
		apply: 'session',
		expected_outcome: ['Model changed to gpt-5.5-mini'],
		saved_default: null,
		outcome_deadline_ms: 20_000,
	},
	scripted_settings: { model: 'gpt-5.5-mini', effort: 'low', service_tier: null },
	initial_selection: null,
	status: { footer_cells: ['gpt-5.5-mini low', '258K window'] },
	reply_deadline_ms: 120_000,
};

const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;

function checkpointOf(scene: string): CodexCheckpoint {
	const step = compareJourney(DECLARATION).steps.find((entry) => entry.checkpoint.scene === scene);
	if (step === undefined) throw new Error(`no scene ${scene}`);
	return step.checkpoint;
}

test('the script is send, steer, model, status, then the shared /quit', () => {
	expect(COMPARE_ACTION_SCRIPT).toEqual(['send', 'steer', 'model', 'status', 'quit']);
	const journey = compareJourney(DECLARATION);
	expect(journey.name).toBe('codex-acp-o5-codex-compare');
	expect(journey.whole_run_errors).toBe('forbid');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'send-composed',
		'send-replied',
		'steer-prompt-composed',
		'steer-turn-running',
		'steer-composed',
		'steer-replied',
		'model-command-ready',
		'model-catalogue-ready',
		'model-row-selected',
		'model-applied',
		'status-footer',
	]);
	// Steer text is typed after the long prompt's Enter, then submitted with its own Enter.
	const actions = journey.steps.map((step) => step.actions);
	expect(actions[3]).toEqual([{ kind: 'key', key: 'Enter' }]);
	expect(actions[4]).toEqual([{ kind: 'text', text: DECLARATION.steer.steer_text }]);
	expect(actions[5]).toEqual([{ kind: 'key', key: 'Enter' }]);
});

test('the steer text is accepted only while the turn is still running', () => {
	const composed = checkpointOf('steer-composed');
	const typed = `› ${DECLARATION.steer.steer_text}`;
	const hint = `  ${CODEX_0_165_TEXT.inTurnComposerHint}`;
	// Running, shown either way Codex 0.166.0 draws it (Correction 51).
	expect(gridShowsCheckpoint([typed, '• Working (3s • esc to interrupt)'], composed)).toBe(true);
	// The D0 r1 c50 shape: the reply streams, Working is gone, the in-turn hint shows.
	expect(gridShowsCheckpoint(['  63. Rivers nourish reeds.', typed, hint], composed)).toBe(true);
	// The reply already finished: neither signal, so the steer would have been a new turn.
	expect(gridShowsCheckpoint([typed, `• ${DECLARATION.steer.completion}`], composed)).toBe(false);
	expect(gridShowsCheckpoint([typed], composed)).toBe(false);
	// The hint without the steer text typed.
	expect(gridShowsCheckpoint(['› ', hint], composed)).toBe(false);
	// The steer's completion already on screen, even with a running signal.
	expect(gridShowsCheckpoint([typed, hint, `• ${DECLARATION.steer.completion}`], composed)).toBe(
		false,
	);
});

test('require_any needs one text of each group, and a one-text group is refused', () => {
	const base: CodexCheckpoint = {
		scene: 'any-of',
		require: ['A'],
		require_any: [['B', 'C']],
		forbid: [],
		deadline_ms: 1_000,
	};
	expect(gridShowsCheckpoint(['A B'], base)).toBe(true);
	expect(gridShowsCheckpoint(['A C'], base)).toBe(true);
	expect(gridShowsCheckpoint(['A'], base)).toBe(false);
	expect(() => codexCheckpointSchema.parse({ ...base, require_any: [['B']] })).toThrow();
});

test('each reply line must come from its own reply, and a stale one cannot pass', () => {
	const sent = checkpointOf('send-replied');
	expect(gridShowsCheckpoint([`• ${DECLARATION.send.completion}`, EMPTY_COMPOSER], sent)).toBe(
		true,
	);
	expect(gridShowsCheckpoint([EMPTY_COMPOSER], sent)).toBe(false);
	expect(
		gridShowsCheckpoint(
			[`• ${DECLARATION.send.completion}`, '• Working (1s • esc to interrupt)', EMPTY_COMPOSER],
			sent,
		),
	).toBe(false);
	const steered = checkpointOf('steer-replied');
	// The first reply on screen does not satisfy the steered reply.
	expect(gridShowsCheckpoint([`• ${DECLARATION.send.completion}`, EMPTY_COMPOSER], steered)).toBe(
		false,
	);
	expect(gridShowsCheckpoint([`• ${DECLARATION.steer.completion}`, EMPTY_COMPOSER], steered)).toBe(
		true,
	);
	// And the steered reply may not already be on screen before the steer is typed.
	expect(
		gridShowsCheckpoint(
			[`› ${DECLARATION.steer.prompt}`, `• ${DECLARATION.steer.completion}`],
			checkpointOf('steer-prompt-composed'),
		),
	).toBe(false);
});

test('the footer must show every declared cell with an idle composer and no popup', () => {
	const footer = checkpointOf('status-footer');
	const line = '  gpt-5.5-mini low · 258K window';
	expect(gridShowsCheckpoint([EMPTY_COMPOSER, line], footer)).toBe(true);
	// Model without the context window: the window evidence Verifier limitation 2 asked for is missing.
	expect(gridShowsCheckpoint([EMPTY_COMPOSER, '  gpt-5.5-mini low'], footer)).toBe(false);
	expect(gridShowsCheckpoint([EMPTY_COMPOSER, '  gpt-5.5-mini low · 770K window'], footer)).toBe(
		false,
	);
	expect(gridShowsCheckpoint(['Select Model', EMPTY_COMPOSER, line], footer)).toBe(false);
});

test('a default apply, a thin footer and echo-able completions are refused', () => {
	expect(() =>
		compareJourneySchema.parse({
			...DECLARATION,
			model: { ...DECLARATION.model, apply: 'default', expected_outcome: [] },
		}),
	).toThrow();
	expect(() =>
		compareJourneySchema.parse({ ...DECLARATION, status: { footer_cells: ['gpt-5.5-mini low'] } }),
	).toThrow();
	expect(() =>
		compareJourney({
			...DECLARATION,
			steer: { ...DECLARATION.steer, completion: DECLARATION.send.completion },
		}),
	).toThrow('echo could satisfy');
	expect(() =>
		compareJourney({
			...DECLARATION,
			send: { ...DECLARATION.send, completion: DECLARATION.steer.steer_text },
		}),
	).toThrow();
	expect(() =>
		compareJourney({
			...DECLARATION,
			status: { footer_cells: ['hyacinth', '258K window'] },
		}),
	).toThrow('footer cell');
});

test('the Router model guard runs before the send Enter, and a failed guard sends no Enter', async () => {
	const { createTerminalCore, snapshotGrid } = await import('../terminal/terminal-grid.ts');
	const { driveCodexJourney } = await import('./codex-acp-run.ts');
	const core = await createTerminalCore(120, 36);
	core.writeRaw(Buffer.from(`\u001b[2J\u001b[H${EMPTY_COMPOSER}\u001b[1;3H`));
	const sent: string[] = [];
	let generation = 0;
	const driver = {
		frameGeneration: (): number => generation,
		send: async (bytes: Uint8Array): Promise<void> => {
			sent.push(Buffer.from(bytes).toString());
			generation += 1;
			core.writeRaw(Buffer.from(`\u001b[2J\u001b[H› ${DECLARATION.send.prompt}`));
		},
		waitForGrid: async () => snapshotGrid(core),
	};
	const guarded: string[] = [];
	await expect(
		driveCodexJourney(
			driver,
			compareJourney(DECLARATION),
			'legacy',
			async (): Promise<void> => {},
			null,
			null,
			async (): Promise<void> => {
				guarded.push(`guard after ${sent.length} writes`);
				throw new Error('selected model is not the declared Haiku');
			},
			'/launch/cwd',
		),
	).rejects.toThrow('selected model is not the declared Haiku');
	core.dispose();
	expect(guarded).toEqual(['guard after 1 writes']);
	expect(sent).toEqual([DECLARATION.send.prompt]);
});
