import { expect, test } from 'vitest';

import {
	codexEffortLabel,
	codexSavedDefaultMessage,
	modelJourney,
} from './codex-acp-model-scenes.ts';
import type { ModelJourneyInput } from './codex-acp-model-scenes.ts';
import { gridShowsCheckpoint, gridShowsSelectedMenuRow } from './codex-acp-scenes.ts';

// The slash popup row for /model (slash_command.rs:135), distinct from the composer echo.
const MODEL_ROW = '› /model      choose what model and reasoning effort to use';

test('/model Enter follows only the drawn command row, never the composer echo alone', () => {
	const [ready, submitted] = modelJourney(MODEL_INPUT).steps;
	if (ready === undefined || submitted === undefined) throw new Error('command scene missing');
	expect(ready.checkpoint.scene).toBe('model-command-ready');
	expect(ready.actions.every((action) => action.kind === 'text')).toBe(true);
	expect(submitted.actions[0]).toEqual({ kind: 'key', key: 'Enter' });
	expect(gridShowsCheckpoint(['› /model', MODEL_ROW], ready.checkpoint)).toBe(true);
	expect(gridShowsCheckpoint(['› /model'], ready.checkpoint)).toBe(false);
});

// The Router's refusal of a Codex default save (router_session_app_server_methods.rs, cd9668cd5).
const ROUTER_SAVE_REFUSAL =
	'Saving Codex configuration/defaults is not supported on this ACP endpoint';

test('/model waits for the refreshed catalogue and each selected row before its next key', () => {
	const journey = modelJourney(MODEL_INPUT);
	expect(journey.name).toBe('codex-acp-j2-claude-model-default');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-catalogue-ready',
		'model-row-selected',
		'reasoning-popup-open',
		'effort-selected',
		'model-applied',
	]);
	// Only the declared keys: Enter opens, the menu keys move, Enter applies.
	expect(journey.steps.map((step) => step.actions)).toEqual([
		[{ kind: 'text', text: '/model' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Down' }],
		[{ kind: 'key', key: 'Enter' }],
		[{ kind: 'key', key: 'Down' }],
		[{ kind: 'key', key: 'Enter' }],
	]);
	const [, catalogue, row, reasoning, effort, applied] = journey.steps.map(
		(step) => step.checkpoint,
	);
	if (!catalogue || !row || !reasoning || !effort || !applied)
		throw new Error('model scene missing');
	const refreshed = [
		'  Select Model and Effort',
		'› 1. Sonnet 4.6 (current)  Balanced',
		'  2. Haiku 5.5  Fast',
	];
	expect(gridShowsCheckpoint(refreshed, catalogue)).toBe(true);
	for (const screen of [
		// The cached bootstrap view before FetchModels returns.
		['  Select Model and Effort', '› 1. Use provider settings (current)'],
		// The refreshed rows with the stale bootstrap row still drawn.
		[...refreshed, '  3. Use provider settings'],
		// The command still in the composer.
		['› /model'],
		// Another catalogue: the declared page rows are missing.
		['  Select Model and Effort', '› 1. Other Model (current)', '  2. Another Model'],
	])
		expect(gridShowsCheckpoint(screen, catalogue), screen.join('|')).toBe(false);
	// With no stale rows declared (the bound list keeps the delegation), only the page proves refresh.
	const keptDelegation = modelJourney({ ...MODEL_INPUT, staleRows: [] }).steps[1]?.checkpoint;
	if (keptDelegation === undefined) throw new Error('catalogue scene missing');
	expect(
		gridShowsCheckpoint(
			['  Select Model and Effort', '› 1. Use provider settings (current)'],
			keptDelegation,
		),
	).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Model and Effort', '  1. Sonnet 4.6 (current)', '› 2. Haiku 5.5  Fast'],
			row,
		),
	).toBe(true);
	for (const selected of [
		'› 1. Sonnet 4.6 (current)',
		'  2. Haiku 5.5  Fast',
		'› 2. Haiku 5.5 (current)',
		'› 3. Haiku 5.5',
		'› 2. Haiku 5.5x',
	])
		expect(gridShowsCheckpoint(['  Select Model and Effort', selected], row), selected).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Reasoning Level for Haiku 5.5', '› 1. Low (default)'],
			reasoning,
		),
	).toBe(true);
	expect(gridShowsCheckpoint(['  Select Reasoning Level for Sonnet 4.6'], reasoning)).toBe(false);
	expect(
		gridShowsCheckpoint(
			['  Select Reasoning Level for Haiku 5.5', '  1. Low (default)', '› 2. High'],
			effort,
		),
	).toBe(true);
	// Codex labels the native value; the raw value, another marker or another row is not this row.
	for (const selected of [
		'› 2. high',
		'› 2. High (default)',
		'› 2. High (current)',
		'› 1. Low (default)',
	])
		expect(
			gridShowsCheckpoint(['  Select Reasoning Level for Haiku 5.5', selected], effort),
			selected,
		).toBe(false);
	// The regular PersistModelSelection success cell (event_dispatch.rs:2575-2584).
	expect(gridShowsCheckpoint(['• Model changed to claude-haiku-5-5 high'], applied)).toBe(true);
	for (const screen of [
		// Another sibling message that saves differently or not at all.
		['• Model changed to claude-haiku-5-5 high for this conversation'],
		['• Model changed to claude-haiku-5-5 high for this session only'],
		// A popup still open over the success cell.
		['• Model changed to claude-haiku-5-5 high', '  Select Reasoning Level for Haiku 5.5'],
		['• Model changed to claude-haiku-5-5 high', '  Select Model and Effort'],
		// The success cell twice, or beside the red save error.
		['• Model changed to claude-haiku-5-5 high', '• Model changed to claude-haiku-5-5 high'],
		['• Model changed to claude-haiku-5-5 high', '■ Failed to save default model: io error'],
		// The text not as its own info cell.
		['  Model changed to claude-haiku-5-5 high'],
		['• Model changed to claude-haiku-5-5 xhigh'],
		['› /model'],
	])
		expect(gridShowsCheckpoint(screen, applied), screen.join('|')).toBe(false);
	// Before the apply key, the success text already on screen means it is not this action's.
	expect(
		gridShowsCheckpoint([...refreshed, '• Model changed to claude-haiku-5-5 high'], catalogue),
	).toBe(false);
	expect(journey.whole_run_errors).toBe('forbid');
});

