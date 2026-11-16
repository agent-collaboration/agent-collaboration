import { expect, test } from 'vitest';

import {
	refusedJourney,
	refusedJourneySchema,
	resumeJourney,
	resumeJourneySchema,
	tierJourney,
	tierJourneySchema,
} from './codex-acp-moved-scenes.ts';
import type {
	RefusedJourneyDeclaration,
	ResumeJourneyDeclaration,
	TierJourneyDeclaration,
} from './codex-acp-moved-scenes.ts';
import { codexJourneySchema, expectedErrorCells, gridShowsCheckpoint } from './codex-acp-scenes.ts';
import type { CodexCheckpoint, CodexJourney } from './codex-acp-scenes.ts';

const EMPTY_COMPOSER = `› Ask Codex to do anything${' '.repeat(94)}`;

function checkpointOf(journey: CodexJourney, scene: string): CodexCheckpoint {
	const step = journey.steps.find((entry) => entry.checkpoint.scene === scene);
	if (step === undefined) throw new Error(`no scene ${scene}`);
	return step.checkpoint;
}

// Placeholders stand where 0.166.0 texts are grounded later by one discovery run.
const RESUME: ResumeJourneyDeclaration = {
	kind: 'resume',
	provider: 'codex',
	command_row: 'DISCOVERY-resume-command-row',
	picker: {
		header: 'DISCOVERY-resume-picker-header',
		rows: ['DISCOVERY-thread-launch', 'DISCOVERY-thread-other'],
		navigation: [{ kind: 'key', key: 'Down' }],
		target_row: 'DISCOVERY-thread-other',
	},
	resumed: {
		history_markers: ['DISCOVERY-history-of-other'],
		other_thread_markers: ['DISCOVERY-history-of-launch'],
	},
	expected_thread_id: '019a0000-0000-7000-8000-0000000000b2',
	deadline_ms: 30_000,
};

test('resume: command, picker, selected row, then the other thread opened with an empty composer', () => {
	const journey = resumeJourney(RESUME);
	expect(journey.name).toBe('codex-acp-o5-codex-resume');
	expect(journey.whole_run_errors).toBe('forbid');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'resume-command-ready',
		'resume-picker-open',
		'resume-row-selected',
		'resume-opened',
	]);
	expect(journey.steps[0]?.actions).toEqual([{ kind: 'text', text: '/resume' }]);
	expect(journey.steps[2]?.actions).toEqual(RESUME.picker.navigation);
});

test('resume: without navigation the picker frame itself must show the target row selected', () => {
	const journey = resumeJourney({ ...RESUME, picker: { ...RESUME.picker, navigation: [] } });
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'resume-command-ready',
		'resume-picker-open',
		'resume-opened',
	]);
	expect(checkpointOf(journey, 'resume-picker-open').require).toContain('› DISCOVERY-thread-other');
});

test('negative: /resume resumed another thread, so the launch thread history is drawn', () => {
	const opened = checkpointOf(resumeJourney(RESUME), 'resume-opened');
	expect(gridShowsCheckpoint(['DISCOVERY-history-of-other', EMPTY_COMPOSER], opened)).toBe(true);
	expect(
		gridShowsCheckpoint(
			['DISCOVERY-history-of-launch', 'DISCOVERY-history-of-other', EMPTY_COMPOSER],
			opened,
		),
	).toBe(false);
	expect(gridShowsCheckpoint(['DISCOVERY-history-of-launch', EMPTY_COMPOSER], opened)).toBe(false);
});

test('negative: the picker still open, or the wrong row selected, is not the opened thread', () => {
	const journey = resumeJourney(RESUME);
	const opened = checkpointOf(journey, 'resume-opened');
	expect(
		gridShowsCheckpoint(
			['DISCOVERY-resume-picker-header', 'DISCOVERY-history-of-other', EMPTY_COMPOSER],
			opened,
		),
	).toBe(false);
	const selected = checkpointOf(journey, 'resume-row-selected');
	const picker = ['DISCOVERY-resume-picker-header'];
	expect(gridShowsCheckpoint([...picker, '› DISCOVERY-thread-other'], selected)).toBe(true);
	expect(gridShowsCheckpoint([...picker, '› DISCOVERY-thread-launch'], selected)).toBe(false);
});

function refuseResume(patch: Partial<ResumeJourneyDeclaration>): void {
	expect(() => resumeJourneySchema.parse({ ...RESUME, ...patch })).toThrow();
}

test('resume schema refuses a marker the picker could draw, a missing row and a shared marker', () => {
	const refuse = refuseResume;
	refuse({ resumed: { ...RESUME.resumed, history_markers: ['DISCOVERY-thread-other'] } });
	refuse({ picker: { ...RESUME.picker, target_row: 'not a row' } });
	refuse({ resumed: { ...RESUME.resumed, other_thread_markers: ['DISCOVERY-history-of-other'] } });
	refuse({ expected_thread_id: 'not-a-uuid' });
});

const REFUSED: RefusedJourneyDeclaration = {
	kind: 'refused',
	provider: 'claude',
	commands: [
		{ command: '/fork', refusal: 'DISCOVERY-fork-refusal' },
		{ command: '/status extra', refusal: 'DISCOVERY-status-refusal' },
	],
};

test('refused commands: each is typed, refused with its own error cell, composer empty again', () => {
	const journey = refusedJourney(REFUSED);
	expect(journey.whole_run_errors).toBe('scoped');
	expect(journey.steps.map((step) => step.checkpoint.scene)).toEqual([
		'refused-1-command-ready',
		'refused-1-refused',
		'refused-2-command-ready',
		'refused-2-refused',
	]);
	// The scoped rule's allowed cells are exactly the declared refusals.
	expect(expectedErrorCells(journey)).toEqual([
		'DISCOVERY-fork-refusal',
		'DISCOVERY-status-refusal',
	]);
});

test('negative: an undeclared error cell is not the refusal', () => {
	const refused = checkpointOf(refusedJourney(REFUSED), 'refused-1-refused');
	expect(gridShowsCheckpoint(['■ DISCOVERY-fork-refusal', EMPTY_COMPOSER], refused)).toBe(true);
	expect(gridShowsCheckpoint(['■ Failed to load hooks', EMPTY_COMPOSER], refused)).toBe(false);
	// The right text with more after it is another cell.
	expect(
		gridShowsCheckpoint(['■ DISCOVERY-fork-refusal and the CLI exited', EMPTY_COMPOSER], refused),
	).toBe(false);
});

test('negative: a refused command that ends the CLI never draws the empty composer', () => {
	const refused = checkpointOf(refusedJourney(REFUSED), 'refused-1-refused');
	// The refusal was drawn, then the CLI exited and the shell prompt took the screen.
	expect(gridShowsCheckpoint(['■ DISCOVERY-fork-refusal', '$ '], refused)).toBe(false);
	expect(gridShowsCheckpoint(['■ DISCOVERY-fork-refusal', '› /fork'], refused)).toBe(false);
});

test('the refusal must not already be on screen before its command is submitted', () => {
	const ready = checkpointOf(refusedJourney(REFUSED), 'refused-1-command-ready');
	expect(gridShowsCheckpoint(['› /fork'], ready)).toBe(true);
	expect(gridShowsCheckpoint(['■ DISCOVERY-fork-refusal', '› /fork'], ready)).toBe(false);
});

function refuseCommands(commands: RefusedJourneyDeclaration['commands']): void {
	expect(() => refusedJourneySchema.parse({ ...REFUSED, commands })).toThrow();
}

test('refused schema refuses repeats, overlapping texts and an echo of the refusal', () => {
	const refuse = refuseCommands;
	refuse([]);
	refuse([
		{ command: '/fork', refusal: 'a refusal' },
		{ command: '/fork', refusal: 'another refusal' },
	]);
	refuse([
		{ command: '/fork', refusal: 'not supported' },
		{ command: '/status', refusal: 'not supported here' },
	]);
	refuse([{ command: '/fork now', refusal: 'now' }]);
	refuse([{ command: 'fork', refusal: 'no slash' }]);
});