test('/model folds a selection already drawn into the frame that shows it, and refuses advanced efforts', () => {
	// The target is already selected when the refreshed catalogue appears: no navigation step.
	const direct = modelJourney({
		...MODEL_INPUT,
		modelNavigation: [],
		target: { displayName: 'Sonnet 4.6', number: 1, isCurrent: true },
		effort: null,
		apply: 'session',
		expectedOutcome: ['Model changed to claude-sonnet-4-6', 'for this session only'],
		savedDefault: null,
	});
	expect(direct.steps.map((step) => step.checkpoint.scene)).toEqual([
		'model-command-ready',
		'model-catalogue-ready',
		'model-applied',
	]);
	expect(direct.steps[1]?.checkpoint.selected_menu_row).toEqual({
		number: 1,
		label: 'Sonnet 4.6',
		default: false,
		current: true,
	});
	expect(direct.steps[2]?.actions).toEqual([{ kind: 'text', text: 's' }]);
	// A session apply keeps its declared outcome and no whole-run error gate.
	expect(direct.steps[2]?.checkpoint.info_line).toBeUndefined();
	expect(direct.whole_run_errors).toBeUndefined();
	const effortFirst = modelJourney({
		...MODEL_INPUT,
		effort: { ...MODEL_EFFORT, navigation: [], number: 1, value: 'low', isDefault: true },
		savedDefault: { model: 'claude-haiku-5-5', effort: 'low' },
	});
	expect(effortFirst.steps.map((step) => step.checkpoint.scene)).not.toContain('effort-selected');
	expect(effortFirst.steps[3]?.checkpoint.selected_menu_row).toEqual({
		number: 1,
		label: 'Low',
		default: true,
		current: false,
	});
	for (const value of ['max', 'ultra'])
		expect(() => modelJourney({ ...MODEL_INPUT, effort: { ...MODEL_EFFORT, value } })).toThrow(
			'More reasoning',
		);
	expect(() => modelJourney({ ...MODEL_INPUT, cataloguePage: [] })).toThrow('catalogue page');
	expect(codexEffortLabel('xhigh')).toBe('Extra high');
	expect(codexEffortLabel('provider')).toBe('provider');
});