test('a scoped journey with no declared error cell is refused', () => {
	expect(() =>
		codexJourneySchema.parse({
			name: 'codex-acp-scoped',
			steps: [
				{
					actions: [],
					checkpoint: { scene: 'only', require: ['x'], forbid: [], deadline_ms: 1000 },
				},
			],
			whole_run_errors: 'scoped',
		}),
	).toThrow('at least one expected error cell');
});

const TIER: TierJourneyDeclaration = {
	kind: 'tier',
	provider: 'codex',
	model: {
		catalogue_page: ['gpt-5.5', 'gpt-5.5-mini'],
		stale_rows: [],
		model_navigation: [{ kind: 'key', key: 'Down' }],
		target: { display_name: 'gpt-5.5-mini', number: 2, is_current: false },
		effort: { navigation: [], value: 'low', number: 1, is_default: false, is_current: false },
		apply: 'session',
		expected_outcome: ['Model changed to gpt-5.5-mini'],
		saved_default: null,
		outcome_deadline_ms: 20_000,
	},
	tier: {
		open: [{ kind: 'key', key: 'Tab' }],
		popup_header: 'DISCOVERY-tier-popup-header',
		navigation: [{ kind: 'key', key: 'Down' }],
		row: { number: 2, label: 'DISCOVERY-tier-fast', is_current: false, is_default: false },
		apply: { kind: 'key', key: 'Enter' },
		outcome: ['DISCOVERY-tier-applied'],
		outcome_deadline_ms: 20_000,
		initial_wire_value: null,
		wire_value: 'priority',
	},
	scripted_settings: { model: 'gpt-5.5-mini', effort: 'low', service_tier: 'priority' },
};

test('tier: the session /model pick is followed by the declared tier choice', () => {
	const journey = tierJourney(TIER);
	expect(journey.name).toBe('codex-acp-o5-codex-tier');
	const scenes = journey.steps.map((step) => step.checkpoint.scene);
	expect(scenes.slice(0, 2)).toEqual(['model-command-ready', 'model-catalogue-ready']);
	expect(scenes.at(-1)).toBe('tier-applied');
	expect(scenes.slice(-3)).toEqual(['tier-popup-open', 'tier-row-selected', 'tier-applied']);
	expect(journey.whole_run_errors).toBe('forbid');
});

test('negative: the wrong tier row selected, or the popup left open, is not the pick', () => {
	const journey = tierJourney(TIER);
	const selected = checkpointOf(journey, 'tier-row-selected');
	const popup = ['DISCOVERY-tier-popup-header'];
	expect(gridShowsCheckpoint([...popup, '› 2. DISCOVERY-tier-fast'], selected)).toBe(true);
	expect(gridShowsCheckpoint([...popup, '› 1. DISCOVERY-tier-standard'], selected)).toBe(false);
	const applied = checkpointOf(journey, 'tier-applied');
	expect(gridShowsCheckpoint(['DISCOVERY-tier-applied'], applied)).toBe(true);
	expect(gridShowsCheckpoint([...popup, 'DISCOVERY-tier-applied'], applied)).toBe(false);
});

function refuseTier(declaration: unknown): void {
	expect(() => tierJourneySchema.parse(declaration)).toThrow();
}

test('tier schema refuses a pick without /model, a default apply and a tier that cannot be seen', () => {
	const refuse = refuseTier;
	const { model: _model, ...withoutModel } = TIER;
	refuse(withoutModel);
	refuse({ ...TIER, model: { ...TIER.model, apply: 'default' } });
	// The pick equals the tier before it; the scripted settings end with another tier.
	refuse({ ...TIER, tier: { ...TIER.tier, initial_wire_value: 'priority' } });
	refuse({ ...TIER, scripted_settings: { ...TIER.scripted_settings, service_tier: null } });
	refuse({ ...TIER, scripted_settings: { ...TIER.scripted_settings, effort: 'high' } });
});