// A placeholder refusal for the oracle; a real plan declares the endpoint's text from its source.
const MODEL_EFFORT = {
	navigation: [{ kind: 'key', key: 'Down' }],
	value: 'high',
	number: 2,
	isDefault: false,
	isCurrent: false,
} as const;
const MODEL_INPUT: ModelJourneyInput = {
	provider: 'claude',
	cataloguePage: ['Sonnet 4.6', 'Haiku 5.5'],
	staleRows: ['Use provider settings'],
	modelNavigation: [{ kind: 'key', key: 'Down' }],
	target: { displayName: 'Haiku 5.5', number: 2, isCurrent: false },
	effort: MODEL_EFFORT,
	apply: 'default',
	expectedOutcome: [],
	savedDefault: { model: 'claude-haiku-5-5', effort: 'high' },
	outcomeDeadlineMs: 8_000,
};

test('the default apply never accepts the old default-save refusal as its outcome', () => {
	const applied = modelJourney(MODEL_INPUT).steps.at(-1)?.checkpoint;
	if (applied === undefined) throw new Error('model apply scene missing');
	// The red frame the earlier refusal produced (pre-M1 Router), alone or after a success line.
	for (const screen of [
		[`■ Failed to save default model: ${ROUTER_SAVE_REFUSAL}`],
		[
			'• Model changed to claude-haiku-5-5 high',
			`■ Failed to save default model: ${ROUTER_SAVE_REFUSAL}`,
		],
	])
		expect(gridShowsCheckpoint(screen, applied), screen.join('|')).toBe(false);
	// The refusal can no longer be declared as a default apply's outcome.
	expect(() =>
		modelJourney({
			...MODEL_INPUT,
			expectedOutcome: ['Failed to save default model', ROUTER_SAVE_REFUSAL],
		}),
	).toThrow('only the saved model and effort');
	expect(() => modelJourney({ ...MODEL_INPUT, savedDefault: null })).toThrow(
		'only the saved model and effort',
	);
	expect(() =>
		modelJourney({ ...MODEL_INPUT, apply: 'session', expectedOutcome: ['Model changed'] }),
	).toThrow('saves no default');
	expect(() =>
		modelJourney({ ...MODEL_INPUT, savedDefault: { model: 'claude-haiku-5-5', effort: 'low' } }),
	).toThrow('chosen effort row');
});

test('the saved-default cell follows the native formatter, not the menu label', () => {
	// Literal expected text from app/event_dispatch.rs:2575-2584 and config_persistence.rs:1036-1047.
	for (const [saved, message] of [
		[{ model: 'claude-haiku-5-5', effort: 'xhigh' }, 'Model changed to claude-haiku-5-5 xhigh'],
		[{ model: 'grok-4.6', effort: 'none' }, 'Model changed to grok-4.6 default'],
		[{ model: 'gpt-x', effort: 'turbo-plus' }, 'Model changed to gpt-x turbo-plus'],
		[{ model: 'codex-auto-fast', effort: 'high' }, 'Model changed to codex-auto-fast'],
	] as const)
		expect(codexSavedDefaultMessage(saved)).toBe(message);
	for (const saved of [
		{ model: 'two words', effort: 'high' },
		{ model: 'claude-haiku-5-5', effort: 'high for this conversation' },
		{ model: 'claude-haiku-5-5', effort: '' },
	])
		expect(() => codexSavedDefaultMessage(saved)).toThrow('native printable values');
});

test("an effort row carries Codex's label for the value and nothing appended", () => {
	const high = { number: 2, label: codexEffortLabel('high'), default: false, current: false };
	expect(gridShowsSelectedMenuRow(['› 2. High  Deeper reasoning'], high)).toBe(true);
	for (const line of ['› 2. High Extra', '› 2. High plus'])
		expect(gridShowsSelectedMenuRow([line], high), line).toBe(false);
});
